use drain_rs::DrainTree;
use grok::Grok;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::LazyLock;

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

/// Metadata tracked per cluster (sample, first/last line numbers)
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
    /// Masks every line before the tree sees it (see [`Masker`]).
    masker: Masker,
    /// Metadata tracked per cluster, keyed by the cluster's stable id.
    ///
    /// Not by its template: a cluster's template is rewritten as the cluster
    /// generalizes (a position its members disagree on becomes `<*>`), so a
    /// template-keyed entry would be orphaned on the first rewrite — the sample
    /// and `first_line` would jump to whichever line triggered it, and the dead
    /// entry would leak. The id is also cheaper: no per-line normalization or
    /// owned key `String`.
    metadata: HashMap<u64, TemplateMetadata>,
}

impl DrainState {
    fn new(config: DrainConfig) -> Self {
        let config = config.sanitized();
        // Masking lives in kelora's Masker, which sees whole lines and so can
        // do what drain's per-token pass cannot; the tree therefore gets no
        // filter patterns of its own. Handing drain the patterns as well would
        // mask the placeholders a second time — every name containing a digit
        // (`<ipv4>`, `<sha256>`) re-matches the num pattern.
        let mut grok = build_grok();
        let tree = DrainTree::new()
            .max_depth(to_u16(config.depth))
            .max_children(to_u16(config.max_children))
            .min_similarity(config.similarity as f32)
            .filter_patterns(Vec::new())
            .build_patterns(&mut grok);
        let masker = Masker::new(&config.filters);
        Self {
            config,
            tree,
            masker,
            metadata: HashMap::new(),
        }
    }

    /// Per-line hot path: add `text` to the tree and update the cluster's
    /// metadata, returning the raw template string, its match count, whether
    /// this was the cluster's first sighting, and the cluster's id.
    ///
    /// The template id (a SHA256 hash) is deliberately *not* computed here — it
    /// is display-only and derived once per template in [`templates`].
    fn record(
        &mut self,
        text: &str,
        line_num: Option<usize>,
    ) -> Result<(String, usize, bool, u64), String> {
        // The tree sees the masked line; `text` stays the stored sample.
        let masked = self.masker.mask_line(text);
        let cluster = self
            .tree
            .add_log_line(&masked)
            .ok_or_else(|| "Drain failed to match or create a cluster".to_string())?;
        let count = usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX);
        let template = cluster.as_string();
        let cluster_id = cluster.id();
        let is_new = count == 1;

        match self.metadata.get_mut(&cluster_id) {
            // Hit path (the common case): no allocation, just refresh last_line.
            Some(meta) => {
                if let Some(ln) = line_num {
                    meta.last_line = Some(ln);
                }
            }
            None => {
                self.metadata.insert(
                    cluster_id,
                    TemplateMetadata {
                        sample: text.to_string(),
                        first_line: line_num,
                        last_line: line_num,
                    },
                );
            }
        }

        Ok((template, count, is_new, cluster_id))
    }

    fn ingest(&mut self, text: &str, line_num: Option<usize>) -> Result<DrainResult, String> {
        let (template, count, is_new, cluster_id) = self.record(text, line_num)?;
        let template_id = generate_template_id(&template);

        // `record` guarantees the entry exists; the fallback keeps this total.
        let (sample, first_line, last_line) = match self.metadata.get(&cluster_id) {
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
                let meta = self.metadata.get(&cluster.id());
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
        // Specific before generic. Since a tie at the same position goes to the
        // longer match (see `Masker::next_match`), this order only decides
        // between patterns that cover exactly the same span.
        "%{KELORA_ISO8601:timestamp}",
        "%{KELORA_DATE:date}",
        "%{KELORA_TIME:time}",
        "%{KELORA_NUM:num}",
    ]
}

/// Calendar timestamps that span several space-separated tokens, which drain's
/// per-token masking can never see: `ctime(3)`/`asctime` (`Mon Jun 13 03:55:15
/// 2005`), the same form without the year, and the bare syslog `%b %e %H:%M:%S`
/// prefix. Left alone, the numeric parts mask but the weekday/month names stay
/// literal, so one logical message splits across a template per weekday/month —
/// and each surviving template is *labelled* with one weekday/month while
/// covering events that had others.
///
/// Day-of-month is `\d{1,2}` with `\x20+` separators so `Jul  1` (asctime pads
/// to width 2) collapses like `Jun 13`; that padding otherwise yields an empty
/// token and a different token count, which alone is enough to split a cluster.
static CALENDAR_TS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \b
        (?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\x20+)?           # optional ctime weekday
        (?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\x20+
        \d{1,2}\x20+                                        # day of month
        \d{1,2}:\d{2}:\d{2}(?:\.\d+)?                       # time of day
        (?:\x20+\d{4})?                                     # optional year
        \b",
    )
    .expect("failed to compile calendar timestamp regex")
});

/// How drain spells a generalized token, in a tree and in a stored template.
const WILDCARD: &str = "<*>";

/// One token after masking: either a placeholder/literal, or drain's wildcard
/// (`<*>`, which a stored template also spells that way).
#[derive(Debug, Clone, PartialEq)]
enum MaskedToken {
    WildCard,
    Val(String),
}

impl MaskedToken {
    fn as_str(&self) -> &str {
        match self {
            MaskedToken::WildCard => WILDCARD,
            MaskedToken::Val(v) => v.as_str(),
        }
    }
}

/// kelora's masking pass, run before a line reaches the drain tree.
///
/// It is the only masking pass: the tree is built with **no** filter patterns
/// (see [`DrainState::new`]), so drain tokenizes what comes out of here
/// verbatim. That is deliberate — `drain_rs::DrainTree::process` masks one
/// space-separated token at a time and replaces the *whole* token as soon as a
/// pattern matches anywhere inside it, which throws away exactly the literal
/// text that names a message. This pass differs in three ways:
///
/// 1. A pattern cannot span a space, so multi-token calendar timestamps never
///    mask (see [`CALENDAR_TS_RE`]).
/// 2. A match replaces only the span it covers, not the whole token, so the
///    literal part survives: `worker-3` masks to `worker-<num>` rather than
///    `<num>`, `HTTP/1.1` to `HTTP/<version>`, `uid=0` to `uid=<num>` (which
///    also makes it consistent with `tty=ssh`, whose key survived only because
///    nothing matched it), and `took=1.5s,retries=3` to
///    `took=<duration>,retries=<num>` — whole-token masking rendered that last
///    one as `took=<version>`, silently deleting `,retries=3`.
/// 3. A span only masks when a word doesn't run into it (see
///    [`is_word_delimited`]), so a digit that belongs to a word stays put:
///    `ssh2`, `utf8`, `sha256`, `eth0` and `log4j` are words, not numbers, and
///    whole-token masking turned every one of them into `<num>`.
///
/// Leaving drain a second pass would also undo the result: placeholder names
/// containing a digit re-match the num pattern, so `rhost=<ipv4>` would collapse
/// to `<num>` on the "4".
#[derive(Debug)]
struct Masker {
    /// Compiled filter patterns, in the same order and built the same way as
    /// `DrainTree::build_patterns` (named captures only, uncompilable patterns
    /// skipped).
    patterns: Vec<grok::Pattern>,
    /// Set when the default filter set is in force. An explicit `filters:` list
    /// means "mask exactly these", so a caller who overrides it gets their
    /// patterns and nothing else — no calendar-timestamp collapse, and no
    /// [`cannot_match_defaults`] shortcut (which knows what the defaults need).
    defaults: bool,
}

impl Masker {
    fn new(filters: &[String]) -> Self {
        let mut grok = build_grok();
        let filter_strs: Vec<&str> = if filters.is_empty() {
            default_filter_patterns()
        } else {
            filters.iter().map(|s| s.as_str()).collect()
        };
        let patterns: Vec<grok::Pattern> = filter_strs
            .iter()
            .filter_map(|p| grok.compile(p, true).ok())
            .collect();
        Self {
            patterns,
            defaults: filters.is_empty(),
        }
    }

    /// Mask `line` into the tokens drain should cluster on.
    fn mask_tokens(&self, line: &str) -> Vec<MaskedToken> {
        let line = if self.defaults {
            CALENDAR_TS_RE.replace_all(line, "<timestamp>")
        } else {
            Cow::Borrowed(line)
        };
        // One scratch buffer per line, reused by every token (see `mask_token`).
        let mut spans = Vec::with_capacity(self.patterns.len());
        // split(' ') (not split_whitespace) keeps drain's token count, including
        // the empty tokens a run of spaces produces.
        line.split(' ')
            .map(|t| self.mask_token(t.trim(), &mut spans))
            .collect()
    }

    /// The masked line drain ingests: [`Self::mask_tokens`] joined back with
    /// single spaces, which is how drain itself renders a template.
    fn mask_line(&self, line: &str) -> String {
        let tokens = self.mask_tokens(line);
        let mut out = String::with_capacity(line.len());
        for (i, token) in tokens.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(token.as_str());
        }
        out
    }

    /// Replace every filter match inside `token` with its placeholder, keeping
    /// the text around them (see this type's docs for why that matters).
    ///
    /// `spans` is caller-owned scratch space holding one candidate span per
    /// filter pattern; its contents are meaningless between calls.
    fn mask_token(&self, token: &str, spans: &mut Vec<Option<Span>>) -> MaskedToken {
        if self.defaults && cannot_match_defaults(token) {
            return MaskedToken::Val(token.to_string());
        }
        spans.clear();
        spans.extend(self.patterns.iter().map(|p| next_span(p, token, 0)));

        // Left untouched, the token is returned as-is: no allocation for the
        // plain words that make up most of a line.
        let mut out: Option<String> = None;
        let mut pos = 0;
        while let Some((start, end, name)) = self.next_match(token, pos, spans) {
            let out = out.get_or_insert_with(|| String::with_capacity(token.len()));
            out.push_str(&token[pos..start]);
            match name {
                Some(name) => {
                    out.push('<');
                    out.push_str(name);
                    out.push('>');
                }
                None => out.push_str(WILDCARD),
            }
            pos = end;
        }
        match out {
            None => MaskedToken::Val(token.to_string()),
            Some(mut out) => {
                out.push_str(&token[pos..]);
                if out == WILDCARD {
                    MaskedToken::WildCard
                } else {
                    MaskedToken::Val(out)
                }
            }
        }
    }

    /// The span to mask next, at or after `from`: leftmost wins, longest breaks
    /// a tie, filter order breaks what's left. The name is the pattern's alias,
    /// or `None` for an alias-free pattern (drain's wildcard).
    ///
    /// Leftmost-longest rather than drain's plain filter order because a span is
    /// now only as wide as its match: `1.5s` under filter order matches
    /// `version` (`1.5`) before `duration`, and where whole-token masking hid
    /// that as `<version>`, substituting in place would leave `<version>s`.
    /// Preferring the longer match at the same position picks `<duration>`, and
    /// makes the specific-before-generic ordering of the filter list a
    /// tie-breaker instead of a load-bearing detail.
    fn next_match(
        &self,
        token: &str,
        from: usize,
        spans: &mut [Option<Span>],
    ) -> Option<(usize, usize, Option<&str>)> {
        let mut best: Option<(usize, usize, Option<&str>)> = None;
        for (pattern, span) in self.patterns.iter().zip(spans.iter_mut()) {
            // Only a span the scan has already run past needs re-searching.
            // `None` never goes stale: `next_span` searches forward, so a
            // pattern with no match from here has none from later either.
            if span.is_some_and(|(start, _)| start < from) {
                *span = next_span(pattern, token, from);
            }
            let Some((start, end)) = *span else {
                continue;
            };
            let better = match best {
                None => true,
                Some((best_start, best_end, _)) => {
                    start < best_start || (start == best_start && end > best_end)
                }
            };
            if better {
                best = Some((start, end, pattern.alias()));
            }
        }
        best
    }
}

/// Byte offsets `(start, end)` of a match inside one token.
type Span = (usize, usize);

/// Whether `token` provably matches none of the default filter patterns, so the
/// whole pattern loop can be skipped — which is most words in a log line.
///
/// Every default pattern needs an ASCII digit (`num`, `version`, the timestamps,
/// …), a non-alphanumeric character (`path` a `/`, `email` an `@`, `ipv6` a `:`,
/// `fqdn` a `.`, …), or 32+ characters (`md5`, `sha1`, `sha256`, whose hex runs
/// can be all letters). A short run of plain ASCII letters has none of the
/// three. Only sound for the default set, hence [`Masker::defaults`].
fn cannot_match_defaults(token: &str) -> bool {
    token.len() < 32 && token.bytes().all(|b| b.is_ascii_alphabetic())
}

/// The leftmost match of `pattern` at or after `from` that is safe to mask,
/// skipping the ones a word runs into (see [`is_word_delimited`]).
fn next_span(pattern: &grok::Pattern, token: &str, from: usize) -> Option<Span> {
    let mut at = from;
    while at <= token.len() {
        let (start, end) = pattern.find_at(token, at)?;
        // An empty match would never advance `pos` in mask_token; skip it.
        if end > start && is_word_delimited(token, start, end) {
            return Some((start, end));
        }
        at = resume_after(token, start);
    }
    None
}

/// Where to resume a pattern's search after one of its matches was rejected:
/// past the whole alphanumeric run the match started inside, so a long
/// alphanumeric blob costs one search rather than one per character. Always
/// past `start`, so the search terminates.
fn resume_after(token: &str, start: usize) -> usize {
    let rest = &token[start..];
    let run = rest
        .char_indices()
        .find(|(_, c)| !c.is_alphanumeric())
        .map_or(rest.len(), |(i, _)| i);
    start + run.max(rest.chars().next().map_or(1, char::len_utf8))
}

/// Whether `text[start..end]` can be masked without cutting a word in half:
/// neither neighbouring character is alphanumeric.
///
/// This is what keeps `ssh2`, `utf8`, `sha256`, `eth0` and `log4j` intact —
/// their digits are part of the word, and whole-token masking turned all five
/// into `<num>` — while `worker-3`, `uid=0` and `session_12345` still mask,
/// because there a delimiter separates the word from the number.
///
/// `_` counts as a delimiter, matching Drain3's default masking, so `x86_64`
/// does become `x86_<num>`. That is the right way round: a constant token masks
/// harmlessly (it is constant either way), whereas a varying one is far better
/// off as `session_<num>` than as the bare `<*>` drain would generalize it to.
fn is_word_delimited(text: &str, start: usize, end: usize) -> bool {
    !text[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
        && !text[end..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
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

/// One template of a [`FrozenTemplateSet`], parsed back into the same token
/// representation a masked line has ([`MaskedToken`]).
struct FrozenEntry {
    tokens: Vec<MaskedToken>,
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
    masker: Masker,
}

impl FrozenTemplateSet {
    fn new(templates: Vec<String>, filters: &[String]) -> Self {
        let mut by_len: HashMap<usize, Vec<FrozenEntry>> = HashMap::new();
        for template in templates {
            // as_string() joins tokens with single spaces, so split(' ')
            // round-trips them (including empty tokens from blank runs).
            let tokens: Vec<MaskedToken> = template
                .split(' ')
                .map(|t| {
                    if t == "<*>" {
                        MaskedToken::WildCard
                    } else {
                        MaskedToken::Val(t.to_string())
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

        Self {
            by_len,
            masker: Masker::new(filters),
        }
    }

    /// Match `text` against the frozen set, returning the best template with
    /// the same token count (drain's similarity ordering: fraction of exact
    /// token matches first, wildcard coverage as the tie-breaker). Returns
    /// None only when no template has that token count.
    pub fn match_text(&self, text: &str) -> Option<&str> {
        // Masked the same way the ingest path masks, so a line's tokens are
        // comparable with the templates mined from it.
        let tokens = self.masker.mask_tokens(text);
        let candidates = self.by_len.get(&tokens.len())?;
        let len = tokens.len() as f32;
        let mut best: Option<(f32, u32, &FrozenEntry)> = None;
        for entry in candidates {
            let mut exact = 0f32;
            let mut approximate = 0u32;
            for (pattern, token) in entry.tokens.iter().zip(tokens.iter()) {
                if pattern == token {
                    exact += 1.0;
                } else if matches!(pattern, MaskedToken::WildCard) {
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
    fn masks_ctime_dates_as_one_timestamp_token() {
        // ctime(3) dates span five tokens, so drain's per-token masking leaves
        // the weekday and month literal: one message splits into a template per
        // weekday/month, each labeled with a weekday it does not own. All three
        // lines below are the same message.
        let mut drain = DrainState::new(DrainConfig::default());
        for (line, at) in [
            (1, "Mon Jun 13 03:55:15 2005"),
            (2, "Fri Jul  1 04:11:02 2005"), // asctime pads the day to width 2
            (3, "Sun Jul 10 05:00:00 2005"),
        ] {
            drain
                .record(
                    &format!("connection from 10.0.0.{} at {}", line, at),
                    Some(line),
                )
                .expect("record");
        }

        let templates = drain.templates();
        assert_eq!(templates.len(), 1, "got {:?}", templates);
        assert_eq!(
            templates[0].template,
            "connection from <ipv4> at <timestamp>"
        );
        assert_eq!(templates[0].count, 3);
    }

    #[test]
    fn masks_syslog_and_asctime_calendar_forms() {
        // Same shape without the weekday (asctime minus %a) and without the
        // year (the syslog prefix, which reaches drain whenever the whole line
        // is the drained field).
        let masker = Masker::new(&[]);
        assert_eq!(
            masker.mask_line("at Jun 13 03:55:15 2005"),
            "at <timestamp>"
        );
        assert_eq!(
            masker.mask_line("Jun 14 15:16:01 combo"),
            "<timestamp> combo"
        );
        // A date with no time of day is not a calendar timestamp: only the day
        // number masks, as before.
        assert_eq!(masker.mask_line("expires Jun 13"), "expires Jun <num>");
    }

    #[test]
    fn masks_only_the_value_of_a_key_value_token() {
        let masker = Masker::new(&[]);
        // The key is the discriminating part; masking the whole token loses it.
        assert_eq!(
            masker.mask_line("logname= uid=0 euid=500 tty=ssh"),
            "logname= uid=<num> euid=<num> tty=ssh"
        );
        // Placeholder names containing digits must survive: a second masking
        // pass would collapse `rhost=<ipv4>` to `<num>` on the "4".
        assert_eq!(masker.mask_line("rhost=10.0.0.1"), "rhost=<ipv4>");
        assert_eq!(masker.mask_line("dir=/var/log/messages"), "dir=<path>");
        // A token that is nothing but the value masks to the placeholder alone.
        assert_eq!(masker.mask_line("connect 10.0.0.1"), "connect <ipv4>");
        assert_eq!(masker.mask_line("count -= 1"), "count -= <num>");
        // Substituting in place keeps the delimiter, whatever it is.
        assert_eq!(masker.mask_line("=42"), "=<num>");
        // Several values in one token: each masks on its own. Whole-token
        // masking rendered this as `took=<version>`, deleting `,retries=3`.
        assert_eq!(
            masker.mask_line("took=1.5s,retries=3"),
            "took=<duration>,retries=<num>"
        );
    }

    #[test]
    fn key_value_masking_keeps_unmatched_values_verbatim() {
        // `tty=ssh` already kept its key (nothing matched the value); the fix
        // makes `uid=0` behave the same way rather than the other way round.
        let masker = Masker::new(&[]);
        assert_eq!(masker.mask_line("tty=ssh"), "tty=ssh");
        assert_eq!(masker.mask_line("ruser="), "ruser=");
    }

    #[test]
    fn keeps_digits_that_belong_to_a_word() {
        // Every one of these masked to a bare `<num>` when a match anywhere in a
        // token replaced the whole token: the word naming the message was lost.
        let masker = Masker::new(&[]);
        assert_eq!(
            masker.mask_line("Accepted publickey for root port 22 ssh2"),
            "Accepted publickey for root port <num> ssh2"
        );
        assert_eq!(
            masker.mask_line("decoded utf8 via base64 using sha256 and log4j on eth0"),
            "decoded utf8 via base64 using sha256 and log4j on eth0"
        );
        // A delimiter between word and number still masks — that number varies.
        assert_eq!(masker.mask_line("worker-3"), "worker-<num>");
        assert_eq!(masker.mask_line("session_12345"), "session_<num>");
        // `_` is a delimiter (as in Drain3), so a constant like this masks too.
        // Harmless: constant either way, and it keeps `session_<num>` working.
        assert_eq!(masker.mask_line("host x86_64"), "host x86_<num>");
    }

    #[test]
    fn keeps_the_literal_part_of_a_partially_matched_token() {
        let masker = Masker::new(&[]);
        // The literal prefix/suffix is what names the message; whole-token
        // masking dropped it (`<version>`, `<path>`, `<timestamp>`).
        assert_eq!(
            masker.mask_line("GET /api/v1/users?id=5 HTTP/1.1 200"),
            "GET <path>?id=<num> HTTP/<version> <num>"
        );
        assert_eq!(
            masker.mask_line("[2026-07-29T10:00:00Z] done"),
            "[<timestamp>] done"
        );
        // Repeats of one pattern in a token all mask, not just the first.
        assert_eq!(
            masker.mask_line("10.0.0.1:53->10.0.0.2:80"),
            "<ipv4_port>-><ipv4_port>"
        );
    }

    #[test]
    fn prefers_the_longest_placeholder_at_a_position() {
        // `version` precedes `duration` in the filter list and matches `1.5` of
        // `1.5s`. Whole-token masking hid that behind `<version>`; masking in
        // place would expose it as `<version>s` if the longer match didn't win.
        let masker = Masker::new(&[]);
        assert_eq!(masker.mask_line("in 1.5s"), "in <duration>");
        // Same rule, one place further down: date-only vs. the full ISO stamp.
        assert_eq!(
            masker.mask_line("at 2026-07-29T10:00:00Z"),
            "at <timestamp>"
        );
        assert_eq!(masker.mask_line("on 2026-07-29"), "on <date>");
    }

    #[test]
    fn explicit_filters_opt_out_of_the_calendar_collapse() {
        // An explicit filter list means "mask exactly these", so the added
        // multi-token timestamp handling stays off and only ipv4 masks.
        let filters = vec!["%{IPV4:ipv4}".to_string()];
        let masker = Masker::new(&filters);
        assert_eq!(
            masker.mask_line("from 10.0.0.1 at Mon Jun 13 03:55:15 2005"),
            "from <ipv4> at Mon Jun 13 03:55:15 2005"
        );
        // Value-only masking is structural, so it applies to custom filters too.
        assert_eq!(masker.mask_line("rhost=10.0.0.1"), "rhost=<ipv4>");
    }

    #[test]
    fn generalizes_a_position_the_cluster_members_disagree_on() {
        // Drain's defining step: where a cluster's members disagree, the
        // template says `<*>`. `drain_rs::LogCluster::add_log` never ran it — it
        // compared each incoming token against itself instead of against the
        // stored one — so a template stayed whatever its first line produced
        // while counting lines that said something else. On the loghub
        // `Linux_2k.log` sample that made one template read "session closed for
        // user cyrus" over 123 events, 80 of which named news, test or root.
        // Pinned here because kelora's suite is what exercises the patched copy
        // in `vendor/drain-rs`.
        let mut drain = DrainState::new(DrainConfig::default());
        for user in ["cyrus", "news", "test"] {
            drain
                .record(
                    &format!(
                        "combo sshd(pam_unix)[19937]: session closed for user {}",
                        user
                    ),
                    None,
                )
                .expect("record");
        }

        let templates = drain.templates();
        assert_eq!(templates.len(), 1, "got {:?}", templates);
        assert_eq!(
            templates[0].template,
            "combo <function>[<num>]: session closed for user <*>"
        );
        assert_eq!(templates[0].count, 3);
        // The stored sample still shows a real line, so the concrete value is
        // one `--drain=full` away.
        assert_eq!(
            templates[0].sample,
            "combo sshd(pam_unix)[19937]: session closed for user cyrus"
        );
    }

    #[test]
    fn frozen_set_matches_the_template_a_line_was_mined_into() {
        // --drain-diff pass 2 masks lines with the frozen set, so its masking
        // must stay identical to the ingest path's.
        let mut drain = DrainState::new(DrainConfig::default());
        for line in 1..=3 {
            drain
                .record(
                    &format!(
                        "connection from 10.0.0.{} at Mon Jun 13 03:55:1{} 2005 uid=0",
                        line, line
                    ),
                    Some(line),
                )
                .expect("record");
        }
        let templates = drain.templates();
        assert_eq!(templates.len(), 1, "got {:?}", templates);

        let frozen = FrozenTemplateSet::new(
            templates.iter().map(|t| t.template.clone()).collect(),
            &drain.config.filters,
        );
        assert_eq!(
            frozen.match_text("connection from 10.0.0.9 at Sun Jul 10 05:00:00 2005 uid=500"),
            Some(templates[0].template.as_str())
        );
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
    fn whitespace_variant_templates_keep_their_own_metadata() {
        // Two messages whose Drain templates differ only by internal whitespace
        // are separate clusters (the token counts differ) that hash to the same
        // template_id. Metadata is per cluster, so each reports its own sample
        // and line — the id collision is confined to the displayed id.
        let mut drain = DrainState::new(DrainConfig::default());
        let a = drain.ingest("alpha  bravo", Some(1)).expect("first ingest");
        let b = drain.ingest("alpha bravo", Some(2)).expect("second ingest");

        assert_eq!(
            a.template_id, b.template_id,
            "ids collide via normalization"
        );
        assert_eq!(a.sample, "alpha  bravo");
        assert_eq!(a.first_line, Some(1));
        assert_eq!(b.sample, "alpha bravo");
        assert_eq!(b.first_line, Some(2));
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
