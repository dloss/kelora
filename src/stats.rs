use crate::rhai_functions::datetime::DurationWrapper;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct TimestampFieldStat {
    pub detected: usize,
    pub parsed: usize,
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
    pub script_executions: usize,
    pub errors: usize, // Kept for backward compatibility, but lines_errors is more specific
    pub processing_time: Duration,
    pub start_time: Option<Instant>,
    pub discovered_levels: BTreeSet<String>,
    pub discovered_keys: BTreeSet<String>,
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
    pub detected_format: Option<String>, // Format detected for this processing session
}

// Script execution statistics
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptStageId {
    pub stage_type: String,  // "filter", "exec", "begin", "end", "span_close"
    pub stage_number: usize, // 0 for single stages, 1-based for multiple
    pub script_name: Option<String>, // Filename if loaded from file
}

#[derive(Debug, Clone, Default)]
pub struct ScriptStageStat {
    pub calls: usize,
    pub total_time: Duration,
}

impl ScriptStageStat {
    pub fn avg_time(&self) -> Duration {
        if self.calls > 0 {
            self.total_time / self.calls as u32
        } else {
            Duration::ZERO
        }
    }
}

// Allow disabling stats collection when diagnostics/stats are suppressed
static COLLECT_STATS: AtomicBool = AtomicBool::new(true);
static COLLECT_SCRIPT_STATS: AtomicBool = AtomicBool::new(false);

// File open failures use atomic counter since they can happen on any thread (e.g., decompression threads)
static FILES_FAILED_TO_OPEN: AtomicUsize = AtomicUsize::new(0);
static FAILED_FILE_SAMPLES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
const MAX_FAILED_FILE_SAMPLES: usize = 3;

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

// Thread-local storage for statistics (following track_count pattern)
thread_local! {
    static THREAD_STATS: RefCell<ProcessingStats> = RefCell::new(ProcessingStats::new());
    static SCRIPT_STATS: RefCell<IndexMap<ScriptStageId, ScriptStageStat>> = RefCell::new(IndexMap::new());
}

// Public API functions for stats collection (following track_count pattern)
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

pub fn stats_set_detected_format(format: String) {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().detected_format = Some(format);
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

pub fn stats_add_error() {
    if !stats_enabled() {
        return;
    }
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().errors += 1;
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
        s
    })
}

pub fn stats_file_open_failed(path: &str) {
    if !stats_enabled() {
        return;
    }
    // Use atomic counter since file opening can happen on any thread (e.g., decompression threads)
    FILES_FAILED_TO_OPEN.fetch_add(1, Ordering::Relaxed);
    push_failed_file_sample(path);
}

pub fn stats_record_timestamp_detection(field_name: &str, _raw_value: &str, parsed: bool) {
    if !stats_enabled() {
        return;
    }
    let field = field_name.to_string();
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.timestamp_detected_events += 1;

        if parsed {
            stats.timestamp_parsed_events += 1;
        }

        let entry = stats.timestamp_fields.entry(field).or_default();
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

        summary.push('.');

        if let Some(hint_text) = hint {
            summary.push_str(&format!(" Hint: {}", hint_text));
        }

        summary
    }

    /// Extract discovered levels and keys from tracking data (for sequential processing)
    pub fn extract_discovered_from_tracking(
        &mut self,
        tracking_data: &std::collections::HashMap<String, rhai::Dynamic>,
    ) {
        // Extract discovered levels from tracking data
        if let Some(levels_dynamic) = tracking_data.get("__kelora_stats_discovered_levels") {
            if let Ok(levels_array) = levels_dynamic.clone().into_array() {
                for level in levels_array {
                    if let Ok(level_str) = level.into_string() {
                        self.discovered_levels.insert(level_str);
                    }
                }
            }
        }

        // Extract discovered keys from tracking data
        if let Some(keys_dynamic) = tracking_data.get("__kelora_stats_discovered_keys") {
            if let Ok(keys_array) = keys_dynamic.clone().into_array() {
                for key in keys_array {
                    if let Ok(key_str) = key.into_string() {
                        self.discovered_keys.insert(key_str);
                    }
                }
            }
        }
    }

    /// Format stats according to the specification
    pub fn format_stats(&self, _multiline_enabled: bool) -> String {
        self.format_stats_internal(_multiline_enabled, false)
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

        // Show detected format if available
        if let Some(ref format) = self.detected_format {
            output.push_str(&format!("Detected format: {}\n", format));
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

        if self.yearless_timestamps > 0 {
            let warning_msg = format!(
                "Year-less timestamp format detected ({} parse{})\n\
                 Format lacks year (e.g., \"Dec 31 23:59:59\")\n\
                 Year inferred using heuristic (±1 year from current date)\n\
                 Timestamps >18 months old may be incorrect",
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

        // Levels seen: (only if we have discovered levels)
        if !self.discovered_levels.is_empty() {
            let levels: Vec<String> = self.discovered_levels.iter().cloned().collect();
            output.push_str(&format!("Levels seen: {}\n", levels.join(",")));
        }

        // Keys seen: (only if we have discovered keys)
        if !self.discovered_keys.is_empty() {
            let keys: Vec<String> = self.discovered_keys.iter().cloned().collect();
            output.push_str(&format!("Keys seen: {}\n", keys.join(",")));
        }

        output.trim_end().to_string()
    }

    /// Check if any errors occurred during processing
    pub fn has_errors(&self) -> bool {
        self.lines_errors > 0 || self.files_failed_to_open > 0
    }

    /// Format a concise error summary for default output (when errors occur)
    pub fn format_error_summary(&self) -> String {
        if !self.has_errors() {
            return String::new();
        }

        let mut parts = Vec::new();

        // Show parse errors
        if self.lines_errors > 0 {
            parts.push(format!(
                "{} parse error{}",
                self.lines_errors,
                if self.lines_errors == 1 { "" } else { "s" }
            ));
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

// ============================================================================
// Script Stats API
// ============================================================================

pub fn set_collect_script_stats(enabled: bool) {
    COLLECT_SCRIPT_STATS.store(enabled, Ordering::Relaxed);
}

pub fn script_stats_enabled() -> bool {
    COLLECT_SCRIPT_STATS.load(Ordering::Relaxed)
}

/// Record script execution time for a stage
pub fn script_stats_record(stage_id: ScriptStageId, duration: Duration) {
    if !script_stats_enabled() {
        return;
    }
    SCRIPT_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        let entry = stats.entry(stage_id).or_default();
        entry.calls += 1;
        entry.total_time += duration;
    });
}

/// Get all script stats from current thread
pub fn get_thread_script_stats() -> IndexMap<ScriptStageId, ScriptStageStat> {
    SCRIPT_STATS.with(|stats| stats.borrow().clone())
}

/// Format script stats for display
pub fn format_script_stats(stats_map: &IndexMap<ScriptStageId, ScriptStageStat>) -> String {
    if stats_map.is_empty() {
        return "Script stats: (no scripts executed)".to_string();
    }

    let mut output = String::from("Script stats:\n");
    let mut total_time = Duration::ZERO;

    // Count how many stages of each type exist and assign type-relative numbers
    let mut stage_type_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut stage_type_indices: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut stage_relative_numbers: std::collections::HashMap<
        (String, usize, Option<String>),
        usize,
    > = std::collections::HashMap::new();

    for stage_id in stats_map.keys() {
        *stage_type_counts
            .entry(stage_id.stage_type.clone())
            .or_insert(0) += 1;
    }

    for stage_id in stats_map.keys() {
        let current_index = stage_type_indices
            .entry(stage_id.stage_type.clone())
            .or_insert(0);
        *current_index += 1;
        stage_relative_numbers.insert(
            (
                stage_id.stage_type.clone(),
                stage_id.stage_number,
                stage_id.script_name.clone(),
            ),
            *current_index,
        );
    }

    for (stage_id, stat) in stats_map.iter() {
        total_time += stat.total_time;

        // Check if there are multiple stages of this type
        let has_multiple = stage_type_counts
            .get(&stage_id.stage_type)
            .copied()
            .unwrap_or(1)
            > 1;

        // Get type-relative number
        let relative_num = stage_relative_numbers
            .get(&(
                stage_id.stage_type.clone(),
                stage_id.stage_number,
                stage_id.script_name.clone(),
            ))
            .copied()
            .unwrap_or(1);

        // Format stage label
        let stage_label = if let Some(ref script_name) = stage_id.script_name {
            // File-based script: show filename
            if has_multiple {
                format!("  --{} ({}):", stage_id.stage_type, script_name)
            } else {
                format!("  --{}:", stage_id.stage_type)
            }
        } else if has_multiple {
            // Multiple stages of same type: show type-relative number
            format!("  --{} ({}):", stage_id.stage_type, relative_num)
        } else {
            // Single stage
            format!("  --{}:", stage_id.stage_type)
        };

        // Format time (use ms for times < 1s, s otherwise)
        let time_str = if stat.total_time.as_secs() == 0 {
            format!("{} ms", stat.total_time.as_millis())
        } else {
            format!("{:.2} s", stat.total_time.as_secs_f64())
        };

        // Format average time (use ns for times < 1us, us for < 1ms, ms otherwise)
        let avg = stat.avg_time();
        let avg_str = if avg.as_nanos() < 1_000 {
            format!("{} ns", avg.as_nanos())
        } else if avg.as_micros() < 1_000 {
            format!("{} µs", avg.as_micros())
        } else if avg.as_millis() < 1_000 {
            format!("{} ms", avg.as_millis())
        } else {
            format!("{:.3} s", avg.as_secs_f64())
        };

        // Format call count with thousand separators
        let calls_str = format_with_separators(stat.calls);

        output.push_str(&format!(
            "{:<30} {} ({} calls, {}/call)\n",
            stage_label, time_str, calls_str, avg_str
        ));
    }

    // Add total if there are multiple stages
    if stats_map.len() > 1 {
        output.push('\n');
        let total_str = if total_time.as_secs() == 0 {
            format!("{} ms", total_time.as_millis())
        } else {
            format!("{:.2} s", total_time.as_secs_f64())
        };
        output.push_str(&format!("  Total:                         {}\n", total_str));
    }

    output.trim_end().to_string()
}

/// Format number with thousand separators (e.g., 12,483,771)
fn format_with_separators(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();

    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }

    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn reset_thread_stats() {
        THREAD_STATS.with(|stats| {
            *stats.borrow_mut() = ProcessingStats::new();
        });
    }

    #[test]
    fn stats_counters_accumulate_expected_values() {
        reset_thread_stats();

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
    fn extract_discovered_from_tracking_loads_sets() {
        let mut stats = ProcessingStats::new();
        let mut tracking: HashMap<String, rhai::Dynamic> = HashMap::new();

        let levels = vec![rhai::Dynamic::from("INFO")];
        tracking.insert(
            "__kelora_stats_discovered_levels".to_string(),
            rhai::Dynamic::from(levels),
        );

        let keys = vec![rhai::Dynamic::from("request_id")];
        tracking.insert(
            "__kelora_stats_discovered_keys".to_string(),
            rhai::Dynamic::from(keys),
        );

        stats.extract_discovered_from_tracking(&tracking);

        assert!(stats.discovered_levels.contains("INFO"));
        assert!(stats.discovered_keys.contains("request_id"));
    }

    #[test]
    fn timestamp_stats_track_detection_and_absence() {
        reset_thread_stats();

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
}
