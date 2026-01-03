use super::{PipelineContext, ScriptResult, ScriptStage};
use crate::config::TimestampFilterConfig;
use crate::engine::RhaiEngine;
use crate::event::Event;
use crate::rhai_functions::file_ops;
use crate::rhai_functions::{absorb, columns, emit};
use anyhow::Result;

/// Cached event along with whether it satisfied the stage filter.
struct ContextBufferEntry {
    event: Event,
    is_match: bool,
    context_type: crate::event::ContextType,
}

/// Filter stage implementation
pub struct FilterStage {
    compiled_filter: crate::engine::CompiledExpression,
    stage_number: usize,
    // Context processing state
    context_config: Option<crate::config::ContextConfig>,
    buffer: std::collections::VecDeque<ContextBufferEntry>,
    after_counter: usize,
    pending_output: std::collections::VecDeque<Event>,
}

impl FilterStage {
    pub fn new(filter: String, engine: &mut RhaiEngine) -> Result<Self> {
        let compiled_filter = engine.compile_filter(&filter)?;
        Ok(Self {
            compiled_filter,
            stage_number: 0,
            context_config: None,
            buffer: std::collections::VecDeque::new(),
            after_counter: 0,
            pending_output: std::collections::VecDeque::new(),
        })
    }

    pub fn with_stage_number(mut self, stage_number: usize) -> Self {
        self.stage_number = stage_number;
        self
    }

    pub fn with_context(mut self, context_config: crate::config::ContextConfig) -> Self {
        if context_config.is_active() {
            let buffer_capacity = context_config.before_context + context_config.after_context + 1;
            self.buffer = std::collections::VecDeque::with_capacity(buffer_capacity);
            self.context_config = Some(context_config);
        }
        self
    }

    fn has_context(&self) -> bool {
        self.context_config.as_ref().is_some_and(|c| c.is_active())
    }

    fn evaluate_filter(&mut self, event: &Event, ctx: &mut PipelineContext) -> Result<bool> {
        columns::set_parse_cols_strict(ctx.config.strict);
        absorb::set_absorb_strict(ctx.config.strict);

        file_ops::clear_pending_ops();

        let eval_result = if ctx.window.is_empty() {
            ctx.rhai.execute_compiled_filter(
                &self.compiled_filter,
                event,
                &mut ctx.tracker,
                &mut ctx.internal_tracker,
            )
        } else {
            ctx.rhai.execute_compiled_filter_with_window(
                &self.compiled_filter,
                event,
                &ctx.window,
                &mut ctx.tracker,
                &mut ctx.internal_tracker,
            )
        };

        match eval_result {
            Ok(value) => {
                let ops = file_ops::take_pending_ops();
                if !ops.is_empty() {
                    ctx.pending_file_ops.extend(ops);
                }
                Ok(value)
            }
            Err(err) => {
                file_ops::clear_pending_ops();
                Err(err)
            }
        }
    }

    fn process_with_context(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult {
        let (before_context, after_context) = {
            let config = self.context_config.as_ref().unwrap();
            (config.before_context, config.after_context)
        };

        // Handle pending output first
        if let Some(pending) = self.pending_output.pop_front() {
            self.pending_output.push_back(event);
            return ScriptResult::Emit(pending);
        }

        // Add event to buffer
        self.buffer.push_back(ContextBufferEntry {
            event: event.clone(),
            is_match: false,
            context_type: crate::event::ContextType::None,
        });

        // Check if current event matches filter
        let is_match = match self.evaluate_filter(&event, ctx) {
            Ok(result) => result,
            Err(e) => {
                crate::rhai_functions::tracking::track_error(
                    "filter",
                    ctx.meta.line_num,
                    &format!("Filter error: {}", e),
                    Some(&event.original_line),
                    ctx.meta.filename.as_deref(),
                    ctx.config.verbose,
                    ctx.config.quiet_level,
                    Some(&ctx.config),
                    None,
                );

                if e.downcast_ref::<crate::engine::ConfMutationError>()
                    .is_some()
                    || ctx.config.strict
                {
                    return ScriptResult::Error(format!("Filter error: {}", e));
                } else {
                    false // Filter errors evaluate to false in resilient mode
                }
            }
        };

        if crate::rhai_functions::process::take_skip_request() {
            return ScriptResult::Skip;
        }

        if let Some(last) = self.buffer.back_mut() {
            last.is_match = is_match;
            if is_match {
                last.context_type = crate::event::ContextType::Match;
            }
        }

        if is_match {
            // We have a match! Emit before-context, match, and prepare after-context
            let mut output_events = Vec::new();

            // Emit before-context lines
            let buffer_len = self.buffer.len();
            let start_idx = if buffer_len > before_context + 1 {
                buffer_len - before_context - 1
            } else {
                0
            };

            for i in start_idx..buffer_len - 1 {
                if let Some(buffered) = self.buffer.get_mut(i) {
                    let mut before_event = buffered.event.clone();
                    let context_type = if buffered.is_match {
                        crate::event::ContextType::Match
                    } else {
                        match buffered.context_type {
                            crate::event::ContextType::After | crate::event::ContextType::Both => {
                                crate::event::ContextType::Both
                            }
                            _ => crate::event::ContextType::Before,
                        }
                    };
                    buffered.context_type = context_type;
                    before_event.context_type = context_type;
                    output_events.push(before_event);
                }
            }

            // Emit the match itself
            let mut match_event = event;
            match_event.context_type = crate::event::ContextType::Match;
            output_events.push(match_event);

            // Set up after-context
            self.after_counter = after_context;

            // Keep buffer size manageable
            let max_buffer_size = before_context + after_context + 1;
            while self.buffer.len() > max_buffer_size {
                self.buffer.pop_front();
            }

            if output_events.len() == 1 {
                ScriptResult::Emit(output_events.into_iter().next().unwrap())
            } else {
                ScriptResult::EmitMultiple(output_events)
            }
        } else {
            // No match - treat as after-context if we're within an active window
            if self.after_counter > 0 {
                self.after_counter -= 1;
                let mut after_event = event;

                let updated_context_type = if let Some(last) = self.buffer.back_mut() {
                    last.context_type = match last.context_type {
                        crate::event::ContextType::Before | crate::event::ContextType::Both => {
                            crate::event::ContextType::Both
                        }
                        _ => crate::event::ContextType::After,
                    };
                    last.context_type
                } else {
                    crate::event::ContextType::After
                };
                after_event.context_type = updated_context_type;

                let max_buffer_size = before_context + after_context + 1;
                while self.buffer.len() > max_buffer_size {
                    self.buffer.pop_front();
                }

                return ScriptResult::Emit(after_event);
            }

            // Not a match, keep buffer size manageable
            let max_buffer_size = before_context + after_context + 1;
            while self.buffer.len() > max_buffer_size {
                self.buffer.pop_front();
            }
            ScriptResult::Skip
        }
    }
}

impl ScriptStage for FilterStage {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult {
        // Add stage-specific tracing
        if let Some(ref tracer) = ctx.rhai.get_execution_tracer() {
            tracer.trace_stage_execution(self.stage_number, "filter");
        }

        if self.has_context() {
            return self.process_with_context(event, ctx);
        }

        // Original non-context filtering logic
        let result = self.evaluate_filter(&event, ctx);

        match result {
            Ok(result) => {
                if crate::rhai_functions::process::take_skip_request() {
                    return ScriptResult::Skip;
                }

                if result {
                    ScriptResult::Emit(event)
                } else {
                    ScriptResult::Skip
                }
            }
            Err(e) => {
                crate::rhai_functions::tracking::track_error(
                    "filter",
                    ctx.meta.line_num,
                    &format!("Filter error: {}", e),
                    Some(&event.original_line),
                    ctx.meta.filename.as_deref(),
                    ctx.config.verbose,
                    ctx.config.quiet_level,
                    Some(&ctx.config),
                    None,
                );

                // New resiliency model: filter errors evaluate to false (Skip)
                // unless in strict mode, where they still propagate as errors
                if e.downcast_ref::<crate::engine::ConfMutationError>()
                    .is_some()
                    || ctx.config.strict
                {
                    ScriptResult::Error(format!("Filter error: {}", e))
                } else {
                    ScriptResult::Skip
                }
            }
        }
    }
}

/// Exec stage implementation
pub struct ExecStage {
    compiled_exec: crate::engine::CompiledExpression,
    stage_number: usize,
}

impl ExecStage {
    pub fn new(exec: String, engine: &mut RhaiEngine) -> Result<Self> {
        let compiled_exec = engine.compile_exec(&exec)?;
        Ok(Self {
            compiled_exec,
            stage_number: 0,
        })
    }

    pub fn with_stage_number(mut self, stage_number: usize) -> Self {
        self.stage_number = stage_number;
        self
    }
}

impl ScriptStage for ExecStage {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult {
        // Add stage-specific tracing
        if let Some(ref tracer) = ctx.rhai.get_execution_tracer() {
            tracer.trace_stage_execution(self.stage_number, "exec");
        }

        // Clear any previous emission state
        crate::rhai_functions::emit::clear_suppression_flag();

        // Atomic execution: work on a copy of the event for rollback behavior
        let mut event_copy = event.clone();

        columns::set_parse_cols_strict(ctx.config.strict);
        absorb::set_absorb_strict(ctx.config.strict);
        emit::set_emit_strict(ctx.config.strict);

        file_ops::clear_pending_ops();

        let result = if ctx.window.is_empty() {
            // No window context - use standard method
            ctx.rhai.execute_compiled_exec(
                &self.compiled_exec,
                &mut event_copy,
                &mut ctx.tracker,
                &mut ctx.internal_tracker,
            )
        } else {
            // Window context available - use window-aware method
            ctx.rhai.execute_compiled_exec_with_window(
                &self.compiled_exec,
                &mut event_copy,
                &ctx.window,
                &mut ctx.tracker,
                &mut ctx.internal_tracker,
            )
        };

        match result {
            Ok(()) => {
                let ops = file_ops::take_pending_ops();
                if !ops.is_empty() {
                    ctx.pending_file_ops.extend(ops);
                }

                if crate::rhai_functions::process::take_skip_request() {
                    // Drop any deferred emissions and suppression flags to avoid leaking into the next event
                    crate::rhai_functions::emit::clear_suppression_flag();
                    let _ = crate::rhai_functions::emit::get_and_clear_pending_emissions();
                    return ScriptResult::Skip;
                }

                // Check for deferred emissions from emit_each()
                let pending_emissions =
                    crate::rhai_functions::emit::get_and_clear_pending_emissions();
                let should_suppress = crate::rhai_functions::emit::should_suppress_current_event();

                if !pending_emissions.is_empty() {
                    // Convert pending emissions to events and emit them
                    let mut emitted_events = Vec::new();

                    for emission_map in pending_emissions {
                        let mut new_event =
                            Event::default_with_line(event_copy.original_line.clone());
                        new_event.line_num = event_copy.line_num;
                        new_event.filename = event_copy.filename.clone();

                        // Convert Rhai Map to Event fields
                        for (key, value) in emission_map {
                            new_event.fields.insert(key.to_string(), value);
                        }

                        emitted_events.push(new_event);
                    }

                    // Return multiple events - the first is primary, rest are additional
                    if should_suppress {
                        // Suppress original, return only emitted events
                        ScriptResult::EmitMultiple(emitted_events)
                    } else {
                        // Keep original and add emitted events
                        let mut all_events = vec![event_copy];
                        all_events.extend(emitted_events);
                        ScriptResult::EmitMultiple(all_events)
                    }
                } else if should_suppress {
                    // emit_each was called but no events were actually emitted
                    // Still suppress the original as per specification
                    ScriptResult::Skip
                } else {
                    // Normal execution: commit the modified event
                    ScriptResult::Emit(event_copy)
                }
            }
            Err(e) => {
                file_ops::clear_pending_ops();
                // Clear emission state on error
                crate::rhai_functions::emit::clear_suppression_flag();
                let _ = crate::rhai_functions::emit::get_and_clear_pending_emissions();

                let error_msg = format!("{:#}", e);

                // Extract the core error message and any suggestions from the enhanced diagnostic
                let error_lines = error_msg.lines();

                // Look for the "Error:" line (from enhanced error format)
                let base_error = error_lines
                    .clone()
                    .find(|line| line.trim().starts_with("Error:"))
                    .and_then(|line| line.trim().strip_prefix("Error:"))
                    .map(|s| s.trim())
                    // Fallback: look for "Rhai:" prefix (older format)
                    .or_else(|| {
                        error_msg
                            .lines()
                            .find(|line| line.trim().starts_with("Rhai:"))
                            .and_then(|line| line.trim().strip_prefix("Rhai:"))
                            .map(|s| s.trim())
                    })
                    // Final fallback: use full message
                    .unwrap_or(&error_msg);

                // Extract suggestion if present
                // Format is either "  💡 suggestion" (with emoji) or "  Hint: suggestion" (without)
                let suggestion = error_msg
                    .lines()
                    .find(|line| {
                        let trimmed = line.trim();
                        trimmed.starts_with("💡") || trimmed.starts_with("Hint:")
                    })
                    .map(|line| {
                        // Strip leading whitespace and prefix
                        line.trim()
                            .strip_prefix("💡")
                            .or_else(|| line.trim().strip_prefix("Hint:"))
                            .unwrap_or(line.trim())
                            .trim()
                            .to_string()
                    });

                // Try to identify which field is missing from the script
                let mut custom_message = None;
                if suggestion
                    .as_ref()
                    .is_some_and(|s| s.contains("Field is missing"))
                {
                    // Extract method/function name from the error (e.g., "trim" from "Function not found: trim (())")
                    let method_name = base_error
                        .strip_prefix("Function not found: ")
                        .and_then(|rest| rest.split_whitespace().next())
                        .and_then(|name| name.split('(').next());

                    // Simple pattern match: look for e.fieldname in the script
                    if let Some(field) =
                        self.compiled_exec
                            .source()
                            .split_whitespace()
                            .find_map(|token| {
                                if token.starts_with("e.") {
                                    // Extract field name: e.field -> field, e.field.method() -> field
                                    token
                                        .strip_prefix("e.")
                                        .and_then(|rest| {
                                            rest.split(|c: char| !c.is_alphanumeric() && c != '_')
                                                .next()
                                        })
                                        .filter(|f| !f.is_empty())
                                } else {
                                    None
                                }
                            })
                    {
                        // Create a cleaner error message that replaces the verbose Rhai error
                        custom_message = Some(if let Some(method) = method_name {
                            format!(
                                "Cannot call {}() - field '{}' is missing for this event. Use e.has(\"{}\") or e.get_path(\"{}\", default)",
                                method, field, field, field
                            )
                        } else {
                            format!(
                                "Field '{}' is missing for this event. Use e.has(\"{}\") or e.get_path(\"{}\", default)",
                                field, field, field
                            )
                        });
                    }
                }

                // Use custom message if we identified the field, otherwise combine base error with suggestion
                let error_for_summary = if let Some(msg) = custom_message {
                    msg
                } else if let Some(hint) = suggestion {
                    format!("{}. {}", base_error, hint)
                } else {
                    base_error.to_string()
                };

                crate::rhai_functions::tracking::track_error(
                    "exec",
                    ctx.meta.line_num,
                    &error_for_summary,
                    Some(&event.original_line),
                    ctx.meta.filename.as_deref(),
                    ctx.config.verbose,
                    ctx.config.quiet_level,
                    Some(&ctx.config),
                    None,
                );

                // New resiliency model: atomic rollback - return original event unchanged
                // unless in strict mode, where errors still propagate
                if e.downcast_ref::<crate::engine::ConfMutationError>()
                    .is_some()
                    || ctx.config.strict
                {
                    crate::rhai_functions::process::clear_skip_request();
                    ScriptResult::Error(error_msg.clone())
                } else {
                    crate::rhai_functions::process::clear_skip_request();
                    // Rollback: return original event unchanged
                    ScriptResult::Emit(event)
                }
            }
        }
    }
}

/// Begin stage for --begin expressions
pub struct BeginStage {
    compiled_begin: Option<crate::engine::CompiledExpression>,
}

impl BeginStage {
    pub fn new(begin: Option<String>, engine: &mut RhaiEngine) -> Result<Self> {
        let compiled_begin = if let Some(begin_expr) = begin {
            Some(engine.compile_begin(&begin_expr)?)
        } else {
            None
        };
        Ok(Self { compiled_begin })
    }

    pub fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        if let Some(ref compiled) = self.compiled_begin {
            columns::set_parse_cols_strict(ctx.config.strict);
            absorb::set_absorb_strict(ctx.config.strict);
            file_ops::clear_pending_ops();
            let _init_map = ctx.rhai.execute_compiled_begin(
                compiled,
                &mut ctx.tracker,
                &mut ctx.internal_tracker,
            )?;
            let ops = file_ops::take_pending_ops();
            file_ops::execute_ops(&ops)?;
            Ok(())
        } else {
            Ok(())
        }
    }
}

/// End stage for --end expressions
pub struct EndStage {
    compiled_end: Option<crate::engine::CompiledExpression>,
}

impl EndStage {
    pub fn new(end: Option<String>, engine: &mut RhaiEngine) -> Result<Self> {
        let compiled_end = if let Some(end_expr) = end {
            Some(engine.compile_end(&end_expr)?)
        } else {
            None
        };
        Ok(Self { compiled_end })
    }

    pub fn execute(&self, ctx: &PipelineContext) -> Result<()> {
        if let Some(ref compiled) = self.compiled_end {
            columns::set_parse_cols_strict(ctx.config.strict);
            absorb::set_absorb_strict(ctx.config.strict);
            file_ops::clear_pending_ops();
            ctx.rhai.execute_compiled_end(compiled, &ctx.tracker)?;
            let ops = file_ops::take_pending_ops();
            file_ops::execute_ops(&ops)
        } else {
            Ok(())
        }
    }
}

/// Level filtering stage for --levels and --exclude-levels options
pub struct LevelFilterStage {
    levels: Vec<String>,
    exclude_levels: Vec<String>,
    // Context processing state
    context_config: Option<crate::config::ContextConfig>,
    buffer: std::collections::VecDeque<ContextBufferEntry>,
    after_counter: usize,
    pending_output: std::collections::VecDeque<Event>,
}

impl LevelFilterStage {
    pub fn new(levels: Vec<String>, exclude_levels: Vec<String>) -> Self {
        Self {
            levels,
            exclude_levels,
            context_config: None,
            buffer: std::collections::VecDeque::new(),
            after_counter: 0,
            pending_output: std::collections::VecDeque::new(),
        }
    }

    /// Check if any filtering is needed
    pub fn is_active(&self) -> bool {
        !self.levels.is_empty() || !self.exclude_levels.is_empty()
    }

    pub fn with_context(mut self, context_config: crate::config::ContextConfig) -> Self {
        if context_config.is_active() {
            let buffer_capacity = context_config.before_context + context_config.after_context + 1;
            self.buffer = std::collections::VecDeque::with_capacity(buffer_capacity);
            self.context_config = Some(context_config);
        }
        self
    }

    fn has_context(&self) -> bool {
        self.context_config.as_ref().is_some_and(|c| c.is_active())
    }

    fn evaluate_level_filter(&self, event: &Event) -> bool {
        if !self.is_active() {
            return true;
        }

        // Get the level from the event fields map - check all possible level field names
        let event_level = {
            let mut found_level: Option<String> = None;

            // Check all known level field names in the event's fields map
            for level_field_name in crate::event::LEVEL_FIELD_NAMES {
                if let Some(value) = event.fields.get(*level_field_name) {
                    if let Ok(level_str) = value.clone().into_string() {
                        found_level = Some(level_str);
                        break;
                    }
                }
            }

            match found_level {
                Some(level) => level,
                None => {
                    // If no level field is found, check if we should include or exclude
                    if self.levels.is_empty() {
                        // Only exclude_levels specified, and no level found - include by default
                        return true;
                    } else {
                        // levels specified but no level found - exclude
                        return false;
                    }
                }
            }
        };

        // Apply exclude_levels first (higher priority) - case-insensitive
        if !self.exclude_levels.is_empty() {
            for exclude_level in &self.exclude_levels {
                if event_level.eq_ignore_ascii_case(exclude_level) {
                    return false;
                }
            }
        }

        // Apply levels filter - case-insensitive
        if !self.levels.is_empty() {
            for level in &self.levels {
                if event_level.eq_ignore_ascii_case(level) {
                    return true;
                }
            }
            // No match found in levels list - exclude
            return false;
        }

        // No levels specified, only exclude_levels - include by default
        true
    }

    fn process_with_context(&mut self, event: Event, _ctx: &mut PipelineContext) -> ScriptResult {
        let (before_context, after_context) = {
            let config = self.context_config.as_ref().unwrap();
            (config.before_context, config.after_context)
        };

        // Handle pending output first
        if let Some(pending) = self.pending_output.pop_front() {
            self.pending_output.push_back(event);
            return ScriptResult::Emit(pending);
        }

        // Add event to buffer
        self.buffer.push_back(ContextBufferEntry {
            event: event.clone(),
            is_match: false,
            context_type: crate::event::ContextType::None,
        });

        // Check if current event matches level filter
        let is_match = self.evaluate_level_filter(&event);

        if let Some(last) = self.buffer.back_mut() {
            last.is_match = is_match;
            if is_match {
                last.context_type = crate::event::ContextType::Match;
            }
        }

        if is_match {
            // We have a match! Emit before-context, match, and prepare after-context
            let mut output_events = Vec::new();

            // Emit before-context lines
            let buffer_len = self.buffer.len();
            let start_idx = if buffer_len > before_context + 1 {
                buffer_len - before_context - 1
            } else {
                0
            };

            for i in start_idx..buffer_len - 1 {
                if let Some(buffered) = self.buffer.get_mut(i) {
                    if !buffered.is_match {
                        continue;
                    }

                    let mut before_event = buffered.event.clone();
                    let context_type = match buffered.context_type {
                        crate::event::ContextType::After | crate::event::ContextType::Both => {
                            crate::event::ContextType::Both
                        }
                        crate::event::ContextType::Match => crate::event::ContextType::Match,
                        _ => crate::event::ContextType::Before,
                    };
                    buffered.context_type = context_type;
                    before_event.context_type = context_type;
                    output_events.push(before_event);
                }
            }

            // Emit the match itself
            let mut match_event = event;
            match_event.context_type = crate::event::ContextType::Match;
            output_events.push(match_event);

            // Set up after-context
            self.after_counter = after_context;

            // Keep buffer size manageable
            let max_buffer_size = before_context + after_context + 1;
            while self.buffer.len() > max_buffer_size {
                self.buffer.pop_front();
            }

            if output_events.len() == 1 {
                ScriptResult::Emit(output_events.into_iter().next().unwrap())
            } else {
                ScriptResult::EmitMultiple(output_events)
            }
        } else {
            if self.after_counter > 0 {
                self.after_counter -= 1;

                // Event doesn't pass the filter but still counts toward the after-context window
                let max_buffer_size = before_context + after_context + 1;
                while self.buffer.len() > max_buffer_size {
                    self.buffer.pop_front();
                }

                return ScriptResult::Skip;
            }

            // Not a match, keep buffer size manageable
            let max_buffer_size = before_context + after_context + 1;
            while self.buffer.len() > max_buffer_size {
                self.buffer.pop_front();
            }
            ScriptResult::Skip
        }
    }
}

impl ScriptStage for LevelFilterStage {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult {
        if !self.is_active() {
            return ScriptResult::Emit(event);
        }

        if self.has_context() {
            return self.process_with_context(event, ctx);
        }

        // Original non-context level filtering logic
        let is_match = self.evaluate_level_filter(&event);
        if is_match {
            ScriptResult::Emit(event)
        } else {
            ScriptResult::Skip
        }
    }
}

/// Key filtering stage for --keys and --exclude-keys options
pub struct KeyFilterStage {
    keys: Vec<String>,
    exclude_keys: Vec<String>,
}

impl KeyFilterStage {
    pub fn new(keys: Vec<String>, exclude_keys: Vec<String>) -> Self {
        Self { keys, exclude_keys }
    }

    /// Check if any filtering is needed
    pub fn is_active(&self) -> bool {
        !self.keys.is_empty() || !self.exclude_keys.is_empty()
    }
}

impl ScriptStage for KeyFilterStage {
    fn apply(&mut self, mut event: Event, _ctx: &mut PipelineContext) -> ScriptResult {
        if !self.is_active() {
            return ScriptResult::Emit(event);
        }

        // Get available keys from the event
        let available_keys: Vec<String> = event.fields.keys().cloned().collect();

        // Calculate effective keys preserving the order specified by self.keys
        let effective_keys = {
            let mut result_keys = if self.keys.is_empty() {
                // If no keys specified, start with all available keys
                available_keys
            } else {
                // If keys specified, iterate through self.keys and only include those that exist in the event
                // This preserves the order specified in self.keys rather than the original event order
                self.keys
                    .iter()
                    .filter(|key| available_keys.contains(key))
                    .cloned()
                    .collect()
            };

            // Apply exclusions (higher priority)
            result_keys.retain(|key| !self.exclude_keys.contains(key));

            result_keys
        };

        // Apply the filtering
        event.filter_keys(&effective_keys);

        // Only mark as key-filtered when the user explicitly requested an order via --keys.
        // Preserve caller-specified ordering only when --keys was provided.
        event.key_filtered = !self.keys.is_empty();

        // If any key filtering was applied and no fields remain, skip this event
        if self.is_active() && event.fields.is_empty() {
            ScriptResult::Skip
        } else {
            ScriptResult::Emit(event)
        }
    }
}

/// Drain template mining stage (sequential-only, summary-driven)
pub struct DrainStage {
    field_name: String,
}

impl DrainStage {
    pub fn new(field_name: String) -> Self {
        Self { field_name }
    }
}

impl ScriptStage for DrainStage {
    fn apply(&mut self, event: Event, _ctx: &mut PipelineContext) -> ScriptResult {
        if let Some(value) = event.fields.get(&self.field_name) {
            let text = if value.is_string() {
                value.clone().into_string().unwrap_or_default()
            } else {
                value.to_string()
            };

            if !text.is_empty() {
                if let Err(err) = crate::drain::drain_template(&text, None, event.line_num) {
                    return ScriptResult::Error(err);
                }
            }
        }

        ScriptResult::Emit(event)
    }
}

/// Timestamp filter stage for --since and --until filtering
pub struct TimestampFilterStage {
    config: TimestampFilterConfig,
}

impl TimestampFilterStage {
    pub fn new(config: TimestampFilterConfig) -> Self {
        Self { config }
    }
}

impl ScriptStage for TimestampFilterStage {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult {
        // Get the parsed timestamp from the event
        let event_timestamp = match event.parsed_ts {
            Some(ts) => ts,
            None => {
                // No timestamp available - use new resiliency model
                if ctx.config.strict {
                    // Stop processing on missing timestamp in strict mode
                    return ScriptResult::Error(
                        "Event has no valid timestamp for --since/--until filtering".to_string(),
                    );
                } else {
                    // Filter out events without valid timestamps (resilient mode)
                    return ScriptResult::Skip;
                }
            }
        };

        // Check since filter (event must be >= since)
        if let Some(since) = self.config.since {
            if event_timestamp < since {
                return ScriptResult::Skip;
            }
        }

        // Check until filter (event must be <= until)
        if let Some(until) = self.config.until {
            if event_timestamp > until {
                return ScriptResult::Skip;
            }
        }

        // Event is within the time range
        ScriptResult::Emit(event)
    }
}

/// Normalize the primary timestamp field to RFC3339 once scripts have run
pub struct TimestampConversionStage {
    ts_config: crate::timestamp::TsConfig,
}

impl TimestampConversionStage {
    pub fn new(
        ts_field: Option<String>,
        ts_format: Option<String>,
        default_timezone: Option<String>,
    ) -> Self {
        Self {
            ts_config: crate::timestamp::TsConfig {
                custom_field: ts_field,
                custom_format: ts_format,
                default_timezone,
            },
        }
    }

    fn target_field(&self, event: &Event) -> Option<String> {
        if let Some(ref custom_field) = self.ts_config.custom_field {
            if event.fields.contains_key(custom_field) {
                return Some(custom_field.clone());
            }
        }

        crate::timestamp::identify_timestamp_field(&event.fields, &self.ts_config)
            .map(|(field, _)| field)
    }
}

impl ScriptStage for TimestampConversionStage {
    fn apply(&mut self, mut event: Event, _ctx: &mut PipelineContext) -> ScriptResult {
        event.extract_timestamp_with_config(None, &self.ts_config);

        let parsed_ts = match event.parsed_ts {
            Some(ts) => ts,
            None => return ScriptResult::Emit(event),
        };

        if let Some(field_name) = self.target_field(&event) {
            if let Some(value) = event.fields.get_mut(&field_name) {
                *value = rhai::Dynamic::from(parsed_ts.to_rfc3339());
            }
        }

        ScriptResult::Emit(event)
    }
}

// ContextStage removed - context processing is now integrated into FilterStage and LevelFilterStage

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TimestampFilterConfig;
    use crate::pipeline::{MetaData, PipelineConfig};
    use chrono::{Duration, Utc};
    use rhai::Dynamic;

    fn default_pipeline_config() -> PipelineConfig {
        PipelineConfig {
            brief: false,
            wrap: true,
            pretty: false,
            color_mode: crate::config::ColorMode::Auto,
            timestamp_formatting: crate::config::TimestampFormatConfig::default(),
            strict: false,
            verbose: 0,
            quiet_events: false,
            suppress_diagnostics: false,
            silent: false,
            suppress_script_output: false,
            quiet_level: 0,
            emoji_mode: crate::config::EmojiMode::Auto,
            input_files: vec![],
            allow_fs_writes: false,
            format_name: None,
        }
    }

    fn ctx_with_engine(engine: crate::engine::RhaiEngine) -> PipelineContext {
        PipelineContext {
            config: default_pipeline_config(),
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: engine,
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn exec_stage_respects_skip_request() {
        crate::rhai_functions::process::clear_skip_request();
        crate::rhai_functions::emit::clear_suppression_flag();

        let mut engine = crate::engine::RhaiEngine::new();
        let script = r#"
            let derived = #{ message: "should be dropped" };
            emit_each([derived]);
            skip();
            e.value = 42;
        "#;
        let mut stage = ExecStage::new(script.to_string(), &mut engine)
            .expect("exec compilation should succeed");

        let mut ctx = ctx_with_engine(engine);
        let event = Event::default();

        let result = stage.apply(event, &mut ctx);
        assert!(matches!(result, ScriptResult::Skip));
        assert!(crate::rhai_functions::emit::get_and_clear_pending_emissions().is_empty());
        assert!(!crate::rhai_functions::emit::should_suppress_current_event());
        assert!(!crate::rhai_functions::process::is_skip_requested());
    }

    #[test]
    fn filter_stage_marks_overlapping_matches_as_match() {
        let mut engine = crate::engine::RhaiEngine::new();
        let mut stage = FilterStage::new("e.method == \"HEAD\"".to_string(), &mut engine)
            .expect("filter compilation should succeed")
            .with_context(crate::config::ContextConfig::new(1, 1));

        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true,
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: engine,
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        let methods = ["POST", "HEAD", "HEAD", "GET"];
        let mut outputs = Vec::new();

        for (idx, method) in methods.iter().enumerate() {
            let mut event = Event::default();
            event.set_field("method".to_string(), Dynamic::from((*method).to_string()));
            event.set_field("id".to_string(), Dynamic::from((idx + 1) as i64));

            match stage.apply(event, &mut ctx) {
                ScriptResult::Emit(emitted) => outputs.push(emitted),
                ScriptResult::EmitMultiple(mut many) => outputs.append(&mut many),
                ScriptResult::Skip => {}
                ScriptResult::Error(err) => panic!("unexpected filter error: {}", err),
            }
        }

        let get_method = |event: &Event| {
            event
                .fields
                .get("method")
                .and_then(|value| value.clone().try_cast::<String>())
        };

        let method_is_head = |event: &Event| get_method(event).as_deref() == Some("HEAD");

        let head_after_count = outputs.iter().filter(|event| {
            method_is_head(event) && event.context_type == crate::event::ContextType::After
        });
        assert_eq!(
            head_after_count.count(),
            0,
            "HEAD events that satisfy the filter must not be marked as after-context",
        );

        let head_before_count = outputs.iter().filter(|event| {
            method_is_head(event) && event.context_type == crate::event::ContextType::Before
        });
        assert_eq!(
            head_before_count.count(),
            0,
            "HEAD events that satisfy the filter must not be marked as before-context",
        );

        let second_head_match = outputs.iter().find(|event| {
            event
                .fields
                .get("id")
                .and_then(|value| value.clone().try_cast::<i64>())
                == Some(3)
                && event.context_type == crate::event::ContextType::Match
        });
        assert!(
            second_head_match.is_some(),
            "Expected the overlapping HEAD event to receive the match marker",
        );

        let first_head_match = outputs.iter().find(|event| {
            event
                .fields
                .get("id")
                .and_then(|value| value.clone().try_cast::<i64>())
                == Some(2)
                && event.context_type == crate::event::ContextType::Match
        });
        assert!(
            first_head_match.is_some(),
            "Expected the first HEAD event to retain the match marker when re-emitted as context",
        );
    }

    #[test]
    fn filter_stage_marks_overlapping_context_with_both_marker() {
        let mut engine = crate::engine::RhaiEngine::new();
        let mut stage = FilterStage::new("e.method == \"DELETE\"".to_string(), &mut engine)
            .expect("filter compilation should succeed")
            .with_context(crate::config::ContextConfig::new(1, 1));

        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true,
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: engine,
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        let methods = ["GET", "DELETE", "PUT", "DELETE"];
        let mut outputs = Vec::new();

        for (idx, method) in methods.iter().enumerate() {
            let mut event = Event::default();
            event.set_field("method".to_string(), Dynamic::from((*method).to_string()));
            event.set_field("ordinal".to_string(), Dynamic::from((idx + 1) as i64));

            match stage.apply(event, &mut ctx) {
                ScriptResult::Emit(emitted) => outputs.push(emitted),
                ScriptResult::EmitMultiple(mut many) => outputs.append(&mut many),
                ScriptResult::Skip => {}
                ScriptResult::Error(err) => panic!("unexpected filter error: {}", err),
            }
        }

        let put_events: Vec<_> = outputs
            .iter()
            .filter(|event| {
                event
                    .fields
                    .get("method")
                    .and_then(|value| value.clone().try_cast::<String>())
                    .as_deref()
                    == Some("PUT")
            })
            .collect();

        assert!(
            put_events
                .iter()
                .any(|event| event.context_type == crate::event::ContextType::After),
            "Expected PUT event to first appear as after-context",
        );

        assert!(
            put_events
                .iter()
                .any(|event| event.context_type == crate::event::ContextType::Both),
            "Expected PUT event to be re-emitted with the overlapping context marker",
        );
    }

    #[test]
    fn level_filter_context_respects_exclude_levels() {
        let mut stage =
            LevelFilterStage::new(vec![], vec!["debug".to_string(), "info".to_string()])
                .with_context(crate::config::ContextConfig::new(1, 0));

        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true,
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: crate::engine::RhaiEngine::new(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        let make_event = |level: &str, msg: &str| {
            let mut event = Event::default();
            event.set_field("level".to_string(), Dynamic::from(level.to_string()));
            event.set_field("msg".to_string(), Dynamic::from(msg.to_string()));
            event
        };

        let events = vec![
            make_event("debug", "debug message"),
            make_event("error", "error"),
            make_event("info", "info message"),
        ];

        let mut outputs = Vec::new();
        for event in events {
            match stage.apply(event, &mut ctx) {
                ScriptResult::Emit(emitted) => outputs.push(emitted),
                ScriptResult::EmitMultiple(mut many) => outputs.append(&mut many),
                ScriptResult::Skip => {}
                ScriptResult::Error(err) => panic!("unexpected level filter error: {}", err),
            }
        }

        assert!(outputs.iter().all(|event| {
            event
                .fields
                .get("level")
                .and_then(|value| value.clone().try_cast::<String>())
                .map(|level| level != "debug" && level != "info")
                .unwrap_or(true)
        }));

        assert!(outputs.iter().any(|event| {
            event
                .fields
                .get("level")
                .and_then(|value| value.clone().try_cast::<String>())
                == Some("error".to_string())
                && event.context_type == crate::event::ContextType::Match
        }));
    }

    #[test]
    fn test_timestamp_filter_stage_since() {
        let since = Utc::now() - Duration::hours(1);
        let config = TimestampFilterConfig {
            since: Some(since),
            until: None,
        };
        let mut stage = TimestampFilterStage::new(config);

        // Create dummy context
        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true, // Default to enabled
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: crate::engine::RhaiEngine::new(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        // Test event before since time (should be skipped)
        let old_event = crate::event::Event {
            parsed_ts: Some(since - Duration::minutes(30)),
            ..Default::default()
        };

        let result = stage.apply(old_event, &mut ctx);
        matches!(result, ScriptResult::Skip);

        // Test event after since time (should be emitted)
        let new_event = crate::event::Event {
            parsed_ts: Some(since + Duration::minutes(30)),
            ..Default::default()
        };

        let result = stage.apply(new_event, &mut ctx);
        matches!(result, ScriptResult::Emit(_));
    }

    #[test]
    fn test_timestamp_filter_stage_until() {
        let until = Utc::now() - Duration::hours(1);
        let config = TimestampFilterConfig {
            since: None,
            until: Some(until),
        };
        let mut stage = TimestampFilterStage::new(config);

        // Create dummy context
        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true, // Default to enabled
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: crate::engine::RhaiEngine::new(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        // Test event before until time (should be emitted)
        let old_event = crate::event::Event {
            parsed_ts: Some(until - Duration::minutes(30)),
            ..Default::default()
        };

        let result = stage.apply(old_event, &mut ctx);
        matches!(result, ScriptResult::Emit(_));

        // Test event after until time (should be skipped)
        let new_event = crate::event::Event {
            parsed_ts: Some(until + Duration::minutes(30)),
            ..Default::default()
        };

        let result = stage.apply(new_event, &mut ctx);
        matches!(result, ScriptResult::Skip);
    }

    #[test]
    fn test_timestamp_filter_stage_no_timestamp() {
        let config = TimestampFilterConfig {
            since: Some(Utc::now() - Duration::hours(1)),
            until: Some(Utc::now() + Duration::hours(1)),
        };
        let mut stage = TimestampFilterStage::new(config);

        // Create dummy context
        let mut ctx = PipelineContext {
            config: PipelineConfig {
                brief: false,
                wrap: true, // Default to enabled
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: vec![],
                allow_fs_writes: false,
                format_name: None,
            },
            tracker: std::collections::HashMap::new(),
            internal_tracker: std::collections::HashMap::new(),
            window: Vec::new(),
            rhai: crate::engine::RhaiEngine::new(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        // Test event without timestamp (should be emitted - pass through behavior)
        let event_no_ts = crate::event::Event {
            parsed_ts: None,
            ..Default::default()
        };

        let result = stage.apply(event_no_ts, &mut ctx);
        matches!(result, ScriptResult::Emit(_));
    }
}
