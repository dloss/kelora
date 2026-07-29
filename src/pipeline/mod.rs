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
pub mod span_summary;
pub mod stages;
pub(crate) mod trace_presets;

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
    // Collect discovered level. The first level field that is present and
    // stringifies is authoritative — exactly the precedence LevelFilterStage
    // applies. We must stop at that field even when its value was already seen
    // (or is empty): otherwise a repeated primary level (e.g. a second `WARN`)
    // falls through to a lower-priority field like `severity` and records its
    // value ("high") as a level the `-l` filter can never match.
    for level_field_name in crate::event::LEVEL_FIELD_NAMES {
        if let Some(value) = event.fields.get(*level_field_name) {
            if let Ok(level_str) = value.clone().into_string() {
                if !level_str.is_empty() && ctx.discovered_levels.insert(level_str.clone()) {
                    ctx.internal_stats
                        .discovered_levels
                        .insert(level_str.clone());
                    crate::stats::stats_add_discovered_level(level_str.clone());
                }
                break; // Only the first present level field is authoritative
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

    // Collect output level. Same first-field-wins precedence as the input-side
    // collector and LevelFilterStage: stop at the first present level field so
    // lower-priority fields (e.g. `severity`) are never mistaken for the level.
    for level_field_name in crate::event::LEVEL_FIELD_NAMES {
        if let Some(value) = event.fields.get(*level_field_name) {
            if let Ok(level_str) = value.clone().into_string() {
                if !level_str.is_empty() && ctx.discovered_levels_output.insert(level_str.clone()) {
                    ctx.internal_stats
                        .discovered_levels_output
                        .insert(level_str.clone());

                    // Add to thread-local stats (for sequential)
                    crate::stats::stats_add_output_level(level_str);
                }
                break; // Only the first present level field is authoritative
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
    /// `--span-summary` rows for spans that closed since the last drain.
    ///
    /// Spans close from three places, only two of which are near an `outputs`
    /// vec, so rows queue here and are drained at the one choke point every
    /// input path funnels through (`process_chunk`) plus once in the final
    /// flush. Ordering therefore stays "row after the window's last event".
    pub pending_span_rows: Vec<String>,
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub brief: bool,
    pub wrap: crate::config::WrapMode,
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
    /// Suppress warnings 🔸
    pub suppress_warnings: bool,
    /// Suppress hints 💡
    pub suppress_hints: bool,
    /// Suppress all stdout/stderr emitters except the fatal line
    pub silent: bool,
    /// Suppress Rhai print/eprint and side-effect warnings
    pub suppress_script_output: bool,
    /// Legacy quiet level (derived)
    pub quiet_level: u8,
    /// Emoji mode for error output
    pub emoji_mode: crate::config::EmojiMode,
    /// Legend mode for map output formatters (levelmap/keymap/tailmap)
    pub legend_mode: crate::config::LegendMode,
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
    /// True when the record being processed was assembled from more than one
    /// physical line (a multiline strategy grouped it). A parse failure on such
    /// a record under a one-record-per-line grammar points at the multiline
    /// settings rather than at the data, and the error summary says so.
    pub multiline_assembled: bool,
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

    /// Parse a line, materializing only the fields the projection asks for.
    ///
    /// The default ignores the projection and materializes every field, which
    /// is always correct (a downstream `KeyFilterStage` drops the extras). A
    /// parser that can cheaply skip building `Dynamic` values for unwanted keys
    /// overrides this *and* [`EventParser::supports_projection`]. Callers must
    /// only pass a non-`All` projection to a parser that reports support; the
    /// pipeline builder enforces this via the safety gate.
    fn parse_projected(
        &self,
        line: &str,
        _projection: &crate::projection::Projection,
    ) -> Result<Event> {
        self.parse(line)
    }

    /// Whether this parser honors a non-`All` [`crate::projection::Projection`]
    /// in [`EventParser::parse_projected`]. Defaults to `false` (fail safe): an
    /// unsupported parser forces the whole pipeline to `Projection::All`, so
    /// projection can never silently change results for a parser that ignores
    /// it. A parser (or wrapper) that overrides `parse_projected` must report
    /// `true` here — wrappers delegate to their inner parser.
    fn supports_projection(&self) -> bool {
        false
    }

    /// Whether this parser extracts the log level as text that appears
    /// *verbatim* in the raw line. When true, a line whose level field will
    /// match `--levels` is guaranteed to contain that level token as a
    /// case-insensitive substring, which lets the raw-line level pre-filter
    /// safely drop non-matching lines before parsing (see
    /// `Pipeline::level_prefilter_needles`).
    ///
    /// Defaults to `false` (pre-filter disabled) — the safe choice. Enable it
    /// only for parsers whose level comes straight from the line text (e.g.
    /// `json`, `logfmt`). Parsers that *derive* the level from a non-textual
    /// encoding — syslog priority numbers, severity mapping tables, stream-name
    /// or default inference — must leave it `false`, or the pre-filter could
    /// drop a line that would have matched (a false negative).
    fn level_appears_verbatim(&self) -> bool {
        false
    }
}

/// Optional line-level filtering before parsing
pub trait LineFilter: Send {
    fn should_keep(&self, line: &str) -> bool;
}

/// One physical input line with its provenance, as fed to a [`Chunker`].
#[derive(Debug, Clone)]
pub struct ChunkLine {
    pub text: String,
    pub line_num: usize,
    pub filename: Option<String>,
}

/// One assembled logical record, carrying the provenance of its *first*
/// physical line so event metadata reports where the record started rather
/// than where it happened to be flushed.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub text: String,
    pub first_line_num: usize,
    pub filename: Option<String>,
    pub line_count: usize,
}

/// Group physical lines into logical records (multiline events, CSV records
/// with embedded newlines).
///
/// Completed records are appended to `out`: a single fed line may complete
/// zero, one, or several records (e.g. a line that both terminates the
/// previous event and forms a complete event itself), and an out-parameter
/// makes that unlosable — the previous `Option<String>` shape forced a
/// one-slot pending buffer that silently dropped any third record.
pub trait Chunker: Send {
    fn feed_line(&mut self, line: ChunkLine, out: &mut Vec<Chunk>);
    /// Flush any buffered partial record (file boundary, idle timeout, EOF).
    fn flush(&mut self, out: &mut Vec<Chunk>);
    fn has_pending(&self) -> bool;
    /// True once if a record was split by the line cap since the last call;
    /// the driver turns this into a once-per-run warning.
    fn take_cap_hit(&mut self) -> bool {
        false
    }
    /// True once when a language preset strategy (`--multiline java|python|go`)
    /// has repeatedly started events on timestamped header lines — the input
    /// has reliable headers, so the driver hints once that `--multiline
    /// timestamp` also keeps non-stacktrace continuation lines together.
    fn take_preset_ts_hint(&mut self) -> bool {
        false
    }
    /// True when every fed line is exactly one record (no buffering ever).
    /// Lets the pipeline skip the chunk-buffer machinery on the hot path.
    fn is_passthrough(&self) -> bool {
        false
    }
}

/// Manage sliding window of events (future feature)
pub trait WindowManager: Send {
    fn get_window(&self) -> Vec<Event>; // includes current as window[0]
    fn update(&mut self, current: &Event);
}

/// Core script processing stage (filters, execs, etc.)
pub trait ScriptStage: Send {
    fn apply(&mut self, event: Event, ctx: &mut PipelineContext) -> ScriptResult;

    /// Whether this stage reads the `window` variable. Used to skip per-event
    /// window maintenance entirely when no stage observes it.
    fn uses_window(&self) -> bool {
        false
    }

    /// The event fields this stage may observe, used to compute projection
    /// pushdown at pipeline construction (see [`crate::projection`]).
    ///
    /// Defaults to [`crate::projection::Demand::All`] so that a stage added
    /// without considering projection fails safe: it forces the whole pipeline
    /// to materialize every field, exactly as before projection existed. A
    /// stage that observes a bounded, statically known set of fields overrides
    /// this to return [`crate::projection::Demand::Fields`] (or `Nothing`).
    fn field_demands(&self) -> crate::projection::Demand {
        crate::projection::Demand::All
    }
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
    /// The `--since`/`--until` bounds, kept alongside the stage that enforces
    /// them so the emit path can tell when a printed event's final timestamp
    /// lies outside the window the user asked for (#345). `None` when no time
    /// window is set, which is also when the check is skipped entirely.
    pub timestamp_window: Option<crate::config::TimestampFilterConfig>,
    /// Whether per-event window maintenance is needed: true if `--window` was
    /// set or any script stage reads the `window` variable. When false, the
    /// window manager is never touched, avoiding two event clones per line.
    pub window_active: bool,
    /// Lowercased needles for the raw-line level pre-filter. Non-empty only when
    /// the safety gate (see `PipelineBuilder`) allows it: an include-only
    /// `--levels` filter that is the first stage, a verbatim-level parser, no
    /// context, and no observability feature that would diverge. When non-empty,
    /// an assembled chunk containing none of these needles (case-insensitive) is
    /// dropped before parsing, since its parsed level could not match `--levels`.
    /// Empty means the pre-filter is inert — one branch per line, no behavior
    /// change.
    pub level_prefilter_needles: Vec<Vec<u8>>,
    /// Which fields the parser must materialize (see [`crate::projection`]).
    /// [`crate::projection::Projection::All`] unless the safety gate at build
    /// time proved a bounded field set suffices, in which case the parser skips
    /// building `Dynamic` values for fields nothing downstream can observe.
    pub projection: crate::projection::Projection,
    /// Reusable buffer for chunker output, so the per-line hot path does not
    /// allocate a fresh Vec for every fed line.
    pub chunk_buf: Vec<Chunk>,
    /// Cached `chunker.is_passthrough()`: when true, `process_line` skips the
    /// chunk-buffer machinery entirely (measured ~3% on the line hot path).
    pub chunker_is_passthrough: bool,
}

impl Pipeline {
    /// Process a single line through the entire pipeline
    /// This is the core method used by both sequential and parallel processing
    pub fn process_line(
        &mut self,
        line: ChunkLine,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        // Line filter stage
        if let Some(filter) = &self.line_filter {
            if !filter.should_keep(&line.text) {
                return Ok(Vec::new());
            }
        }

        // Chunker stage (for multi-line records). Pass-through chunking is
        // the overwhelmingly common case; hand the line straight to the
        // parser without touching the chunk buffer.
        if self.chunker_is_passthrough {
            ctx.meta.line_num = Some(line.line_num);
            ctx.meta.filename = line.filename;
            ctx.meta.multiline_assembled = false;
            return self.process_chunk(line.text, ctx);
        }

        let mut chunks = std::mem::take(&mut self.chunk_buf);
        chunks.clear();
        self.chunker.feed_line(line, &mut chunks);
        self.process_chunks(chunks, ctx)
    }

    /// Flush any remaining chunks from the chunker
    pub fn flush(&mut self, ctx: &mut PipelineContext) -> Result<Vec<FormattedOutput>> {
        let mut chunks = std::mem::take(&mut self.chunk_buf);
        chunks.clear();
        self.chunker.flush(&mut chunks);
        self.process_chunks(chunks, ctx)
    }

    /// Run completed chunks through the parse/script/format stages, pointing
    /// event metadata at each chunk's first physical line. Returns the reusable
    /// chunk buffer to `self.chunk_buf` so the hot path stays allocation-free.
    fn process_chunks(
        &mut self,
        mut chunks: Vec<Chunk>,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        // Fast paths for the overwhelmingly common cases (pass-through
        // chunking yields exactly one chunk per line; buffering yields zero):
        // hand back process_chunk's Vec directly instead of copying it.
        if chunks.is_empty() {
            self.chunk_buf = chunks;
            return Ok(Vec::new());
        }
        if chunks.len() == 1 {
            let chunk = chunks.pop().expect("len checked");
            self.chunk_buf = chunks;
            ctx.meta.line_num = Some(chunk.first_line_num);
            ctx.meta.filename = chunk.filename;
            ctx.meta.multiline_assembled = chunk.line_count > 1;
            return self.process_chunk(chunk.text, ctx);
        }

        let mut outputs = Vec::new();
        let mut result = Ok(());
        for chunk in chunks.drain(..) {
            ctx.meta.line_num = Some(chunk.first_line_num);
            ctx.meta.filename = chunk.filename;
            ctx.meta.multiline_assembled = chunk.line_count > 1;
            match self.process_chunk(chunk.text, ctx) {
                Ok(formatted) => outputs.extend(formatted),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.chunk_buf = chunks;
        result.map(|_| outputs)
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

    /// Close the final open span and return any rows it produced, including
    /// those left queued by earlier closes.
    pub fn finish_spans(&mut self, ctx: &mut PipelineContext) -> Result<Vec<FormattedOutput>> {
        if let Some(span_processor) = self.span_processor.as_mut() {
            span_processor.finish(ctx)?;
        }
        let mut outputs = Vec::new();
        Self::drain_span_rows(ctx, &mut outputs);
        Ok(outputs)
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

                // Persist so the "script" count survives later engine calls
                // (see the parse error path).
                stages::persist_error_tracking(ctx);

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
                    // The window ran before the script stages and judged the
                    // *parser's* timestamp. This refreshed value is the one
                    // being printed, so when the two disagree the output
                    // carries a timestamp the user's window excluded — either a
                    // script rewrote the field, or the event was created after
                    // the window had already run. Count it here (free: the
                    // reparse above happens either way) and let the run report
                    // it once at the end (#345).
                    //
                    // Scope follows the claim the warning makes: this is about
                    // the timestamp kelora prints. An event whose timestamp
                    // field `--keys`/`--exclude-keys` already removed reparses
                    // to None and is not counted — there is no printed
                    // timestamp left to contradict the window, and the value
                    // the script wrote is gone by this point.
                    if let Some(window) = &self.timestamp_window {
                        let escaped = window.since.is_some_and(|since| result_ts < since)
                            || window.until.is_some_and(|until| result_ts > until);
                        if escaped {
                            crate::stats::stats_add_window_escaped_event();
                        }
                    }
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

    /// Move any queued `--span-summary` rows into the output stream.
    ///
    /// Rows are `FormattedOutput` data, so `quiet_events` (which only swaps in a
    /// null *formatter*) leaves them alone, exactly like `--freq`'s table. They
    /// carry no timestamp: `--gaps` measures distance between events, and a
    /// rollup row is not one.
    fn drain_span_rows(ctx: &mut PipelineContext, outputs: &mut Vec<FormattedOutput>) {
        if ctx.pending_span_rows.is_empty() {
            return;
        }
        for row in ctx.pending_span_rows.drain(..) {
            outputs.push(FormattedOutput::new(row, None));
        }
    }

    fn process_chunk(
        &mut self,
        chunk: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        let mut outputs = self.process_chunk_inner(chunk, ctx)?;
        Self::drain_span_rows(ctx, &mut outputs);
        Ok(outputs)
    }

    fn process_chunk_inner(
        &mut self,
        chunk: String,
        ctx: &mut PipelineContext,
    ) -> Result<Vec<FormattedOutput>> {
        // Raw-line level pre-filter. Runs here — after multiline assembly, so a
        // continuation line can never be lost — and before parsing, so a line
        // whose level cannot match `--levels` is dropped without the parse +
        // FieldMap allocation cost. Inert (empty needles) unless the safety gate
        // enabled it at build time, so the disabled cost is one branch per line.
        // A dropped line is skipped exactly like the pre-parse line filters
        // (--keep-lines/--ignore-lines): no event is created, so it does not
        // count toward event/parse stats. The gate guarantees those counters are
        // unobservable when the pre-filter is active.
        if !self.level_prefilter_needles.is_empty()
            && !stages::raw_line_matches_level_needles(
                chunk.as_bytes(),
                &self.level_prefilter_needles,
            )
        {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        // Parse stage
        let mut event = match self.parser.parse_projected(&chunk, &self.projection) {
            Ok(mut e) => {
                // Event was successfully created from chunk
                crate::stats::stats_add_event_created();
                ctx.internal_stats.events_created += 1;
                // Always-on parse-success counter (mirrors track_error("parse")):
                // a run where the parser never once succeeded but logged errors is
                // a wrong-format/unusable-input failure, surfaced via the exit code
                // independently of --stats collection. See stage_failed_completely.
                crate::rhai_functions::tracking::record_parse_success(&mut ctx.internal_tracker);

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
                crate::stats::stats_record_parse_error_sample(&chunk);
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

                // Provenance for the multiline hint in the error summary: this
                // record was glued together from several physical lines, so a
                // one-record-per-line grammar was never going to parse it — and
                // the resulting drop makes multiline grouping *lower* the event
                // count, which is the opposite of what the user asked for.
                if ctx.meta.multiline_assembled {
                    crate::rhai_functions::tracking::record_multiline_parse_error();
                }

                // track_error writes only the thread-local tracker; persist into
                // ctx so a later --filter/--exec engine call (which reinstalls
                // ctx.internal_tracker over the thread state) cannot wipe the
                // parse error count out of the summary and the exit-code gate.
                stages::persist_error_tracking(ctx);

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

        // Update window manager (skipped entirely when no stage observes the
        // `window` variable and --window was not set, avoiding two event clones).
        if self.window_active {
            self.window_manager.update(&event);
            ctx.window = self.window_manager.get_window();
        }

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

                                // This path keeps processing in resilient mode, so
                                // without persisting, a later engine call would wipe
                                // the "script" count — and the unrecoverable-script
                                // exit-code check would miss it.
                                stages::persist_error_tracking(ctx);

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

    /// True once if the multiline line cap split an event since the last call.
    pub fn take_multiline_cap_hit(&mut self) -> bool {
        self.chunker.take_cap_hit()
    }

    /// True once when a preset strategy keeps meeting timestamped headers.
    pub fn take_multiline_preset_ts_hint(&mut self) -> bool {
        self.chunker.take_preset_ts_hint()
    }
}
