#![allow(dead_code)] // Pipeline API exposes embedding/legacy hooks not all used by the current binary
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rhai::Dynamic;
use std::collections::{HashMap, HashSet};

use crate::engine::RhaiEngine;
use crate::event::{Event, SpanStatus};
use crate::rhai_functions::file_ops::{self, FileOp};
use span::SpanProcessor;

// Re-export submodules
pub mod builders;
pub mod defaults;
pub mod multiline;
pub mod prefix_extractor;
pub mod prefix_parser;
pub mod section_selector;
mod span;
pub mod stages;

// Re-export main types for convenience
pub use builders::*;
pub use defaults::*;
pub use multiline::*;
pub use prefix_extractor::*;
pub use prefix_parser::*;
pub use section_selector::*;
pub use stages::*;

/// Formatted output from the pipeline with optional timestamp metadata
#[derive(Debug, Clone)]
pub struct FormattedOutput {
    pub line: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub file_ops: Vec<FileOp>,
}

impl FormattedOutput {
    pub fn new(line: String, timestamp: Option<DateTime<Utc>>) -> Self {
        Self {
            line,
            timestamp,
            file_ops: Vec::new(),
        }
    }

    pub fn with_ops(line: String, timestamp: Option<DateTime<Utc>>, file_ops: Vec<FileOp>) -> Self {
        Self {
            line,
            timestamp,
            file_ops,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InternalStats {
    pub lines_output: u64,
    pub lines_errors: u64,
    pub events_created: u64,
    pub events_output: u64,
    pub events_filtered: u64,
    pub discovered_levels: HashSet<String>,
    pub discovered_keys: HashSet<String>,
    pub discovered_levels_output: HashSet<String>,
    pub discovered_keys_output: HashSet<String>,
}

/// Helper function to collect discovered levels and keys from an event for stats
fn collect_discovered_levels_and_keys(event: &Event, ctx: &mut PipelineContext) {
    if !crate::stats::stats_enabled() {
        return;
    }
    // Collect discovered level
    for level_field_name in crate::event::LEVEL_FIELD_NAMES {
        if let Some(value) = event.fields.get(*level_field_name) {
            if let Ok(level_str) = value.clone().into_string() {
                if !level_str.is_empty() && ctx.discovered_levels.insert(level_str.clone()) {
                    ctx.internal_stats
                        .discovered_levels
                        .insert(level_str.clone());
                    crate::stats::stats_add_discovered_level(level_str.clone());

                    break; // Only take the first level field found
                }
            }
        }
    }

    // Collect discovered keys
    for field_key in event.fields.keys() {
        if ctx.discovered_keys.insert(field_key.clone()) {
            ctx.internal_stats.discovered_keys.insert(field_key.clone());
            crate::stats::stats_add_discovered_key(field_key.clone());
        }
    }
}

/// Helper function to collect output levels and keys for stats (after filtering)
fn collect_output_levels_and_keys(event: &Event, ctx: &mut PipelineContext) {
    if !crate::stats::stats_enabled() {
        return;
    }

    // Collect output level
    for level_field_name in crate::event::LEVEL_FIELD_NAMES {
        if let Some(value) = event.fields.get(*level_field_name) {
            if let Ok(level_str) = value.clone().into_string() {
                if !level_str.is_empty() && ctx.discovered_levels_output.insert(level_str.clone()) {
                    ctx.internal_stats
                        .discovered_levels_output
                        .insert(level_str.clone());

                    // Add to thread-local stats (for sequential)
                    crate::stats::stats_add_output_level(level_str);

                    break; // Only take the first level field found
                }
            }
        }
    }

    // Collect output keys
    for field_key in event.fields.keys() {
        if ctx.discovered_keys_output.insert(field_key.clone()) {
            ctx.internal_stats
                .discovered_keys_output
                .insert(field_key.clone());

            // Add to thread-local stats (for sequential)
            crate::stats::stats_add_output_key(field_key.clone());
        }
    }
}

/// Core pipeline result types
#[derive(Debug, Clone)]
pub enum ScriptResult {
    Skip,
    Emit(Event),
    EmitMultiple(Vec<Event>), // For future emit_each() support
    Error(String),
}

impl ScriptResult {
    /// Try to unwrap the event from Emit variant, returns error if not Emit
    pub fn try_unwrap_emit(self) -> Result<Event> {
        match self {
            ScriptResult::Emit(event) => Ok(event),
            ScriptResult::Skip => Err(anyhow::anyhow!("Expected ScriptResult::Emit, got Skip")),
            ScriptResult::EmitMultiple(_) => Err(anyhow::anyhow!(
                "Expected ScriptResult::Emit, got EmitMultiple"
            )),
            ScriptResult::Error(msg) => Err(anyhow::anyhow!(
                "Expected ScriptResult::Emit, got Error: {}",
                msg
            )),
        }
    }
}

/// Shared context passed between pipeline stages
pub struct PipelineContext {
    pub config: PipelineConfig,
    pub tracker: HashMap<String, Dynamic>,
    pub internal_tracker: HashMap<String, Dynamic>,
    pub internal_stats: InternalStats,
    pub window: Vec<Event>, // window[0] = current event, rest are previous
    pub rhai: RhaiEngine,
    pub meta: MetaData,
    pub pending_file_ops: Vec<FileOp>,
    pub discovered_levels: HashSet<String>,
    pub discovered_keys: HashSet<String>,
    pub discovered_levels_output: HashSet<String>,
    pub discovered_keys_output: HashSet<String>,
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub brief: bool,
    pub wrap: bool,
    pub pretty: bool,
    pub color_mode: crate::config::ColorMode,
    /// Timestamp formatting configuration (display-only)
    pub timestamp_formatting: crate::config::TimestampFormatConfig,
    /// Exit on first error (fail-fast behavior) - new resiliency model
    pub strict: bool,
    /// Show detailed error information - new resiliency model (levels: 0-3)
    pub verbose: u8,
    /// Suppress formatter/event output
    pub quiet_events: bool,
    /// Suppress diagnostics and summaries
    pub suppress_diagnostics: bool,
    /// Suppress all stdout/stderr emitters except the fatal line
    pub silent: bool,
    /// Suppress Rhai print/eprint and side-effect warnings
    pub suppress_script_output: bool,
    /// Legacy quiet level (derived)
    pub quiet_level: u8,
    /// Emoji mode for error output
    pub emoji_mode: crate::config::EmojiMode,
    /// Input files for smart error message formatting
    pub input_files: Vec<String>,
    /// Allow Rhai scripts to create directories and write files on disk
    pub allow_fs_writes: bool,
    /// Format name (for error reporting)
    pub format_name: Option<String>,
}

/// Metadata about current processing context
#[derive(Debug, Clone, Default)]
pub struct MetaData {
    pub filename: Option<String>,
    pub line_num: Option<usize>,
    pub span_status: Option<crate::event::SpanStatus>,
    pub span_id: Option<String>,
    pub span_start: Option<DateTime<Utc>>,
    pub span_end: Option<DateTime<Utc>>,
}

/// Core pipeline traits
///
/// Parse raw text lines into structured events
pub trait EventParser: Send + Sync {
    fn parse(&self, line: &str) -> Result<Event>;
}

/// Optional line-level filtering before parsing
pub trait LineFilter: Send {
    fn should_keep(&self, line: &str) -> bool;
}

/// Handle multi-line log records (future feature)
pub trait Chunker: Send {
    fn feed_line(&mut self, line: String) -> Option<String>;
    fn flush(&mut self) -> Option<String>;
    fn has_pending(&self) -> bool;
}

/// Manage sliding window of events (future feature)
pub trait WindowManager: Send {
    fn get_window(&self) -> Vec<Event>; // includes current as window[0]
    fn update(&mut self, current: &Event);
}

/// Core script processing stage (filters, execs, etc.)
pub trait ScriptStage: Send {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult;
}

/// Optional event limiting (--take N)
pub trait EventLimiter: Send {
    fn allow(&mut self) -> bool;
    fn is_exhausted(&self) -> bool;
}

/// Format events for output
pub trait Formatter: Send + Sync {
    fn format(&self, event: &Event) -> String;

    /// Flush any pending formatter state at the end of processing
    fn finish(&self) -> Option<String> {
        None
    }
}

/// Write formatted output
pub trait OutputWriter: Send {
    fn write(&mut self, line: &str) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
}

/// Main pipeline structure
pub struct Pipeline {
    pub line_filter: Option<Box<dyn LineFilter>>,
    pub chunker: Box<dyn Chunker>,
    pub parser: Box<dyn EventParser>,
    pub script_stages: Vec<Box<dyn ScriptStage>>,
    pub limiter: Option<Box<dyn EventLimiter>>,
    pub formatter: Box<dyn Formatter>,
    pub output: Box<dyn OutputWriter>,
    pub window_manager: Box<dyn WindowManager>,
    pub span_processor: Option<SpanProcessor>,
    pub ts_config: crate::timestamp::TsConfig,
}

impl Pipeline {
    /// Process a single line through the entire pipeline
    /// This is the core method used by both sequential and parallel processing
    pub fn process_line(
        &mut self,
        line: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        // Line filter stage
        if let Some(filter) = &self.line_filter {
            if !filter.should_keep(&line) {
                return Ok(Vec::new());
            }
        }

        // Chunker stage (for multi-line records)
        if let Some(chunk) = self.chunker.feed_line(line) {
            self.process_chunk(chunk, ctx)
        } else {
            Ok(Vec::new())
        }
    }

    /// Flush any remaining chunks from the chunker
    pub fn flush(&mut self, ctx: &mut PipelineContext) -> Result<Vec<FormattedOutput>> {
        if let Some(chunk) = self.chunker.flush() {
            // Process chunk directly, not through feed_line
            self.process_chunk_directly(chunk, ctx)
        } else {
            Ok(Vec::new())
        }
    }

    /// Process a complete event string (for pre-chunked multiline events)
    /// Skips the chunking stage and goes directly to parsing
    pub fn process_event_string(
        &mut self,
        event_string: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        self.process_chunk_directly(event_string, ctx)
    }

    /// Flush formatter state to emit any remaining buffered output
    pub fn finish_formatter(&self) -> Option<FormattedOutput> {
        self.formatter
            .finish()
            .map(|line| FormattedOutput::new(line, None))
    }

    pub fn finish_spans(&mut self, ctx: &mut PipelineContext) -> Result<()> {
        if let Some(span_processor) = self.span_processor.as_mut() {
            span_processor.finish(ctx)?;
        }
        Ok(())
    }

    fn apply_script_result(
        &mut self,
        result: ScriptResult,
        ctx: &mut PipelineContext,
        outputs: &mut Vec<FormattedOutput>,
    ) -> Result<()> {
        match result {
            ScriptResult::Emit(event) => {
                let ops = std::mem::take(&mut ctx.pending_file_ops);
                self.apply_single_event(event, ctx, outputs, ops)?;

                if let Some(span) = self.span_processor.as_mut() {
                    span.complete_pending();
                }
            }
            ScriptResult::EmitMultiple(events) => {
                let mut ops = std::mem::take(&mut ctx.pending_file_ops);

                for (idx, event) in events.into_iter().enumerate() {
                    let event_ops = if idx == 0 {
                        std::mem::take(&mut ops)
                    } else {
                        Vec::new()
                    };
                    self.apply_single_event(event, ctx, outputs, event_ops)?;
                }

                if !ops.is_empty() {
                    outputs.push(FormattedOutput::with_ops(String::new(), None, ops));
                }

                if let Some(span) = self.span_processor.as_mut() {
                    span.complete_pending();
                }
            }
            ScriptResult::Skip => {
                crate::stats::stats_add_event_filtered();
                ctx.internal_stats.events_filtered += 1;

                if let Some(span) = self.span_processor.as_mut() {
                    span.handle_skip(ctx);
                    span.complete_pending();
                }

                let ops = std::mem::take(&mut ctx.pending_file_ops);
                if !ops.is_empty() {
                    outputs.push(FormattedOutput::with_ops(String::new(), None, ops));
                }
            }
            ScriptResult::Error(msg) => {
                ctx.pending_file_ops.clear();
                file_ops::clear_pending_ops();

                if let Some(span) = self.span_processor.as_mut() {
                    span.complete_pending();
                }

                crate::rhai_functions::tracking::track_error(
                    "script",
                    ctx.meta.line_num,
                    &msg,
                    None,
                    ctx.meta.filename.as_deref(),
                    ctx.config.verbose,
                    ctx.config.quiet_level,
                    Some(&ctx.config),
                    None,
                );

                return Err(anyhow!(msg));
            }
        }

        Ok(())
    }

    fn apply_single_event(
        &mut self,
        mut event: Event,
        ctx: &mut PipelineContext,
        outputs: &mut Vec<FormattedOutput>,
        ops: Vec<FileOp>,
    ) -> Result<()> {
        if let Some(span) = self.span_processor.as_mut() {
            span.prepare_emitted_event(&mut event);
        }

        if self.limiter.as_mut().is_none_or(|l| l.allow()) {
            if event.fields.is_empty() {
                event.span.status = Some(SpanStatus::Filtered);
                crate::stats::stats_add_event_filtered();
                ctx.internal_stats.events_filtered += 1;

                if let Some(span) = self.span_processor.as_mut() {
                    span.handle_skip(ctx);
                }

                if !ops.is_empty() {
                    outputs.push(FormattedOutput::with_ops(String::new(), None, ops));
                }
            } else {
                crate::stats::stats_add_event_output();
                ctx.internal_stats.events_output += 1;

                // Collect output levels and keys for stats
                collect_output_levels_and_keys(&event, ctx);

                // Field discovery: observe output fields (post-filter)
                if crate::field_discovery::is_enabled()
                    && crate::field_discovery::is_discover_final()
                {
                    crate::field_discovery::observe_event_fields(&event.fields);
                }

                // Refresh parsed_ts after script stages so stats and output both see the
                // final timestamp value without cloning the whole event.
                event.parsed_ts = None;
                event.extract_timestamp_with_config(None, &self.ts_config);
                if let Some(result_ts) = event.parsed_ts {
                    crate::stats::stats_update_result_timestamp(result_ts);
                }

                if let Some(span) = self.span_processor.as_mut() {
                    span.record_emitted_event(&event, ctx)?;
                }

                let formatted = self.formatter.format(&event);
                let timestamp = event.parsed_ts;
                outputs.push(FormattedOutput::with_ops(formatted, timestamp, ops));
            }
        } else {
            crate::stats::stats_add_event_filtered();
            ctx.internal_stats.events_filtered += 1;

            event.span.status = Some(SpanStatus::Filtered);
            if let Some(span) = self.span_processor.as_mut() {
                span.handle_skip(ctx);
            }

            if !ops.is_empty() {
                outputs.push(FormattedOutput::with_ops(String::new(), None, ops));
            }
        }

        Ok(())
    }

    /// Process a chunk directly without going through the chunker
    fn process_chunk_directly(
        &mut self,
        chunk: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        self.process_chunk(chunk, ctx)
    }

    fn process_chunk(
        &mut self,
        chunk: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        let mut results = Vec::new();

        // Parse stage
        let mut event = match self.parser.parse(&chunk) {
            Ok(mut e) => {
                // Event was successfully created from chunk
                crate::stats::stats_add_event_created();
                ctx.internal_stats.events_created += 1;

                // Track timestamp for time span statistics
                if let Some(ts) = e.parsed_ts {
                    crate::stats::stats_update_timestamp(ts);
                }

                // Collect discovered levels and keys for stats
                collect_discovered_levels_and_keys(&e, ctx);

                // Field discovery: observe input fields (pre-script)
                if crate::field_discovery::is_enabled()
                    && !crate::field_discovery::is_discover_final()
                {
                    crate::field_discovery::observe_event_fields(&e.fields);
                }

                // Copy metadata from context to event
                if let Some(line_num) = ctx.meta.line_num {
                    e.set_metadata(line_num, ctx.meta.filename.clone());
                }

                e
            }
            Err(err) => {
                crate::stats::stats_add_line_error();
                ctx.internal_stats.lines_errors += 1;

                // Use unified error tracking system
                crate::rhai_functions::tracking::track_error(
                    "parse",
                    ctx.meta.line_num,
                    &err.to_string(),
                    Some(&chunk),
                    ctx.meta.filename.as_deref(),
                    ctx.config.verbose,
                    ctx.config.quiet_level,
                    Some(&ctx.config),
                    ctx.config.format_name.as_deref(),
                );

                // New resiliency model: skip unparseable lines by default,
                // only propagate errors in strict mode
                if ctx.config.strict {
                    return Err(err);
                } else {
                    // Skip this line and continue processing
                    return Ok(results);
                }
            }
        };

        if let Some(span_processor) = self.span_processor.as_mut() {
            span_processor.prepare_event(&mut event, ctx)?;
        }

        // Update window manager
        self.window_manager.update(&event);
        ctx.window = self.window_manager.get_window();

        // Reset per-event skip flag for Rhai skip()
        crate::rhai_functions::process::clear_skip_request();

        file_ops::clear_pending_ops();
        ctx.pending_file_ops.clear();

        // Apply script stages (filters, execs, etc.)
        let mut result = ScriptResult::Emit(event);

        for stage in &mut self.script_stages {
            result = match result {
                ScriptResult::Emit(event) => stage.apply(event, ctx),
                ScriptResult::EmitMultiple(events) => {
                    // Process each event through remaining stages
                    let mut multi_results = Vec::new();
                    for event in events {
                        let original_line = event.original_line.clone(); // Capture before consuming
                        match stage.apply(event, ctx) {
                            ScriptResult::Emit(e) => multi_results.push(e),
                            ScriptResult::EmitMultiple(mut es) => multi_results.append(&mut es),
                            ScriptResult::Skip => {}
                            ScriptResult::Error(msg) => {
                                // Use unified error tracking system
                                crate::rhai_functions::tracking::track_error(
                                    "script",
                                    ctx.meta.line_num,
                                    &msg,
                                    Some(&original_line),
                                    ctx.meta.filename.as_deref(),
                                    ctx.config.verbose,
                                    ctx.config.quiet_level,
                                    Some(&ctx.config),
                                    None,
                                );

                                // New resiliency model: use strict flag
                                if ctx.config.strict {
                                    return Err(anyhow::anyhow!(msg));
                                } else {
                                    // Skip errors in resilient mode and continue processing
                                    return Ok(results);
                                }
                            }
                        }
                    }
                    ScriptResult::EmitMultiple(multi_results)
                }
                other => other, // Skip or Error, stop processing
            };

            match &result {
                ScriptResult::Skip | ScriptResult::Error(_) => break,
                _ => {}
            }
        }

        // Handle final result
        let remaining_ops = file_ops::take_pending_ops();
        if !remaining_ops.is_empty() {
            ctx.pending_file_ops.extend(remaining_ops);
        }

        self.apply_script_result(result, ctx, &mut results)?;

        Ok(results)
    }

    /// Check if the event limiter (--take N) is exhausted
    pub fn is_take_limit_exhausted(&self) -> bool {
        self.limiter.as_ref().is_some_and(|l| l.is_exhausted())
    }

    /// Check if the chunker currently holds a partial chunk that hasn't been emitted yet
    pub fn has_pending_chunk(&self) -> bool {
        self.chunker.has_pending()
    }
}
