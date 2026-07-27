use rhai::{EvalAltResult, Map, Scope};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::RhaiEngine;

thread_local! {
    /// Answers for function-not-found signatures already seen on this thread.
    ///
    /// A script bug fails on *every* event, and the answer depends only on the
    /// signature, so without this the catalogue is rescanned hundreds of thousands
    /// of times to produce one line of output the user sees once.
    static FUNCTION_HINTS: RefCell<HashMap<String, Option<String>>> =
        RefCell::new(HashMap::new());
}

/// Distinct signatures worth remembering per thread. A run has a handful of
/// genuinely different failures; the cap only bounds a pathological script.
const FUNCTION_HINT_CACHE_LIMIT: usize = 64;

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
        let hint_prefix = if self.debug_config.use_emoji {
            "💡"
        } else {
            "Hint:"
        };

        // Header mirrors the non-debug diagnostic ("<stage> error"). The caller
        // already prefixes "<Stage> error:", so a "Error: Stage <stage> failed"
        // header here read as the redundant "Filter error: Error: Stage filter failed".
        if self.debug_config.use_emoji {
            output.push_str(&format!("🔸 {stage} error\n"));
        } else {
            output.push_str(&format!("{stage} error\n"));
        }
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
            // Traversing into a missing intermediate (e.g. `e.user.role` when
            // `user` is absent) leaves a () in the chain, so the next property
            // access fails with a getter-not-registered error on type '()'.
            // Surface the same missing-field guidance the other paths give.
            EvalAltResult::ErrorDotExpr(msg, _) if msg.contains("type '()'") => Some(
                "A field in the path is missing, so the value is (). \
                 Use e.get_path('a.b', default) to read nested fields safely, \
                 or e.has_path('a.b') to check the path exists first."
                    .to_string(),
            ),
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

    /// Methods that only exist on a datetime, so seeing one called on a string is
    /// unambiguous: the user has the raw timestamp text, not the parsed value.
    ///
    /// Derived from the engine and checked by `datetime_only_methods_are_exhaustive`.
    /// Names shared with strings (`to_string`, `to_debug`) are deliberately absent —
    /// they succeed on a string, so they never reach this path.
    const DATETIME_ONLY_METHODS: &'static [&'static str] = &[
        "ceil_to",
        "day",
        "format",
        "hour",
        "minute",
        "month",
        "round_to",
        "second",
        "timezone_name",
        "to_iso",
        "to_local",
        "to_timezone",
        "to_utc",
        "ts_nanos",
        "year",
    ];

    /// `_`-separated tokens, sorted, so names that differ only in word order
    /// compare equal: `regex_extract` against kelora's `extract_regex`. Worth
    /// special-casing because the rest of the ecosystem spells it noun-first
    /// (`regexp_extract` in Spark/Hive, `REGEXP_EXTRACT` in BigQuery), and edit
    /// distance scores that transposition as almost unrelated.
    fn sorted_tokens(name: &str) -> Vec<&str> {
        let mut tokens: Vec<&str> = name.split('_').filter(|t| !t.is_empty()).collect();
        tokens.sort_unstable();
        tokens
    }

    /// True when the first argument of a failed call was string-typed. Rhai renders
    /// the receiver as the first argument, e.g.
    /// `format (&str | ImmutableString | String, &str | ImmutableString | String)`.
    fn first_arg_is_string(func_sig: &str) -> bool {
        let Some(args_start) = func_sig.find('(') else {
            return false;
        };
        let args = &func_sig[args_start + 1..];
        let first = args.split(',').next().unwrap_or("").trim();
        first.contains("ImmutableString") || first.contains("&str") || first == "String"
    }

    /// Number of arguments in a rendered signature, receiver included. Rhai lists
    /// each argument's accepted types separated by `|`, never by a comma, so counting
    /// top-level commas is enough.
    fn arg_count(func_sig: &str) -> usize {
        let Some(open) = func_sig.find('(') else {
            return 0;
        };
        let args = func_sig[open + 1..].trim_end_matches(')').trim();
        if args.is_empty() {
            0
        } else {
            args.split(',').count()
        }
    }

    /// Replace the generic not-found text for a datetime method called on a string.
    ///
    /// The type half is always true. The pointer to `meta.parsed_ts` is hedged on
    /// purpose: it is only the answer when the string *is* the event's timestamp —
    /// `meta.parsed_ts` is `()` when no timestamp was detected, and irrelevant when
    /// the receiver was some other string field.
    fn suggest_datetime_receiver(func_name: &str, func_sig: &str) -> Option<String> {
        if !Self::DATETIME_ONLY_METHODS.contains(&func_name) || !Self::first_arg_is_string(func_sig)
        {
            return None;
        }
        // Show the call the way the user would type it: the receiver counts as the
        // first argument, so a single-argument signature takes no arguments at all.
        let args = if Self::arg_count(func_sig) > 1 {
            "..."
        } else {
            ""
        };
        Some(format!(
            "{func_name}() is a datetime method, but it was called on a string. \
             If that string is the event's timestamp, use the already-parsed value: \
             meta.parsed_ts.{func_name}({args}). Otherwise parse it first: \
             text.to_datetime().{func_name}({args})."
        ))
    }

    fn suggest_function_alternatives(&self, func_sig: &str) -> Option<String> {
        if let Some(cached) = FUNCTION_HINTS.with(|hints| hints.borrow().get(func_sig).cloned()) {
            return cached;
        }
        let hint = self.compute_function_alternatives(func_sig);
        FUNCTION_HINTS.with(|hints| {
            let mut hints = hints.borrow_mut();
            if hints.len() < FUNCTION_HINT_CACHE_LIMIT {
                hints.insert(func_sig.to_string(), hint.clone());
            }
        });
        hint
    }

    /// Depends only on `func_sig`, which is what makes the cache above sound.
    fn compute_function_alternatives(&self, func_sig: &str) -> Option<String> {
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

        // A datetime method reached for on a string is a type error, not a typo:
        // the name exists, it just lives on a datetime. Say so instead of letting
        // edit distance offer an unrelated name.
        if let Some(hint) = Self::suggest_datetime_receiver(func_name, func_sig) {
            return Some(hint);
        }

        let called = func_name.to_lowercase();
        let called_tokens = Self::sorted_tokens(&called);

        let mut best: Vec<(String, f64)> = RhaiEngine::function_catalog()
            .iter()
            .map(|candidate| {
                let candidate_lower = candidate.to_lowercase();
                // Treat a pure token transposition as an exact hit so it outranks
                // any edit-distance neighbour.
                let sim = if called_tokens.len() > 1
                    && Self::sorted_tokens(&candidate_lower) == called_tokens
                {
                    1.0
                } else {
                    self.calculate_similarity(&called, &candidate_lower)
                };
                (candidate.to_string(), sim)
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
        let bullet = if self.debug_config.use_emoji {
            "🔹 "
        } else {
            ""
        };

        match stage {
            "filter" => {
                help.push_str(&format!("\n   {bullet}Filter stage tips:\n"));
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
                help.push_str(&format!("\n   {bullet}Exec stage tips:\n"));
                help.push_str("   • Use 'e.new_field = value' to add fields to events\n");
                help.push_str("   • Use 'e.field = ()' to remove fields from events\n");
                help.push_str("   • Use 'e = ()' to remove entire event (filter out)\n");
                help.push_str("   • Use 'let variable = value' for temporary variables\n");
                help.push_str("   • Use 'print(\"debug: \" + value)' for debugging output\n");
            }
            "begin" => {
                help.push_str(&format!("\n   {bullet}Begin stage tips:\n"));
                help.push_str("   • Use 'conf.field = value' to set global initialization data\n");
                help.push_str("   • Use 'read_file(\"path\")' to load external data\n");
                help.push_str("   • Variables set here are available in all event processing\n");
            }
            "end" => {
                help.push_str(&format!("\n   {bullet}End stage tips:\n"));
                help.push_str("   • Use 'metrics.key' to access accumulated tracking data\n");
                help.push_str("   • Use 'print()' to output final results\n");
                help.push_str("   • This runs after all events are processed\n");
            }
            _ => {}
        }

        help
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Function name and the type of its first parameter, e.g.
    /// `format(_: &mut ...DateTimeWrapper, _: string)` -> `("format", "&mut ...DateTimeWrapper")`.
    /// Only the first parameter is read, and no input type contains a comma, so
    /// splitting on the first `,`/`)` is enough. Returns `None` for operator entries.
    fn name_and_receiver(sig: &str) -> Option<(&str, &str)> {
        let open = sig.find('(')?;
        let name = &sig[..open];
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return None;
        }
        let rest = &sig[open + 1..];
        let end = rest.find([',', ')'])?;
        let first = rest[..end].trim();
        // Rendered as `name: type`; the parameter name carries no information here.
        let ty = first.split_once(':').map_or(first, |(_, t)| t.trim());
        Some((name, ty))
    }

    #[test]
    fn datetime_only_methods_are_exhaustive() {
        let mut engine = rhai::Engine::new();
        crate::rhai_functions::register_all_functions(&mut engine);
        // Standard packages included on purpose: `to_string`/`to_debug` accept a
        // string only via Rhai's stdlib, and omitting it would wrongly classify
        // them as datetime-only and hijack errors that have nothing to do with time.
        let sigs = engine.gen_fn_signatures(true);

        let mut on_datetime: BTreeSet<&str> = BTreeSet::new();
        let mut on_string: BTreeSet<&str> = BTreeSet::new();
        for sig in &sigs {
            let Some((name, receiver)) = name_and_receiver(sig) else {
                continue;
            };
            if receiver.contains("datetime::DateTimeWrapper") {
                on_datetime.insert(name);
            }
            if matches!(
                receiver,
                "string" | "ImmutableString" | "&str" | "&mut string" | "Dynamic" | "&mut Dynamic"
            ) {
                on_string.insert(name);
            }
        }

        let derived: BTreeSet<&str> = on_datetime.difference(&on_string).copied().collect();
        let declared: BTreeSet<&str> = ErrorEnhancer::DATETIME_ONLY_METHODS
            .iter()
            .copied()
            .collect();

        assert_eq!(
            derived, declared,
            "DATETIME_ONLY_METHODS is out of sync with the engine; it must list exactly \
             the methods that exist on a datetime and not on a string"
        );
    }

    #[test]
    fn datetime_hint_ignores_non_string_receivers() {
        // A datetime method failing on some other type is a different mistake, so the
        // string-specific advice must not fire.
        assert!(ErrorEnhancer::suggest_datetime_receiver("format", "format (i64, i64)").is_none());
        assert!(ErrorEnhancer::suggest_datetime_receiver("year", "year (map)").is_none());
    }

    #[test]
    fn datetime_hint_matches_the_method_arity() {
        let no_args =
            ErrorEnhancer::suggest_datetime_receiver("hour", "hour (&str | ImmutableString)")
                .expect("hour on a string should be recognised");
        assert!(
            no_args.contains("meta.parsed_ts.hour()"),
            "a no-argument method should not be shown taking arguments; got: {no_args}"
        );

        let with_args = ErrorEnhancer::suggest_datetime_receiver(
            "format",
            "format (&str | ImmutableString, &str | ImmutableString)",
        )
        .expect("format on a string should be recognised");
        assert!(
            with_args.contains("meta.parsed_ts.format(...)"),
            "a method taking arguments should show them; got: {with_args}"
        );
    }

    #[test]
    fn repeated_lookups_return_the_cached_answer() {
        // The same failure recurs on every event, so the second lookup comes from the
        // cache; it must be the same answer, keyed on the signature and nothing else.
        let enhancer = ErrorEnhancer::new(DebugConfig::new(0));
        let first = enhancer.suggest_function_alternatives("regex_extract (string, string)");
        let second = enhancer.suggest_function_alternatives("regex_extract (string, string)");
        assert_eq!(first, second);
        assert!(first
            .expect("expected a suggestion")
            .contains("extract_regex"));

        // A different signature must not collide with the cached one.
        let other = enhancer.suggest_function_alternatives("hour (&str | ImmutableString)");
        assert!(
            other.expect("expected a hint").contains("datetime method"),
            "each signature must get its own answer"
        );
    }

    #[test]
    fn token_transposition_compares_equal() {
        assert_eq!(
            ErrorEnhancer::sorted_tokens("regex_extract"),
            ErrorEnhancer::sorted_tokens("extract_regex")
        );
        assert_ne!(
            ErrorEnhancer::sorted_tokens("extract_ip"),
            ErrorEnhancer::sorted_tokens("extract_url")
        );
    }
}
