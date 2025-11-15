use crate::rhai_functions::datetime::DurationWrapper;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::BTreeSet;
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
}

// Thread-local storage for statistics (following track_count pattern)
thread_local! {
    static THREAD_STATS: RefCell<ProcessingStats> = RefCell::new(ProcessingStats::new());
}

// Public API functions for stats collection (following track_count pattern)
// Note: These functions are conditionally called based on config.output.stats flag
#[allow(dead_code)] // Used conditionally in lib.rs when stats are enabled
pub fn stats_add_line_read() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_read += 1;
    });
}

#[allow(dead_code)] // Used conditionally in lib.rs when stats are enabled
pub fn stats_add_line_output() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_output += 1;
    });
}

#[allow(dead_code)] // Used conditionally in lib.rs when stats are enabled
pub fn stats_add_line_filtered() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().lines_filtered += 1;
    });
}

pub fn stats_add_event_created() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_created += 1;
    });
}

pub fn stats_add_event_output() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_output += 1;
    });
}

pub fn stats_add_event_filtered() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().events_filtered += 1;
    });
}

pub fn stats_set_timestamp_override(field: Option<String>, format: Option<String>) {
    THREAD_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.timestamp_override_field = field;
        stats.timestamp_override_format = format;
        stats.timestamp_override_failed = false;
        stats.timestamp_override_warning = None;
    });
}

pub fn stats_add_late_event() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().late_events += 1;
    });
}

pub fn stats_add_yearless_timestamp() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().yearless_timestamps += 1;
    });
}

#[allow(dead_code)] // Used conditionally in lib.rs when stats are enabled
pub fn stats_add_error() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().errors += 1;
    });
}

pub fn stats_start_timer() {
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().start_time = Some(Instant::now());
    });
}

pub fn stats_finish_processing() {
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
    THREAD_STATS.with(|stats| stats.borrow().clone())
}

pub fn stats_record_timestamp_detection(field_name: &str, _raw_value: &str, parsed: bool) {
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
    THREAD_STATS.with(|stats| {
        stats.borrow_mut().timestamp_absent_events += 1;
    });
}

pub fn stats_update_timestamp(timestamp: DateTime<Utc>) {
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
    #[allow(dead_code)] // Used in sequential processing, but clippy doesn't detect it properly
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
    #[allow(dead_code)] // Used in main.rs when stats are enabled
    pub fn format_stats(&self, _multiline_enabled: bool) -> String {
        self.format_stats_internal(_multiline_enabled, false)
    }

    /// Format stats for signal handlers (skips line counts which are always 0)
    #[allow(dead_code)]
    pub fn format_stats_for_signal(&self, _multiline_enabled: bool) -> String {
        self.format_stats_internal(_multiline_enabled, true)
    }

    fn format_stats_internal(&self, _multiline_enabled: bool, skip_line_counts: bool) -> String {
        let mut output = String::new();

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

        if self.yearless_timestamps > 0 {
            output.push_str(&format!(
                "Warning: Year-less timestamp format detected ({} parse{})\n",
                self.yearless_timestamps,
                if self.yearless_timestamps == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            output.push_str("  Format lacks year (e.g., \"Dec 31 23:59:59\")\n");
            output.push_str("  Year inferred using heuristic (±1 year from current date)\n");
            output.push_str("  Timestamps >18 months old may be incorrect\n");
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
    #[allow(dead_code)] // Used by main.rs binary target, not detected by clippy in lib context
    pub fn has_errors(&self) -> bool {
        self.lines_errors > 0
    }

    /// Format a concise error summary for default output (when errors occur)
    #[allow(dead_code)] // Used by main.rs binary target, not detected by clippy in lib context
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
