use crate::stats::ProcessingStats;
use rhai::Dynamic;
use std::collections::HashSet;
use std::path::Path;

use super::{with_internal_tracking, TrackingSnapshot};

/// Per-record script kinds that the resilient exit-code model treats as
/// recoverable-until-zero-success: `parse`, `filter`, and `exec`. A stage of
/// one of these kinds that logged at least one error but never once succeeded
/// is a deterministic operator error (wrong format, field typo, type bug), not
/// data noise, so it fails the run even without `--strict`.
const PER_RECORD_KINDS: [&str; 3] = ["parse", "filter", "exec"];

/// Record that a per-record stage (`kind` in [`PER_RECORD_KINDS`]) processed one
/// event without error. Paired with `__kelora_error_count_{kind}`, this lets the
/// exit-code model distinguish "errored on some events" (recovered) from "never
/// once succeeded" (a broken stage → exit 1).
///
/// Stored in the always-on internal tracker, so the signal is independent of
/// `--stats` / `--no-diagnostics` collection, and summed across parallel workers
/// via the `count` merge op — exactly like the matching error counter.
pub fn record_stage_success(kind: &str) {
    with_internal_tracking(|state| {
        let key = format!("__kelora_success_count_{}", kind);
        if let Some(existing) = state.get_mut(&key) {
            // Steady-state path (every event): cheap in-place increment.
            let current = existing.as_int().unwrap_or(0);
            *existing = Dynamic::from(current + 1);
        } else {
            // Register the merge op so parallel workers' counts are summed.
            state.insert(format!("__op_{}", key), Dynamic::from("count"));
            state.insert(key, Dynamic::from(1_i64));
        }
    });
}

fn internal_count(snapshot: &TrackingSnapshot, key: &str) -> i64 {
    snapshot
        .internal
        .get(key)
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0)
}

/// The per-record half of the v2 exit-code model: true when any `parse`,
/// `filter`, or `exec` stage logged at least one error but never once succeeded.
///
/// This is checked in resilient (non-`--strict`) mode. Unlike a global
/// `errors >= events` ratio it is correct for multi-stage pipelines — a broken
/// `--exec` behind a selective `--filter` is caught, because each kind tracks
/// its own successes — and it reads only the always-on tracker, so it holds
/// under `--no-diagnostics` and in `--metrics`/`--drain`.
pub fn stage_failed_completely(snapshot: &TrackingSnapshot) -> bool {
    PER_RECORD_KINDS.iter().any(|kind| {
        let errors = internal_count(snapshot, &format!("__kelora_error_count_{}", kind));
        let successes = internal_count(snapshot, &format!("__kelora_success_count_{}", kind));
        errors > 0 && successes == 0
    })
}

/// Format filename for error display based on input context.
/// Returns line number only for single file/stdin, basename for multiple files
/// without conflicts, and full path when basenames conflict.
pub(crate) fn format_error_location(
    line_num: Option<usize>,
    filename: Option<&str>,
    input_files: &[String],
) -> String {
    match (line_num, filename) {
        (Some(line), Some(fname)) => {
            if input_files.is_empty() || input_files.len() == 1 {
                format!("line {}", line)
            } else {
                let basenames: HashSet<_> = input_files
                    .iter()
                    .map(|f| {
                        Path::new(f)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                    })
                    .collect();

                if basenames.len() == input_files.len() {
                    let basename = Path::new(fname)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    format!("{}:{}", basename, line)
                } else {
                    format!("{}:{}", fname, line)
                }
            }
        }
        (Some(line), None) => format!("line {}", line),
        _ => "unknown".to_string(),
    }
}

fn format_sample_location(sample: &rhai::Map) -> String {
    let line = sample
        .get("line_num")
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0);
    let filename = sample
        .get("filename")
        .and_then(|v| v.clone().into_string().ok())
        .filter(|name| !name.is_empty() && name != "stdin");

    filename
        .map(|name| format!("{}:{}", name, line))
        .unwrap_or_else(|| format!("line {}", line))
}

#[allow(clippy::too_many_arguments)]
pub fn track_error(
    error_type: &str,
    line_num: Option<usize>,
    message: &str,
    original_line: Option<&str>,
    filename: Option<&str>,
    verbose: u8,
    quiet_level: u8,
    config: Option<&crate::pipeline::PipelineConfig>,
    format_name: Option<&str>,
) {
    with_internal_tracking(|state| {
        let count_key = format!("__kelora_error_count_{}", error_type);
        let current_count = state
            .get(&count_key)
            .cloned()
            .unwrap_or(Dynamic::from(0i64));
        let new_count = current_count.as_int().unwrap_or(0) + 1;
        state.insert(count_key.clone(), Dynamic::from(new_count));
        state.insert(format!("__op_{}", count_key), Dynamic::from("count"));

        if verbose > 0 && quiet_level == 0 {
            let use_emoji = if let Some(cfg) = config {
                crate::tty::should_use_emoji_with_mode(&cfg.emoji_mode, &cfg.color_mode)
            } else {
                crate::tty::should_use_emoji_for_stderr()
            };
            let prefix = if use_emoji { "⚠️ " } else { "kelora: " };

            let input_files = config.map(|c| c.input_files.as_slice()).unwrap_or(&[]);
            let location = format_error_location(line_num, filename, input_files);
            let mut formatted_error = if error_type == "parse" {
                let format_info = if let Some(fmt) = format_name {
                    format!(" (format: {})", fmt)
                } else {
                    String::new()
                };

                if !location.is_empty() && location != "unknown" {
                    format!("{}{}{}: {}", prefix, location, format_info, message)
                } else {
                    format!("{}{}{}", prefix, format_info.trim_start(), message)
                }
            } else if !location.is_empty() && location != "unknown" {
                format!("{}{}: {} - {}", prefix, location, error_type, message)
            } else {
                format!("{}{} - {}", prefix, error_type, message)
            };

            if error_type == "parse" && format_name.is_some() && verbose > 0 {
                let hint = "\n  Hint: Input may contain mixed formats. Consider preprocessing:\n    - Split by format: grep '^{' input.log | kelora -f json\n    - Use multiline detection: kelora -M 'regex:match=^{' -f json";
                formatted_error.push_str(hint);
            }

            if crate::rhai_functions::strings::is_parallel_mode() {
                crate::rhai_functions::strings::capture_stderr(formatted_error.clone());
                if verbose >= 2 && error_type == "parse" {
                    if let Some(line) = original_line {
                        crate::rhai_functions::strings::capture_stderr(format!("    {}", line));
                        if verbose >= 3 {
                            let non_ascii_count = line.chars().filter(|c| !c.is_ascii()).count();
                            let control_char_count = line
                                .chars()
                                .filter(|c| {
                                    c.is_control() && *c != '\t' && *c != '\n' && *c != '\r'
                                })
                                .count();
                            let line_info = format!(
                                "    (length: {} chars, non_ascii: {}, control_chars: {}, starts: {:?}, ends: {:?})",
                                line.len(),
                                non_ascii_count,
                                control_char_count,
                                line.chars().next().unwrap_or('\0'),
                                line.chars().last().unwrap_or('\0')
                            );
                            crate::rhai_functions::strings::capture_stderr(line_info);
                        }
                    }
                }
            } else {
                crate::rhai_functions::strings::capture_stderr(formatted_error.clone());
                eprintln!("{}", formatted_error);
                if verbose >= 2 && error_type == "parse" {
                    if let Some(line) = original_line {
                        let indented_line = format!("    {}", line);
                        crate::rhai_functions::strings::capture_stderr(indented_line.clone());
                        eprintln!("{}", indented_line);
                        if verbose >= 3 {
                            let non_ascii_count = line.chars().filter(|c| !c.is_ascii()).count();
                            let control_char_count = line
                                .chars()
                                .filter(|c| {
                                    c.is_control() && *c != '\t' && *c != '\n' && *c != '\r'
                                })
                                .count();
                            let line_info = format!(
                                "    (length: {} chars, non_ascii: {}, control_chars: {}, starts: {:?}, ends: {:?})",
                                line.len(),
                                non_ascii_count,
                                control_char_count,
                                line.chars().next().unwrap_or('\0'),
                                line.chars().last().unwrap_or('\0')
                            );
                            crate::rhai_functions::strings::capture_stderr(line_info.clone());
                            eprintln!("{}", line_info);
                        }
                    }
                }
            }
        }

        let samples_key = format!("__kelora_error_samples_{}", error_type);
        let current_samples = state
            .get(&samples_key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current_samples.into_array() {
            if arr.len() < 3 {
                let mut sample_obj = rhai::Map::new();
                sample_obj.insert("error_type".into(), Dynamic::from(error_type.to_string()));
                sample_obj.insert(
                    "line_num".into(),
                    Dynamic::from(line_num.unwrap_or(0) as i64),
                );
                sample_obj.insert("message".into(), Dynamic::from(message.to_string()));
                if let Some(line) = original_line {
                    sample_obj.insert("original_line".into(), Dynamic::from(line.to_string()));
                }
                if let Some(filename) = filename {
                    sample_obj.insert("filename".into(), Dynamic::from(filename.to_string()));
                }

                arr.push(Dynamic::from(sample_obj));
            }

            state.insert(samples_key.clone(), Dynamic::from(arr));
            state.insert(format!("__op_{}", samples_key), Dynamic::from("unique"));
        }
    });
}

#[cfg(test)]
pub fn has_errors_in_tracking(snapshot: &TrackingSnapshot) -> bool {
    has_errors_in_tracking_with_policy(snapshot, true)
}

pub fn has_errors_in_tracking_with_policy(
    snapshot: &TrackingSnapshot,
    include_recovered_runtime_errors: bool,
) -> bool {
    for (key, value) in &snapshot.internal {
        if let Some(error_type) = key.strip_prefix("__kelora_error_count_") {
            if !include_recovered_runtime_errors && is_recovered_runtime_error(error_type) {
                continue;
            }

            if let Ok(count) = value.as_int() {
                if count > 0 {
                    return true;
                }
            }
        }
    }
    false
}

fn is_recovered_runtime_error(error_type: &str) -> bool {
    matches!(error_type, "exec" | "filter")
}

pub fn format_fatal_error_line(snapshot: &TrackingSnapshot) -> String {
    let mut total_errors = 0;
    let mut error_types = Vec::new();
    let mut all_samples: Vec<rhai::Map> = Vec::new();

    for (key, value) in &snapshot.internal {
        if let Some(error_type) = key.strip_prefix("__kelora_error_count_") {
            if let Ok(count) = value.as_int() {
                if count > 0 {
                    total_errors += count;
                    error_types.push((error_type.to_string(), count));
                }
            }
        }
    }

    if total_errors == 0 {
        return "fatal error encountered".to_string();
    }

    for (key, value) in &snapshot.internal {
        if let Some(_error_type) = key.strip_prefix("__kelora_error_samples_") {
            if let Ok(sample_array) = value.clone().into_array() {
                for sample in sample_array {
                    if let Some(sample_map) = sample.try_cast::<rhai::Map>() {
                        all_samples.push(sample_map);
                    }
                }
            }
        }
    }

    if error_types.len() == 1 {
        let (error_type, count) = &error_types[0];

        if *count == 1 && !all_samples.is_empty() {
            let sample = &all_samples[0];
            let message = sample
                .get("message")
                .and_then(|v| v.clone().into_string().ok())
                .unwrap_or_else(|| "unknown error".to_string());
            let message = if message.len() > 80 {
                format!("{}...", &message[..77])
            } else {
                message
            };
            format!("1 {} error: {}", error_type, message)
        } else if *count <= 3 && all_samples.len() as i64 == *count {
            let locations: Vec<String> = all_samples.iter().map(format_sample_location).collect();

            format!(
                "{} {} errors at {}",
                count,
                error_type,
                locations.join(", ")
            )
        } else if !all_samples.is_empty() {
            let first_location = format_sample_location(&all_samples[0]);

            format!(
                "{} {} errors (first at: {})",
                count, error_type, first_location
            )
        } else {
            format!("{} {} errors", count, error_type)
        }
    } else if total_errors <= 10 {
        let breakdown: Vec<String> = error_types
            .iter()
            .map(|(t, c)| format!("{} {}", c, t))
            .collect();

        format!("{} errors: {}", total_errors, breakdown.join(", "))
    } else {
        format!("{} errors (mixed types)", total_errors)
    }
}

pub fn extract_error_summary_from_tracking(
    snapshot: &TrackingSnapshot,
    verbose: u8,
    stats: Option<&ProcessingStats>,
    config: Option<&crate::config::KeloraConfig>,
) -> Option<String> {
    let mut total_errors = 0;
    let mut error_types = Vec::new();
    let mut sample_objects: Vec<rhai::Map> = Vec::new();

    for (key, value) in &snapshot.internal {
        if let Some(error_type) = key.strip_prefix("__kelora_error_count_") {
            if let Ok(count) = value.as_int() {
                if count > 0 {
                    total_errors += count;
                    error_types.push((error_type.to_string(), count));
                }
            }
        }
    }

    if total_errors == 0 {
        return None;
    }

    for (key, value) in &snapshot.internal {
        if let Some(_error_type) = key.strip_prefix("__kelora_error_samples_") {
            if let Ok(sample_array) = value.clone().into_array() {
                for sample in sample_array {
                    if let Some(sample_map) = sample.try_cast::<rhai::Map>() {
                        sample_objects.push(sample_map);
                    }
                }
            }
        }
    }

    let mut summary = String::new();

    let primary_error_type = if error_types.len() == 1 {
        &error_types[0].0
    } else {
        "mixed"
    };

    if primary_error_type == "mixed" {
        summary.push_str(&format!("Mixed errors: {} total", total_errors));
    } else {
        summary.push_str(&format!(
            "{}{} errors: {} total",
            primary_error_type.chars().next().unwrap().to_uppercase(),
            &primary_error_type[1..],
            total_errors
        ));
    }

    if config.is_some_and(|c| !c.processing.strict)
        && is_recovered_runtime_error(primary_error_type)
    {
        if let Some(stats) = stats {
            if stats.events_created > 0 {
                let pct = (total_errors as f64 / stats.events_created as f64) * 100.0;
                if total_errors as usize >= stats.events_created {
                    // The scope ("every event") is a factual correctness signal and
                    // is part of the error summary, which surfaces unless --silent.
                    // A stage that erred on every event never once succeeded, so the
                    // run already exits non-zero (see stage_failed_completely) — the
                    // coaching reflects that rather than telling the user to add
                    // --strict to get a failure they already have.
                    summary.push_str(", affecting every event (non-zero exit)");
                    // The follow-up coaching is advisory (a typo/script-bug tip), so
                    // it honors --no-diagnostics and the suppression implied by
                    // data-only modes. Re-enable with --diagnostics.
                    if config.is_some_and(|c| !c.processing.suppress_diagnostics) {
                        summary.push_str(
                            "\n  This usually means a script bug or field-name typo. Use --verbose to inspect each error.",
                        );
                    }
                } else if pct >= 90.0 {
                    summary.push_str(&format!(", affecting {:.1}% of events", pct));
                }
            }
        }
    }

    let mut shown_samples = 0;
    for sample_obj in &sample_objects {
        if shown_samples >= 3 {
            break;
        }

        let message = sample_obj
            .get("message")
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "unknown error".to_string());
        let original_line = sample_obj
            .get("original_line")
            .and_then(|v| v.clone().into_string().ok());
        let location = format_sample_location(sample_obj);

        summary.push_str(&format!("\n  {}: {}", location, message));

        if verbose > 0 {
            if let Some(orig_line) = original_line {
                let display_line = if orig_line.len() > 100 {
                    format!("{}...", &orig_line[..97])
                } else {
                    orig_line
                };
                summary.push_str(&format!("\n    {}", display_line));
            }
        }

        shown_samples += 1;
    }

    if total_errors as usize > shown_samples {
        let remaining = total_errors as usize - shown_samples;
        let message = if verbose > 0 {
            "All errors shown during processing. Use --no-diagnostics to suppress this summary."
        } else {
            "Use -v to see each error or --no-diagnostics to suppress this summary."
        };

        summary.push_str(&format!("\n  [+{} more. {}]", remaining, message));
    }

    if let Some(stats) = stats {
        if stats.yearless_timestamps > 0 {
            let warning_msg = format!(
                "Year-less timestamp format detected ({} parse{})\n\
                   Format lacks year (e.g., \"Dec 31 23:59:59\")\n\
                   Year inferred using heuristic (+/- 1 year from current date)\n\
                   Timestamps >18 months old may be incorrect",
                stats.yearless_timestamps,
                if stats.yearless_timestamps == 1 {
                    ""
                } else {
                    "s"
                }
            );
            summary.push_str("\n  ");
            summary.push_str(
                &crate::config::format_warning_message_auto(&warning_msg).replace('\n', "\n  "),
            );
        }
    }

    Some(summary)
}

#[cfg(test)]
mod stage_outcome_tests {
    use super::*;
    use rhai::Dynamic;

    /// Build a snapshot from `(error_count, success_count)` pairs keyed by kind.
    fn snapshot(kinds: &[(&str, i64, i64)]) -> TrackingSnapshot {
        let mut internal = std::collections::HashMap::new();
        for (kind, errors, successes) in kinds {
            if *errors != 0 {
                internal.insert(
                    format!("__kelora_error_count_{}", kind),
                    Dynamic::from(*errors),
                );
            }
            if *successes != 0 {
                internal.insert(
                    format!("__kelora_success_count_{}", kind),
                    Dynamic::from(*successes),
                );
            }
        }
        TrackingSnapshot::from_parts(std::collections::HashMap::new(), internal)
    }

    #[test]
    fn clean_run_is_not_a_failure() {
        assert!(!stage_failed_completely(&snapshot(&[("filter", 0, 10)])));
    }

    #[test]
    fn partial_errors_are_recovered() {
        // Errors on some events, but the stage also succeeded on others.
        assert!(!stage_failed_completely(&snapshot(&[("exec", 3, 7)])));
        assert!(!stage_failed_completely(&snapshot(&[("parse", 2, 1)])));
    }

    #[test]
    fn zero_success_with_errors_fails() {
        assert!(stage_failed_completely(&snapshot(&[("filter", 5, 0)])));
        assert!(stage_failed_completely(&snapshot(&[("exec", 1, 0)])));
        // Every line failed to parse (wrong format / unusable input).
        assert!(stage_failed_completely(&snapshot(&[("parse", 4, 0)])));
    }

    #[test]
    fn broken_exec_behind_selective_filter_is_per_stage() {
        // The filter succeeded on every event it saw; the exec errored on every
        // event it saw. A global error/event ratio would miss this — per-kind
        // success tracking catches it.
        assert!(stage_failed_completely(&snapshot(&[
            ("filter", 0, 4),
            ("exec", 2, 0),
        ])));
    }

    #[test]
    fn no_errors_no_successes_is_not_a_failure() {
        // Empty input / no events reached the stage: nothing errored, so nothing
        // failed.
        assert!(!stage_failed_completely(&snapshot(&[])));
    }

    #[test]
    fn record_stage_success_increments_and_registers_merge_op() {
        // Reset the thread-local internal tracker so the test is deterministic
        // regardless of what ran before on this worker thread.
        with_internal_tracking(|state| state.clear());
        record_stage_success("filter");
        record_stage_success("filter");
        with_internal_tracking(|state| {
            assert_eq!(
                state
                    .get("__kelora_success_count_filter")
                    .and_then(|v| v.as_int().ok()),
                Some(2)
            );
            // The merge op must be registered so parallel workers' counts sum.
            assert_eq!(
                state
                    .get("__op___kelora_success_count_filter")
                    .and_then(|v| v.clone().into_string().ok())
                    .as_deref(),
                Some("count")
            );
        });
    }
}
