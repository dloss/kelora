use rhai::{EvalAltResult, Map, Scope};
use std::sync::{Arc, Mutex};

use super::RhaiEngine;

#[derive(Debug, Clone)]
pub struct DebugConfig {
    pub verbosity: u8,
    pub use_emoji: bool,
}

impl DebugConfig {
    pub fn new(verbose_count: u8) -> Self {
        DebugConfig {
            verbosity: verbose_count,
            use_emoji: true, // Default to true, will be overridden
        }
    }

    pub fn with_emoji(mut self, use_emoji: bool) -> Self {
        self.use_emoji = use_emoji;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.verbosity > 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    pub position: Option<rhai::Position>,
    pub source_snippet: Option<String>,
    pub last_operation: Option<String>,
    pub error_location: Option<String>,
}

pub struct DebugTracker {
    pub config: DebugConfig,
    pub(crate) context: Arc<Mutex<ExecutionContext>>,
}

impl DebugTracker {
    pub fn new(config: DebugConfig) -> Self {
        DebugTracker {
            config,
            context: Arc::new(Mutex::new(ExecutionContext::default())),
        }
    }

    pub fn log_basic(&self, message: &str) {
        if self.config.is_enabled() && self.config.verbosity >= 1 {
            eprintln!("{}", message);
        }
    }

    pub fn log_detailed(&self, stage: &str, event_num: u64, operation: &str) {
        if self.config.is_enabled() && self.config.verbosity >= 2 {
            eprintln!("Trace: Event #{} {} → {}", event_num, stage, operation);
        }
    }

    pub fn log_step(&self, step_info: &str, result: &str) {
        if self.config.is_enabled() && self.config.verbosity >= 3 {
            eprintln!("  → {} → {}", step_info, result);
        }
    }

    pub fn update_context(&self, position: Option<rhai::Position>, source: Option<&str>) {
        if self.config.is_enabled() {
            if let Ok(mut ctx) = self.context.lock() {
                ctx.position = position;
                ctx.source_snippet = source.map(|s| s.to_string());
            }
        }
    }

    pub fn get_context(&self) -> ExecutionContext {
        if let Ok(ctx) = self.context.lock() {
            ctx.clone()
        } else {
            ExecutionContext::default()
        }
    }
}

impl Clone for DebugTracker {
    fn clone(&self) -> Self {
        DebugTracker {
            config: self.config.clone(),
            context: Arc::clone(&self.context),
        }
    }
}

pub struct ErrorEnhancer {
    debug_config: DebugConfig,
}

impl ErrorEnhancer {
    pub fn new(debug_config: DebugConfig) -> Self {
        ErrorEnhancer { debug_config }
    }

    pub fn enhance_error(
        &self,
        error: &EvalAltResult,
        scope: &Scope,
        script: &str,
        stage: &str,
        execution_context: &ExecutionContext,
    ) -> String {
        let mut output = String::new();
        let (stage_prefix, hint_prefix) = if self.debug_config.use_emoji {
            ("🔸", "💡")
        } else {
            ("Error:", "Hint:")
        };

        output.push_str(&format!("{stage_prefix} Stage {} failed\n", stage));
        output.push_str(&format!("  Code: {}\n", script.trim()));
        output.push_str(&format!("  Error: {}\n", error));

        if let Some(pos) = &execution_context.position {
            output.push_str(&format!("   Position: {}\n", pos));
        }

        if let Some(suggestions) = self.generate_suggestions(error, scope, Some(script)) {
            output.push_str(&format!("   {hint_prefix} {}\n", suggestions));
        }

        if self.debug_config.is_enabled() {
            output.push_str("\n   Variables in scope:\n");
            for (name, _is_const, value) in scope.iter() {
                let preview = format!("{:?}", value);
                let preview = if preview.len() > 50 {
                    format!("{}...", &preview[..47])
                } else {
                    preview
                };
                output.push_str(&format!(
                    "   • {}: {} = {}\n",
                    name,
                    value.type_name(),
                    preview
                ));
            }
        }

        output.push_str(&self.get_stage_help(stage, error));
        output
    }

    pub(crate) fn generate_suggestions(
        &self,
        error: &EvalAltResult,
        scope: &Scope,
        script: Option<&str>,
    ) -> Option<String> {
        let base = match error {
            EvalAltResult::ErrorVariableNotFound(var_name, _) => {
                // The most common newcomer mistake is referencing an event field
                // without the `e.` prefix (e.g. `status` instead of `e.status`).
                // If the bare identifier matches—or closely resembles—a real field
                // on the event, point straight at `e.<field>` rather than fall back
                // to scope-variable lookups that only know about e/meta/conf/line.
                if let Some(hint) = self.suggest_event_field_prefix(var_name, scope) {
                    Some(hint)
                } else {
                    let similar = self.find_similar_variables(var_name, scope);
                    if !similar.is_empty() {
                        Some(format!("Did you mean: {}?", similar.join(", ")))
                    } else if var_name.contains('.') {
                        Some("Check if the field exists and use safe access like 'if \"field\" in e { e.field } else { \"default\" }'".to_string())
                    } else if var_name.starts_with("e.") {
                        Some("Try using bracket notation for special characters: e[\"field-name\"] or e[\"field.with.dots\"]".to_string())
                    } else {
                        Some("Available variables: e (event), meta (metadata), conf (initialization data), line (raw line)".to_string())
                    }
                }
            }
            EvalAltResult::ErrorPropertyNotFound(prop_name, _) => {
                let mut suggestions = Vec::new();

                if let Some(fields) = self.event_field_names(scope) {
                    let similar: Vec<_> = fields
                        .iter()
                        .filter(|name| {
                            let sim = self.calculate_similarity(
                                &prop_name.to_lowercase(),
                                &name.to_lowercase(),
                            );
                            sim > 0.6
                                || name.contains(prop_name)
                                || prop_name.contains(name.as_str())
                        })
                        .take(3)
                        .cloned()
                        .collect();
                    if !similar.is_empty() {
                        suggestions.push(format!("Did you mean field: {}?", similar.join(", ")));
                    } else {
                        let preview: Vec<_> = fields.into_iter().take(5).collect();
                        if !preview.is_empty() {
                            suggestions.push(format!(
                                "Available fields include: {}{}",
                                preview.join(", "),
                                if preview.len() == 5 { " ..." } else { "" }
                            ));
                        }
                    }
                }

                suggestions
                    .push("Try `--stats` or `-F inspect` to see available fields".to_string());
                Some(suggestions.join(" "))
            }
            EvalAltResult::ErrorIndexNotFound(index, _) => Some(format!(
                "Index '{}' not found. Check array bounds with 'if e.array.len() > {} {{ ... }}'",
                index, index
            )),
            EvalAltResult::ErrorFunctionNotFound(func_sig, _) => {
                self.suggest_function_alternatives(func_sig)
            }
            EvalAltResult::ErrorMismatchDataType(expected, actual, _) => {
                let mut hints = vec![format!(
                    "Type mismatch: expected {}, got {}.",
                    expected, actual
                )];

                if expected.contains("bool") {
                    hints.push(
                        "Filters must return true/false; use comparisons like `e.level == \"ERROR\"` or `contains(...)`"
                            .to_string(),
                    );
                }
                if actual.contains("()") || expected.contains("()") {
                    hints.push(
                        "Missing fields return () by default; guard with e.has(\"field\") or e.get(\"field\", default) before chaining"
                            .to_string(),
                    );
                }
                hints.push(
                    "Use type_of() to check types or to_string()/to_number()/parse_json() for conversion"
                        .to_string(),
                );

                Some(hints.join(" "))
            }
            EvalAltResult::ErrorRuntime(msg, _) => {
                let msg_str = msg.to_string();
                if msg_str.contains("got ()") {
                    Some(
                        "Received (), which means a field is missing or returned no value. \
                         Use e.get_path('field.path', default) to provide defaults, \
                         or e.has_path('field.path') to check if a field exists first."
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            _ => None,
        };

        let raw_string_hint = script.and_then(|script| Self::raw_string_hint(error, script));

        match (base, raw_string_hint) {
            (Some(base), Some(hint)) => Some(format!("{} {}", base, hint)),
            (Some(base), None) => Some(base),
            (None, Some(hint)) => Some(hint),
            (None, None) => None,
        }
    }

    pub(crate) fn raw_string_hint(error: &EvalAltResult, script: &str) -> Option<String> {
        match error {
            EvalAltResult::ErrorParsing(_, _) if Self::contains_rust_raw_string(script) => Some(
                "It looks like a Rust raw string (r\"...\"). Rhai raw strings use #\"...\"# (or ##\"...\"## for embedded quotes)."
                    .to_string(),
            ),
            _ => None,
        }
    }

    fn contains_rust_raw_string(script: &str) -> bool {
        let bytes = script.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'r' {
                let prev = if i == 0 { None } else { Some(bytes[i - 1]) };
                let starts_token = prev.is_none_or(|c| !Self::is_ident_char(c));
                if starts_token {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b'#' {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'"' {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    fn is_ident_char(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_'
    }

    fn suggest_function_alternatives(&self, func_sig: &str) -> Option<String> {
        if func_sig.contains("()") {
            let func_name = func_sig.split('(').next().unwrap_or("").trim();

            if matches!(
                func_name,
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "=="
                    | "!="
                    | "<"
                    | ">"
                    | "<="
                    | ">="
                    | "&&"
                    | "||"
                    | "&"
                    | "|"
                    | "^"
            ) {
                return Some(format!(
                    "Field is missing. Use e.has(\"field\") or e.get_path(\"field\", default) before using '{}'",
                    func_name
                ));
            }

            if func_sig.contains(" (())")
                || func_sig.contains("((), ")
                || func_sig.contains(", ())")
            {
                return Some(
                    "Field is missing. Use e.has(\"field\") to check, or e.get_path(\"field\", default) to provide a default"
                        .to_string(),
                );
            }
        }

        let func_name = func_sig.split('(').next().unwrap_or(func_sig).trim();

        let mut best: Vec<(String, f64)> = RhaiEngine::function_catalog()
            .into_iter()
            .map(|candidate| {
                let sim =
                    self.calculate_similarity(&func_name.to_lowercase(), &candidate.to_lowercase());
                (candidate, sim)
            })
            .filter(|(_, sim)| *sim > 0.45)
            .collect();

        best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        best.truncate(3);

        if !best.is_empty() {
            return Some(format!(
                "Did you mean: {}?",
                best.iter()
                    .map(|(candidate, _)| candidate.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        match func_name {
            "length" => Some("Use 'len()' instead of 'length()'".to_string()),
            "size" => Some("Use 'len()' instead of 'size()'".to_string()),
            "substr" | "substring" => Some(
                "Use string slicing: s[start..end] or extract_regex() for pattern matching"
                    .to_string(),
            ),
            "indexOf" | "index_of" => Some(
                "Use 'contains()' to check existence or 'split()' to find positions".to_string(),
            ),
            "push_back" | "append" => Some("Use 'push()' to add elements to arrays".to_string()),
            "to_int" | "parseInt" => {
                Some("Use 'parse()' or to_number() for type conversion".to_string())
            }
            "to_str" | "toString" => Some("Use 'to_string()' for string conversion".to_string()),
            "match" => Some(
                "Use 'extract_regex()' for regex matching or 'contains()' for simple checks"
                    .to_string(),
            ),
            name if name.ends_with("_re") => Some(
                "Regex functions: extract_regex(), extract_regexes(), extract_regex_maps(), split_regex(), replace_regex()"
                    .to_string(),
            ),
            _ => None,
        }
    }

    fn find_similar_variables(&self, target: &str, scope: &Scope) -> Vec<String> {
        let mut suggestions = Vec::new();
        let target_lower = target.to_lowercase();

        for (name, _is_const, _value) in scope.iter() {
            let name_lower = name.to_lowercase();
            let similarity = self.calculate_similarity(&target_lower, &name_lower);

            if similarity > 0.6
                || name_lower.contains(&target_lower)
                || target_lower.contains(&name_lower)
                || self.has_common_prefix(&target_lower, &name_lower)
            {
                suggestions.push(name.to_string());
            }
        }

        suggestions.sort_by(|a, b| {
            let sim_a = self.calculate_similarity(&target_lower, &a.to_lowercase());
            let sim_b = self.calculate_similarity(&target_lower, &b.to_lowercase());
            sim_b
                .partial_cmp(&sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        suggestions.truncate(3);
        suggestions
    }

    fn event_field_names(&self, scope: &Scope) -> Option<Vec<String>> {
        if let Some(e_map) = scope.get_value::<Map>("e") {
            let mut keys: Vec<String> = e_map
                .into_keys()
                .map(|k| k.to_string())
                .filter(|k| !k.is_empty())
                .collect();
            keys.sort();
            keys.dedup();
            return Some(keys);
        }
        None
    }

    /// If a bare identifier (used without the `e.` prefix) matches or resembles a
    /// field on the event, suggest the prefixed form `e.<field>`. Returns `None`
    /// when the identifier already carries a dot/prefix or no field is a good
    /// match, so the caller can fall back to scope-variable suggestions.
    fn suggest_event_field_prefix(&self, var_name: &str, scope: &Scope) -> Option<String> {
        if var_name.contains('.') {
            return None;
        }
        let fields = self.event_field_names(scope)?;

        // Exact field match: the missing `e.` prefix is the whole problem.
        if fields.iter().any(|f| f.as_str() == var_name) {
            return Some(format!(
                "Did you mean: e.{var_name}? Event fields are accessed through `e`, e.g. `e.{var_name}`."
            ));
        }

        // Otherwise offer the closest field names, already prefixed.
        let target_lower = var_name.to_lowercase();
        let similar: Vec<String> = fields
            .iter()
            .filter(|name| {
                let name_lower = name.to_lowercase();
                // `>=` (not `>`) so boundary-similarity transpositions like
                // `levle` -> `level` (distance 2 over 5 chars == 0.6) are caught.
                self.calculate_similarity(&target_lower, &name_lower) >= 0.6
                    || name_lower.contains(&target_lower)
                    || target_lower.contains(&name_lower)
            })
            .take(3)
            .map(|name| format!("e.{name}"))
            .collect();

        if similar.is_empty() {
            None
        } else {
            Some(format!("Did you mean: {}?", similar.join(", ")))
        }
    }

    fn calculate_similarity(&self, s1: &str, s2: &str) -> f64 {
        if s1 == s2 {
            return 1.0;
        }
        if s1.is_empty() || s2.is_empty() {
            return 0.0;
        }

        let max_len = s1.len().max(s2.len());
        let distance = self.levenshtein_distance(s1, s2);
        1.0 - (distance as f64 / max_len as f64)
    }

    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let chars1: Vec<char> = s1.chars().collect();
        let chars2: Vec<char> = s2.chars().collect();
        let len1 = chars1.len();
        let len2 = chars2.len();

        if len1 == 0 {
            return len2;
        }
        if len2 == 0 {
            return len1;
        }

        let mut prev_row: Vec<usize> = (0..=len2).collect();

        for i in 1..=len1 {
            let mut curr_row = vec![i];

            for j in 1..=len2 {
                let cost = if chars1[i - 1] == chars2[j - 1] { 0 } else { 1 };
                curr_row.push(
                    (curr_row[j - 1] + 1)
                        .min(prev_row[j] + 1)
                        .min(prev_row[j - 1] + cost),
                );
            }

            prev_row = curr_row;
        }

        prev_row[len2]
    }

    fn has_common_prefix(&self, s1: &str, s2: &str) -> bool {
        if s1.len() < 2 || s2.len() < 2 {
            return false;
        }
        let prefix_len = 2.min(s1.len()).min(s2.len());
        s1[..prefix_len] == s2[..prefix_len]
    }

    fn get_stage_help(&self, stage: &str, error: &EvalAltResult) -> String {
        let mut help = String::new();

        match stage {
            "filter" => {
                help.push_str("\n   🔹 Filter stage tips:\n");
                help.push_str("   • Filters must return true/false (boolean values)\n");
                help.push_str("   • Use 'e.field_name' to access event fields\n");
                help.push_str(
                    "   • Use 'e[\"field-with-special-chars\"]' for complex field names\n",
                );
                help.push_str("   • Use 'if \"field\" in e { ... }' to check field existence\n");

                if let EvalAltResult::ErrorMismatchDataType(_, _, _) = error {
                    help.push_str(
                        "   • Remember: filters need boolean results, not strings or numbers\n",
                    );
                }
            }
            "exec" => {
                help.push_str("\n   🔹 Exec stage tips:\n");
                help.push_str("   • Use 'e.new_field = value' to add fields to events\n");
                help.push_str("   • Use 'e.field = ()' to remove fields from events\n");
                help.push_str("   • Use 'e = ()' to remove entire event (filter out)\n");
                help.push_str("   • Use 'let variable = value' for temporary variables\n");
                help.push_str("   • Use 'print(\"debug: \" + value)' for debugging output\n");
            }
            "begin" => {
                help.push_str("\n   🔹 Begin stage tips:\n");
                help.push_str("   • Use 'conf.field = value' to set global initialization data\n");
                help.push_str("   • Use 'read_file(\"path\")' to load external data\n");
                help.push_str("   • Variables set here are available in all event processing\n");
            }
            "end" => {
                help.push_str("\n   🔹 End stage tips:\n");
                help.push_str("   • Use 'metrics.key' to access accumulated tracking data\n");
                help.push_str("   • Use 'print()' to output final results\n");
                help.push_str("   • This runs after all events are processed\n");
            }
            _ => {}
        }

        help
    }
}
