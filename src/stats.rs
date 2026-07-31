use crate::rhai_functions::datetime::DurationWrapper;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct TimestampFieldStat {
    pub detected: usize,
    pub parsed: usize,
}

/// Minimal abstraction over the two count-map types used for grouping counters
/// (`std::HashMap` and `IndexMap`), so [`bump`] can serve both without a macro.
trait CountMap {
    fn get_count_mut(&mut self, key: &str) -> Option<&mut usize>;
    fn insert_count(&mut self, key: String, value: usize);
}

impl CountMap for HashMap<String, usize> {
    fn get_count_mut(&mut self, key: &str) -> Option<&mut usize> {
        self.get_mut(key)
    }
    fn insert_count(&mut self, key: String, value: usize) {
        self.insert(key, value);
    }
}

impl CountMap for IndexMap<String, usize> {
    fn get_count_mut(&mut self, key: &str) -> Option<&mut usize> {
        self.get_mut(key)
    }
    fn insert_count(&mut self, key: String, value: usize) {
        self.insert(key, value);
    }
}

/// Increment the counter for `key`, allocating an owned key only on the miss
/// path. Replaces the `entry(key.to_string()).or_insert(0) += 1` pattern, which
/// allocated a fresh `String` on every call regardless of whether the key was
/// already present. Group keys here are drawn from tiny closed vocabularies
/// (formats, assertion expressions), so the hit path is overwhelmingly common
/// and now costs a borrowed hash lookup rather than an allocation.
fn bump<M: CountMap>(map: &mut M, key: &str) {
    if let Some(count) = map.get_count_mut(key) {
        *count += 1;
    } else {
        map.insert_count(key.to_string(), 1);
    }
}

/// Statistics collected during log processing
#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    pub lines_read: usize,
    pub lines_output: usize,
    pub lines_filtered: usize,
    pub lines_errors: usize, // Parse errors (regardless of error handling strategy)
    pub events_created: usize,
    pub events_output: usize,
    pub events_filtered: usize,
    pub late_events: usize,
    pub files_processed: usize,
    pub files_failed_to_open: usize, // Files that failed to open (I/O errors)
    pub failed_file_samples: Vec<String>,
    pub recoverable_error_samples: Vec<String>,
    pub script_executions: usize,
    pub errors: usize, // Kept for backward compatibility, but lines_errors is more specific
    pub processing_time: Duration,
    pub start_time: Option<Instant>,
    pub discovered_levels: BTreeSet<String>,
    pub discovered_keys: BTreeSet<String>,
    pub discovered_levels_output: BTreeSet<String>,
    pub discovered_keys_output: BTreeSet<String>,
    pub first_timestamp: Option<DateTime<Utc>>,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub first_result_timestamp: Option<DateTime<Utc>>,
    pub last_result_timestamp: Option<DateTime<Utc>>,
    pub timestamp_detected_events: usize,
    pub timestamp_parsed_events: usize,
    pub timestamp_absent_events: usize,
    pub timestamp_fields: IndexMap<String, TimestampFieldStat>,
    pub timestamp_override_field: Option<String>,
    pub timestamp_override_format: Option<String>,
    pub timestamp_override_failed: bool,
    pub timestamp_override_warning: Option<String>,
    pub yearless_timestamps: usize, // Count of timestamps parsed with year inference
    /// Count of naive timestamps (no zone offset) resolved using the default
    /// timezone. Drives the #287 diagnostic that surfaces the silent UTC
    /// assumption; the explicit-vs-default gate is applied at emit time.
    pub naive_timestamps: usize,
    /// Count of printed events whose timestamp, re-read after the script
    /// stages, falls outside the `--since`/`--until` window. The window runs
    /// before the script stages and reads the parser's timestamp, so a script
    /// can neither move an event into the window nor out of it; this counts the
    /// events where that gap became visible in the output (#345).
    pub window_escaped_events: usize,
    pub detected_format: Option<String>, // Format detected for this processing session
    pub detected_format_counts: IndexMap<String, usize>, // Per-file detected format counts
    /// Per-format event counts when running in cascade mode. Empty otherwise.
    /// Keyed by the short format name used in `_format` (e.g. "json", "line").
    pub cascade_format_counts: IndexMap<String, usize>,
    /// Number of cascade events whose parsed record already carried a field
    /// literally named `_format`, so the cascade tag was skipped to keep the
    /// log's own value. A recovery, not an error: exit code stays 0 (#406).
    pub cascade_format_collisions: usize,
    pub assertion_failures: usize, // Total assertion failures
    pub assertion_failures_by_expr: HashMap<String, usize>, // Per-assertion tracking
    pub csv_rows_extra_columns: usize, // CSV/TSV rows wider than the header (extras kept as cN)
    pub csv_rows_missing_columns: usize, // CSV/TSV rows narrower than the header (fields absent)
    pub csv_overflow_start_column: Option<usize>, // Lowest 1-based column where overflow began
    /// First raw line that failed to parse, captured for diagnostics. Used to
    /// re-detect a likely secondary format when auto-detection locked onto one
    /// format but the input turned out to be mixed (see detection.rs).
    pub first_parse_error_sample: Option<String>,
    /// Number of input lines that contained invalid UTF-8 and were decoded
    /// losslessly (`U+FFFD` substitution). Surfaced as a diagnostic so recovery
    /// is visible rather than silent; does not count as an error (see #239).
    pub decode_warnings: usize,
    /// First line where a UTF-8 replacement occurred, captured for diagnostics.
    pub first_decode_warning_sample: Option<String>,
    /// Number of input lines that exceeded `--max-line-bytes` and were truncated
    /// to the cap (resilient default). A recovery, not an error: exit code stays
    /// 0. See SECURITY.md ("Input-pipeline limits").
    pub truncated_lines: usize,
    /// The byte cap in effect when a truncation occurred, for the diagnostic.
    pub line_byte_cap: usize,
    /// Number of input lines the raw-line level pre-filter dropped before parsing
    /// (see `Pipeline::level_prefilter_needles`). A dropped line creates no event,
    /// so `events_created` alone cannot tell "empty input" from "every line was
    /// pre-filtered" — this counter is what distinguishes them (#369).
    pub lines_prefiltered: usize,
    /// True when the level pre-filter was armed for this run. While it is, the
    /// parser only ever sees lines containing a requested level token, so
    /// `discovered_levels` holds a *subset* of the levels actually in the input
    /// and must not be reported as the complete vocabulary (#369).
    pub level_prefilter_active: bool,
}

// Allow disabling stats collection when diagnostics/stats are suppressed
static COLLECT_STATS: AtomicBool = AtomicBool::new(true);

// File open failures use atomic counter since they can happen on any thread (e.g., decompression threads)
static FILES_FAILED_TO_OPEN: AtomicUsize = AtomicUsize::new(0);
static FAILED_FILE_SAMPLES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
const MAX_FAILED_FILE_SAMPLES: usize = 3;
static RECOVERABLE_ERROR_SAMPLES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
#[cfg(test)]
const MAX_RECOVERABLE_ERROR_SAMPLES: usize = 3;
// First raw line that failed to parse (process-wide, set once). Captured on any
// thread so it survives both sequential and parallel processing.
static FIRST_PARSE_ERROR_SAMPLE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
// Cap the stored sample so a pathological long line can't bloat memory or the
// emitted warning.
const MAX_PARSE_ERROR_SAMPLE_LEN: usize = 1024;
// A reported time span wider than this (~10 years) is treated as a symptom
// rather than a fact: real log files rarely span a decade, but a single
// mis-parsed timestamp format (e.g. a 2-digit year read as year 17) stretches
// the range by centuries.
const IMPLAUSIBLE_SPAN_DAYS: i64 = 3653;
// Lines decoded with U+FFFD substitution (invalid UTF-8). Atomic + OnceLock
// because lossy decoding happens on reader/worker threads, like file failures.
static DECODE_WARNINGS: AtomicUsize = AtomicUsize::new(0);
static FIRST_DECODE_WARNING_SAMPLE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
// Lines truncated by the --max-line-bytes circuit breaker. Atomic because
// truncation happens on reader threads, like decode warnings and file failures.
static TRUNCATED_LINES: AtomicUsize = AtomicUsize::new(0);
static LINE_BYTE_CAP: AtomicUsize = AtomicUsize::new(0);

// The level pre-filter drops lines on whichever thread parses them (worker
// threads under --parallel), so its counter and armed flag live in process-wide
// atomics and are merged like the truncation counters above.
static LINES_PREFILTERED: AtomicUsize = AtomicUsize::new(0);
static LEVEL_PREFILTER_ACTIVE: AtomicBool = AtomicBool::new(false);

// Cascade `_format` tags skipped because the parsed record already had that
// field. Atomic for the same reason as the pre-filter counter (parsing runs on
// worker threads under --parallel), and deliberately *not* gated on
// `stats_enabled()`: it drives a default-on warning, so it must be counted on
// every run, not only under --stats (#406).
static CASCADE_FORMAT_COLLISIONS: AtomicUsize = AtomicUsize::new(0);

pub fn set_collect_stats(enabled: bool) {
    COLLECT_STATS.store(enabled, Ordering::Relaxed);
}

pub fn stats_enabled() -> bool {
    COLLECT_STATS.load(Ordering::Relaxed)
}

fn push_failed_file_sample(path: &str) {
    let samples = FAILED_FILE_SAMPLES.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut list) = samples.lock() {
        if list.len() < MAX_FAILED_FILE_SAMPLES && !list.iter().any(|p| p == path) {
            list.push(path.to_string());
        }
    }
}

fn failed_file_samples() -> Vec<String> {
    FAILED_FILE_SAMPLES
        .get()
        .and_then(|samples| samples.lock().ok().map(|v| v.clone()))
        .unwrap_or_default()
}

#[cfg(test)]
fn push_recoverable_error_sample(message: &str) {
    let samples = RECOVERABLE_ERROR_SAMPLES.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut list) = samples.lock() {
        if list.len() < MAX_RECOVERABLE_ERROR_SAMPLES && !list.iter().any(|m| m == message) {
            list.push(message.to_string());
        }
    }
}

fn recoverable_error_samples() -> Vec<String> {
    RECOVERABLE_ERROR_SAMPLES
        .get()
        .and_then(|samples| samples.lock().ok().map(|v| v.clone()))
        .unwrap_or_default()
}

/// Record the first raw line that failed to parse. Only the first one is kept;
/// later failures are ignored. The sample is truncated to a bounded length.
pub fn stats_record_parse_error_sample(line: &str) {
    if !stats_enabled() {
        return;
    }
    let slot = FIRST_PARSE_ERROR_SAMPLE.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        if current.is_none() {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            let sample: String = trimmed.chars().take(MAX_PARSE_ERROR_SAMPLE_LEN).collect();
            *current = Some(sample);
        }
    }
}

/// Returns the first captured parse-error sample line, if any.
pub fn first_parse_error_sample() -> Option<String> {
    FIRST_PARSE_ERROR_SAMPLE
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|v| v.clone()))
}

/// Record that an input line contained invalid UTF-8 and was decoded losslessly.
/// Counts on any thread (reader/worker) and keeps the first line as a sample.
/// Unlike parse errors, this is a warning, not a failure: it must not affect the
/// exit code, so it is deliberately excluded from `has_errors()`.
pub fn stats_record_decode_warning(decoded_line: &str) {
    if !stats_enabled() {
        return;
    }
    DECODE_WARNINGS.fetch_add(1, Ordering::Relaxed);
    let slot = FIRST_DECODE_WARNING_SAMPLE.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        if current.is_none() {
            let trimmed = decoded_line.trim_end_matches(['\r', '\n']);
            let sample: String = trimmed.chars().take(MAX_PARSE_ERROR_SAMPLE_LEN).collect();
            *current = Some(sample);
        }
    }
}

/// Returns the first captured decode-warning sample line, if any.
fn first_decode_warning_sample() -> Option<String> {
    FIRST_DECODE_WARNING_SAMPLE
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|v| v.clone()))
}

/// Number of lines decoded with U+FFFD substitution (process-wide). Exposed so
/// the parallel tracker can merge it into its final stats, like parse-error
/// samples.
pub fn decode_warning_count() -> usize {
    DECODE_WARNINGS.load(Ordering::Relaxed)
}

/// First decode-warning sample line (process-wide), for the parallel path.
pub fn decode_warning_sample() -> Option<String> {
    first_decode_warning_sample()
}

/// Record that an input line exceeded `--max-line-bytes` and was truncated to
/// `cap`. Counts on any reader thread; the cap is stored so the diagnostic can
/// name it. Like a decode warning, this is a recovery and never affects the exit
/// code (deliberately excluded from `has_errors()`).
pub fn stats_record_line_truncation(cap: usize) {
    if !stats_enabled() {
        return;
    }
    TRUNCATED_LINES.fetch_add(1, Ordering::Relaxed);
    LINE_BYTE_CAP.store(cap, Ordering::Relaxed);
}

/// Number of lines truncated by the circuit breaker (process-wide). Exposed so
/// the parallel tracker can merge it into its final stats.
pub fn truncated_line_count() -> usize {
    TRUNCATED_LINES.load(Ordering::Relaxed)
}

/// The byte cap that was in effect when truncation occurred (process-wide).
pub fn truncation_byte_cap() -> usize {
    LINE_BYTE_CAP.load(Ordering::Relaxed)
}

/// Record that the raw-line level pre-filter dropped a line before parsing.
///
/// Gated on stats collection like the other per-line counters: the pre-filter's
/// only observer is the zero-result hint, and `collect_stats` is already true
/// whenever hints can fire (see `runner.rs`), so `--no-diagnostics` keeps the
/// fast path with no counter cost.
pub fn stats_record_level_prefilter_drop() {
    if !stats_enabled() {
        return;
    }
    LINES_PREFILTERED.fetch_add(1, Ordering::Relaxed);
}

/// Number of lines dropped by the level pre-filter (process-wide). Exposed so
/// the parallel tracker can merge it into its final stats.
pub fn level_prefiltered_line_count() -> usize {
    LINES_PREFILTERED.load(Ordering::Relaxed)
}

/// Arm/disarm the "level pre-filter was active" flag at pipeline build time.
/// Not gated on stats collection: it describes the pipeline, not an event.
pub fn set_level_prefilter_active(active: bool) {
    LEVEL_PREFILTER_ACTIVE.store(active, Ordering::Relaxed);
}

/// Whether the level pre-filter was armed for this run (process-wide).
pub fn level_prefilter_active() -> bool {
    LEVEL_PREFILTER_ACTIVE.load(Ordering::Relaxed)
}

// Thread-local storage for statistics (following track_freq pattern)
thread_local! {
    static THREAD_STATS: RefCell<ProcessingStats> = RefCell::new(ProcessingStats::new());
}

// Public API functions for stats collection (following track_freq pattern)
// Note: These functions are conditionally called based on config.output.stats flag
pub fn stats_add_line_read() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_read += 1;
    });
}

pub fn stats_add_line_output() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_output += 1;
    });
}

pub fn stats_add_line_filtered() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_filtered += 1;
    });
}

pub fn stats_add_event_created() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_created += 1;
    });
}

pub fn stats_add_event_output() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_output += 1;
    });
}

pub fn stats_add_event_filtered() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_filtered += 1;
    });
}

pub fn stats_set_timestamp_override(field: Option<String>, format: Option<String>) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.timestamp_override_field = field;
        stats.timestamp_override_format = format;
        stats.timestamp_override_failed = false;
        stats.timestamp_override_warning = None;
    });
}

/// Record that a cascade parser successfully matched `format` on one line.
pub fn stats_add_cascade_format_hit(format: &str) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        bump(&mut stats.cascade_format_counts, format);
    });
}

/// Record that a cascade parser skipped its `_format` tag because the parsed
/// record already carried that field, so the log's own value was kept (#406).
///
/// Not gated on stats collection: this drives a warning that shows by default,
/// so it has to be counted on every run. Collisions are rare and the counter is
/// only reached on a cascade hit, so always counting costs nothing measurable.
pub fn stats_add_cascade_format_collision() {
    CASCADE_FORMAT_COLLISIONS.fetch_add(1, Ordering::Relaxed);
}

/// Number of skipped cascade `_format` tags (process-wide). Exposed so the
/// parallel tracker can merge it into its final stats.
pub fn cascade_format_collision_count() -> usize {
    CASCADE_FORMAT_COLLISIONS.load(Ordering::Relaxed)
}

pub fn stats_set_detected_format(format: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().detected_format = Some(format);
    });
}

pub fn stats_add_detected_format_hit(format: &str) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        bump(&mut stats.detected_format_counts, format);
    });
}

pub fn stats_add_late_event() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().late_events += 1;
    });
}

pub fn stats_add_yearless_timestamp() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().yearless_timestamps += 1;
    });
}

/// Record a printed event whose final timestamp falls outside `--since`/
/// `--until` (see [`ProcessingStats::window_escaped_events`]).
pub fn stats_add_window_escaped_event() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().window_escaped_events += 1;
    });
}

pub fn stats_add_naive_timestamp() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().naive_timestamps += 1;
    });
}

pub fn stats_add_error() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().errors += 1;
    });
}

pub fn stats_add_line_error() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.lines_errors += 1;
        stats.errors += 1; // Backward compatibility: errors tracked parse failures.
    });
}

#[cfg(test)]
pub fn stats_add_recoverable_error_sample(message: &str) {
    if !stats_enabled() {
        return;
    }
    push_recoverable_error_sample(message);
}

pub fn stats_add_csv_row_extra_columns(start_column: usize) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.csv_rows_extra_columns += 1;
        stats.csv_overflow_start_column = Some(
            stats
                .csv_overflow_start_column
                .map_or(start_column, |c| c.min(start_column)),
        );
    });
}

pub fn stats_add_csv_row_missing_columns() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().csv_rows_missing_columns += 1;
    });
}

pub fn stats_add_assertion_failure(expression: &str) {
    // Not gated by stats collection: an --assert violation is an explicit
    // data-quality gate that must fail the run (exit 1) in every mode, including
    // --no-diagnostics and data-only modes. Assertion failures are rare events
    // (only failures increment), so always counting them costs nothing.
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.assertion_failures += 1;
        bump(&mut stats.assertion_failures_by_expr, expression);
    });
}

pub fn stats_start_timer() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().start_time = Some(Instant::now());
    });
}

pub fn stats_finish_processing() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        if let Some(start) = stats.start_time {
            stats.processing_time = start.elapsed();
        }

        let warning = stats.build_timestamp_override_warning();
        stats.timestamp_override_failed = warning.is_some();
        stats.timestamp_override_warning = warning;
    });
}

pub fn get_thread_stats() -> ProcessingStats {
    THREAD_STATS.with(|stats| {
        let mut s = stats.borrow().clone();
        // Merge in atomic counter for file failures (can happen on any thread)
        s.files_failed_to_open = FILES_FAILED_TO_OPEN.load(Ordering::Relaxed);
        s.failed_file_samples = failed_file_samples();
        s.recoverable_error_samples = recoverable_error_samples();
        s.first_parse_error_sample = first_parse_error_sample();
        s.decode_warnings = DECODE_WARNINGS.load(Ordering::Relaxed);
        s.first_decode_warning_sample = first_decode_warning_sample();
        s.truncated_lines = TRUNCATED_LINES.load(Ordering::Relaxed);
        s.line_byte_cap = LINE_BYTE_CAP.load(Ordering::Relaxed);
        s.lines_prefiltered = LINES_PREFILTERED.load(Ordering::Relaxed);
        s.level_prefilter_active = LEVEL_PREFILTER_ACTIVE.load(Ordering::Relaxed);
        s.cascade_format_collisions = CASCADE_FORMAT_COLLISIONS.load(Ordering::Relaxed);
        s
    })
}

pub fn stats_file_open_failed(path: &str) {
    // Not gated by stats collection: a named input that can't be opened is a
    // structural failure that must fail the run (exit 1) in every mode, including
    // --no-diagnostics. File opens are rare events, so always counting is free.
    // Uses an atomic counter since file opening can happen on any thread (e.g.,
    // decompression threads).
    FILES_FAILED_TO_OPEN.fetch_add(1, Ordering::Relaxed);
    push_failed_file_sample(path);
}

/// Process-wide count of files that failed to open. Exposed so the parallel
/// tracker can merge it into final stats: file opens happen on reader/
/// decompression threads and are recorded in this global atomic, not in the
/// per-worker stats that `merge_worker_stats` accumulates.
pub fn files_failed_to_open_count() -> usize {
    FILES_FAILED_TO_OPEN.load(Ordering::Relaxed)
}

/// Process-wide samples of files that failed to open (for the parallel path).
pub fn failed_file_samples_snapshot() -> Vec<String> {
    failed_file_samples()
}

pub fn stats_record_timestamp_detection(field_name: &str, _raw_value: &str, parsed: bool) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.timestamp_detected_events += 1;

        if parsed {
            stats.timestamp_parsed_events += 1;
        }

        // Hot path: this fires once per event that carries a timestamp field, and
        // the field name is drawn from a tiny vocabulary. Look up by index first
        // (borrowed, no alloc) and only allocate an owned key the first time a new
        // field name is seen. `get_index_of` sidesteps the borrow-checker conflict
        // that a `get_mut`-else-`entry` chain would hit.
        let entry = match stats.timestamp_fields.get_index_of(field_name) {
            Some(idx) => &mut stats.timestamp_fields[idx],
            None => stats
                .timestamp_fields
                .entry(field_name.to_string())
                .or_default(),
        };
        entry.detected += 1;
        if parsed {
            entry.parsed += 1;
        }
    });
}

pub fn stats_record_timestamp_absent() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().timestamp_absent_events += 1;
    });
}

pub fn stats_update_timestamp(timestamp: DateTime<Utc>) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        match stats.first_timestamp {
            None => {
                stats.first_timestamp = Some(timestamp);
                stats.last_timestamp = Some(timestamp);
            }
            Some(first) => {
                if timestamp < first {
                    stats.first_timestamp = Some(timestamp);
                }
                match stats.last_timestamp {
                    None => stats.last_timestamp = Some(timestamp),
                    Some(last) => {
                        if timestamp > last {
                            stats.last_timestamp = Some(timestamp);
                        }
                    }
                }
            }
        }
    });
}

pub fn stats_update_result_timestamp(timestamp: DateTime<Utc>) {
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        match stats.first_result_timestamp {
            None => {
                stats.first_result_timestamp = Some(timestamp);
                stats.last_result_timestamp = Some(timestamp);
            }
            Some(first) => {
                if timestamp < first {
                    stats.first_result_timestamp = Some(timestamp);
                }
                match stats.last_result_timestamp {
                    None => stats.last_result_timestamp = Some(timestamp),
                    Some(last) => {
                        if timestamp > last {
                            stats.last_result_timestamp = Some(timestamp);
                        }
                    }
                }
            }
        }
    });
}

pub fn stats_add_output_level(level: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().discovered_levels_output.insert(level);
    });
}

pub fn stats_add_discovered_level(level: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().discovered_levels.insert(level);
    });
}

pub fn stats_add_discovered_key(key: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().discovered_keys.insert(key);
    });
}

pub fn stats_add_output_key(key: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().discovered_keys_output.insert(key);
    });
}

impl ProcessingStats {
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    fn build_timestamp_override_warning(&self) -> Option<String> {
        let override_active =
            self.timestamp_override_field.is_some() || self.timestamp_override_format.is_some();
        if !override_active
            || self.events_created == 0
            || self.timestamp_parsed_events > 0
            || (self.timestamp_detected_events == 0 && self.timestamp_absent_events == 0)
        {
            return None;
        }

        let mut reasons = Vec::new();
        if let Some(field) = &self.timestamp_override_field {
            if self.timestamp_detected_events == 0 {
                reasons.push(format!("--ts-field {} was not found in the input", field));
            } else {
                reasons.push(format!("--ts-field {} values could not be parsed", field));
            }
        }

        if let Some(format) = &self.timestamp_override_format {
            if self.timestamp_detected_events == 0 {
                reasons.push(format!(
                    "--ts-format '{}' had no timestamp fields to apply to",
                    format
                ));
            } else {
                reasons.push(format!(
                    "--ts-format '{}' did not match any timestamp values",
                    format
                ));
            }
        }

        if reasons.is_empty() {
            reasons.push("custom timestamp override did not parse any timestamps".to_string());
        }

        Some(reasons.join("; "))
    }

    fn format_timestamp_summary(&self) -> String {
        if self.events_created == 0
            && self.timestamp_detected_events == 0
            && self.timestamp_absent_events == 0
        {
            if let Some(field) = &self.timestamp_override_field {
                return format!("Timestamp: {} (--ts-field) - no events processed.", field);
            }
            return "Timestamp: no events processed.".to_string();
        }

        let detected = self.timestamp_detected_events;
        let parsed = self.timestamp_parsed_events;
        let pct = if detected > 0 {
            (parsed as f64 / detected as f64) * 100.0
        } else {
            0.0
        };

        let (descriptor, mut hint) = if let Some(field) = &self.timestamp_override_field {
            let descriptor = if detected == 0 {
                format!("{} (--ts-field) - not found", field)
            } else {
                format!("{} (--ts-field)", field)
            };

            let hint = if detected == 0 {
                Some("Verify the field name or remove --ts-field to auto-detect.")
            } else if parsed < detected {
                Some("Adjust --ts-format.")
            } else {
                None
            };

            (descriptor, hint)
        } else {
            match self.timestamp_fields.len() {
                0 => {
                    let events = if self.timestamp_absent_events > 0 {
                        self.timestamp_absent_events
                    } else {
                        self.events_created
                    };
                    let descriptor = if events > 0 {
                        format!("(none found, {} events)", events)
                    } else {
                        "(none found)".to_string()
                    };
                    (descriptor, Some("Try --ts-field or --ts-format."))
                }
                1 => {
                    let field = self.timestamp_fields.keys().next().unwrap();
                    let descriptor = format!("{} (auto-detected)", field);
                    let hint = if parsed < detected {
                        Some("Try --ts-field or --ts-format.")
                    } else {
                        None
                    };
                    (descriptor, hint)
                }
                _ => {
                    let names = self
                        .timestamp_fields
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let descriptor = format!("{} (auto-detected)", names);
                    let hint = if parsed < detected {
                        Some("Try --ts-field or --ts-format.")
                    } else {
                        None
                    };
                    (descriptor, hint)
                }
            }
        };

        if detected == 0 && self.timestamp_fields.is_empty() && hint.is_none() {
            hint = Some("Try --ts-field or --ts-format.");
        }

        let mut summary = format!(
            "Timestamp: {} - {}/{} parsed ({:.1}%)",
            descriptor, parsed, detected, pct
        );

        if self.timestamp_absent_events > 0 {
            summary.push_str(&format!("; {} missing", self.timestamp_absent_events));
        }

        // The moment a user learns a timestamp was parsed is the moment they want
        // to know how to reach it — otherwise they re-parse the raw field with
        // to_datetime(). Only worth saying when something actually parsed.
        if parsed > 0 {
            summary.push_str(" — access via meta.parsed_ts");
        }

        summary.push('.');

        if let Some(hint_text) = hint {
            summary.push_str(&format!(" Hint: {}", hint_text));
        }

        summary
    }

    /// Format stats according to the specification
    pub fn format_stats(&self, _multiline_enabled: bool) -> String {
        self.format_stats_internal(_multiline_enabled, false)
    }

    /// Render the same run statistics as a machine-readable JSON object
    /// (`--stats=json`). The schema mirrors the table view's logical groups so
    /// the two stay in sync; fields that the table omits when empty (ragged
    /// rows, output time span, output keys/levels) are likewise only present
    /// here when they carry information.
    pub fn format_stats_json(&self) -> String {
        use serde_json::{json, Map, Value};

        fn timespan(first: Option<DateTime<Utc>>, last: Option<DateTime<Utc>>) -> Option<Value> {
            let (first, last) = (first?, last?);
            Some(json!({
                "start": first.to_rfc3339(),
                "end": last.to_rfc3339(),
                "duration_seconds": (last - first).num_milliseconds() as f64 / 1000.0,
            }))
        }

        let mut root = Map::new();

        // Format detection
        let mut format = Map::new();
        if let Some(ref f) = self.detected_format {
            format.insert("detected".to_string(), json!(f));
        }
        if !self.detected_format_counts.is_empty() {
            let counts: Map<String, Value> = self
                .detected_format_counts
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            format.insert("per_file".to_string(), Value::Object(counts));
        }
        if !self.cascade_format_counts.is_empty() {
            let counts: Map<String, Value> = self
                .cascade_format_counts
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            format.insert("cascade".to_string(), Value::Object(counts));
        }
        if self.cascade_format_collisions > 0 {
            format.insert(
                "cascade_tag_skipped".to_string(),
                json!(self.cascade_format_collisions),
            );
        }
        if !format.is_empty() {
            root.insert("format".to_string(), Value::Object(format));
        }

        root.insert(
            "lines".to_string(),
            json!({
                "read": self.lines_read,
                "filtered": self.lines_filtered,
                "errors": self.lines_errors,
            }),
        );
        root.insert(
            "events".to_string(),
            json!({
                "created": self.events_created,
                "output": self.events_output,
                "filtered": self.events_filtered,
                "late": self.late_events,
            }),
        );

        let duration_secs = self.processing_time.as_secs_f64();
        let lines_per_second = if duration_secs > 0.0 && self.lines_read > 0 {
            serde_json::Number::from_f64(self.lines_read as f64 / duration_secs).map(Value::Number)
        } else {
            None
        };
        root.insert(
            "throughput".to_string(),
            json!({
                "lines_per_second": lines_per_second,
                "duration_ms": self.processing_time.as_millis() as u64,
            }),
        );

        let ts_fields: Vec<String> = if let Some(field) = &self.timestamp_override_field {
            vec![field.clone()]
        } else {
            self.timestamp_fields.keys().cloned().collect()
        };
        root.insert(
            "timestamp".to_string(),
            json!({
                "fields": ts_fields,
                "overridden": self.timestamp_override_field.is_some(),
                "detected": self.timestamp_detected_events,
                "parsed": self.timestamp_parsed_events,
                "absent": self.timestamp_absent_events,
                "yearless_inferred": self.yearless_timestamps,
                "outside_window": self.window_escaped_events,
            }),
        );

        let mut time_span = Map::new();
        if let Some(span) = timespan(self.first_timestamp, self.last_timestamp) {
            time_span.insert("input".to_string(), span);
        }
        if let Some(span) = timespan(self.first_result_timestamp, self.last_result_timestamp) {
            time_span.insert("output".to_string(), span);
        }
        if !time_span.is_empty() {
            root.insert("time_span".to_string(), Value::Object(time_span));
        }

        if !self.discovered_levels.is_empty() {
            let mut levels = Map::new();
            levels.insert(
                "seen".to_string(),
                json!(self.discovered_levels.iter().collect::<Vec<_>>()),
            );
            if !self.discovered_levels_output.is_empty()
                && self.discovered_levels_output != self.discovered_levels
            {
                levels.insert(
                    "output".to_string(),
                    json!(self.discovered_levels_output.iter().collect::<Vec<_>>()),
                );
            }
            root.insert("levels".to_string(), Value::Object(levels));
        }

        if !self.discovered_keys.is_empty() {
            let mut keys = Map::new();
            keys.insert(
                "seen".to_string(),
                json!(self.discovered_keys.iter().collect::<Vec<_>>()),
            );
            if !self.discovered_keys_output.is_empty()
                && self.discovered_keys_output != self.discovered_keys
            {
                keys.insert(
                    "output".to_string(),
                    json!(self.discovered_keys_output.iter().collect::<Vec<_>>()),
                );
            }
            root.insert("keys".to_string(), Value::Object(keys));
        }

        if self.csv_rows_extra_columns > 0 || self.csv_rows_missing_columns > 0 {
            root.insert(
                "ragged_rows".to_string(),
                json!({
                    "extra_columns": self.csv_rows_extra_columns,
                    "missing_columns": self.csv_rows_missing_columns,
                }),
            );
        }

        if self.decode_warnings > 0 {
            root.insert("decode_warnings".to_string(), json!(self.decode_warnings));
        }
        if self.assertion_failures > 0 {
            root.insert(
                "assertion_failures".to_string(),
                json!(self.assertion_failures),
            );
        }
        if self.files_processed > 0 || self.files_failed_to_open > 0 {
            root.insert(
                "files".to_string(),
                json!({
                    "processed": self.files_processed,
                    "failed_to_open": self.files_failed_to_open,
                }),
            );
        }

        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".to_string())
    }

    /// Format stats for signal handlers
    ///
    /// `include_line_counts` should only be true when we have accurate mid-run
    /// counters (e.g., sequential mode). Parallel mode uses partial stats, so
    /// keep line counts suppressed there to avoid misleading zeros.
    pub fn format_stats_for_signal(
        &self,
        _multiline_enabled: bool,
        include_line_counts: bool,
    ) -> String {
        self.format_stats_internal(_multiline_enabled, !include_line_counts)
    }

    fn format_stats_internal(&self, _multiline_enabled: bool, skip_line_counts: bool) -> String {
        let mut output = String::new();

        if !self.detected_format_counts.is_empty() {
            let parts: Vec<String> = self
                .detected_format_counts
                .iter()
                .map(|(name, count)| {
                    let suffix = if *count == 1 { "file" } else { "files" };
                    format!("{}={} {}", name, count, suffix)
                })
                .collect();
            output.push_str(&format!("Detected formats: {}\n", parts.join(", ")));
        } else if let Some(ref format) = self.detected_format {
            output.push_str(&format!("Detected format: {}\n", format));
        }

        // Show cascade format distribution (only present in cascade mode)
        if !self.cascade_format_counts.is_empty() {
            let parts: Vec<String> = self
                .cascade_format_counts
                .iter()
                .map(|(name, count)| format!("{}={}", name, count))
                .collect();
            output.push_str(&format!("Cascade formats: {}\n", parts.join(", ")));
        }

        // Lines processed: N total, N filtered (X%), N errors (Y%)
        // Skip this line when called from signal handler (line counts are always 0 there)
        if !skip_line_counts {
            let lines_filtered_pct = if self.lines_read > 0 {
                format!(
                    " ({:.1}%)",
                    (self.lines_filtered as f64 / self.lines_read as f64) * 100.0
                )
            } else {
                String::new()
            };
            let lines_errors_pct = if self.lines_read > 0 {
                format!(
                    " ({:.1}%)",
                    (self.lines_errors as f64 / self.lines_read as f64) * 100.0
                )
            } else {
                String::new()
            };
            output.push_str(&format!(
                "Lines processed: {} total, {} filtered{}, {} errors{}\n",
                self.lines_read,
                self.lines_filtered,
                lines_filtered_pct,
                self.lines_errors,
                lines_errors_pct
            ));
        }

        // Ragged CSV/TSV rows (only present for csv/tsv inputs)
        if let Some(ragged) = self.format_ragged_rows_summary() {
            output.push_str(&format!("{}\n", ragged));
        }

        // Events created: N total, N output, N filtered (X%)
        let events_filtered_pct = if self.events_created > 0 {
            format!(
                " ({:.1}%)",
                (self.events_filtered as f64 / self.events_created as f64) * 100.0
            )
        } else {
            String::new()
        };
        output.push_str(&format!(
            "Events created: {} total, {} output, {} filtered{}\n",
            self.events_created, self.events_output, self.events_filtered, events_filtered_pct
        ));

        if self.late_events > 0 {
            output.push_str(&format!("Late events: {}\n", self.late_events));
        }

        // Throughput: N lines/s in Nms
        let duration_secs = self.processing_time.as_secs_f64();
        if duration_secs > 0.0 && self.lines_read > 0 {
            let throughput = self.lines_read as f64 / duration_secs;
            if duration_secs < 1.0 {
                output.push_str(&format!(
                    "Throughput: {:.0} lines/s in {:.0}ms\n",
                    throughput,
                    self.processing_time.as_millis()
                ));
            } else {
                output.push_str(&format!(
                    "Throughput: {:.0} lines/s in {:.2}s\n",
                    throughput, duration_secs
                ));
            }
        }

        // Timestamp parsing summary
        output.push_str(&format!("{}\n", self.format_timestamp_summary()));

        if let Some(message) = &self.timestamp_override_warning {
            output.push_str(&format!("Warning: {}\n", message));
        }

        if self.files_failed_to_open > 0 {
            output.push_str(&crate::config::format_error_message_auto(&format!(
                "Failed to open {} file{}",
                self.files_failed_to_open,
                if self.files_failed_to_open == 1 {
                    ""
                } else {
                    "s"
                }
            )));
            output.push('\n');
        }

        if let Some(message) = self.format_decode_warning() {
            output.push_str(&crate::config::format_warning_message_auto(&message));
            output.push('\n');
        }

        if self.yearless_timestamps > 0 {
            let warning_msg = format!(
                "Year-less timestamps detected ({} timestamp{}): year guessed via ±1yr heuristic, >18mo old may be wrong. Override with --input-year YYYY.",
                self.yearless_timestamps,
                if self.yearless_timestamps == 1 {
                    ""
                } else {
                    "s"
                }
            );
            output.push_str(&crate::config::format_warning_message_auto(&warning_msg));
            output.push('\n');
        }

        if let Some(message) = self.format_window_escape_warning() {
            output.push_str(&crate::config::format_warning_message_auto(&message));
            output.push('\n');
        }

        if let Some(message) = self.format_cascade_collision_warning() {
            output.push_str(&crate::config::format_warning_message_auto(&message));
            output.push('\n');
        }

        // Time span: show generic label when identical, specific labels when different
        let has_original = self.first_timestamp.is_some() && self.last_timestamp.is_some();
        let has_result =
            self.first_result_timestamp.is_some() && self.last_result_timestamp.is_some();

        if has_original {
            let first = self.first_timestamp.unwrap();
            let last = self.last_timestamp.unwrap();

            // Check if result timespan differs from original
            let is_different = has_result
                && (self.first_timestamp != self.first_result_timestamp
                    || self.last_timestamp != self.last_result_timestamp);

            let label = if is_different {
                "Input time span (before filtering)"
            } else {
                "Time span"
            };

            if first == last {
                output.push_str(&format!(
                    "{}: {} (single timestamp)\n",
                    label,
                    first.to_rfc3339()
                ));
            } else {
                let duration = last - first;
                let duration_wrapper = DurationWrapper::new(duration);
                output.push_str(&format!(
                    "{}: {} to {} ({})\n",
                    label,
                    first.to_rfc3339(),
                    last.to_rfc3339(),
                    duration_wrapper
                ));

                // A span this wide is almost never real log data; it usually means
                // some lines parsed under the wrong timestamp format. Say so rather
                // than reporting "100% parsed" beside an implausible range.
                if duration > chrono::Duration::days(IMPLAUSIBLE_SPAN_DAYS) {
                    output.push_str(&crate::config::format_warning_message_auto(&format!(
                        "Time span covers {} — check for mixed timestamp formats, \
                         two-digit years, or clock skew across sources",
                        duration_wrapper
                    )));
                    output.push('\n');
                }
            }

            // Show result time span only when different
            if is_different {
                let result_first = self.first_result_timestamp.unwrap();
                let result_last = self.last_result_timestamp.unwrap();

                if result_first == result_last {
                    output.push_str(&format!(
                        "Output time span (after filtering): {} (single timestamp)\n",
                        result_first.to_rfc3339()
                    ));
                } else {
                    let duration = result_last - result_first;
                    let duration_wrapper = DurationWrapper::new(duration);
                    output.push_str(&format!(
                        "Output time span (after filtering): {} to {} ({})\n",
                        result_first.to_rfc3339(),
                        result_last.to_rfc3339(),
                        duration_wrapper
                    ));
                }
            }
        }

        // Levels seen/output: show output only when different from input
        if !self.discovered_levels.is_empty() {
            let levels_input: Vec<String> = self.discovered_levels.iter().cloned().collect();
            let levels_output: Vec<String> =
                self.discovered_levels_output.iter().cloned().collect();

            if self.discovered_levels_output.is_empty()
                || self.discovered_levels == self.discovered_levels_output
            {
                // No changes, show single line
                output.push_str(&format!("Levels seen: {}\n", levels_input.join(",")));
            } else {
                // Changes occurred, show both for comparison
                output.push_str(&format!("Levels seen: {}\n", levels_input.join(",")));
                output.push_str(&format!("Levels output: {}\n", levels_output.join(",")));
            }
        }

        // Keys seen/output: show output only when different from input
        if !self.discovered_keys.is_empty() {
            let keys_input: Vec<String> = self.discovered_keys.iter().cloned().collect();
            let keys_output: Vec<String> = self.discovered_keys_output.iter().cloned().collect();

            if self.discovered_keys_output.is_empty()
                || self.discovered_keys == self.discovered_keys_output
            {
                // No changes, show single line
                output.push_str(&format!("Keys seen: {}\n", keys_input.join(",")));
            } else {
                // Changes occurred, show both for comparison
                output.push_str(&format!("Keys seen: {}\n", keys_input.join(",")));
                output.push_str(&format!("Keys output: {}\n", keys_output.join(",")));
            }
        }

        output.trim_end().to_string()
    }

    /// One-line summary of ragged CSV/TSV rows, or None when none occurred.
    /// Factual only — callers that want to suggest --strict append their own advice.
    pub fn format_ragged_rows_summary(&self) -> Option<String> {
        if self.csv_rows_extra_columns == 0 && self.csv_rows_missing_columns == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if self.csv_rows_extra_columns > 0 {
            let kept_as = match self.csv_overflow_start_column {
                Some(col) => format!("c{}+", col),
                None => "cN fields".to_string(),
            };
            parts.push(format!(
                "{} row{} with more columns than expected (extras kept as {})",
                self.csv_rows_extra_columns,
                if self.csv_rows_extra_columns == 1 {
                    ""
                } else {
                    "s"
                },
                kept_as
            ));
        }
        if self.csv_rows_missing_columns > 0 {
            parts.push(format!(
                "{} row{} with fewer columns than expected (missing fields left absent)",
                self.csv_rows_missing_columns,
                if self.csv_rows_missing_columns == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        Some(format!("Ragged rows: {}", parts.join(", ")))
    }

    /// Check if any errors occurred during processing.
    ///
    /// Used for *reporting* (whether to print an error summary), not for the exit
    /// code — a partial parse failure has errors worth reporting but is recovered.
    /// For the exit-code decision use [`has_fatal_errors`](Self::has_fatal_errors).
    pub fn has_errors(&self) -> bool {
        self.lines_errors > 0 || self.files_failed_to_open > 0 || self.assertion_failures > 0
    }

    /// Stats-side inputs to the exit-code decision (the structural and
    /// explicit-gate axes of the v2 error model). The per-record axis (parse /
    /// filter / exec "never once succeeded") lives in the always-on tracker via
    /// [`stage_failed_completely`](crate::rhai_functions::tracking::stage_failed_completely);
    /// this covers only what the tracker doesn't:
    ///
    /// - **Structural** — a named input file that could not be opened is an
    ///   invocation/environment error, never data noise, so it fails the run in
    ///   any mode.
    /// - **Explicit gate** — an `--assert` violation fails the run in any mode.
    /// - **Strict** — under `--strict`, *any* parse error is fatal (strict also
    ///   aborts on the first such line before reaching here; this is the
    ///   belt-and-suspenders end-of-run check). In resilient mode parse errors
    ///   are recovered unless the parser never once succeeded, which the tracker
    ///   detects.
    pub fn has_fatal_errors(&self, strict: bool) -> bool {
        if self.files_failed_to_open > 0 || self.assertion_failures > 0 {
            return true;
        }
        strict && self.lines_errors > 0
    }

    /// Format the lossy-UTF-8 decode warning, if any lines were affected.
    /// Returned separately from `format_error_summary` because decode warnings
    /// are recoveries, not failures, and must not influence the exit code (#239).
    pub fn format_decode_warning(&self) -> Option<String> {
        if self.decode_warnings == 0 {
            return None;
        }
        let mut message = format!(
            "{} line{} contained invalid UTF-8, decoded with U+FFFD substitution",
            self.decode_warnings,
            if self.decode_warnings == 1 { "" } else { "s" }
        );
        if let Some(sample) = &self.first_decode_warning_sample {
            message.push_str(&format!(" (first: {})", sample));
        }
        Some(message)
    }

    /// Warning for printed events whose timestamp lies outside `--since`/
    /// `--until`. Returns `None` when every printed event stayed inside the
    /// window, which is every run that does not build or rewrite a timestamp in
    /// a script stage.
    ///
    /// The window is a selection over the parser's timestamp and runs before
    /// the script stages (see `docs/concepts/pipeline-model.md`), so a script
    /// assignment cannot be enforced by it in either direction. Rather than
    /// guess from the script text, this reports the case that actually
    /// occurred: the timestamp kelora ends up printing is not one the window
    /// would have admitted (#345).
    pub fn format_window_escape_warning(&self) -> Option<String> {
        if self.window_escaped_events == 0 {
            return None;
        }
        Some(format!(
            "{} printed event{} carr{} a timestamp outside --since/--until: the time window reads the timestamp the parser produced, and runs before every script stage. A timestamp a script writes or builds is not filtered by it — narrow those with a --filter after the stage that sets them. See --help-time.",
            self.window_escaped_events,
            if self.window_escaped_events == 1 {
                ""
            } else {
                "s"
            },
            if self.window_escaped_events == 1 {
                "ies"
            } else {
                "y"
            },
        ))
    }

    /// Warning for lines clipped by the `--max-line-bytes` circuit breaker.
    /// Returns `None` when nothing was truncated.
    pub fn format_line_truncation_warning(&self) -> Option<String> {
        if self.truncated_lines == 0 {
            return None;
        }
        Some(format!(
            "{} line{} exceeded --max-line-bytes ({}) and {} truncated",
            self.truncated_lines,
            if self.truncated_lines == 1 { "" } else { "s" },
            crate::byte_size::format_byte_size(self.line_byte_cap as u64),
            if self.truncated_lines == 1 {
                "was"
            } else {
                "were"
            }
        ))
    }

    /// Warning for cascade events that already carried a `_format` field, so the
    /// cascade tag was skipped rather than overwriting the log's own value.
    /// Returns `None` when nothing collided, which is every ordinary run.
    ///
    /// The tag is the recoverable half of the collision — `--stats` prints the
    /// per-format breakdown either way — so data wins and the missing tag is
    /// explained here rather than left mysterious. Since #404 a plain `-f auto`
    /// run can pick a cascade on its own, so the advice names the way back to
    /// single-format parsing (#406).
    ///
    /// Deliberately does not promise the `-v` detection notice: that fires only
    /// for an auto-selected cascade, and this warning is reachable from an
    /// explicit `-f json,line` too.
    pub fn format_cascade_collision_warning(&self) -> Option<String> {
        if self.cascade_format_collisions == 0 {
            return None;
        }
        Some(format!(
            "{} event{} already had a '{}' field, so the cascade format tag was not added to {} — the input value was kept. The per-format breakdown is in --stats. Pass a single explicit -f FORMAT to parse with one format instead.",
            self.cascade_format_collisions,
            if self.cascade_format_collisions == 1 {
                ""
            } else {
                "s"
            },
            crate::parsers::FORMAT_FIELD,
            if self.cascade_format_collisions == 1 {
                "it"
            } else {
                "them"
            },
        ))
    }

    /// Format a concise error summary for default output (when errors occur)
    pub fn format_error_summary(&self) -> String {
        if !self.has_errors() {
            return String::new();
        }

        let mut parts = Vec::new();

        // Show parse errors
        if self.lines_errors > 0 {
            let mut message = format!(
                "{} parse error{}",
                self.lines_errors,
                if self.lines_errors == 1 { "" } else { "s" }
            );
            if let Some(sample) = self.recoverable_error_samples.first() {
                message.push_str(&format!(" (first: {})", sample));
            }
            parts.push(message);
        }

        // Show events filtered (could indicate filter errors converted to false)
        if self.events_filtered > 0 {
            parts.push(format!(
                "{} event{} filtered",
                self.events_filtered,
                if self.events_filtered == 1 { "" } else { "s" }
            ));
        }

        if self.files_failed_to_open > 0 {
            let mut message = format!(
                "{} file{} failed to open",
                self.files_failed_to_open,
                if self.files_failed_to_open == 1 {
                    ""
                } else {
                    "s"
                }
            );

            if !self.failed_file_samples.is_empty() {
                let total = self.files_failed_to_open;
                let sample_joined = self
                    .failed_file_samples
                    .iter()
                    .take(MAX_FAILED_FILE_SAMPLES)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");

                if total > self.failed_file_samples.len() {
                    message.push_str(&format!(" ({}, ...)", sample_joined));
                } else {
                    message.push_str(&format!(" ({})", sample_joined));
                }
            }

            parts.push(message);
        }

        // Show assertion failures
        if self.assertion_failures > 0 {
            parts.push(format!(
                "{} assertion failure{}",
                self.assertion_failures,
                if self.assertion_failures == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }

        if parts.is_empty() {
            return String::new();
        }

        if self.timestamp_override_failed {
            if let Some(message) = &self.timestamp_override_warning {
                parts.push(message.clone());
            }
        }

        if self.yearless_timestamps > 0 {
            parts.push(format!(
                "{} year-less timestamp{} (±1yr heuristic)",
                self.yearless_timestamps,
                if self.yearless_timestamps == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }

        format!("Processing completed with {}", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `FILES_FAILED_TO_OPEN`, `FAILED_FILE_SAMPLES`, and `RECOVERABLE_ERROR_SAMPLES`
    // are process-global, so concurrently running tests would clobber each other's
    // samples between push and read. Serialize the stats tests on a single lock,
    // held for the duration of each test, to keep that shared state stable.
    static STATS_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[must_use = "hold the guard for the whole test to keep global stats state stable"]
    fn reset_thread_stats() -> std::sync::MutexGuard<'static, ()> {
        let guard = STATS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        THREAD_STATS.with(|stats| {
            *stats.borrow_mut() = ProcessingStats::new();
        });
        FILES_FAILED_TO_OPEN.store(0, Ordering::Relaxed);
        if let Some(samples) = FAILED_FILE_SAMPLES.get() {
            samples.lock().expect("failed file sample lock").clear();
        }
        if let Some(samples) = RECOVERABLE_ERROR_SAMPLES.get() {
            samples
                .lock()
                .expect("recoverable error sample lock")
                .clear();
        }
        guard
    }

    #[test]
    fn stats_counters_accumulate_expected_values() {
        let _stats_guard = reset_thread_stats();

        stats_add_line_read();
        stats_add_line_filtered();
        stats_add_line_output();
        stats_add_event_created();
        stats_add_event_output();
        stats_add_event_filtered();
        stats_add_error();

        let stats = get_thread_stats();

        assert_eq!(stats.lines_read, 1);
        assert_eq!(stats.lines_filtered, 1);
        assert_eq!(stats.lines_output, 1);
        assert_eq!(stats.events_created, 1);
        assert_eq!(stats.events_output, 1);
        assert_eq!(stats.events_filtered, 1);
        assert_eq!(stats.errors, 1);
    }

    #[test]
    fn discovered_field_helpers_load_sets() {
        let _stats_guard = reset_thread_stats();

        stats_add_discovered_level("INFO".to_string());
        stats_add_discovered_key("request_id".to_string());

        let stats = get_thread_stats();
        assert!(stats.discovered_levels.contains("INFO"));
        assert!(stats.discovered_keys.contains("request_id"));
    }

    #[test]
    fn timestamp_stats_track_detection_and_absence() {
        let _stats_guard = reset_thread_stats();

        stats_record_timestamp_detection("timestamp", "2024-05-19T12:34:56Z", true);
        stats_record_timestamp_detection("timestamp", "not-a-date", false);
        stats_record_timestamp_absent();

        let stats = get_thread_stats();

        assert_eq!(stats.timestamp_detected_events, 2);
        assert_eq!(stats.timestamp_parsed_events, 1);
        assert_eq!(stats.timestamp_absent_events, 1);

        let field_stats = stats
            .timestamp_fields
            .get("timestamp")
            .expect("field stats");
        assert_eq!(field_stats.detected, 2);
        assert_eq!(field_stats.parsed, 1);
    }

    #[test]
    fn error_summary_includes_first_recoverable_error_sample() {
        let _stats_guard = reset_thread_stats();

        stats_add_line_error();
        stats_add_recoverable_error_sample(
            "input for 'api.log' is not sorted at line 42: 2026-04-09T10:01:00Z < previous 2026-04-09T10:04:00Z",
        );

        let summary = get_thread_stats().format_error_summary();

        assert!(summary.contains("1 parse error"));
        assert!(summary.contains("api.log"));
        assert!(summary.contains("not sorted at line 42"));
    }
}
