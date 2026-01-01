use once_cell::sync::Lazy;
use regex::Regex;
use rhai::{Array, Dynamic, Engine, Map};
use std::collections::HashSet;
use std::sync::Mutex;

/// Cache of regex patterns we've already warned about to avoid spamming stderr.
static REGEX_WARNING_CACHE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Emit a one-time warning for an invalid regex pattern.
fn warn_invalid_regex(pattern: &str, error: &regex::Error) {
    let mut cache = match REGEX_WARNING_CACHE.lock() {
        Ok(guard) => guard,
        Err(_) => return, // Poisoned mutex, skip warning
    };

    if cache.insert(pattern.to_string()) {
        let message = crate::config::format_warning_message_auto(&format!(
            "invalid regex '{}': {}",
            pattern, error
        ));
        eprintln!("{}", message);
    }
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("extract_regex", |text: &str, pattern: &str| -> String {
        match Regex::new(pattern) {
            Ok(re) => {
                if let Some(captures) = re.captures(text) {
                    if captures.len() > 1 {
                        captures
                            .get(1)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        captures
                            .get(0)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    }
                } else {
                    String::new()
                }
            }
            Err(e) => {
                warn_invalid_regex(pattern, &e);
                String::new()
            }
        }
    });

    engine.register_fn(
        "extract_regex",
        |text: &str, pattern: &str, group: i64| -> String {
            match Regex::new(pattern) {
                Ok(re) => {
                    if let Some(captures) = re.captures(text) {
                        let group_idx = if group < 0 { 0 } else { group as usize };
                        captures
                            .get(group_idx)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    }
                }
                Err(e) => {
                    warn_invalid_regex(pattern, &e);
                    String::new()
                }
            }
        },
    );

    engine.register_fn("extract_regexes", |text: &str, pattern: &str| -> Array {
        match Regex::new(pattern) {
            Ok(re) => {
                let mut results = Array::new();
                for captures in re.captures_iter(text) {
                    if captures.len() > 1 {
                        let groups: Array = captures
                            .iter()
                            .skip(1)
                            .filter_map(|m| {
                                m.map(|match_| Dynamic::from(match_.as_str().to_string()))
                            })
                            .collect();
                        results.push(Dynamic::from(groups));
                    } else if let Some(full_match) = captures.get(0) {
                        results.push(Dynamic::from(full_match.as_str().to_string()));
                    }
                }
                results
            }
            Err(e) => {
                warn_invalid_regex(pattern, &e);
                Array::new()
            }
        }
    });

    engine.register_fn(
        "extract_regexes",
        |text: &str, pattern: &str, group: i64| -> Array {
            match Regex::new(pattern) {
                Ok(re) => {
                    let mut results = Array::new();
                    let group_idx = if group < 0 { 0 } else { group as usize };

                    for captures in re.captures_iter(text) {
                        if let Some(group_match) = captures.get(group_idx) {
                            results.push(Dynamic::from(group_match.as_str().to_string()));
                        }
                    }
                    results
                }
                Err(e) => {
                    warn_invalid_regex(pattern, &e);
                    Array::new()
                }
            }
        },
    );

    engine.register_fn(
        "extract_re_maps",
        |text: &str, pattern: &str, field_name: &str| -> Array {
            match Regex::new(pattern) {
                Ok(re) => {
                    let mut results = Array::new();
                    for captures in re.captures_iter(text) {
                        let match_value = if captures.len() > 1 {
                            captures.get(1).map(|m| m.as_str()).unwrap_or("")
                        } else {
                            captures.get(0).map(|m| m.as_str()).unwrap_or("")
                        };

                        let mut map = Map::new();
                        map.insert(field_name.into(), Dynamic::from(match_value.to_string()));
                        results.push(Dynamic::from(map));
                    }
                    results
                }
                Err(e) => {
                    warn_invalid_regex(pattern, &e);
                    Array::new()
                }
            }
        },
    );

    engine.register_fn("split_re", |text: &str, pattern: &str| -> Array {
        match Regex::new(pattern) {
            Ok(re) => re
                .split(text)
                .map(|s| Dynamic::from(s.to_string()))
                .collect(),
            Err(e) => {
                warn_invalid_regex(pattern, &e);
                vec![Dynamic::from(text.to_string())]
            }
        }
    });

    engine.register_fn(
        "replace_re",
        |text: &str, pattern: &str, replacement: &str| -> String {
            match Regex::new(pattern) {
                Ok(re) => re.replace_all(text, replacement).to_string(),
                Err(e) => {
                    warn_invalid_regex(pattern, &e);
                    text.to_string()
                }
            }
        },
    );
}
