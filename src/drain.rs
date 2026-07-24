use drain_rs::DrainTree;
use grok::Grok;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryFrom;

#[derive(Debug, Clone, PartialEq)]
pub struct DrainConfig {
    pub depth: usize,
    pub max_children: usize,
    pub similarity: f64,
    pub filters: Vec<String>,
}

impl Default for DrainConfig {
    fn default() -> Self {
        Self {
            depth: 4,
            max_children: 100,
            similarity: 0.4,
            filters: Vec::new(),
        }
    }
}

impl DrainConfig {
    pub fn sanitized(&self) -> Self {
        let depth = self.depth.max(2);
        let max_children = self.max_children.max(1);
        let similarity = self.similarity.clamp(0.0, 1.0);
        Self {
            depth,
            max_children,
            similarity,
            filters: self.filters.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DrainTemplate {
    pub template: String,
    pub template_id: String,
    pub count: usize,
    pub sample: String,
    pub first_line: Option<usize>,
    pub last_line: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DrainResult {
    pub template: String,
    pub template_id: String,
    pub count: usize,
    pub is_new: bool,
    pub sample: String,
    pub first_line: Option<usize>,
    pub last_line: Option<usize>,
}

/// Metadata tracked per template (sample, first/last line numbers)
#[derive(Debug, Clone)]
struct TemplateMetadata {
    sample: String,
    first_line: Option<usize>,
    last_line: Option<usize>,
}

#[derive(Debug)]
struct DrainState {
    config: DrainConfig,
    tree: DrainTree,
    /// Metadata tracked per normalized template string (see `normalize_template`
    /// — the key is behaviorally identical to the template id, minus the hash).
    metadata: HashMap<String, TemplateMetadata>,
}

impl DrainState {
    fn new(config: DrainConfig) -> Self {
        let config = config.sanitized();
        let mut grok = build_grok();
        let filter_patterns = if config.filters.is_empty() {
            default_filter_patterns()
        } else {
            config.filters.iter().map(|s| s.as_str()).collect()
        };
        let tree = DrainTree::new()
            .max_depth(to_u16(config.depth))
            .max_children(to_u16(config.max_children))
            .min_similarity(config.similarity as f32)
            .filter_patterns(filter_patterns)
            .build_patterns(&mut grok);
        Self {
            config,
            tree,
            metadata: HashMap::new(),
        }
    }

    /// Per-line hot path: add `text` to the tree and update per-template
    /// metadata, returning the raw template string, its match count, and whether
    /// this was the template's first sighting.
    ///
    /// The template id (a SHA256 hash) is deliberately *not* computed here — it
    /// is display-only and derived once per template in [`templates`]. The
    /// metadata key is looked up borrowed, so an owned key `String` is allocated
    /// only the first time a template is seen, not on every line.
    fn record(
        &mut self,
        text: &str,
        line_num: Option<usize>,
    ) -> Result<(String, usize, bool), String> {
        let cluster = self
            .tree
            .add_log_line(text)
            .ok_or_else(|| "Drain failed to match or create a cluster".to_string())?;
        let count = usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX);
        let template = cluster.as_string();
        let is_new = count == 1;

        let key = normalize_template(&template);
        if let Some(meta) = self.metadata.get_mut(&key) {
            // Hit path (the common case): no allocation, just refresh last_line.
            if let Some(ln) = line_num {
                meta.last_line = Some(ln);
            }
        } else {
            self.metadata.insert(
                key,
                TemplateMetadata {
                    sample: text.to_string(),
                    first_line: line_num,
                    last_line: line_num,
                },
            );
        }

        Ok((template, count, is_new))
    }

    fn ingest(&mut self, text: &str, line_num: Option<usize>) -> Result<DrainResult, String> {
        let (template, count, is_new) = self.record(text, line_num)?;
        let template_id = generate_template_id(&template);

        // `record` guarantees the entry exists; the fallback keeps this total.
        let (sample, first_line, last_line) =
            match self.metadata.get(&normalize_template(&template)) {
                Some(meta) => (meta.sample.clone(), meta.first_line, meta.last_line),
                None => (text.to_string(), line_num, line_num),
            };

        Ok(DrainResult {
            template,
            template_id,
            count,
            is_new,
            sample,
            first_line,
            last_line,
        })
    }

    fn templates(&self) -> Vec<DrainTemplate> {
        let mut templates: Vec<DrainTemplate> = self
            .tree
            .log_groups()
            .into_iter()
            .map(|cluster| {
                let template = cluster.as_string();
                let template_id = generate_template_id(&template);
                let meta = self.metadata.get(&normalize_template(&template));
                DrainTemplate {
                    template,
                    template_id,
                    count: usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX),
                    sample: meta.map(|m| m.sample.clone()).unwrap_or_default(),
                    first_line: meta.and_then(|m| m.first_line),
                    last_line: meta.and_then(|m| m.last_line),
                }
            })
            .collect();

        templates.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.template.cmp(&b.template))
        });

        templates
    }
}

/// Generate a stable, deterministic template ID from a template string.
///
/// Format: `v1:<hash>` where hash is SHA256 truncated to 16 hex characters.
///
/// The v1 algorithm:
/// - Normalizes whitespace (splits and rejoins with single spaces)
/// - Computes SHA256 hash of the normalized template
/// - Returns first 8 bytes (64 bits) as hex with "v1:" prefix
///
/// The version prefix allows future algorithm changes without breaking
/// existing saved IDs. This function's behavior must remain stable forever
/// to support long-term template ID persistence and comparison.
pub fn generate_template_id(template: &str) -> String {
    // Normalize whitespace for consistent hashing across formatting variations.
    let normalized = normalize_template(template);

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();

    // v1: prefix for version identification
    format!("v1:{}", hex::encode(&result[..8]))
}

/// Collapse leading/trailing/internal whitespace runs to single spaces.
///
/// This is the exact normalization [`generate_template_id`] hashes, so it
/// *defines* template identity: two template strings that normalize alike belong
/// to one metadata entry. Keying per-template metadata on this normalized string
/// (rather than on the SHA256 template id) preserves the id's collision behavior
/// byte-for-byte while keeping the expensive hash off the per-line hot path — it
/// is only needed for the displayed `template_id`, computed once per template at
/// output time. Builds the result in a single allocation (no intermediate
/// `Vec`); `split_whitespace` never yields empty tokens, so the join is exact.
fn normalize_template(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    for token in template.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}

fn to_u16(value: usize) -> u16 {
    value.min(u16::MAX as usize) as u16
}

fn build_grok() -> Grok {
    let mut grok = Grok::with_patterns();
    for (name, pattern) in custom_grok_definitions() {
        grok.insert_definition(name, pattern);
    }
    grok
}

fn custom_grok_definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        ("KELORA_IPV4_PORT", r"(?:\d{1,3}\.){3}\d{1,3}:\d{1,5}"),
        (
            "KELORA_FQDN",
            r"(?:[a-z](?:[a-z0-9-]{0,63}[a-z0-9])?\.){2,}[a-z0-9][a-z0-9-]{0,8}",
        ),
        ("KELORA_MD5", r"[a-fA-F0-9]{32}"),
        ("KELORA_SHA1", r"[a-fA-F0-9]{40}"),
        ("KELORA_SHA256", r"[a-fA-F0-9]{64}"),
        // Require at least 2 path components to avoid matching ratios like "20/20"
        ("KELORA_PATH", r"/[A-Za-z0-9._-]+(?:/[A-Za-z0-9._-]+)+"),
        ("KELORA_OAUTH", r"ya29\.[0-9A-Za-z_-]+"),
        ("KELORA_FUNCTION", r"[A-Za-z0-9_.]+\([^)]*\)"),
        ("KELORA_HEXCOLOR", r"#[0-9A-Fa-f]{6}"),
        (
            "KELORA_VERSION",
            r"[vV]?\d+\.\d+(?:\.\d+)?(?:-[A-Za-z0-9]+)?",
        ),
        ("KELORA_HEXNUM", r"0x[0-9A-Fa-f]+"),
        ("KELORA_DURATION", r"\d+(?:\.\d+)?(?:us|ms|[smhd])"),
        // ISO8601 timestamps: 2025-01-15T10:00:00Z (T-separator, single token)
        (
            "KELORA_ISO8601",
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
        ),
        // Date only: 2025-01-15 (for space-separated timestamps)
        ("KELORA_DATE", r"\d{4}-\d{2}-\d{2}"),
        // Time only: 10:00:00 or 10:00:00.123 (for space-separated timestamps)
        ("KELORA_TIME", r"\d{2}:\d{2}:\d{2}(?:\.\d+)?"),
        ("KELORA_NUM", r"[+-]?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?"),
    ]
}

fn default_filter_patterns() -> Vec<&'static str> {
    vec![
        "%{KELORA_IPV4_PORT:ipv4_port}",
        "%{IPV4:ipv4}",
        "%{IPV6:ipv6}",
        "%{EMAILADDRESS:email}",
        "%{URI:url}",
        "%{KELORA_FQDN:fqdn}",
        "%{UUID:uuid}",
        "%{MAC:mac}",
        "%{KELORA_MD5:md5}",
        "%{KELORA_SHA1:sha1}",
        "%{KELORA_SHA256:sha256}",
        "%{KELORA_PATH:path}",
        "%{KELORA_OAUTH:oauth}",
        "%{KELORA_FUNCTION:function}",
        "%{KELORA_HEXCOLOR:hexcolor}",
        "%{KELORA_VERSION:version}",
        "%{KELORA_HEXNUM:hexnum}",
        "%{KELORA_DURATION:duration}",
        // Timestamps before NUM so they're matched as a unit
        "%{KELORA_ISO8601:timestamp}",
        "%{KELORA_DATE:date}",
        "%{KELORA_TIME:time}",
        "%{KELORA_NUM:num}",
    ]
}

thread_local! {
    static DRAIN_STATE: RefCell<Option<DrainState>> = const { RefCell::new(None) };
}

pub fn reset() {
    DRAIN_STATE.with(|state| {
        *state.borrow_mut() = None;
    });
}

/// Lazily initialize the thread-local drain state, validating that a
/// caller-supplied config matches any already in effect. Shared by the two
/// ingest entry points below.
fn ensure_state(
    state: &mut Option<DrainState>,
    config: &Option<DrainConfig>,
) -> Result<(), String> {
    match (state.as_ref(), config) {
        (None, Some(cfg)) => {
            *state = Some(DrainState::new(cfg.clone()));
        }
        (None, None) => {
            *state = Some(DrainState::new(DrainConfig::default()));
        }
        (Some(existing), Some(cfg)) => {
            let sanitized = cfg.sanitized();
            if existing.config != sanitized {
                return Err("Drain config already initialized with different options".into());
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn drain_template(
    text: &str,
    config: Option<DrainConfig>,
    line_num: Option<usize>,
) -> Result<DrainResult, String> {
    DRAIN_STATE.with(|state| {
        let mut state = state.borrow_mut();
        ensure_state(&mut state, &config)?;
        let drain = state
            .as_mut()
            .ok_or_else(|| "Drain state not initialized".to_string())?;
        drain.ingest(text, line_num)
    })
}

/// Ingest a line for the `--drain` CLI pipeline, which only needs the model
/// updated and does not consume the per-line [`DrainResult`]. Skips building the
/// result (template id hash, sample clone) entirely — templates and their
/// metadata are emitted once at the end via [`drain_templates`].
pub fn drain_record(
    text: &str,
    config: Option<DrainConfig>,
    line_num: Option<usize>,
) -> Result<(), String> {
    DRAIN_STATE.with(|state| {
        let mut state = state.borrow_mut();
        ensure_state(&mut state, &config)?;
        let drain = state
            .as_mut()
            .ok_or_else(|| "Drain state not initialized".to_string())?;
        drain.record(text, line_num).map(|_| ())
    })
}

pub fn drain_templates() -> Vec<DrainTemplate> {
    DRAIN_STATE.with(|state| match state.borrow().as_ref() {
        Some(drain) => drain.templates(),
        None => Vec::new(),
    })
}

/// One template of a [`FrozenTemplateSet`], parsed back into drain's token
/// representation ("<*>" is the wildcard, everything else is a literal token).
#[derive(Debug, Clone, PartialEq)]
enum FrozenToken {
    WildCard,
    Val(String),
}

struct FrozenEntry {
    tokens: Vec<FrozenToken>,
    template: String,
}

/// A read-only snapshot of a mined template set that can match text against it
/// without touching (or trusting) the live drain tree.
///
/// This is the frozen-matching primitive behind `--drain-diff` pass 2, and is
/// deliberately a self-contained type so a future "match against a saved
/// template set" mode can reuse it.
///
/// Why not `DrainTree::log_group` (the crate's inference mode)? Its lookup is
/// asymmetric with insertion: `add_log_line` converts any path token
/// containing a digit into a wildcard tree key, while `log_group` walks the
/// tree with the raw token — so a line whose masked token *name* contains a
/// digit (e.g. `<ipv4>`, `<sha256>`) within the branch prefix never finds its
/// own cluster. Instead, this matcher scores a line against every template
/// with the same token count using drain's own similarity function (exact
/// matches first, then wildcard coverage) and returns the best. Skipping the
/// tree routing searches a superset of the leaf drain would have picked from,
/// so a line always finds at least the cluster it was mined into.
pub struct FrozenTemplateSet {
    by_len: HashMap<usize, Vec<FrozenEntry>>,
    patterns: Vec<grok::Pattern>,
}

impl FrozenTemplateSet {
    fn new(templates: Vec<String>, filters: &[String]) -> Self {
        let mut grok = build_grok();
        let filter_strs: Vec<&str> = if filters.is_empty() {
            default_filter_patterns()
        } else {
            filters.iter().map(|s| s.as_str()).collect()
        };
        // Mirror DrainTree::build_patterns exactly: named captures only,
        // patterns that fail to compile are skipped.
        let patterns: Vec<grok::Pattern> = filter_strs
            .iter()
            .filter_map(|p| grok.compile(p, true).ok())
            .collect();

        let mut by_len: HashMap<usize, Vec<FrozenEntry>> = HashMap::new();
        for template in templates {
            // as_string() joins tokens with single spaces, so split(' ')
            // round-trips them (including empty tokens from blank runs).
            let tokens: Vec<FrozenToken> = template
                .split(' ')
                .map(|t| {
                    if t == "<*>" {
                        FrozenToken::WildCard
                    } else {
                        FrozenToken::Val(t.to_string())
                    }
                })
                .collect();
            by_len
                .entry(tokens.len())
                .or_default()
                .push(FrozenEntry { tokens, template });
        }
        // log_groups() iterates HashMaps, so its order varies between runs;
        // sort so similarity ties resolve deterministically.
        for entries in by_len.values_mut() {
            entries.sort_by(|a, b| a.template.cmp(&b.template));
        }

        Self { by_len, patterns }
    }

    /// Tokenize and mask a line exactly like `DrainTree::process`: split on
    /// single spaces, trim each token, first matching grok pattern wins and
    /// replaces the token with `<name>`.
    fn mask(&self, line: &str) -> Vec<FrozenToken> {
        line.split(' ')
            .map(|t| t.trim())
            .map(|t| {
                match self
                    .patterns
                    .iter()
                    .map(|p| p.match_against(t))
                    .find(|o| o.is_some())
                {
                    Some(Some(matches)) => match matches.iter().next() {
                        Some((name, _)) => FrozenToken::Val(format!("<{}>", name)),
                        None => FrozenToken::WildCard,
                    },
                    _ => FrozenToken::Val(t.to_string()),
                }
            })
            .collect()
    }

    /// Match `text` against the frozen set, returning the best template with
    /// the same token count (drain's similarity ordering: fraction of exact
    /// token matches first, wildcard coverage as the tie-breaker). Returns
    /// None only when no template has that token count.
    pub fn match_text(&self, text: &str) -> Option<&str> {
        let tokens = self.mask(text);
        let candidates = self.by_len.get(&tokens.len())?;
        let len = tokens.len() as f32;
        let mut best: Option<(f32, u32, &FrozenEntry)> = None;
        for entry in candidates {
            let mut exact = 0f32;
            let mut approximate = 0u32;
            for (pattern, token) in entry.tokens.iter().zip(tokens.iter()) {
                if pattern == token {
                    exact += 1.0;
                } else if matches!(pattern, FrozenToken::WildCard) {
                    approximate += 1;
                }
            }
            let exact = exact / len;
            let better = match &best {
                None => true,
                Some((best_exact, best_approx, _)) => {
                    exact > *best_exact || (exact == *best_exact && approximate > *best_approx)
                }
            };
            if better {
                best = Some((exact, approximate, entry));
            }
        }
        best.map(|(_, _, entry)| entry.template.as_str())
    }
}

/// Snapshot the current thread's mined templates into a [`FrozenTemplateSet`].
/// Empty (matches nothing) when no drain state exists.
pub fn frozen_template_set() -> FrozenTemplateSet {
    DRAIN_STATE.with(|state| {
        let state = state.borrow();
        match state.as_ref() {
            Some(drain) => {
                let templates = drain
                    .tree
                    .log_groups()
                    .into_iter()
                    .map(|cluster| cluster.as_string())
                    .collect();
                FrozenTemplateSet::new(templates, &drain.config.filters)
            }
            None => FrozenTemplateSet::new(Vec::new(), &[]),
        }
    })
}

/// Format templates for table output
/// Format determines output detail level:
/// - Table: clean output with count + template only
/// - Full: adds indented line ranges and samples below each template
pub fn format_templates_output(
    templates: &[DrainTemplate],
    format: crate::cli::DrainFormat,
) -> String {
    if templates.is_empty() {
        return "No templates found".to_string();
    }

    if matches!(format, crate::cli::DrainFormat::Id) {
        return format_templates_id_output(templates);
    }

    let mut output = String::new();
    output.push_str(&format!("templates ({} items):\n", templates.len()));

    // Find max count width for right-alignment
    let max_count_width = templates
        .iter()
        .map(|t| t.count.to_string().len())
        .max()
        .unwrap_or(1);

    for template in templates {
        // Table format: just count + template (clean)
        output.push_str(&format!(
            "  {:>width$}: {}\n",
            template.count,
            template.template,
            width = max_count_width
        ));

        // Full format: add metadata on indented lines below
        if matches!(format, crate::cli::DrainFormat::Full) {
            output.push_str(&format!("     id: {}\n", template.template_id));
            if let Some(line_summary) = format_line_summary(template.first_line, template.last_line)
            {
                output.push_str(&format!("     {}\n", line_summary));
            }

            // Add sample
            if !template.sample.is_empty() {
                let sample = escape_sample(&template.sample);
                output.push_str(&format!("     sample: \"{}\"\n", sample));
            }

            output.push('\n');
        }
    }

    output.trim_end().to_string()
}

/// Format templates as JSON array
pub fn format_templates_json(templates: &[DrainTemplate]) -> String {
    let json_templates: Vec<serde_json::Value> = templates
        .iter()
        .map(|t| {
            let mut obj = serde_json::json!({
                "template": t.template,
                "template_id": t.template_id,
                "count": t.count,
                "sample": t.sample,
            });
            if let Some(first) = t.first_line {
                obj["first_line"] = serde_json::json!(first);
            }
            if let Some(last) = t.last_line {
                obj["last_line"] = serde_json::json!(last);
            }
            obj
        })
        .collect();
    serde_json::to_string_pretty(&json_templates).unwrap_or_else(|_| "[]".to_string())
}

/// Escape control characters in sample strings for display
fn escape_sample(s: &str) -> String {
    s.replace('\n', "\\n").replace('\r', "\\r")
}

fn format_line_summary(first: Option<usize>, last: Option<usize>) -> Option<String> {
    match (first, last) {
        (Some(start), Some(end)) if start == end => Some(format!("line: {}", start)),
        (Some(start), Some(end)) => Some(format!("lines: {}-{}", start, end)),
        (Some(start), None) => Some(format!("line: {}", start)),
        (None, Some(end)) => Some(format!("last line: {}", end)),
        (None, None) => None,
    }
}

fn format_templates_id_output(templates: &[DrainTemplate]) -> String {
    let mut sorted: Vec<&DrainTemplate> = templates.iter().collect();
    sorted.sort_by(|a, b| a.template_id.cmp(&b.template_id));

    let mut output = String::new();
    for template in sorted {
        output.push_str(&format!(
            "{}: {}\n",
            template.template_id, template.template
        ));
    }
    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_similar_lines() {
        let mut drain = DrainState::new(DrainConfig::default());
        let a = drain
            .ingest("failed to connect to 10.0.0.1", Some(1))
            .expect("first ingest");
        let b = drain
            .ingest("failed to connect to 10.0.0.2", Some(5))
            .expect("second ingest");

        assert_eq!(a.template, "failed to connect to <ipv4>");
        assert_eq!(b.template, "failed to connect to <ipv4>");
        assert_eq!(a.template_id, b.template_id);
        assert_eq!(b.count, 2);
    }

    #[test]
    fn normalize_template_collapses_whitespace() {
        assert_eq!(normalize_template("a b"), "a b");
        assert_eq!(normalize_template("a  b"), "a b");
        assert_eq!(normalize_template("  a   b  "), "a b");
        assert_eq!(normalize_template(""), "");
        assert_eq!(normalize_template("   "), "");
        assert_eq!(normalize_template("solo"), "solo");
        // Must agree exactly with the normalization generate_template_id hashes:
        // equal normalized forms => equal ids.
        assert_eq!(
            generate_template_id("a  b"),
            generate_template_id(normalize_template("a  b").as_str())
        );
    }

    #[test]
    fn whitespace_variant_templates_share_metadata() {
        // Two messages whose Drain templates differ only by internal whitespace
        // hash to the same template_id, so they share one metadata entry. Keying
        // metadata on the normalized template (instead of the id) must preserve
        // this collision behavior: first-seen sample and first_line win.
        let mut drain = DrainState::new(DrainConfig::default());
        let a = drain.ingest("alpha  bravo", Some(1)).expect("first ingest");
        let b = drain.ingest("alpha bravo", Some(2)).expect("second ingest");

        assert_eq!(
            a.template_id, b.template_id,
            "ids collide via normalization"
        );
        // Shared entry: the second sighting reports the first's sample/first_line.
        assert_eq!(b.sample, "alpha  bravo");
        assert_eq!(b.first_line, Some(1));
        assert_eq!(b.last_line, Some(2));
    }

    #[test]
    fn record_updates_model_without_building_result() {
        // The CLI --drain path uses `record`; it must feed the same tree/metadata
        // that `templates()` reads, so counts and samples match the `ingest` path.
        let mut drain = DrainState::new(DrainConfig::default());
        drain
            .record("connect to 10.0.0.1", Some(1))
            .expect("first record");
        drain
            .record("connect to 10.0.0.2", Some(4))
            .expect("second record");

        let templates = drain.templates();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].template, "connect to <ipv4>");
        assert_eq!(templates[0].count, 2);
        assert_eq!(templates[0].sample, "connect to 10.0.0.1");
        assert_eq!(templates[0].first_line, Some(1));
        assert_eq!(templates[0].last_line, Some(4));
    }

    #[test]
    fn tracks_sample_and_line_numbers() {
        let mut drain = DrainState::new(DrainConfig::default());

        // First occurrence at line 10
        let a = drain
            .ingest("error connecting to 192.168.1.1", Some(10))
            .expect("first ingest");
        assert!(a.is_new);
        assert_eq!(a.sample, "error connecting to 192.168.1.1");
        assert_eq!(a.first_line, Some(10));
        assert_eq!(a.last_line, Some(10));

        // Second occurrence at line 25
        let b = drain
            .ingest("error connecting to 192.168.1.2", Some(25))
            .expect("second ingest");
        assert!(!b.is_new);
        assert_eq!(b.sample, "error connecting to 192.168.1.1"); // Still first sample
        assert_eq!(b.first_line, Some(10)); // First line unchanged
        assert_eq!(b.last_line, Some(25)); // Last line updated

        // Check templates() includes metadata
        let templates = drain.templates();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].sample, "error connecting to 192.168.1.1");
        assert_eq!(templates[0].first_line, Some(10));
        assert_eq!(templates[0].last_line, Some(25));
    }

    #[test]
    fn handles_missing_line_numbers() {
        let mut drain = DrainState::new(DrainConfig::default());

        let a = drain
            .ingest("test message 123", None)
            .expect("first ingest");
        assert_eq!(a.sample, "test message 123");
        assert_eq!(a.first_line, None);
        assert_eq!(a.last_line, None);

        let b = drain
            .ingest("test message 456", Some(50))
            .expect("second ingest");
        assert_eq!(b.first_line, None); // First line stays None
        assert_eq!(b.last_line, Some(50)); // Last line gets updated
    }

    #[test]
    fn template_id_is_stable() {
        let template = "failed to connect to <ipv4>";
        let id1 = generate_template_id(template);
        let id2 = generate_template_id(template);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 19); // "v1:" (3) + 16 hex chars = 19
        assert!(id1.starts_with("v1:"));
    }

    #[test]
    fn template_id_normalizes_whitespace() {
        let id1 = generate_template_id("failed  to  connect");
        let id2 = generate_template_id("failed to connect");
        assert_eq!(id1, id2, "Whitespace should be normalized");
    }

    #[test]
    fn different_templates_have_different_ids() {
        let id1 = generate_template_id("failed to connect to <ipv4>");
        let id2 = generate_template_id("connection successful to <ipv4>");
        assert_ne!(id1, id2);
        assert!(id1.starts_with("v1:"));
        assert!(id2.starts_with("v1:"));
    }

    #[test]
    fn formats_templates_output_table() {
        let template1_id = generate_template_id("a <*> b");
        let template2_id = generate_template_id("x y z");
        let templates = vec![
            DrainTemplate {
                template: "a <*> b".to_string(),
                template_id: template1_id.clone(),
                count: 3,
                sample: "a 123 b".to_string(),
                first_line: Some(1),
                last_line: Some(100),
            },
            DrainTemplate {
                template: "x y z".to_string(),
                template_id: template2_id.clone(),
                count: 1,
                sample: "x y z".to_string(),
                first_line: Some(50),
                last_line: Some(50),
            },
        ];
        // Table format: clean output, no IDs, no line numbers, no samples
        let output = format_templates_output(&templates, crate::cli::DrainFormat::Table);
        assert!(output.starts_with("templates (2 items):"));
        assert!(output.contains("a <*> b"));
        assert!(output.contains("x y z"));
        assert!(!output.contains(&template1_id)); // No IDs in table format
        assert!(!output.contains(&template2_id));
        assert!(!output.contains("lines:")); // No line numbers
        assert!(!output.contains("sample:")); // No samples
    }

    #[test]
    fn formats_templates_output_full() {
        let template1_id = generate_template_id("a <*> b");
        let templates = vec![DrainTemplate {
            template: "a <*> b".to_string(),
            template_id: template1_id.clone(),
            count: 3,
            sample: "a 123 b".to_string(),
            first_line: Some(1),
            last_line: Some(100),
        }];
        // Full format: adds line ranges and samples
        let output = format_templates_output(&templates, crate::cli::DrainFormat::Full);
        assert!(output.contains("a <*> b"));
        assert!(output.contains(&format!("id: {}", template1_id)));
        assert!(output.contains("lines: 1-100"));
        assert!(output.contains("sample: \"a 123 b\""));
    }

    #[test]
    fn formats_templates_output_id() {
        let template1_id = generate_template_id("a <*> b");
        let template2_id = generate_template_id("x y z");
        let templates = vec![
            DrainTemplate {
                template: "a <*> b".to_string(),
                template_id: template1_id.clone(),
                count: 3,
                sample: "a 123 b".to_string(),
                first_line: Some(1),
                last_line: Some(100),
            },
            DrainTemplate {
                template: "x y z".to_string(),
                template_id: template2_id.clone(),
                count: 1,
                sample: "x y z".to_string(),
                first_line: Some(50),
                last_line: Some(50),
            },
        ];
        let output = format_templates_output(&templates, crate::cli::DrainFormat::Id);
        assert!(output.contains(&format!("{}: a <*> b", template1_id)));
        assert!(output.contains(&format!("{}: x y z", template2_id)));
        let mut ids = [template1_id.clone(), template2_id.clone()];
        ids.sort();
        let first_line = output.lines().next().expect("first line");
        assert!(first_line.starts_with(&format!("{}:", ids[0])));
    }

    #[test]
    fn formats_templates_json() {
        let templates = vec![DrainTemplate {
            template: "error <ipv4>".to_string(),
            template_id: generate_template_id("error <ipv4>"),
            count: 5,
            sample: "error 192.168.1.1".to_string(),
            first_line: Some(10),
            last_line: Some(50),
        }];
        let json = format_templates_json(&templates);
        assert!(json.contains("\"template\": \"error <ipv4>\""));
        assert!(json.contains("\"count\": 5"));
        assert!(json.contains("\"sample\": \"error 192.168.1.1\""));
        assert!(json.contains("\"first_line\": 10"));
        assert!(json.contains("\"last_line\": 50"));
    }

    #[test]
    fn escapes_newlines_in_samples() {
        let sample_with_newlines = "line1\nline2\r\nline3";
        let escaped = escape_sample(sample_with_newlines);
        assert!(!escaped.contains('\n'));
        assert!(!escaped.contains('\r'));
        assert!(escaped.contains("\\n"));
        assert!(escaped.contains("\\r"));
    }
}
