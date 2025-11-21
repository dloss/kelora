#![allow(dead_code)] // Debugging/tracing scaffolding kept for verbose dev builds and future CLI toggles
use anyhow::{Context, Result};
use indexmap::IndexMap;
use rhai::{Dynamic, Engine, EvalAltResult, Scope, AST};
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
        output.push_str(&format!("⚠️ Stage {} failed\n", stage));
        output.push_str(&format!("  Code: {}\n", script.trim()));
        output.push_str(&format!("  Error: {}\n", error));

        // Add execution context if available
        if let Some(pos) = &execution_context.position {
            output.push_str(&format!("   Position: {}\n", pos));
        }

        // Show scope information if debug enabled
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

            // Add suggestions based on error type
            if let Some(suggestions) = self.generate_suggestions(error, scope) {
                output.push_str(&format!("\n   💡 {}\n", suggestions));
            }

            // Add stage-specific help
            output.push_str(&self.get_stage_help(stage, error));
        }

        output
    }

    fn generate_suggestions(&self, error: &EvalAltResult, scope: &Scope) -> Option<String> {
        match error {
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
            },
            EvalAltResult::ErrorPropertyNotFound(prop_name, _) => {
                Some(format!("Property '{}' not found. Try `--stats` or `-F inspect` to see available fields", prop_name))
            },
            EvalAltResult::ErrorIndexNotFound(index, _) => {
                Some(format!("Index '{}' not found. Check array bounds with 'if e.array.len() > {} {{ ... }}'", index, index))
            },
            EvalAltResult::ErrorFunctionNotFound(func_sig, _) => {
                self.suggest_function_alternatives(func_sig)
            },
            EvalAltResult::ErrorMismatchDataType(expected, actual, _) => {
                Some(format!("Type mismatch: expected {}, got {}. Use type_of() to check types or to_string(), to_number() for conversion", expected, actual))
            },
            _ => None
        }
    }

    fn suggest_function_alternatives(&self, func_sig: &str) -> Option<String> {
        let func_name = func_sig.split('(').next().unwrap_or(func_sig).trim();

        // Common function alternatives
        match func_name {
            "length" => Some("Use 'len()' instead of 'length()'".to_string()),
            "size" => Some("Use 'len()' instead of 'size()'".to_string()),
            "substr" | "substring" => Some(
                "Use string slicing: s[start..end] or extract_re() for pattern matching"
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
                "Use 'extract_re()' for regex matching or 'contains()' for simple checks"
                    .to_string(),
            ),
            name if name.ends_with("_re") => Some(
                "Regex functions: extract_re(), extract_all_re(), split_re(), replace_re()"
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

#[derive(Clone)]
pub struct CompiledExpression {
    ast: AST,
    expr: String,
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
    debug_tracker: Option<DebugTracker>,
    execution_tracer: Option<ExecutionTracer>,
}

impl Clone for RhaiEngine {
    fn clone(&self) -> Self {
        let mut engine = Engine::new();
        // Use Simple optimization, not Full. Full optimization breaks side-effect functions
        // like track_count("key"), print("msg"), emit_each(), etc. by trying to evaluate them at
        // compile time when their arguments are constants. These functions MUST run at runtime.
        engine.set_optimization_level(rhai::OptimizationLevel::Simple);

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
            debug_tracker: self.debug_tracker.clone(),
            execution_tracer: self.execution_tracer.clone(),
        }
    }
}

impl RhaiEngine {
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
        if Self::is_likely_type_mismatch(&func_signature, func_name) {
            let expected_types = Self::get_expected_function_signature(func_name);
            if !expected_types.is_empty() {
                let called_types = Self::extract_called_types(&func_signature);
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

        if suggestions.is_empty() {
            base_msg
        } else {
            format!("{}. Did you mean: {}", base_msg, suggestions.join(", "))
        }
    }

    fn is_likely_type_mismatch(func_signature: &str, func_name: &str) -> bool {
        !Self::get_expected_function_signature(func_name).is_empty() && func_signature.contains('(')
    }

    fn extract_called_types(func_signature: &str) -> String {
        if let Some(start) = func_signature.find('(') {
            if let Some(end) = func_signature.find(')') {
                return func_signature[start + 1..end].to_string();
            }
        }
        "unknown types".to_string()
    }

    fn get_expected_function_signature(func_name: &str) -> String {
        match func_name {
            "extract_re" => "string, regex_pattern, optional_group_index".to_string(),
            "extract_all_re" => "string, regex_pattern, optional_group_index".to_string(),
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

    fn get_function_suggestions(func_name: &str) -> Vec<String> {
        // List of common Rhai built-in functions and our custom functions
        let available_functions = vec![
            // String functions
            "lower",
            "upper",
            "trim",
            "len",
            "contains",
            "starts_with",
            "ends_with",
            "split",
            "replace",
            "substring",
            "to_string",
            "parse",
            // Our custom string functions
            "extract_re",
            "extract_all_re",
            "split_re",
            "replace_re",
            "count",
            "strip",
            "before",
            "after",
            "between",
            "starting_with",
            "ending_with",
            "is_digit",
            "join",
            "extract_ip",
            "extract_ips",
            "mask_ip",
            "is_private_ip",
            "extract_url",
            "extract_domain",
            // Math functions
            "abs",
            "floor",
            "ceil",
            "round",
            "min",
            "max",
            "pow",
            "sqrt",
            // Array functions
            "push",
            "pop",
            "shift",
            "unshift",
            "reverse",
            "sort",
            "clear",
            // Map functions
            "keys",
            "values",
            "remove",
            "contains",
            // Our custom functions
            "parse_json",
            "parse_kv",
            "col",
            "cols",
            "status_class",
            "track_count",
            "track_sum",
            "track_min",
            "track_max",
            "track_avg",
            "track_unique",
            "track_bucket",
            // Utility functions
            "print",
            "debug",
            "type_of",
            "is_def_fn",
        ];

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
                    "extract_re".to_string(),
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
            debug_tracker: None,
            execution_tracer: None,
        }
    }

    pub fn get_execution_tracer(&self) -> &Option<ExecutionTracer> {
        &self.execution_tracer
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
        let ast = self
            .engine
            .compile_expression(filter)
            .with_context(|| format!("Failed to compile filter expression: {}", filter))?;
        Ok(CompiledExpression {
            ast,
            expr: filter.to_string(),
        })
    }

    pub fn compile_exec(&mut self, exec: &str) -> Result<CompiledExpression> {
        let ast = self
            .engine
            .compile(exec)
            .with_context(|| format!("Failed to compile exec script: {}", exec))?;
        Ok(CompiledExpression {
            ast,
            expr: exec.to_string(),
        })
    }

    pub fn compile_begin(&mut self, begin: &str) -> Result<CompiledExpression> {
        let ast = self
            .engine
            .compile(begin)
            .with_context(|| format!("Failed to compile begin expression: {}", begin))?;
        Ok(CompiledExpression {
            ast,
            expr: begin.to_string(),
        })
    }

    pub fn compile_end(&mut self, end: &str) -> Result<CompiledExpression> {
        let ast = self
            .engine
            .compile(end)
            .with_context(|| format!("Failed to compile end expression: {}", end))?;
        Ok(CompiledExpression {
            ast,
            expr: end.to_string(),
        })
    }

    pub fn compile_span_close(&mut self, script: &str) -> Result<CompiledExpression> {
        let ast = self
            .engine
            .compile(script)
            .with_context(|| format!("Failed to compile span-close script: {}", script))?;
        Ok(CompiledExpression {
            ast,
            expr: script.to_string(),
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
        Self::set_thread_tracking_state(metrics, internal);
        let mut scope = self.create_scope_for_event(event);

        // Debug statistics tracking
        debug_stats_increment_events_processed();
        debug_stats_increment_script_executions();

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
            .eval_expression_with_scope::<bool>(&mut scope, &compiled.expr)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = if let Some(ref debug_tracker) = self.debug_tracker {
                    let enhancer = ErrorEnhancer::new(debug_tracker.config.clone());
                    let context = debug_tracker.get_context();
                    enhancer.enhance_error(&e, &scope, &compiled.expr, "filter", &context)
                } else {
                    Self::format_rhai_error(e, "filter expression", &compiled.expr)
                };
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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
        let mut scope = self.create_scope_for_event(event);

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

                let detailed_msg = if let Some(ref debug_tracker) = self.debug_tracker {
                    let enhancer = ErrorEnhancer::new(debug_tracker.config.clone());
                    let context = debug_tracker.get_context();
                    enhancer.enhance_error(&e, &scope, &compiled.expr, "exec", &context)
                } else {
                    Self::format_rhai_error(e, "exec script", &compiled.expr)
                };
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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

        // Add state map (sequential mode) or dummy object (parallel mode)
        if crate::rhai_functions::strings::is_parallel_mode() {
            scope.push("state", crate::rhai_functions::state::StateNotAvailable);
        } else if let Some(ref state_map) = self.state_map {
            scope.push("state", state_map.clone());
        }

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_error(e, "begin expression", &compiled.expr);
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

        // Add state map (sequential mode) or dummy object (parallel mode)
        if crate::rhai_functions::strings::is_parallel_mode() {
            scope.push("state", crate::rhai_functions::state::StateNotAvailable);
        } else if let Some(ref state_map) = self.state_map {
            scope.push("state", state_map.clone());
        }

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_error(e, "end expression", &compiled.expr);
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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

        // Add state map (sequential mode) or dummy object (parallel mode)
        if crate::rhai_functions::strings::is_parallel_mode() {
            scope.push("state", crate::rhai_functions::state::StateNotAvailable);
        } else if let Some(ref state_map) = self.state_map {
            scope.push("state", state_map.clone());
        }

        crate::rhai_functions::file_ops::clear_pending_ops();

        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &compiled.ast)
            .map_err(|e| {
                let detailed_msg = Self::format_rhai_error(e, "span-close script", &compiled.expr);
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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
            .eval_expression_with_scope::<bool>(&mut scope, &compiled.expr)
            .map_err(|e| {
                // Track errors in debug statistics
                debug_stats_increment_errors();

                let detailed_msg = Self::format_rhai_error(e, "filter expression", &compiled.expr);
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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

                let detailed_msg = Self::format_rhai_error(e, "exec script", &compiled.expr);
                anyhow::anyhow!("{}", detailed_msg)
            })?;

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

    fn create_scope_for_event(&self, event: &Event) -> Scope<'_> {
        let mut scope = self.scope_template.clone();

        // Update built-in variables
        scope.set_value("line", event.original_line.clone());

        // Update event map for fields with invalid identifiers
        let mut event_map = rhai::Map::new();
        for (k, v) in &event.fields {
            event_map.insert(k.clone().into(), v.clone());
        }
        scope.set_value("e", event_map);

        // Update metadata
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

        // Add raw line to metadata
        meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));

        scope.set_value("meta", meta_map);

        // Set the frozen conf map
        if let Some(ref conf_map) = self.conf_map {
            scope.set_value("conf", conf_map.clone());
        }

        // Add state map (sequential mode) or dummy object (parallel mode)
        if crate::rhai_functions::strings::is_parallel_mode() {
            scope.push("state", crate::rhai_functions::state::StateNotAvailable);
        } else if let Some(ref state_map) = self.state_map {
            scope.push("state", state_map.clone());
        }

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
}
