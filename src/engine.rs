#![allow(dead_code)] // Debugging/tracing scaffolding kept for verbose dev builds and future CLI toggles
use anyhow::Result;
use indexmap::IndexMap;
use rhai::{BinaryExpr, Dynamic, Engine, EvalAltResult, Expr, FnCallExpr, Scope, Stmt, Token, AST};
use std::collections::HashMap;

use rhai::debugger::{DebuggerCommand, DebuggerEvent};

use crate::event::Event;
use crate::rhai_functions;
use crate::rhai_functions::datetime::DateTimeWrapper;

/// Truncate text for display, respecting UTF-8 character boundaries
fn truncate_for_display(text: &str, max_len: usize) -> String {
    if text.chars().count() > max_len {
        let truncated: String = text.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        text.to_string()
    }
}

use rhai::Map;

// Temporary debug types until module structure is fixed
#[derive(Debug, Clone)]
pub struct DebugConfig {
    pub verbosity: u8,
    pub show_timing: bool,
    pub trace_events: bool,
    pub use_emoji: bool,
}

impl DebugConfig {
    pub fn new(verbose_count: u8) -> Self {
        DebugConfig {
            verbosity: verbose_count,
            show_timing: verbose_count >= 1,
            trace_events: verbose_count >= 2,
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

#[derive(Debug)]
pub struct ConfMutationError;

impl std::fmt::Display for ConfMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "conf map is read-only outside --begin; modifications are not allowed"
        )
    }
}

impl std::error::Error for ConfMutationError {}

fn dynamics_equal(lhs: &Dynamic, rhs: &Dynamic) -> bool {
    if lhs.type_name() != rhs.type_name() {
        return false;
    }

    // Primitive comparisons
    if let (Some(l), Some(r)) = (lhs.as_int().ok(), rhs.as_int().ok()) {
        return l == r;
    }
    if let (Some(l), Some(r)) = (lhs.as_float().ok(), rhs.as_float().ok()) {
        return l == r;
    }
    if let (Some(l), Some(r)) = (lhs.as_bool().ok(), rhs.as_bool().ok()) {
        return l == r;
    }
    if let (Ok(l), Ok(r)) = (lhs.clone().into_string(), rhs.clone().into_string()) {
        return l == r;
    }

    // Array comparison
    if let (Some(l_arr), Some(r_arr)) = (
        lhs.clone().try_cast::<rhai::Array>(),
        rhs.clone().try_cast::<rhai::Array>(),
    ) {
        if l_arr.len() != r_arr.len() {
            return false;
        }
        return l_arr
            .iter()
            .zip(r_arr.iter())
            .all(|(l, r)| dynamics_equal(l, r));
    }

    // Map comparison
    if let (Some(l_map), Some(r_map)) = (
        lhs.clone().try_cast::<rhai::Map>(),
        rhs.clone().try_cast::<rhai::Map>(),
    ) {
        return maps_equal(&l_map, &r_map);
    }

    false
}

fn maps_equal(lhs: &rhai::Map, rhs: &rhai::Map) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }

    lhs.iter().all(|(k, v)| match rhs.get(k) {
        Some(rv) => dynamics_equal(v, rv),
        None => false,
    })
}

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
pub struct DebugTracker {
    pub config: DebugConfig,
    context: Arc<Mutex<ExecutionContext>>,
    event_count: Arc<Mutex<u64>>,
    error_count: Arc<Mutex<u64>>,
}

impl DebugTracker {
    pub fn new(config: DebugConfig) -> Self {
        DebugTracker {
            config,
            context: Arc::new(Mutex::new(ExecutionContext::default())),
            event_count: Arc::new(Mutex::new(0)),
            error_count: Arc::new(Mutex::new(0)),
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

    pub fn log_execution_start(&self, stage: &str, script: &str, event_data: &str) {
        match self.config.verbosity {
            1 => {
                if self.config.is_enabled() {
                    let prefix = if self.config.use_emoji {
                        "🔹"
                    } else {
                        "kelora: "
                    };
                    eprintln!("{} Executing {} stage", prefix, stage);
                }
            }
            2 => {
                if self.config.is_enabled() {
                    eprintln!("{} execution started", stage);
                    eprintln!("  Script: {}", truncate_for_display(script, 100));
                }
            }
            3.. => {
                if self.config.is_enabled() {
                    eprintln!("{} execution trace:", stage);
                    eprintln!("  Script: {}", script.trim());
                    eprintln!("  Event: {}", truncate_for_display(event_data, 150));
                }
            }
            _ => {}
        }
    }

    pub fn log_execution_result(&self, stage: &str, success: bool, result_info: &str) {
        if self.config.is_enabled() && self.config.verbosity >= 2 {
            let status = if success { "✓" } else { "✗" };
            eprintln!("{} {} ({})", stage, status, result_info);
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
            event_count: Arc::clone(&self.event_count),
            error_count: Arc::clone(&self.error_count),
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

        // Basic error info
        output.push_str(&format!("🔸 Stage {} failed\n", stage));
        output.push_str(&format!("  Code: {}\n", script.trim()));
        output.push_str(&format!("  Error: {}\n", error));

        // Add execution context if available
        if let Some(pos) = &execution_context.position {
            output.push_str(&format!("   Position: {}\n", pos));
        }

        // Suggestions and stage tips should be shown even without verbose mode
        if let Some(suggestions) = self.generate_suggestions(error, scope, Some(script)) {
            output.push_str(&format!("   💡 {}\n", suggestions));
        }

        // Show scope information only if debug enabled (can be verbose)
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

        // Add stage-specific help (applies even without debug verbosity)
        output.push_str(&self.get_stage_help(stage, error));

        output
    }

    fn generate_suggestions(
        &self,
        error: &EvalAltResult,
        scope: &Scope,
        script: Option<&str>,
    ) -> Option<String> {
        let base = match error {
            EvalAltResult::ErrorVariableNotFound(var_name, _) => {
                let similar = self.find_similar_variables(var_name, scope);
                if !similar.is_empty() {
                    Some(format!("Did you mean: {}?", similar.join(", ")))
                } else {
                    // Check for common patterns
                    if var_name.contains('.') {
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

                // Offer similar field names from `e` map if available
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
                // Detect custom error messages involving Unit type "()"
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
        // Detect operations on missing fields (unit type)
        if func_sig.contains("()") {
            let func_name = func_sig.split('(').next().unwrap_or("").trim();

            // Check for binary operations with () operand
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

            // Check for method/function calls with () as parameter
            // Matches: "method (())", "method ((), other)", "method (other, ())", etc.
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
                    .map(|(c, _)| c.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Common function alternatives as fallbacks
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
                "Regex functions: extract_regex(), extract_regexes(), split_re(), replace_re()"
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

            // Include variables with good similarity or common patterns
            if similarity > 0.6
                || name_lower.contains(&target_lower)
                || target_lower.contains(&name_lower)
                || self.has_common_prefix(&target_lower, &name_lower)
            {
                suggestions.push(name.to_string());
            }
        }

        // Sort by similarity (best matches first)
        suggestions.sort_by(|a, b| {
            let sim_a = self.calculate_similarity(&target_lower, &a.to_lowercase());
            let sim_b = self.calculate_similarity(&target_lower, &b.to_lowercase());
            sim_b
                .partial_cmp(&sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Return top 3 suggestions
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

    fn calculate_similarity(&self, s1: &str, s2: &str) -> f64 {
        if s1 == s2 {
            return 1.0;
        }
        if s1.is_empty() || s2.is_empty() {
            return 0.0;
        }

        // Simple Levenshtein-based similarity
        let len1 = s1.len();
        let len2 = s2.len();
        let max_len = len1.max(len2);

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
                    (curr_row[j - 1] + 1) // insertion
                        .min(prev_row[j] + 1) // deletion
                        .min(prev_row[j - 1] + cost), // substitution
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

// Execution Tracer for step-by-step debugging
pub struct ExecutionTracer {
    config: DebugConfig,
    current_event: Arc<Mutex<u64>>,
    step_counter: Arc<Mutex<u32>>,
}

impl ExecutionTracer {
    pub fn new(config: DebugConfig) -> Self {
        ExecutionTracer {
            config,
            current_event: Arc::new(Mutex::new(0)),
            step_counter: Arc::new(Mutex::new(0)),
        }
    }

    pub fn trace_stage_execution(&self, stage_number: usize, stage_type: &str) {
        if self.config.verbosity >= 1 {
            let prefix = if self.config.use_emoji {
                "🔹"
            } else {
                "kelora: "
            };
            eprintln!(
                "{}Executing stage {} ({})",
                prefix, stage_number, stage_type
            );
        }
    }

    pub fn trace_step(&self, _event_num: u64, step_info: &str, result: &str) {
        if self.config.verbosity >= 2 {
            eprintln!("  → {} → {}", step_info, result);
        }
    }

    pub fn trace_event_start(&self, event_num: u64, event_data: &str) {
        if self.config.verbosity >= 2 {
            eprintln!("  Filter execution trace for event {}:", event_num);
            eprintln!("    Event: {}", truncate_for_display(event_data, 100));
        }
    }

    pub fn trace_event_result(&self, result: bool, action: &str) {
        if self.config.verbosity >= 2 {
            eprintln!("    Result: {} ({})", result, action);
        }
    }

    pub fn trace_expression_evaluation(&self, expression: &str, intermediate_result: &str) {
        if self.config.verbosity >= 3 {
            eprintln!("    Eval: {} → {}", expression, intermediate_result);
        }
    }

    pub fn trace_variable_access(&self, var_name: &str, value: &str) {
        if self.config.verbosity >= 3 {
            eprintln!(
                "    Access: {} = {}",
                var_name,
                truncate_for_display(value, 30)
            );
        }
    }

    pub fn trace_function_call(&self, func_name: &str, args: &str, result: &str) {
        if self.config.verbosity >= 3 {
            eprintln!(
                "    Call: {}({}) → {}",
                func_name,
                args,
                truncate_for_display(result, 30)
            );
        }
    }

    // Enhanced detailed tracing for -vvv level
    pub fn trace_detailed_step(
        &self,
        context: &str,
        operation: &str,
        input: &str,
        output: &str,
        step_type: &str,
    ) {
        if self.config.verbosity >= 3 {
            let step_num = {
                match self.step_counter.lock() {
                    Ok(mut counter) => {
                        *counter += 1;
                        *counter
                    }
                    Err(_) => {
                        // If mutex is poisoned, continue with a default value
                        eprintln!("Warning: Step counter mutex poisoned, using default");
                        0
                    }
                }
            };

            eprintln!(
                "    [Step {}:{}] {}: {} → {}",
                step_num,
                context,
                operation,
                truncate_for_display(input, 30),
                truncate_for_display(output, 30)
            );

            if step_type != "default" {
                eprintln!("      Type: {}", step_type);
            }
        }
    }

    pub fn trace_scope_inspection(&self, scope: &rhai::Scope) {
        if self.config.verbosity >= 3 {
            eprintln!("    Scope contents:");
            let mut scope_items: Vec<_> = scope.iter().collect();
            scope_items.sort_by(|a, b| a.0.cmp(b.0));
            for (name, _is_const, value) in scope_items {
                let type_info = value.type_name();
                let preview = format!("{:?}", value);
                eprintln!("      {} ({}): {}", name, type_info, preview);
            }
        }
    }

    pub fn trace_ast_node(&self, node_type: &str, position: &str, source: &str) {
        if self.config.verbosity >= 3 {
            eprintln!(
                "    AST: {} at {} → \"{}\"",
                node_type,
                position,
                truncate_for_display(source, 40)
            );
        }
    }

    pub fn next_event(&self) -> u64 {
        if let Ok(mut counter) = self.current_event.lock() {
            *counter += 1;
            *counter
        } else {
            0
        }
    }

    pub fn reset_step_counter(&self) {
        if let Ok(mut counter) = self.step_counter.lock() {
            *counter = 0;
        }
    }
}

impl Clone for ExecutionTracer {
    fn clone(&self) -> Self {
        ExecutionTracer {
            config: self.config.clone(),
            current_event: Arc::clone(&self.current_event),
            step_counter: Arc::clone(&self.step_counter),
        }
    }
}

// Performance and Statistics Tracking using thread-local storage
// This integrates with kelora's parallel processing infrastructure like track_count()

use std::cell::RefCell;

#[derive(Debug, Clone, Default)]
pub struct DebugStatistics {
    pub start_time: Option<std::time::Instant>,
    pub events_processed: u64,
    pub events_passed: u64,
    pub errors_encountered: u64,
    pub script_executions: u64,
}

impl DebugStatistics {
    pub fn new() -> Self {
        DebugStatistics {
            start_time: Some(std::time::Instant::now()),
            events_processed: 0,
            events_passed: 0,
            errors_encountered: 0,
            script_executions: 0,
        }
    }
}

// Thread-local storage for debug statistics (following track_count pattern)
thread_local! {
    static THREAD_DEBUG_STATS: RefCell<DebugStatistics> = RefCell::new(DebugStatistics::new());
}

// Debug statistics collection functions (following stats.rs pattern)
pub fn debug_stats_increment_events_processed() {
    THREAD_DEBUG_STATS.with(|stats| {
        stats.borrow_mut().events_processed += 1;
    });
}

pub fn debug_stats_increment_events_passed() {
    THREAD_DEBUG_STATS.with(|stats| {
        stats.borrow_mut().events_passed += 1;
    });
}

pub fn debug_stats_increment_errors() {
    THREAD_DEBUG_STATS.with(|stats| {
        stats.borrow_mut().errors_encountered += 1;
    });
}

pub fn debug_stats_increment_script_executions() {
    THREAD_DEBUG_STATS.with(|stats| {
        stats.borrow_mut().script_executions += 1;
    });
}

pub fn debug_stats_get_thread_state() -> DebugStatistics {
    THREAD_DEBUG_STATS.with(|stats| stats.borrow().clone())
}

pub fn debug_stats_set_thread_state(stats: &DebugStatistics) {
    THREAD_DEBUG_STATS.with(|local_stats| {
        *local_stats.borrow_mut() = stats.clone();
    });
}

/// Represents the type of access to a field in a Rhai expression
#[derive(Debug, Clone, PartialEq, Eq)]
enum AccessType {
    Read,      // Field is being read/accessed
    Write,     // Field is being assigned (LHS of simple assignment)
    ReadWrite, // Field is both read and written (compound assignments: +=, -=, etc.)
}

/// Represents a field access with its access type
#[derive(Debug, Clone)]
struct FieldAccess {
    field_name: String,
    access_type: AccessType,
}

#[derive(Clone)]
struct NativePredicate {
    root: NativeNode,
}

#[derive(Clone)]
enum NativeNode {
    And(Vec<NativeNode>),
    Or(Vec<NativeNode>),
    Not(Box<NativeNode>),
    Value(NativeValueExpr),
    Compare {
        op: CompareOp,
        lhs: NativeValueExpr,
        rhs: NativeValueExpr,
    },
    /// String method calls like e.field.contains("x")
    StringMethod {
        op: StringOp,
        /// The string value to operate on (e.g., e.message)
        target: NativeValueExpr,
        /// The argument (e.g., "error" in contains("error"))
        arg: NativeValueExpr,
    },
}

#[derive(Clone, Copy)]
enum StringOp {
    Contains,
    StartsWith,
    EndsWith,
}

#[derive(Clone)]
enum NativeValueExpr {
    Field(String),
    Literal(NativeValue),
}

#[derive(Clone, Copy)]
enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone)]
enum NativeValue {
    Bool(bool),
    Int(rhai::INT),
    Float(rhai::FLOAT),
    Str(String),
    Unit,
}

/// Extract field names accessed on variable 'e' from a Rhai AST
///
/// Uses AST walking to find all property access patterns like `e.field_name`
/// including chained method calls like `e.field.to_upper()`.
/// Distinguishes between field reads and writes to avoid false warnings.
///
/// Requires the 'debugging' feature which provides AST access.
fn extract_field_accesses(ast: &AST) -> Vec<FieldAccess> {
    use std::collections::HashMap;

    let mut accesses: HashMap<String, AccessType> = HashMap::new();

    ast.walk(&mut |path| {
        if let Some(node) = path.first() {
            let node_str = format!("{:?}", node);

            // Skip nodes that don't involve Variable(e)
            if !node_str.contains("Variable(e)") {
                return true;
            }

            // Check if this is an assignment statement
            if node_str.contains("Assignment(") {
                extract_assignment_fields(&node_str, &mut accesses);
            } else {
                // Not an assignment - all fields are reads
                extract_read_fields(&node_str, &mut accesses);
            }
        }
        true
    });

    // Convert HashMap to Vec<FieldAccess>
    accesses
        .into_iter()
        .map(|(field_name, access_type)| FieldAccess {
            field_name,
            access_type,
        })
        .collect()
}

/// Flags indicating which scope variables are used by an expression
#[derive(Clone, Copy, Default)]
struct VariableUsage {
    uses_meta: bool,
    uses_conf: bool,
    uses_line: bool,
}

/// Detect which scope variables (meta, conf, line) are used in the AST
fn detect_variable_usage(ast: &AST) -> VariableUsage {
    let mut usage = VariableUsage::default();

    ast.walk(&mut |path| {
        if let Some(node) = path.first() {
            let node_str = format!("{:?}", node);

            // Check for Variable references
            if node_str.contains("Variable(meta)") {
                usage.uses_meta = true;
            }
            if node_str.contains("Variable(conf)") {
                usage.uses_conf = true;
            }
            if node_str.contains("Variable(line)") {
                usage.uses_line = true;
            }
        }
        true
    });

    usage
}

fn build_native_predicate(ast: &AST) -> Option<NativePredicate> {
    let statements = ast.statements();
    if statements.len() != 1 {
        return None;
    }

    let expr = match &statements[0] {
        Stmt::Expr(expr) => expr.as_ref(),
        _ => return None,
    };

    let root = parse_native_node(expr)?;
    Some(NativePredicate { root })
}

fn parse_native_node(expr: &Expr) -> Option<NativeNode> {
    match expr {
        Expr::And(list, _) => {
            let mut nodes = Vec::with_capacity(list.len());
            for item in list.iter() {
                nodes.push(parse_native_node(item)?);
            }
            Some(NativeNode::And(nodes))
        }
        Expr::Or(list, _) => {
            let mut nodes = Vec::with_capacity(list.len());
            for item in list.iter() {
                nodes.push(parse_native_node(item)?);
            }
            Some(NativeNode::Or(nodes))
        }
        Expr::FnCall(call, _) => {
            if let Some(token) = call.op_token.clone() {
                match token {
                    Token::Bang if call.args.len() == 1 => {
                        let node = parse_native_node(&call.args[0])?;
                        Some(NativeNode::Not(Box::new(node)))
                    }
                    Token::EqualsTo => parse_compare_node(CompareOp::Eq, call),
                    Token::NotEqualsTo => parse_compare_node(CompareOp::Ne, call),
                    Token::LessThan => parse_compare_node(CompareOp::Lt, call),
                    Token::LessThanEqualsTo => parse_compare_node(CompareOp::Le, call),
                    Token::GreaterThan => parse_compare_node(CompareOp::Gt, call),
                    Token::GreaterThanEqualsTo => parse_compare_node(CompareOp::Ge, call),
                    _ => None,
                }
            } else {
                None
            }
        }
        Expr::BoolConstant(value, _) => Some(NativeNode::Value(NativeValueExpr::Literal(
            NativeValue::Bool(*value),
        ))),
        // Handle method calls like e.message.contains("error")
        Expr::Dot(binary, _, _) => parse_string_method(binary),
        _ => parse_value_expr(expr).map(NativeNode::Value),
    }
}

/// Parse string method calls like e.field.contains("pattern")
/// AST structure for `e.level.starts_with("ERR")`:
/// Dot { lhs: Variable(e), rhs: Dot { lhs: Property(level), rhs: MethodCall(...) } }
fn parse_string_method(binary: &BinaryExpr) -> Option<NativeNode> {
    // For e.field.method(arg), the structure is:
    // Dot { lhs: Variable(e), rhs: Dot { lhs: Property(field), rhs: MethodCall } }
    // We need to find the innermost Dot that has a MethodCall as rhs

    // Try to extract method call info by walking the Dot chain
    let (field_path, method_name, arg) = extract_method_call(binary)?;

    // Map method name to operation
    let op = match method_name.as_str() {
        "contains" => StringOp::Contains,
        "starts_with" => StringOp::StartsWith,
        "ends_with" => StringOp::EndsWith,
        _ => return None,
    };

    let target = NativeValueExpr::Field(field_path);
    Some(NativeNode::StringMethod { op, target, arg })
}

/// Extract method call info from a Dot expression chain
/// Returns (field_path, method_name, arg) if successful
fn extract_method_call(binary: &BinaryExpr) -> Option<(String, String, NativeValueExpr)> {
    // Check if this is a direct method call: Dot { lhs: field_expr, rhs: MethodCall }
    if let Expr::MethodCall(call, _) = &binary.rhs {
        if call.args.len() == 1 {
            let arg = parse_value_expr(&call.args[0])?;
            let field_path = extract_field_path(&binary.lhs)?;
            return Some((field_path, call.name.to_string(), arg));
        }
    }

    // Check if rhs is another Dot containing a MethodCall
    if let Expr::Dot(inner_binary, _, _) = &binary.rhs {
        // Recursively check the inner Dot
        if let Some((mut path, method, arg)) = extract_method_call(inner_binary) {
            // Prepend the lhs field to the path
            let lhs_path = extract_field_path(&binary.lhs)?;
            if path.is_empty() {
                path = lhs_path;
            } else if !lhs_path.is_empty() {
                path = format!("{}.{}", lhs_path, path);
            }
            return Some((path, method, arg));
        }
    }

    None
}

fn parse_compare_node(op: CompareOp, call: &FnCallExpr) -> Option<NativeNode> {
    if call.args.len() != 2 {
        return None;
    }

    let lhs = parse_value_expr(&call.args[0])?;
    let rhs = parse_value_expr(&call.args[1])?;

    Some(NativeNode::Compare { op, lhs, rhs })
}

fn parse_value_expr(expr: &Expr) -> Option<NativeValueExpr> {
    match expr {
        Expr::BoolConstant(value, _) => Some(NativeValueExpr::Literal(NativeValue::Bool(*value))),
        Expr::IntegerConstant(value, _) => Some(NativeValueExpr::Literal(NativeValue::Int(*value))),
        Expr::FloatConstant(value, _) => {
            Some(NativeValueExpr::Literal(NativeValue::Float(**value)))
        }
        Expr::StringConstant(value, _) => Some(NativeValueExpr::Literal(NativeValue::Str(
            value.to_string(),
        ))),
        Expr::Unit(..) => Some(NativeValueExpr::Literal(NativeValue::Unit)),
        _ => extract_field_path(expr)
            .filter(|path| !path.is_empty())
            .map(NativeValueExpr::Field),
    }
}

fn extract_field_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Dot(binary, options, _) if options.is_empty() => {
            let mut base = extract_field_path(&binary.lhs)?;
            let rhs = extract_field_path(&binary.rhs)?;
            if base.is_empty() {
                base = rhs;
            } else {
                base.push('.');
                base.push_str(&rhs);
            }
            Some(base)
        }
        Expr::Index(binary, options, _) if options.is_empty() => {
            let mut base = extract_field_path(&binary.lhs)?;
            let rhs = match &binary.rhs {
                Expr::StringConstant(s, _) => s.to_string(),
                Expr::IntegerConstant(i, _) => i.to_string(),
                _ => return None,
            };

            if !base.is_empty() {
                base.push('.');
            }
            base.push_str(&rhs);
            Some(base)
        }
        Expr::Property(prop, _) => Some(prop.2.to_string()),
        Expr::Variable(var, ..) => {
            let name = &var.1;
            if name == "e" {
                Some(String::new())
            } else {
                None
            }
        }
        _ => None,
    }
}

impl NativePredicate {
    fn evaluate(&self, event: &Event) -> Option<bool> {
        eval_native_node(&self.root, event)
    }
}

fn eval_native_node(node: &NativeNode, event: &Event) -> Option<bool> {
    match node {
        NativeNode::And(nodes) => {
            let mut result = true;
            for n in nodes {
                let value = eval_native_node(n, event)?;
                result &= value;
                if !result {
                    break;
                }
            }
            Some(result)
        }
        NativeNode::Or(nodes) => {
            let mut result = false;
            for n in nodes {
                let value = eval_native_node(n, event)?;
                result |= value;
                if result {
                    break;
                }
            }
            Some(result)
        }
        NativeNode::Not(child) => eval_native_node(child, event).map(|v| !v),
        NativeNode::Value(expr) => match eval_value_expr(expr, event)? {
            NativeValue::Bool(b) => Some(b),
            _ => None,
        },
        NativeNode::Compare { op, lhs, rhs } => {
            let lhs_val = eval_value_expr(lhs, event)?;
            let rhs_val = eval_value_expr(rhs, event)?;
            compare_values(&lhs_val, &rhs_val, *op)
        }
        NativeNode::StringMethod { op, target, arg } => {
            let target_val = eval_value_expr(target, event)?;
            let arg_val = eval_value_expr(arg, event)?;

            // Both must be strings
            let (target_str, arg_str) = match (&target_val, &arg_val) {
                (NativeValue::Str(t), NativeValue::Str(a)) => (t.as_str(), a.as_str()),
                _ => return None,
            };

            let result = match op {
                StringOp::Contains => target_str.contains(arg_str),
                StringOp::StartsWith => target_str.starts_with(arg_str),
                StringOp::EndsWith => target_str.ends_with(arg_str),
            };
            Some(result)
        }
    }
}

fn eval_value_expr(expr: &NativeValueExpr, event: &Event) -> Option<NativeValue> {
    match expr {
        NativeValueExpr::Literal(value) => Some(value.clone()),
        NativeValueExpr::Field(path) => {
            let value = event.fields.get(path).cloned().unwrap_or(Dynamic::UNIT);
            dynamic_to_native(&value)
        }
    }
}

fn dynamic_to_native(value: &Dynamic) -> Option<NativeValue> {
    if value.is_unit() {
        return Some(NativeValue::Unit);
    }

    if let Ok(b) = value.as_bool() {
        return Some(NativeValue::Bool(b));
    }

    if let Ok(i) = value.as_int() {
        return Some(NativeValue::Int(i));
    }

    if let Ok(f) = value.as_float() {
        return Some(NativeValue::Float(f));
    }

    if let Some(s) = value.clone().try_cast::<rhai::ImmutableString>() {
        return Some(NativeValue::Str(s.to_string()));
    }

    if let Some(s) = value.clone().try_cast::<String>() {
        return Some(NativeValue::Str(s));
    }

    None
}

fn compare_values(lhs: &NativeValue, rhs: &NativeValue, op: CompareOp) -> Option<bool> {
    use CompareOp::*;

    match (lhs, rhs) {
        (NativeValue::Unit, NativeValue::Unit) => match op {
            Eq => Some(true),
            Ne => Some(false),
            _ => Some(false),
        },
        (NativeValue::Unit, _) | (_, NativeValue::Unit) => match op {
            Eq => Some(false),
            Ne => Some(true),
            _ => None,
        },
        (NativeValue::Bool(l), NativeValue::Bool(r)) => match op {
            Eq => Some(l == r),
            Ne => Some(l != r),
            _ => None,
        },
        (NativeValue::Str(l), NativeValue::Str(r)) => match op {
            Eq => Some(l == r),
            Ne => Some(l != r),
            _ => None,
        },
        (NativeValue::Int(l), NativeValue::Int(r)) => {
            compare_numbers(*l as rhai::FLOAT, *r as rhai::FLOAT, op)
        }
        (NativeValue::Float(l), NativeValue::Float(r)) => compare_numbers(*l, *r, op),
        (NativeValue::Int(l), NativeValue::Float(r)) => compare_numbers(*l as rhai::FLOAT, *r, op),
        (NativeValue::Float(l), NativeValue::Int(r)) => compare_numbers(*l, *r as rhai::FLOAT, op),
        _ => None,
    }
}

fn compare_numbers(lhs: rhai::FLOAT, rhs: rhai::FLOAT, op: CompareOp) -> Option<bool> {
    use CompareOp::*;
    match op {
        Eq => Some(lhs == rhs),
        Ne => Some(lhs != rhs),
        Lt => Some(lhs < rhs),
        Le => Some(lhs <= rhs),
        Gt => Some(lhs > rhs),
        Ge => Some(lhs >= rhs),
    }
}

/// Extract field accesses from assignment statements
fn extract_assignment_fields(
    node_str: &str,
    accesses: &mut std::collections::HashMap<String, AccessType>,
) {
    use AccessType::*;

    // Determine if this is a compound assignment (+=, -=, *=, etc.)
    let is_compound = node_str.contains("PlusAssign")
        || node_str.contains("MinusAssign")
        || node_str.contains("MultiplyAssign")
        || node_str.contains("DivideAssign")
        || node_str.contains("ModuloAssign")
        || node_str.contains("PowerOfAssign")
        || node_str.contains("ShiftLeftAssign")
        || node_str.contains("ShiftRightAssign")
        || node_str.contains("AndAssign")
        || node_str.contains("OrAssign")
        || node_str.contains("XOrAssign");

    // For assignments, Rhai uses: Stmt(Assignment((op, BinaryExpr { lhs: ..., rhs: ... })))
    // Find the BinaryExpr within the assignment
    if let Some(binary_start) = node_str.find("BinaryExpr {") {
        let binary_section = &node_str[binary_start..];

        // Extract LHS fields (target of assignment)
        if let Some(lhs_start) = binary_section.find("lhs:") {
            // Find the end of the lhs section (before "rhs:")
            let lhs_section = if let Some(rhs_pos) = binary_section[lhs_start..].find(", rhs:") {
                &binary_section[lhs_start..lhs_start + rhs_pos]
            } else {
                &binary_section[lhs_start..]
            };

            let lhs_fields = extract_fields_from_section(lhs_section);
            for field in lhs_fields {
                if is_compound {
                    // Compound assignment: field is both read and written
                    merge_access_type(accesses, field, ReadWrite);
                } else {
                    // Regular assignment: field is only written
                    merge_access_type(accesses, field, Write);
                }
            }
        }

        // Extract RHS fields (value being assigned)
        if let Some(rhs_start) = binary_section.find("rhs:") {
            let rhs_section = &binary_section[rhs_start..];
            let rhs_fields = extract_fields_from_section(rhs_section);

            for field in rhs_fields {
                // RHS fields are always reads
                merge_access_type(accesses, field, Read);
            }
        }
    }
}

/// Extract field accesses from non-assignment contexts (all reads)
fn extract_read_fields(
    node_str: &str,
    accesses: &mut std::collections::HashMap<String, AccessType>,
) {
    // Non-assignment context - all fields are reads
    let fields = extract_fields_from_section(node_str);
    for field in fields {
        merge_access_type(accesses, field, AccessType::Read);
    }
}

/// Helper function to extract field names from AST node section
fn extract_fields_from_section(section: &str) -> Vec<String> {
    let mut fields = Vec::new();

    // Pattern 1: Direct property access - Variable(e) ... Property(field_name)
    if let Ok(re) = regex::Regex::new(r"Variable\(e\)[^}]*Property\((\w+)\)") {
        for cap in re.captures_iter(section) {
            if let Some(field_name) = cap.get(1) {
                fields.push(field_name.as_str().to_string());
            }
        }
    }

    // Pattern 2: Nested case for method calls - rhs: Dot { lhs: Property(field)
    if section.contains("lhs: Variable(e)") {
        if let Ok(nested_re) = regex::Regex::new(r"rhs: Dot \{ lhs: Property\((\w+)\)") {
            for cap in nested_re.captures_iter(section) {
                if let Some(field_name) = cap.get(1) {
                    fields.push(field_name.as_str().to_string());
                }
            }
        }
    }

    fields
}

/// Merge access types for a field, upgrading to ReadWrite if accessed both ways
fn merge_access_type(
    accesses: &mut std::collections::HashMap<String, AccessType>,
    field: String,
    new_type: AccessType,
) {
    use AccessType::*;

    let current = accesses.entry(field.clone()).or_insert(new_type.clone());

    // Merge logic: if a field is both read and written, mark as ReadWrite
    *current = match (&*current, &new_type) {
        (Read, Write) | (Write, Read) => ReadWrite,
        (Read, ReadWrite) | (ReadWrite, Read) => ReadWrite,
        (Write, ReadWrite) | (ReadWrite, Write) => ReadWrite,
        (ReadWrite, ReadWrite) => ReadWrite,
        _ => new_type,
    };
}

#[derive(Clone)]
pub struct CompiledExpression {
    ast: AST,
    expr: String,
    field_accesses: Vec<FieldAccess>,
    native_predicate: Option<NativePredicate>,
    /// Whether this expression uses the `meta` variable
    uses_meta: bool,
    /// Whether this expression uses the `conf` variable
    uses_conf: bool,
    /// Whether this expression uses the `line` variable
    uses_line: bool,
}

impl CompiledExpression {
    /// Get the source expression
    pub fn source(&self) -> &str {
        &self.expr
    }

    /// Get fields that are READ (including ReadWrite, excluding pure Write)
    pub fn read_fields(&self) -> std::collections::HashSet<String> {
        self.field_accesses
            .iter()
            .filter(|fa| matches!(fa.access_type, AccessType::Read | AccessType::ReadWrite))
            .map(|fa| fa.field_name.clone())
            .collect()
    }

    /// Get fields that are WRITTEN (including ReadWrite, excluding pure Read)
    pub fn written_fields(&self) -> std::collections::HashSet<String> {
        self.field_accesses
            .iter()
            .filter(|fa| matches!(fa.access_type, AccessType::Write | AccessType::ReadWrite))
            .map(|fa| fa.field_name.clone())
            .collect()
    }

    /// Get all accessed fields (backward compatibility)
    pub fn accessed_fields(&self) -> std::collections::HashSet<String> {
        self.field_accesses
            .iter()
            .map(|fa| fa.field_name.clone())
            .collect()
    }
}

pub struct RhaiEngine {
    engine: Engine,
    compiled_filters: Vec<CompiledExpression>,
    compiled_execs: Vec<CompiledExpression>,
    compiled_begin: Option<CompiledExpression>,
    compiled_end: Option<CompiledExpression>,
    scope_template: Scope<'static>,
    suppress_side_effects: bool,
    conf_map: Option<rhai::Map>,
    state_map: Option<crate::rhai_functions::state::StateMap>,
    state_available: bool,
    debug_tracker: Option<DebugTracker>,
    execution_tracer: Option<ExecutionTracer>,
    use_emoji: bool,
}

impl Clone for RhaiEngine {
    fn clone(&self) -> Self {
        let mut engine = Engine::new();
        // Use Simple optimization, not Full. Full optimization breaks side-effect functions
        // like track_count("key"), print("msg"), emit_each(), etc. by trying to evaluate them at
        // compile time when their arguments are constants. These functions MUST run at runtime.
        engine.set_optimization_level(rhai::OptimizationLevel::Simple);

        // Check for shutdown signal during script execution (cooperative cancellation)
        engine.on_progress(|_| {
            if crate::platform::SHOULD_TERMINATE.load(Ordering::Relaxed) {
                // Abort script execution by returning a termination sentinel
                Some(rhai::Dynamic::UNIT)
            } else {
                None
            }
        });

        // Apply the same on_print override as in new(), respecting suppress_side_effects
        let suppress_side_effects = self.suppress_side_effects;
        engine.on_print(move |text| {
            if suppress_side_effects {
                // Suppress all print output
                return;
            }

            if crate::rhai_functions::strings::is_parallel_mode() {
                crate::rhai_functions::strings::capture_print(text.to_string());
            } else {
                println!("{}", text);
            }
        });

        rhai_functions::register_all_functions(&mut engine);

        Self {
            engine,
            compiled_filters: self.compiled_filters.clone(),
            compiled_execs: self.compiled_execs.clone(),
            compiled_begin: self.compiled_begin.clone(),
            compiled_end: self.compiled_end.clone(),
            scope_template: self.scope_template.clone(),
            suppress_side_effects,
            conf_map: self.conf_map.clone(),
            state_map: self.state_map.clone(),
            state_available: self.state_available,
            debug_tracker: self.debug_tracker.clone(),
            execution_tracer: self.execution_tracer.clone(),
            use_emoji: self.use_emoji,
        }
    }
}

impl RhaiEngine {
    /// Render a short diagnostic with stage/name, position, snippet, and the raw Rhai message.
    fn format_rhai_diagnostic(
        err: Box<EvalAltResult>,
        stage: &str,
        script_name: &str,
        script_text: &str,
        scope: Option<&Scope>,
        debug_tracker: Option<&DebugTracker>,
        use_emoji: bool,
    ) -> String {
        let call_stack = Self::collect_call_stack(err.as_ref());
        let err_display = format!("{}", err);

        if let Some(tracker) = debug_tracker {
            let enhancer = ErrorEnhancer::new(tracker.config.clone());
            let context = tracker.get_context();
            if let Some(scope) = scope {
                return enhancer.enhance_error(&err, scope, script_text, stage, &context);
            }
        }

        // Basic header
        let mut output = String::new();
        output.push_str(&format!("{} error\n", stage));

        // Position + snippet
        let pos = err.position();
        if let Some(line_num) = pos.line() {
            let col_num = pos.position().unwrap_or(1);
            output.push_str(&format!(
                "  At {}:{} in {}\n",
                line_num, col_num, script_name
            ));
            if let Some(snippet) = Self::render_snippet(
                script_text,
                line_num.saturating_sub(1),
                col_num.saturating_sub(1),
            ) {
                output.push_str(&snippet);
            }
        } else if pos.is_none() {
            output.push_str(&format!("  In {}\n", script_name));
        } else {
            output.push_str(&format!("  At {} in {}\n", pos, script_name));
        }

        // Raw message from Rhai
        output.push_str(&format!("  Rhai: {}\n", err_display));

        if !call_stack.is_empty() {
            output.push_str("  Call stack (most recent first):\n");
            for (func, pos) in call_stack.iter().rev().take(3) {
                output.push_str(&format!("    • {} @ {}\n", func, pos));
            }
        }

        // Generate suggestions even without debug mode (helps users fix errors)
        let config = DebugConfig::new(0); // Minimal config for suggestion generation
        let enhancer = ErrorEnhancer::new(config);
        let suggestion = if let Some(scope) = scope {
            enhancer.generate_suggestions(&err, scope, Some(script_text))
        } else {
            ErrorEnhancer::raw_string_hint(&err, script_text)
        };
        if let Some(suggestion) = suggestion {
            if use_emoji {
                output.push_str(&format!("  💡 {}\n", suggestion));
            } else {
                output.push_str(&format!("  Hint: {}\n", suggestion));
            }
        }

        output
    }

    /// Collect nested function call frames from Rhai errors.
    fn collect_call_stack(err: &EvalAltResult) -> Vec<(String, rhai::Position)> {
        match err {
            EvalAltResult::ErrorInFunctionCall(func, _src, inner, pos) => {
                let mut frames = vec![(func.clone(), *pos)];
                frames.extend(Self::collect_call_stack(inner.as_ref()));
                frames
            }
            EvalAltResult::ErrorInModule(module, inner, pos) => {
                let mut frames = vec![(format!("module {}", module), *pos)];
                frames.extend(Self::collect_call_stack(inner.as_ref()));
                frames
            }
            _ => Vec::new(),
        }
    }

    /// Build a small two-line snippet with a caret under the offending column.
    fn render_snippet(
        script: &str,
        zero_based_line: usize,
        zero_based_col: usize,
    ) -> Option<String> {
        let lines: Vec<&str> = script.lines().collect();
        let line_content = lines.get(zero_based_line)?.trim_end_matches('\r');
        let line_num = zero_based_line + 1;
        let col_num = zero_based_col + 1;
        let gutter_width = line_num.to_string().len();
        let mut snippet = String::new();
        snippet.push_str(&format!(
            "  {line_num:>width$} | {line_content}\n",
            width = gutter_width
        ));
        let caret_padding = " ".repeat(col_num.saturating_sub(1));
        snippet.push_str(&format!(
            "  {empty:>width$} | {caret_padding}^\n",
            empty = "",
            width = gutter_width
        ));
        Some(snippet)
    }

    // Thread-local state management functions
    pub fn set_thread_tracking_state(
        metrics: &HashMap<String, Dynamic>,
        internal: &HashMap<String, Dynamic>,
    ) {
        rhai_functions::tracking::set_thread_tracking_state(metrics);
        rhai_functions::tracking::set_thread_internal_state(internal);
    }

    pub fn get_thread_tracking_state() -> HashMap<String, Dynamic> {
        rhai_functions::tracking::get_thread_tracking_state()
    }

    pub fn get_thread_internal_state() -> HashMap<String, Dynamic> {
        rhai_functions::tracking::get_thread_internal_state()
    }

    fn format_rhai_error(err: Box<EvalAltResult>, script_name: &str, _script_text: &str) -> String {
        match *err {
            EvalAltResult::ErrorParsing(parse_err, pos) => {
                format!("Syntax error in {} at {}: {}", script_name, pos, parse_err)
            }
            EvalAltResult::ErrorRuntime(runtime_err, pos) => {
                format!(
                    "Runtime error in {} at {}: {}",
                    script_name, pos, runtime_err
                )
            }
            EvalAltResult::ErrorVariableNotFound(var, pos) => {
                format!("Variable '{}' not found in {} at {}", var, script_name, pos)
            }
            EvalAltResult::ErrorFunctionNotFound(func, pos) => {
                Self::format_function_not_found_error(func, script_name, pos)
            }
            EvalAltResult::ErrorMismatchDataType(expected, actual, pos) => {
                format!("Type mismatch in {} at {}: expected {}, got {} (this often indicates a function was called with incorrect argument types)", 
                        script_name, pos, expected, actual)
            }
            EvalAltResult::ErrorInFunctionCall(func, _source, inner_err, pos) => {
                let inner_msg = Self::format_rhai_error(inner_err, "function", "");
                format!(
                    "Error in function '{}' in {} at {}: {}",
                    func, script_name, pos, inner_msg
                )
            }
            EvalAltResult::ErrorPropertyNotFound(prop, pos) => {
                format!(
                    "Property '{}' not found in {} at {}",
                    prop, script_name, pos
                )
            }
            EvalAltResult::ErrorIndexNotFound(index, pos) => {
                format!("Index '{}' not found in {} at {}", index, script_name, pos)
            }
            EvalAltResult::ErrorDotExpr(msg, pos) => {
                format!(
                    "Property access error in {} at {}: {}",
                    script_name, pos, msg
                )
            }
            EvalAltResult::ErrorArithmetic(msg, pos) => {
                format!("Arithmetic error in {} at {}: {}", script_name, pos, msg)
            }
            EvalAltResult::ErrorTooManyOperations(pos) => {
                format!("Too many operations in {} at {}", script_name, pos)
            }
            EvalAltResult::ErrorStackOverflow(pos) => {
                format!("Stack overflow in {} at {}", script_name, pos)
            }
            EvalAltResult::ErrorDataTooLarge(msg, pos) => {
                format!("Data too large in {} at {}: {}", script_name, pos, msg)
            }
            EvalAltResult::ErrorTerminated(val, pos) => {
                format!("Script terminated in {} at {}: {}", script_name, pos, val)
            }
            _ => format!("Error in {}: {}", script_name, err),
        }
    }

    fn format_function_not_found_error(
        func_signature: String,
        script_name: &str,
        pos: rhai::Position,
    ) -> String {
        // Extract function name from signature (before the first '(' or space)
        let func_name = if let Some(paren_pos) = func_signature.find('(') {
            &func_signature[..paren_pos]
        } else if let Some(space_pos) = func_signature.find(' ') {
            &func_signature[..space_pos]
        } else {
            &func_signature
        }
        .trim();

        // Check if this looks like a type mismatch rather than a missing function
        let called_types = Self::extract_called_types(&func_signature);
        if Self::is_likely_type_mismatch(&func_signature, func_name) {
            let expected_types = Self::get_expected_function_signature(func_name);
            if !expected_types.is_empty() {
                return format!(
                    "Wrong argument types for '{}' in {} at {}: got {}, expected {}. Note: x.{}() = {}(x)",
                    func_name,
                    script_name,
                    pos,
                    called_types,
                    expected_types,
                    func_name,
                    func_name
                );
            }
        }

        // Fall back to "function not found" with suggestions
        let base_msg = format!(
            "Function '{}' not found in {} at {}",
            func_signature, script_name, pos
        );
        let suggestions = Self::get_function_suggestions(func_name);
        let mut notes = Vec::new();

        if called_types != "unknown types" {
            notes.push(format!("Called with: {}", called_types));
        }

        if Self::signature_has_unit(&called_types) {
            notes.push(
                "One of the arguments is '()' (missing field?). Use e.has(\"field\") or e.get(\"field\", default) before chaining."
                    .to_string(),
            );
        }

        if suggestions.is_empty() {
            notes.push(format!(
                "Note: method calls are sugar—x.{}(y) == {}(x, y)",
                func_name, func_name
            ));
            format!("{}. {}", base_msg, notes.join(" "))
        } else {
            let mut msg = format!("{}. Did you mean: {}", base_msg, suggestions.join(", "));
            if !notes.is_empty() {
                msg.push_str(&format!(" {}", notes.join(" ")));
            }
            msg
        }
    }

    fn is_likely_type_mismatch(func_signature: &str, func_name: &str) -> bool {
        !Self::get_expected_function_signature(func_name).is_empty() && func_signature.contains('(')
    }

    fn extract_called_types(func_signature: &str) -> String {
        if let Some(start) = func_signature.find('(') {
            if let Some(end) = func_signature.rfind(')') {
                return func_signature[start + 1..end].to_string();
            }
        }
        "unknown types".to_string()
    }

    fn signature_has_unit(called_types: &str) -> bool {
        called_types.contains("()")
    }

    fn get_expected_function_signature(func_name: &str) -> String {
        match func_name {
            "extract_regex" => "string, regex_pattern, optional_group_index".to_string(),
            "extract_regexes" => "string, regex_pattern, optional_group_index".to_string(),
            "split_re" => "string, regex_pattern".to_string(),
            "replace_re" => "string, regex_pattern, replacement".to_string(),
            "before" | "after" => "string, delimiter".to_string(),
            "between" => "string, start_delimiter, end_delimiter".to_string(),
            "starting_with" | "ending_with" => "string, prefix_or_suffix".to_string(),
            "strip" => "string, optional_characters_to_strip".to_string(),
            "join" => "string_separator, array OR array, string_separator".to_string(),
            "extract_ip" | "extract_ips" | "extract_url" | "extract_domain" => "string".to_string(),
            "mask_ip" => "string, optional_octets_to_mask".to_string(),
            "is_private_ip" | "is_digit" => "string".to_string(),
            "parse_json" => "json_string".to_string(),
            "parse_kv" => "string, optional_separator, optional_kv_separator".to_string(),
            "col" => "string, column_selector".to_string(),
            "cols" => "string, column_selectors...".to_string(),
            "status_class" => "status_code_number".to_string(),
            "track_count" => "key".to_string(),
            "track_sum" | "track_min" | "track_max" | "track_avg" => "key, value".to_string(),
            "track_unique" => "key, value".to_string(),
            "track_bucket" => "key, value, bucket_size".to_string(),
            "count" => "string, substring".to_string(),
            // Common string functions that expect strings
            "len" | "trim" => "string".to_string(),
            "contains" | "starts_with" | "ends_with" => "string, substring".to_string(),
            "split" | "replace" => "string, delimiter_or_pattern, optional_replacement".to_string(),
            _ => "".to_string(),
        }
    }

    fn function_catalog() -> Vec<String> {
        // List of common Rhai built-in functions and our custom functions
        vec![
            // String functions
            "lower".to_string(),
            "upper".to_string(),
            "trim".to_string(),
            "len".to_string(),
            "contains".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
            "split".to_string(),
            "replace".to_string(),
            "substring".to_string(),
            "to_string".to_string(),
            "parse".to_string(),
            // Our custom string functions
            "extract_regex".to_string(),
            "extract_regexes".to_string(),
            "split_re".to_string(),
            "replace_re".to_string(),
            "count".to_string(),
            "strip".to_string(),
            "before".to_string(),
            "after".to_string(),
            "between".to_string(),
            "starting_with".to_string(),
            "ending_with".to_string(),
            "is_digit".to_string(),
            "join".to_string(),
            "extract_ip".to_string(),
            "extract_ips".to_string(),
            "mask_ip".to_string(),
            "is_private_ip".to_string(),
            "extract_url".to_string(),
            "extract_domain".to_string(),
            // Math functions
            "abs".to_string(),
            "floor".to_string(),
            "ceil".to_string(),
            "round".to_string(),
            "min".to_string(),
            "max".to_string(),
            "pow".to_string(),
            "sqrt".to_string(),
            // Array functions
            "push".to_string(),
            "pop".to_string(),
            "shift".to_string(),
            "unshift".to_string(),
            "reverse".to_string(),
            "sort".to_string(),
            "clear".to_string(),
            // Map functions
            "keys".to_string(),
            "values".to_string(),
            "remove".to_string(),
            "contains".to_string(),
            // Our custom functions
            "parse_json".to_string(),
            "parse_kv".to_string(),
            "col".to_string(),
            "cols".to_string(),
            "status_class".to_string(),
            "track_count".to_string(),
            "track_sum".to_string(),
            "track_min".to_string(),
            "track_max".to_string(),
            "track_avg".to_string(),
            "track_unique".to_string(),
            "track_bucket".to_string(),
            // Utility functions
            "print".to_string(),
            "debug".to_string(),
            "type_of".to_string(),
            "is_def_fn".to_string(),
        ]
    }

    fn get_function_suggestions(func_name: &str) -> Vec<String> {
        let available_functions = Self::function_catalog();

        // Find functions that are similar to the requested one
        let suggestions: Vec<String> = available_functions
            .iter()
            .filter(|&f| {
                // Check for starts with or contains
                f.starts_with(func_name) || (func_name.len() > 1 && f.contains(func_name))
            })
            .take(3) // Limit to 3 suggestions
            .map(|s| s.to_string())
            .collect();

        // For debugging: always include at least one suggestion if the function name contains common patterns
        if suggestions.is_empty() && func_name.len() > 2 {
            if func_name.contains("extract") {
                return vec![
                    "extract_regex".to_string(),
                    "extract_ip".to_string(),
                    "extract_url".to_string(),
                ];
            } else if func_name.contains("track") {
                return vec![
                    "track_count".to_string(),
                    "track_sum".to_string(),
                    "track_unique".to_string(),
                ];
            } else if func_name.contains("pars") {
                return vec!["parse_json".to_string(), "parse_kv".to_string()];
            }
        }

        suggestions
    }

    pub fn new() -> Self {
        let mut engine = Engine::new();

        // Use Simple optimization, not Full. Full optimization breaks side-effect functions
        // like track_count("key"), print("msg"), emit_each(), etc. by trying to evaluate them at
        // compile time when their arguments are constants. These functions MUST run at runtime.
        engine.set_optimization_level(rhai::OptimizationLevel::Simple);

        // Check for shutdown signal during script execution (cooperative cancellation)
        engine.on_progress(|_| {
            if crate::platform::SHOULD_TERMINATE.load(Ordering::Relaxed) {
                // Abort script execution by returning a termination sentinel
                Some(rhai::Dynamic::UNIT)
            } else {
                None
            }
        });

        // Override the built-in print function to support capture in parallel mode
        // Note: suppress_side_effects is false by default in new()
        engine.on_print(|text| {
            if crate::rhai_functions::strings::is_parallel_mode() {
                // Use both old capture system (for compatibility) and new ordered system
                crate::rhai_functions::strings::capture_print(text.to_string());
                crate::rhai_functions::strings::capture_stdout(text.to_string());
            } else {
                println!("{}", text);
            }
        });

        // Register custom functions for log analysis (includes eprint() for stderr output)
        rhai_functions::register_all_functions(&mut engine);

        // Register variable access callback for tracking functions
        Self::register_variable_resolver(&mut engine);

        let mut scope_template = Scope::new();
        scope_template.push("line", "");
        scope_template.push("e", rhai::Map::new());
        scope_template.push("meta", rhai::Map::new());
        scope_template.push("conf", rhai::Map::new());

        Self {
            engine,
            compiled_filters: Vec::new(),
            compiled_execs: Vec::new(),
            compiled_begin: None,
            compiled_end: None,
            scope_template,
            suppress_side_effects: false,
            conf_map: None,
            state_map: Some(crate::rhai_functions::state::StateMap::new()),
            state_available: true,
            debug_tracker: None,
            execution_tracer: None,
            use_emoji: true,
        }
    }

    pub fn set_use_emoji(&mut self, use_emoji: bool) {
        self.use_emoji = use_emoji;
    }

    pub fn get_execution_tracer(&self) -> &Option<ExecutionTracer> {
        &self.execution_tracer
    }

    /// Get the compiled filters (for benchmarking)
    pub fn compiled_filters(&self) -> &[CompiledExpression] {
        &self.compiled_filters
    }

    pub fn set_state_available(&mut self, available: bool) {
        self.state_available = available;
    }

    fn push_state_to_scope(&self, scope: &mut Scope) {
        if !self.state_available || crate::rhai_functions::strings::is_parallel_mode() {
            scope.push("state", crate::rhai_functions::state::StateNotAvailable);
        } else if let Some(ref state_map) = self.state_map {
            scope.push("state", state_map.clone());
        }
    }

    /// Set up debugging with the provided configuration
    pub fn setup_debugging(&mut self, debug_config: DebugConfig) {
        if !debug_config.is_enabled() {
            return;
        }

        self.debug_tracker = Some(DebugTracker::new(debug_config.clone()));
        self.execution_tracer = Some(ExecutionTracer::new(debug_config.clone()));

        // These unwraps are safe because we just created the debug components above
        let debug_tracker = self
            .debug_tracker
            .as_ref()
            .expect("debug_tracker should be initialized")
            .clone();
        let execution_tracer = self
            .execution_tracer
            .as_ref()
            .expect("execution_tracer should be initialized")
            .clone();
        // Allow deprecated API: register_debugger is marked as volatile/experimental but is the
        // only way to access Rhai's debugging functionality. The API is stable in practice and
        // essential for our debugging features. We'll update when a stable replacement is available.
        #[allow(deprecated)]
        self.engine.register_debugger(
            move |_engine, debugger| {
                // Set up breakpoint-based tracing for enhanced debugging
                if debug_config.trace_events {
                    // Enable step-by-step debugging mode for detailed tracing
                    // Note: Specific breakpoint methods may not be available in current Rhai version
                    // The debugger will still trigger Step events for detailed tracing
                }
                debugger
            },
            move |_context, event, node, source, pos| {
                // Update execution context
                debug_tracker.update_context(Some(pos), source);

                // Enhanced event logging with verbosity levels
                match event {
                    DebuggerEvent::Start => {
                        debug_tracker.log_basic("Script execution started");
                        if debug_tracker.config.verbosity >= 3 {
                            if let Some(src) = source {
                                debug_tracker
                                    .log_step("Starting script", &format!("\"{}\"", src.trim()));
                            }
                        }
                    }
                    DebuggerEvent::End => {
                        debug_tracker.log_basic("Script execution completed");
                    }
                    DebuggerEvent::Step => {
                        // Enhanced step-by-step tracing
                        if debug_tracker.config.verbosity >= 2 {
                            // Use execution tracer for step-level tracing
                            let step_info = format!("Step at {}", pos);
                            if let Some(src) = source {
                                execution_tracer.trace_step(0, &step_info, src);
                            } else {
                                execution_tracer.trace_step(0, &step_info, "unknown");
                            }
                        }

                        if debug_tracker.config.verbosity >= 3 {
                            // Even more detailed tracing for -vvv
                            let step_info = format!("Step at {}", pos);
                            let node_info = format!("{:?}", node);
                            debug_tracker.log_step(&step_info, &node_info);

                            // Use execution tracer for expression-level details
                            if let Some(src) = source {
                                execution_tracer.trace_expression_evaluation(src, "evaluating");
                            }
                        }
                    }
                    DebuggerEvent::BreakPoint(_) => {
                        if debug_tracker.config.verbosity >= 2 {
                            debug_tracker.log_detailed("breakpoint", 0, &format!("hit at {}", pos));
                            // Use execution tracer for breakpoint details
                            if let Some(src) = source {
                                execution_tracer.trace_step(0, "Breakpoint hit", src);
                            }
                        }
                    }
                    // Note: Specific function call events may not be available in current Rhai version
                    // We handle function call tracing through Step events and other mechanisms
                    _ => {
                        // Enhanced event tracing
                        if debug_tracker.config.verbosity >= 3 {
                            let event_name = format!("{:?}", event);
                            debug_tracker.log_step("Debug event", &event_name);

                            // Use execution tracer for detailed event tracking
                            if let Some(src) = source {
                                execution_tracer.trace_step(0, &event_name, src);
                            }
                        }
                    }
                }

                // Update execution context with more details
                if debug_tracker.config.verbosity >= 2 {
                    if let Ok(mut ctx) = debug_tracker.context.lock() {
                        ctx.last_operation = Some(format!("{:?}", event));
                        if let Some(src) = source {
                            ctx.source_snippet = Some(src.to_string());
                        }
                    }
                }

                Ok(DebuggerCommand::Continue)
            },
        );
    }

    /// Set whether to suppress side effects (print, eprint, etc.)
    pub fn set_suppress_side_effects(&mut self, suppress: bool) {
        self.suppress_side_effects = suppress;

        // Set the thread-local flag for eprint and other functions
        crate::rhai_functions::strings::set_suppress_side_effects(suppress);

        // Re-register the print handler with the new suppression setting
        let suppress_copy = suppress;
        self.engine.on_print(move |text| {
            if suppress_copy {
                // Suppress all print output
                return;
            }

            if crate::rhai_functions::strings::is_parallel_mode() {
                crate::rhai_functions::strings::capture_print(text.to_string());
            } else {
                println!("{}", text);
            }
        });
    }

    // Individual compilation methods for pipeline stages
    pub fn compile_filter(&mut self, filter: &str) -> Result<CompiledExpression> {
        let ast = self.engine.compile_expression(filter).map_err(|e| {
            let msg = Self::format_rhai_diagnostic(
                e.into(),
                "filter compilation",
                "filter expression",
                filter,
                None,
                None,
                self.use_emoji,
            );
            anyhow::anyhow!(msg)
        })?;
        let native_predicate = build_native_predicate(&ast);
        let field_accesses = extract_field_accesses(&ast);
        let var_usage = detect_variable_usage(&ast);
        Ok(CompiledExpression {
            ast,
            expr: filter.to_string(),
            field_accesses,
            native_predicate,
            uses_meta: var_usage.uses_meta,
            uses_conf: var_usage.uses_conf,
            uses_line: var_usage.uses_line,
        })
    }

    pub fn compile_exec(&mut self, exec: &str) -> Result<CompiledExpression> {
        let ast = self.engine.compile(exec).map_err(|e| {
            let msg = Self::format_rhai_diagnostic(
                e.into(),
                "exec compilation",
                "exec script",
                exec,
                None,
                None,
                self.use_emoji,
            );
            anyhow::anyhow!(msg)
        })?;
        let field_accesses = extract_field_accesses(&ast);
        let var_usage = detect_variable_usage(&ast);
        Ok(CompiledExpression {
            ast,
            expr: exec.to_string(),
            field_accesses,
            native_predicate: None,
            uses_meta: var_usage.uses_meta,
            uses_conf: var_usage.uses_conf,
            uses_line: var_usage.uses_line,
        })
    }

    pub fn compile_begin(&mut self, begin: &str) -> Result<CompiledExpression> {
        let ast = self.engine.compile(begin).map_err(|e| {
            let msg = Self::format_rhai_diagnostic(
                e.into(),
                "begin compilation",
                "begin script",
                begin,
                None,
                None,
                self.use_emoji,
            );
            anyhow::anyhow!(msg)
        })?;
        let field_accesses = extract_field_accesses(&ast);
        let var_usage = detect_variable_usage(&ast);
        Ok(CompiledExpression {
            ast,
            expr: begin.to_string(),
            field_accesses,
            native_predicate: None,
            uses_meta: var_usage.uses_meta,
            uses_conf: var_usage.uses_conf,
            uses_line: var_usage.uses_line,
        })
    }

    pub fn compile_end(&mut self, end: &str) -> Result<CompiledExpression> {
        let ast = self.engine.compile(end).map_err(|e| {
            let msg = Self::format_rhai_diagnostic(
                e.into(),
                "end compilation",
                "end script",
                end,
                None,
                None,
                self.use_emoji,
            );
            anyhow::anyhow!(msg)
        })?;
        let field_accesses = extract_field_accesses(&ast);
        let var_usage = detect_variable_usage(&ast);
        Ok(CompiledExpression {
            ast,
            expr: end.to_string(),
            field_accesses,
            native_predicate: None,
            uses_meta: var_usage.uses_meta,
            uses_conf: var_usage.uses_conf,
            uses_line: var_usage.uses_line,
        })
    }

    pub fn compile_span_close(&mut self, script: &str) -> Result<CompiledExpression> {
        let ast = self.engine.compile(script).map_err(|e| {
            let msg = Self::format_rhai_diagnostic(
                e.into(),
                "span-close compilation",
                "span-close script",
                script,
                None,
                None,
                self.use_emoji,
            );
            anyhow::anyhow!(msg)
        })?;
        let field_accesses = extract_field_accesses(&ast);
        let var_usage = detect_variable_usage(&ast);
        Ok(CompiledExpression {
            ast,
            expr: script.to_string(),
            field_accesses,
            native_predicate: None,
            uses_meta: var_usage.uses_meta,
            uses_conf: var_usage.uses_conf,
            uses_line: var_usage.uses_line,
        })
    }

    // Individual execution methods for pipeline stages
    pub fn execute_compiled_filter(
        &mut self,
        compiled: &CompiledExpression,
        event: &Event,
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
    ) -> Result<bool> {
        let mut stats_recorded = false;

        if let Some(native) = &compiled.native_predicate {
            Self::set_thread_tracking_state(metrics, internal);
            debug_stats_increment_events_processed();
            debug_stats_increment_script_executions();
            stats_recorded = true;

            if let Some(result) = native.evaluate(event) {
                if result {
                    debug_stats_increment_events_passed();
                }
                *metrics = Self::get_thread_tracking_state();
                *internal = Self::get_thread_internal_state();
                return Ok(result);
            }
        }

        Self::set_thread_tracking_state(metrics, internal);
        let mut scope = self.create_scope_for_event_optimized(
            event,
            compiled.uses_line,
            compiled.uses_meta,
            compiled.uses_conf,
        );

        // Debug statistics tracking
        if !stats_recorded {
            debug_stats_increment_events_processed();
            debug_stats_increment_script_executions();
        }

        // Add execution tracing for filter execution
        if let Some(ref tracer) = self.execution_tracer {
            let event_num = tracer.next_event();
            let event_data = format!("{:?}", event.fields);

            // Level 2+: Detailed execution tracing
            if tracer.config.verbosity >= 2 {
                tracer.trace_event_start(event_num, &event_data);
                eprintln!("  Script: {}", compiled.expr.trim());
            }

            // Enhanced detailed tracing for -vvv
            if tracer.config.verbosity >= 3 {
                tracer.trace_scope_inspection(&scope);
                tracer.trace_detailed_step(
                    "filter",
                    "evaluation",
                    &compiled.expr,
                    "starting",
                    "script",
                );
            }
        }

        let result = self
            .engine
            .eval_ast_with_scope::<bool>(&mut scope, &compiled.ast)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "filter",
                    "filter expression",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        // Add execution result tracing
        if let Some(ref tracer) = self.execution_tracer {
            let action = if result { "passed" } else { "filtered out" };
            tracer.trace_event_result(result, action);

            // Enhanced detailed result tracing
            if tracer.config.verbosity >= 3 {
                let result_str = if result { "true" } else { "false" };
                tracer.trace_detailed_step(
                    "filter",
                    "result",
                    &compiled.expr,
                    result_str,
                    "boolean",
                );
            }
        }

        // Track successful events in debug statistics
        if result {
            debug_stats_increment_events_passed();
        }

        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();
        Ok(result)
    }

    pub fn execute_compiled_exec(
        &mut self,
        compiled: &CompiledExpression,
        event: &mut Event,
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
    ) -> Result<()> {
        Self::set_thread_tracking_state(metrics, internal);
        let mut scope = self.create_scope_for_event_optimized(
            event,
            compiled.uses_line,
            compiled.uses_meta,
            compiled.uses_conf,
        );

        // Debug statistics tracking
        debug_stats_increment_script_executions();

        // Add execution tracing for exec execution
        if let Some(ref tracer) = self.execution_tracer {
            let event_num = tracer.next_event();
            let event_data = format!("{:?}", event.fields);

            // Level 2+: Detailed execution tracing
            if tracer.config.verbosity >= 2 {
                tracer.trace_event_start(event_num, &event_data);
                eprintln!("  Script: {}", compiled.expr.trim());
            }

            // Enhanced detailed tracing for -vvv
            if tracer.config.verbosity >= 3 {
                tracer.trace_scope_inspection(&scope);
                tracer.trace_detailed_step(
                    "exec",
                    "transformation",
                    &compiled.expr,
                    "starting",
                    "script",
                );
            }
        }

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "exec",
                    "exec script",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        // Add execution result tracing
        if let Some(ref tracer) = self.execution_tracer {
            tracer.trace_event_result(true, "executed successfully");

            // Enhanced detailed result tracing
            if tracer.config.verbosity >= 3 {
                tracer.trace_detailed_step(
                    "exec",
                    "result",
                    &compiled.expr,
                    "success",
                    "execution",
                );
            }
        }

        self.update_event_from_scope(event, &scope);
        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();
        Ok(())
    }

    pub fn execute_compiled_begin(
        &mut self,
        compiled: &CompiledExpression,
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
    ) -> Result<rhai::Map> {
        Self::set_thread_tracking_state(metrics, internal);

        // Set begin phase flag to allow read_file/read_lines
        crate::rhai_functions::conf::set_begin_phase(true);

        let mut scope = self.scope_template.clone();

        self.push_state_to_scope(&mut scope);

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "begin",
                    "begin expression",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        // Reset begin phase flag
        crate::rhai_functions::conf::set_begin_phase(false);

        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();

        // Extract the conf map from scope and store it
        let mut conf_map = scope.get_value::<rhai::Map>("conf").unwrap_or_default();

        // Deep freeze the conf map to make it read-only
        crate::rhai_functions::conf::deep_freeze_map(&mut conf_map);

        // Store the frozen conf map
        self.conf_map = Some(conf_map.clone());

        Ok(conf_map)
    }

    pub fn execute_compiled_end(
        &self,
        compiled: &CompiledExpression,
        metrics: &HashMap<String, Dynamic>,
    ) -> Result<()> {
        let mut scope = self.scope_template.clone();
        let mut tracked_map = rhai::Map::new();

        // Convert HashMap to Rhai Map (read-only)
        for (k, v) in metrics.iter() {
            tracked_map.insert(k.clone().into(), v.clone());
        }
        scope.set_value("metrics", tracked_map);

        // Set the frozen conf map (read-only)
        if let Some(ref conf_map) = self.conf_map {
            scope.set_value("conf", conf_map.clone());
        }

        self.push_state_to_scope(&mut scope);

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "end",
                    "end expression",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        Ok(())
    }

    pub fn execute_compiled_span_close(
        &mut self,
        compiled: &CompiledExpression,
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
        span: crate::rhai_functions::span::SpanBinding,
    ) -> Result<()> {
        Self::set_thread_tracking_state(metrics, internal);

        let mut scope = self.scope_template.clone();
        let mut metrics_map = rhai::Map::new();

        for (k, v) in metrics.iter() {
            metrics_map.insert(k.clone().into(), v.clone());
        }
        scope.set_value("metrics", metrics_map);
        scope.push_constant("span", Dynamic::from(span));

        // Set the frozen conf map (read-only)
        if let Some(ref conf_map) = self.conf_map {
            scope.set_value("conf", conf_map.clone());
        }

        self.push_state_to_scope(&mut scope);

        crate::rhai_functions::file_ops::clear_pending_ops();

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "span-close",
                    "span-close script",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        let ops = crate::rhai_functions::file_ops::take_pending_ops();
        crate::rhai_functions::file_ops::execute_ops(&ops)?;

        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();

        Ok(())
    }

    fn register_variable_resolver(_engine: &mut Engine) {
        // For now, keep this empty - we'll implement proper function-based approach
        // Variable resolver is not the right tool for function calls
    }

    // Window-aware execution methods
    pub fn execute_compiled_filter_with_window(
        &mut self,
        compiled: &CompiledExpression,
        event: &Event,
        window: &[Event],
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
    ) -> Result<bool> {
        Self::set_thread_tracking_state(metrics, internal);
        let mut scope = self.create_scope_for_event_with_window(event, window);

        // Debug statistics tracking
        debug_stats_increment_events_processed();
        debug_stats_increment_script_executions();

        // Add execution tracing for windowed filter execution
        if let Some(ref tracer) = self.execution_tracer {
            let event_num = tracer.next_event();
            let event_data = format!("{:?}", event.fields);

            // Level 2+: Detailed execution tracing
            if tracer.config.verbosity >= 2 {
                tracer.trace_event_start(event_num, &event_data);
                eprintln!(
                    "  Script (windowed, size {}): {}",
                    window.len(),
                    compiled.expr.trim()
                );
            }

            // Enhanced detailed tracing for -vvv
            if tracer.config.verbosity >= 3 {
                tracer.trace_scope_inspection(&scope);
                tracer.trace_detailed_step(
                    "windowed-filter",
                    "evaluation",
                    &compiled.expr,
                    "starting",
                    "script",
                );
                tracer.trace_detailed_step(
                    "windowed-filter",
                    "window-size",
                    &window.len().to_string(),
                    &window.len().to_string(),
                    "size",
                );
            }
        }

        let result = self
            .engine
            .eval_ast_with_scope::<bool>(&mut scope, &compiled.ast)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "filter",
                    "filter expression",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        // Add execution result tracing
        if let Some(ref tracer) = self.execution_tracer {
            let action = if result { "passed" } else { "filtered out" };
            tracer.trace_event_result(result, action);

            // Enhanced detailed result tracing
            if tracer.config.verbosity >= 3 {
                let result_str = if result { "true" } else { "false" };
                tracer.trace_detailed_step(
                    "windowed-filter",
                    "result",
                    &compiled.expr,
                    result_str,
                    "boolean",
                );
            }
        }

        // Track successful events in debug statistics
        if result {
            debug_stats_increment_events_passed();
        }

        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();
        Ok(result)
    }

    pub fn execute_compiled_exec_with_window(
        &mut self,
        compiled: &CompiledExpression,
        event: &mut Event,
        window: &[Event],
        metrics: &mut HashMap<String, Dynamic>,
        internal: &mut HashMap<String, Dynamic>,
    ) -> Result<()> {
        Self::set_thread_tracking_state(metrics, internal);
        let mut scope = self.create_scope_for_event_with_window(event, window);

        // Debug statistics tracking
        debug_stats_increment_script_executions();

        // Add execution tracing for windowed exec execution
        if let Some(ref tracer) = self.execution_tracer {
            let event_num = tracer.next_event();
            let event_data = format!("{:?}", event.fields);

            // Level 2+: Detailed execution tracing
            if tracer.config.verbosity >= 2 {
                tracer.trace_event_start(event_num, &event_data);
                eprintln!(
                    "  Script (windowed, size {}): {}",
                    window.len(),
                    compiled.expr.trim()
                );
            }

            // Enhanced detailed tracing for -vvv
            if tracer.config.verbosity >= 3 {
                tracer.trace_scope_inspection(&scope);
                tracer.trace_detailed_step(
                    "windowed-exec",
                    "transformation",
                    &compiled.expr,
                    "starting",
                    "script",
                );
                tracer.trace_detailed_step(
                    "windowed-exec",
                    "window-size",
                    &window.len().to_string(),
                    &window.len().to_string(),
                    "size",
                );
            }
        }

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = Self::format_rhai_diagnostic(
                    e,
                    "exec",
                    "exec script",
                    &compiled.expr,
                    Some(&scope),
                    self.debug_tracker.as_ref(),
                    self.use_emoji,
                );
                anyhow::anyhow!("{}", detailed_msg)
            })?;

        self.assert_conf_not_mutated(&scope)
            .map_err(anyhow::Error::from)?;

        // Add execution result tracing
        if let Some(ref tracer) = self.execution_tracer {
            tracer.trace_event_result(true, "executed successfully");

            // Enhanced detailed result tracing
            if tracer.config.verbosity >= 3 {
                tracer.trace_detailed_step(
                    "windowed-exec",
                    "result",
                    &compiled.expr,
                    "success",
                    "execution",
                );
            }
        }

        self.update_event_from_scope(event, &scope);
        *metrics = Self::get_thread_tracking_state();
        *internal = Self::get_thread_internal_state();
        Ok(())
    }

    fn assert_conf_not_mutated(&self, scope: &Scope) -> Result<(), ConfMutationError> {
        if let Some(original) = &self.conf_map {
            match scope.get_value::<Map>("conf") {
                Some(conf) if maps_equal(&conf, original) => Ok(()),
                _ => Err(ConfMutationError),
            }
        } else {
            Ok(())
        }
    }

    fn create_scope_for_event(&self, event: &Event) -> Scope<'_> {
        // Full scope creation - includes all variables
        self.create_scope_for_event_optimized(event, true, true, true)
    }

    /// Create a scope for event evaluation, optionally skipping unused variables
    fn create_scope_for_event_optimized(
        &self,
        event: &Event,
        needs_line: bool,
        needs_meta: bool,
        needs_conf: bool,
    ) -> Scope<'_> {
        let mut scope = self.scope_template.clone();

        // Update built-in variables - only if needed
        if needs_line {
            scope.set_value("line", event.original_line.clone());
        }

        // Update event map for fields with invalid identifiers
        // Note: We always build the event map since most scripts use `e.*`
        let mut event_map = rhai::Map::new();
        for (k, v) in &event.fields {
            event_map.insert(k.clone().into(), v.clone());
        }
        scope.set_value("e", event_map);

        // Update metadata - only if needed
        if needs_meta {
            let mut meta_map = rhai::Map::new();
            if let Some(line_num) = event.line_num {
                meta_map.insert("line_num".into(), Dynamic::from(line_num as i64));
            }
            if let Some(filename) = &event.filename {
                meta_map.insert("filename".into(), Dynamic::from(filename.clone()));
            }
            if let Some(status) = event.span.status {
                meta_map.insert("span_status".into(), Dynamic::from(status.as_str()));
            }
            if let Some(span_id) = &event.span.span_id {
                meta_map.insert("span_id".into(), Dynamic::from(span_id.clone()));
            }
            if let Some(span_start) = event.span.span_start {
                meta_map.insert(
                    "span_start".into(),
                    Dynamic::from(DateTimeWrapper::from_utc(span_start)),
                );
            }
            if let Some(span_end) = event.span.span_end {
                meta_map.insert(
                    "span_end".into(),
                    Dynamic::from(DateTimeWrapper::from_utc(span_end)),
                );
            }
            if let Some(parsed_ts) = event.parsed_ts {
                meta_map.insert(
                    "parsed_ts".into(),
                    Dynamic::from(DateTimeWrapper::from_utc(parsed_ts)),
                );
            }

            // Add raw line to metadata
            meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));

            scope.set_value("meta", meta_map);
        }

        // Set the frozen conf map - only if needed
        if needs_conf {
            if let Some(ref conf_map) = self.conf_map {
                scope.set_value("conf", conf_map.clone());
            }
        }

        self.push_state_to_scope(&mut scope);

        scope
    }

    fn create_scope_for_event_with_window(&self, event: &Event, window: &[Event]) -> Scope<'_> {
        let mut scope = self.create_scope_for_event(event);

        // Add window array to scope
        let window_array: rhai::Array = window
            .iter()
            .map(|event| {
                let mut event_map = rhai::Map::new();
                // Add all event fields to the map
                for (k, v) in &event.fields {
                    event_map.insert(k.clone().into(), v.clone());
                }
                // Add built-in fields
                event_map.insert("line".into(), Dynamic::from(event.original_line.clone()));
                if let Some(line_num) = event.line_num {
                    event_map.insert("line_num".into(), Dynamic::from(line_num as i64));
                }
                if let Some(filename) = &event.filename {
                    event_map.insert("filename".into(), Dynamic::from(filename.clone()));
                }
                if let Some(status) = event.span.status {
                    event_map.insert("span_status".into(), Dynamic::from(status.as_str()));
                }
                if let Some(span_id) = &event.span.span_id {
                    event_map.insert("span_id".into(), Dynamic::from(span_id.clone()));
                }
                if let Some(span_start) = event.span.span_start {
                    event_map.insert(
                        "span_start".into(),
                        Dynamic::from(DateTimeWrapper::from_utc(span_start)),
                    );
                }
                if let Some(span_end) = event.span.span_end {
                    event_map.insert(
                        "span_end".into(),
                        Dynamic::from(DateTimeWrapper::from_utc(span_end)),
                    );
                }
                Dynamic::from(event_map)
            })
            .collect();

        scope.set_value("window", window_array);
        scope
    }

    fn update_event_from_scope(&self, event: &mut Event, scope: &Scope) {
        // Check if entire event 'e' was set to unit () - clear all fields
        if scope.get_value::<()>("e").is_some() {
            event.fields.clear();
            return;
        }

        // Capture mutations made directly to the `e` event map
        if let Some(obj) = scope.get_value::<Map>("e") {
            let original_order: Vec<String> = event.fields.keys().cloned().collect();
            let mut remaining_entries: Vec<(String, Dynamic)> =
                obj.into_iter().map(|(k, v)| (k.into(), v)).collect();

            let mut reordered_fields = IndexMap::with_capacity(remaining_entries.len());

            for key in &original_order {
                if let Some(pos) = remaining_entries.iter().position(|(k, _)| k == key) {
                    let (_, value) = remaining_entries.remove(pos);
                    if value.is::<()>() {
                        continue;
                    }
                    reordered_fields.insert(key.clone(), value);
                }
            }

            for (key, value) in remaining_entries {
                if value.is::<()>() {
                    continue;
                }
                reordered_fields.insert(key, value);
            }

            event.fields = reordered_fields;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn build_event_with_line(line: &str) -> Event {
        let mut event = Event::with_capacity(line.to_string(), 1);
        event.set_field("line".to_string(), Dynamic::from(line.to_string()));
        event
    }

    #[test]
    fn assignment_replaces_entire_event_map() {
        let engine = RhaiEngine::new();
        let mut event = build_event_with_line("orig line");
        event.set_field("keep".to_string(), Dynamic::from("value"));

        let mut scope = engine.create_scope_for_event(&event);

        let mut new_map = rhai::Map::new();
        new_map.insert("ts".into(), Dynamic::from("2025-09-22"));
        scope.set_value("e", new_map);

        let mut event_clone = event.clone();
        engine.update_event_from_scope(&mut event_clone, &scope);

        assert!(event_clone.fields.get("line").is_none());
        assert!(event_clone.fields.get("keep").is_none());
        assert_eq!(
            event_clone
                .fields
                .get("ts")
                .and_then(|v| v.clone().try_cast::<String>())
                .as_deref(),
            Some("2025-09-22")
        );
        assert_eq!(event_clone.fields.len(), 1);
    }

    #[test]
    fn unit_values_still_remove_fields() {
        let engine = RhaiEngine::new();
        let mut event = build_event_with_line("orig line");
        event.set_field("msg".to_string(), Dynamic::from("hello"));

        let mut scope = engine.create_scope_for_event(&event);

        let mut updated_map = rhai::Map::new();
        updated_map.insert("msg".into(), Dynamic::from("world"));
        updated_map.insert("line".into(), Dynamic::UNIT);
        scope.set_value("e", updated_map);

        let mut event_clone = event.clone();
        engine.update_event_from_scope(&mut event_clone, &scope);

        assert!(event_clone.fields.get("line").is_none());
        assert_eq!(
            event_clone
                .fields
                .get("msg")
                .and_then(|v| v.clone().try_cast::<String>())
                .as_deref(),
            Some("world")
        );
    }

    #[test]
    fn in_place_mutations_preserve_unchanged_fields() {
        let engine = RhaiEngine::new();
        let mut event = build_event_with_line("orig line");
        event.set_field("level".to_string(), Dynamic::from("INFO"));

        let mut scope = engine.create_scope_for_event(&event);

        // Simulate in-place mutation by starting from the existing map values
        let mut mutated_map = scope.get_value::<Map>("e").unwrap();
        mutated_map.insert("level".into(), Dynamic::from("ERROR"));
        scope.set_value("e", mutated_map);

        let mut event_clone = event.clone();
        engine.update_event_from_scope(&mut event_clone, &scope);

        assert!(event_clone.fields.get("line").is_some());
        assert_eq!(
            event_clone
                .fields
                .get("level")
                .and_then(|v| v.clone().try_cast::<String>())
                .as_deref(),
            Some("ERROR")
        );
    }

    #[test]
    fn update_event_preserves_field_order_and_appends_new_keys() {
        let engine = RhaiEngine::new();
        let mut event = build_event_with_line("orig line");
        event.set_field("z".to_string(), Dynamic::from(1_i64));
        event.set_field("a".to_string(), Dynamic::from(2_i64));
        event.set_field("b".to_string(), Dynamic::from(3_i64));

        let mut scope = engine.create_scope_for_event(&event);

        let mut mutated_map = scope.get_value::<Map>("e").unwrap();
        mutated_map.insert("foo".into(), Dynamic::from(42_i64));
        scope.set_value("e", mutated_map);

        let mut event_clone = event.clone();
        engine.update_event_from_scope(&mut event_clone, &scope);

        let keys: Vec<String> = event_clone.fields.keys().cloned().collect();
        assert_eq!(keys, vec!["line", "z", "a", "b", "foo"]);
    }

    #[test]
    fn meta_includes_parsed_timestamp_before_scripts() {
        let engine = RhaiEngine::new();
        let mut event = build_event_with_line("orig line");
        let ts = Utc.timestamp_opt(1_700_000_000, 123_000_000).unwrap();
        event.parsed_ts = Some(ts);

        let scope = engine.create_scope_for_event(&event);
        let meta = scope.get_value::<Map>("meta").expect("meta map");

        let parsed_ts = meta
            .get("parsed_ts")
            .cloned()
            .expect("parsed_ts should be present in meta");
        let dt = parsed_ts
            .try_cast::<crate::rhai_functions::datetime::DateTimeWrapper>()
            .expect("parsed_ts should be a DateTimeWrapper");
        assert_eq!(dt.inner.to_rfc3339(), ts.to_rfc3339());
    }

    #[test]
    fn meta_omits_parsed_timestamp_when_missing() {
        let engine = RhaiEngine::new();
        let event = build_event_with_line("orig line");

        let scope = engine.create_scope_for_event(&event);
        let meta = scope.get_value::<Map>("meta").expect("meta map");
        assert!(
            !meta.contains_key("parsed_ts"),
            "meta.parsed_ts should be absent when event has no parsed timestamp"
        );
    }

    #[test]
    fn render_snippet_marks_correct_line_and_col() {
        let script = "let x = 1;\nlet y = foo(x);\nlet z = y + 1;";
        let snippet = RhaiEngine::render_snippet(script, 1, 7).expect("snippet");
        assert!(snippet.contains("2 | let y = foo(x);"));
        assert!(snippet.contains("^"));
        // caret should land under the 8th character (zero-based 7) of line 2
        let caret_line = snippet.lines().nth(1).unwrap_or_default();
        assert!(caret_line.ends_with("^"));
        assert!(caret_line.contains("|        ^"));
    }

    #[test]
    fn parse_error_suggests_rhai_raw_string_syntax() {
        let mut engine = RhaiEngine::new();
        let err = engine
            .compile_exec("e.line.extract_regex(r\"User (\\\\d+)\")")
            .err()
            .expect("r\"...\" should be invalid in Rhai");
        let msg = err.to_string();
        assert!(
            msg.contains("Rhai raw strings use #\"...\"#"),
            "raw string hint should mention Rhai syntax; got: {msg}"
        );
    }

    #[test]
    fn property_suggestion_shows_available_fields_without_verbose() {
        let config = DebugConfig::new(0);
        let enhancer = ErrorEnhancer::new(config);
        let mut scope = Scope::new();
        let mut e_map = Map::new();
        e_map.insert("status".into(), Dynamic::from("OK"));
        e_map.insert("status_code".into(), Dynamic::from(200_i64));
        scope.push("e", e_map);

        let err = EvalAltResult::ErrorPropertyNotFound("statsu".into(), rhai::Position::NONE);
        let ctx = ExecutionContext::default();
        let out = enhancer.enhance_error(&err, &scope, "e.statsu", "filter", &ctx);

        eprintln!("enhanced error:\n{}", out);
        assert!(
            out.contains("status"),
            "output should surface available fields even when verbosity is zero"
        );
    }

    #[test]
    fn runtime_error_with_unit_type_suggests_get_path() {
        let config = DebugConfig::new(0);
        let enhancer = ErrorEnhancer::new(config);
        let scope = Scope::new();

        let err = EvalAltResult::ErrorRuntime(
            "track_count requires a string key; got ()".into(),
            rhai::Position::NONE,
        );
        let ctx = ExecutionContext::default();
        let out = enhancer.enhance_error(&err, &scope, "track_count(e.endpoint)", "exec", &ctx);

        eprintln!("enhanced error:\n{}", out);
        assert!(
            out.contains("field is missing"),
            "output should explain that () means a missing field"
        );
        assert!(
            out.contains("get_path"),
            "output should suggest get_path for handling missing fields"
        );
        assert!(
            out.contains("has_path"),
            "output should suggest has_path for checking field existence"
        );
    }

    #[test]
    fn native_filter_evaluates_simple_comparisons() {
        let mut engine = RhaiEngine::new();
        let compiled = engine
            .compile_filter("e.level == \"ERROR\" && e.status >= 500")
            .expect("filter should compile");

        assert!(compiled.native_predicate.is_some());

        let mut event = build_event_with_line("line");
        event.set_field("level".to_string(), Dynamic::from("ERROR"));
        event.set_field("status".to_string(), Dynamic::from(500_i64));

        let mut metrics = HashMap::new();
        let mut internal = HashMap::new();
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(result);

        let mut event_nonmatch = event.clone();
        event_nonmatch.set_field("status".to_string(), Dynamic::from(200_i64));
        let result = engine
            .execute_compiled_filter(&compiled, &event_nonmatch, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(!result);
    }

    #[test]
    fn native_filter_handles_string_methods() {
        let mut engine = RhaiEngine::new();

        // String methods now have native predicate support
        let compiled = engine
            .compile_filter("e.level.starts_with(\"ERR\")")
            .expect("filter should compile");
        assert!(
            compiled.native_predicate.is_some(),
            "starts_with should have native predicate"
        );

        // Test contains
        let compiled = engine
            .compile_filter("e.message.contains(\"error\")")
            .expect("filter should compile");
        assert!(
            compiled.native_predicate.is_some(),
            "contains should have native predicate"
        );

        // Test ends_with
        let compiled = engine
            .compile_filter("e.path.ends_with(\".log\")")
            .expect("filter should compile");
        assert!(
            compiled.native_predicate.is_some(),
            "ends_with should have native predicate"
        );

        // Unsupported methods should still fall back to Rhai
        let compiled = engine
            .compile_filter("e.message.to_upper()")
            .expect("filter should compile");
        assert!(
            compiled.native_predicate.is_none(),
            "to_upper should NOT have native predicate"
        );
    }

    #[test]
    fn native_string_methods_evaluate_correctly() {
        let mut engine = RhaiEngine::new();

        // Test starts_with
        let compiled = engine
            .compile_filter("e.level.starts_with(\"ERR\")")
            .expect("filter should compile");

        let mut event = build_event_with_line("test");
        event.set_field("level".to_string(), Dynamic::from("ERROR"));
        let mut metrics = HashMap::new();
        let mut internal = HashMap::new();
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(result, "ERROR should start with ERR");

        event.set_field("level".to_string(), Dynamic::from("WARNING"));
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(!result, "WARNING should not start with ERR");

        // Test contains
        let compiled = engine
            .compile_filter("e.message.contains(\"fail\")")
            .expect("filter should compile");

        event.set_field("message".to_string(), Dynamic::from("connection failed"));
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(result, "'connection failed' should contain 'fail'");

        event.set_field("message".to_string(), Dynamic::from("success"));
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(!result, "'success' should not contain 'fail'");

        // Test ends_with
        let compiled = engine
            .compile_filter("e.path.ends_with(\".log\")")
            .expect("filter should compile");

        event.set_field("path".to_string(), Dynamic::from("/var/log/app.log"));
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(result, "'/var/log/app.log' should end with '.log'");

        event.set_field("path".to_string(), Dynamic::from("/var/log/app.txt"));
        let result = engine
            .execute_compiled_filter(&compiled, &event, &mut metrics, &mut internal)
            .expect("native filter should succeed");
        assert!(!result, "'/var/log/app.txt' should not end with '.log'");
    }

    #[test]
    fn variable_usage_detection() {
        let mut engine = RhaiEngine::new();

        // Filter using only e.* - should not need meta, conf, or line
        let compiled = engine
            .compile_filter("e.level == \"ERROR\"")
            .expect("filter should compile");
        assert!(!compiled.uses_meta, "simple filter should not use meta");
        assert!(!compiled.uses_conf, "simple filter should not use conf");
        assert!(!compiled.uses_line, "simple filter should not use line");

        // Filter using meta
        let compiled = engine
            .compile_filter("meta.line_num > 100")
            .expect("filter should compile");
        assert!(compiled.uses_meta, "filter with meta.* should use meta");
        assert!(!compiled.uses_conf, "filter should not use conf");

        // Filter using conf
        let compiled = engine
            .compile_filter("conf.threshold > 5")
            .expect("filter should compile");
        assert!(compiled.uses_conf, "filter with conf.* should use conf");
        assert!(!compiled.uses_meta, "filter should not use meta");

        // Filter using line
        let compiled = engine
            .compile_filter("line.len() > 100")
            .expect("filter should compile");
        assert!(compiled.uses_line, "filter with line should use line");

        // Combined usage
        let compiled = engine
            .compile_filter("e.level == \"ERROR\" && meta.line_num > 0")
            .expect("filter should compile");
        assert!(compiled.uses_meta, "combined filter should use meta");
        assert!(!compiled.uses_conf, "combined filter should not use conf");
    }

    #[test]
    fn function_suggestion_offers_len_for_length_typo() {
        let config = DebugConfig::new(0);
        let enhancer = ErrorEnhancer::new(config);
        let scope = Scope::new();
        let err =
            EvalAltResult::ErrorFunctionNotFound("length(string)".into(), rhai::Position::NONE);
        let ctx = ExecutionContext::default();
        let out = enhancer.enhance_error(&err, &scope, "length(s)", "filter", &ctx);
        assert!(
            out.contains("len"),
            "function suggestion should offer len() for length typo; got: {out}"
        );
    }

    #[test]
    fn nested_function_errors_show_call_stack() {
        let inner = Box::new(EvalAltResult::ErrorRuntime(
            "boom".into(),
            rhai::Position::new(3, 1),
        ));
        let mid = Box::new(EvalAltResult::ErrorInFunctionCall(
            "child".into(),
            "".into(),
            inner,
            rhai::Position::new(2, 1),
        ));
        let outer = Box::new(EvalAltResult::ErrorInFunctionCall(
            "parent".into(),
            "".into(),
            mid,
            rhai::Position::new(1, 1),
        ));

        let msg = RhaiEngine::format_rhai_diagnostic(
            outer, "filter", "script", "child()", None, None, true,
        );

        assert!(
            msg.contains("Call stack") && msg.contains("parent") && msg.contains("child"),
            "call stack should include nested function frames; got: {msg}"
        );
    }

    #[test]
    fn unit_arg_suggestion_points_to_missing_field() {
        let msg = RhaiEngine::format_function_not_found_error(
            "foo((), string)".to_string(),
            "script",
            rhai::Position::NONE,
        );
        assert!(
            (msg.contains("missing field") || msg.contains("e.has")) && msg.contains("Called with"),
            "unit arg hint should mention missing field guards and show called types; got: {msg}"
        );
    }

    #[test]
    fn type_mismatch_hints_bool_in_filter() {
        let config = DebugConfig::new(0);
        let enhancer = ErrorEnhancer::new(config);
        let scope = Scope::new();
        let err = EvalAltResult::ErrorMismatchDataType(
            "bool".into(),
            "string".into(),
            rhai::Position::NONE,
        );
        let ctx = ExecutionContext::default();
        let out = enhancer.enhance_error(&err, &scope, "e.level", "filter", &ctx);
        assert!(
            out.contains("Filters must return true/false"),
            "type mismatch in filter should remind about boolean return; got: {out}"
        );
    }
}
