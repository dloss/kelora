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
    /// Metadata tracked per template_id
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

    fn ingest(&mut self, text: &str, line_num: Option<usize>) -> Result<DrainResult, String> {
        let cluster = self
            .tree
            .add_log_line(text)
            .ok_or_else(|| "Drain failed to match or create a cluster".to_string())?;
        let count = usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX);
        let template = cluster.as_string();
        let template_id = generate_template_id(&template);
        let is_new = count == 1;

        // Update or create metadata for this template
        let meta = self
            .metadata
            .entry(template_id.clone())
            .or_insert_with(|| TemplateMetadata {
                sample: text.to_string(),
                first_line: line_num,
                last_line: line_num,
            });

        // Update last_line if we have a line number
        if let Some(ln) = line_num {
            meta.last_line = Some(ln);
        }

        Ok(DrainResult {
            template,
            template_id,
            count,
            is_new,
            sample: meta.sample.clone(),
            first_line: meta.first_line,
            last_line: meta.last_line,
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
                let meta = self.metadata.get(&template_id);
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
    // Normalize whitespace for consistent hashing across formatting variations
    let normalized = template.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();

    // v1: prefix for version identification
    format!("v1:{}", hex::encode(&result[..8]))
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

pub fn drain_template(
    text: &str,
    config: Option<DrainConfig>,
    line_num: Option<usize>,
) -> Result<DrainResult, String> {
    DRAIN_STATE.with(|state| {
        let mut state = state.borrow_mut();
        match (state.as_ref(), &config) {
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

        let drain = state
            .as_mut()
            .ok_or_else(|| "Drain state not initialized".to_string())?;
        drain.ingest(text, line_num)
    })
}

pub fn drain_templates() -> Vec<DrainTemplate> {
    DRAIN_STATE.with(|state| match state.borrow().as_ref() {
        Some(drain) => drain.templates(),
        None => Vec::new(),
    })
}

/// Format templates for table output
/// When verbose > 0, includes sample lines for each template
pub fn format_templates_output(templates: &[DrainTemplate], verbose: u8) -> String {
    if templates.is_empty() {
        return "No templates found".to_string();
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
        // Format line range if available
        let line_info = match (template.first_line, template.last_line) {
            (Some(first), Some(last)) if first == last => format!(" L{}", first),
            (Some(first), Some(last)) => format!(" L{}-{}", first, last),
            (Some(first), None) => format!(" L{}", first),
            (None, Some(last)) => format!(" L{}", last),
            (None, None) => String::new(),
        };

        output.push_str(&format!(
            "  {:>width$}: [{}]{} {}\n",
            template.count,
            template.template_id,
            line_info,
            template.template,
            width = max_count_width
        ));

        // Add sample on next line when verbose
        if verbose > 0 && !template.sample.is_empty() {
            let sample = truncate_sample(&template.sample, 80);
            output.push_str(&format!(
                "  {:>width$}  e.g. \"{}\"\n",
                "",
                sample,
                width = max_count_width
            ));
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

/// Truncate a sample string for display, adding ellipsis if needed
fn truncate_sample(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', "\\n").replace('\r', "\\r");
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
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
    fn formats_templates_output_default() {
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
        // Default (verbose=0): no samples shown
        let output = format_templates_output(&templates, 0);
        assert!(output.starts_with("templates (2 items):"));
        assert!(output.contains(&format!("[{}]", template1_id)));
        assert!(output.contains("L1-100")); // Line range for first template
        assert!(output.contains("L50")); // Single line for second template
        assert!(!output.contains("e.g.")); // No samples in default mode
    }

    #[test]
    fn formats_templates_output_verbose() {
        let template1_id = generate_template_id("a <*> b");
        let templates = vec![DrainTemplate {
            template: "a <*> b".to_string(),
            template_id: template1_id.clone(),
            count: 3,
            sample: "a 123 b".to_string(),
            first_line: Some(1),
            last_line: Some(100),
        }];
        // Verbose mode: samples shown
        let output = format_templates_output(&templates, 1);
        assert!(output.contains("e.g. \"a 123 b\""));
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
    fn truncates_long_samples() {
        let long_sample = "a".repeat(200);
        let truncated = truncate_sample(&long_sample, 80);
        assert!(truncated.len() <= 80);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn escapes_newlines_in_samples() {
        let sample_with_newlines = "line1\nline2\r\nline3";
        let escaped = truncate_sample(sample_with_newlines, 100);
        assert!(!escaped.contains('\n'));
        assert!(!escaped.contains('\r'));
        assert!(escaped.contains("\\n"));
        assert!(escaped.contains("\\r"));
    }
}
