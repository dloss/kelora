use drain_rs::DrainTree;
use grok::Grok;
use std::cell::RefCell;
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
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct DrainResult {
    pub template: String,
    pub count: usize,
    pub is_new: bool,
}

#[derive(Debug)]
struct DrainState {
    config: DrainConfig,
    tree: DrainTree,
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
        Self { config, tree }
    }

    fn ingest(&mut self, text: &str) -> Result<DrainResult, String> {
        let cluster = self
            .tree
            .add_log_line(text)
            .ok_or_else(|| "Drain failed to match or create a cluster".to_string())?;
        let count = usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX);
        Ok(DrainResult {
            template: cluster.as_string(),
            count,
            is_new: count == 1,
        })
    }

    fn templates(&self) -> Vec<DrainTemplate> {
        let mut templates: Vec<DrainTemplate> = self
            .tree
            .log_groups()
            .into_iter()
            .map(|cluster| DrainTemplate {
                template: cluster.as_string(),
                count: usize::try_from(cluster.num_matched()).unwrap_or(usize::MAX),
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
        ("KELORA_PATH", r"(?:/[A-Za-z0-9._-]+)+"),
        ("KELORA_OAUTH", r"ya29\.[0-9A-Za-z_-]+"),
        ("KELORA_FUNCTION", r"[A-Za-z0-9_.]+\([^)]*\)"),
        ("KELORA_HEXCOLOR", r"#[0-9A-Fa-f]{6}"),
        (
            "KELORA_VERSION",
            r"[vV]?\d+\.\d+(?:\.\d+)?(?:-[A-Za-z0-9]+)?",
        ),
        ("KELORA_HEXNUM", r"0x[0-9A-Fa-f]+"),
        ("KELORA_DURATION", r"\d+(?:\.\d+)?(?:us|ms|[smhd])"),
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

pub fn drain_template(text: &str, config: Option<DrainConfig>) -> Result<DrainResult, String> {
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
        drain.ingest(text)
    })
}

pub fn drain_templates() -> Vec<DrainTemplate> {
    DRAIN_STATE.with(|state| match state.borrow().as_ref() {
        Some(drain) => drain.templates(),
        None => Vec::new(),
    })
}

pub fn format_templates_output(templates: &[DrainTemplate]) -> String {
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
        output.push_str(&format!(
            "  {:>width$}: {}\n",
            template.count,
            template.template,
            width = max_count_width
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
            .ingest("failed to connect to 10.0.0.1")
            .expect("first ingest");
        let b = drain
            .ingest("failed to connect to 10.0.0.2")
            .expect("second ingest");

        assert_eq!(a.template, "failed to connect to <ipv4>");
        assert_eq!(b.template, "failed to connect to <ipv4>");
        assert_eq!(b.count, 2);
    }

    #[test]
    fn formats_templates_output() {
        let templates = vec![
            DrainTemplate {
                template: "a <*> b".to_string(),
                count: 3,
            },
            DrainTemplate {
                template: "x y z".to_string(),
                count: 1,
            },
        ];
        let output = format_templates_output(&templates);
        assert!(output.starts_with("templates (2 items):"));
        assert!(output.contains("3: a <*> b"));
        assert!(output.contains("1: x y z"));
    }
}
