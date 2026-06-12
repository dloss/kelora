use crate::stats::ProcessingStats;
use rhai::Dynamic;
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use super::{with_internal_tracking, TrackingSnapshot};

thread_local! {
    // "Has this gate already recorded a success this run, on this thread?"
    // The exit-code model only needs success > 0 vs == 0, so after the first
    // success we skip the (per-event) tracker write entirely — the steady-state
    // cost on the parse/filter hot path is just this Cell read. Filters are
    // tracked per stage (bit N = filter stage number N), so a broken second
    // filter is not masked by a working first one. Reset per run via
    // `reset_stage_success_flags` (the flags are not part of the tracking
    // snapshot, so they would otherwise leak across interactive REPL runs on a
    // reused thread).
    static PARSE_SUCCESS_SEEN: Cell<bool> = const { Cell::new(false) };
    static FILTER_STAGE_SUCCESS_BITS: Cell<u64> = const { Cell::new(0) };
}

/// Clear the per-run "success seen" flags. Called once at the start of each run
/// (and each parallel worker) so a fresh run records its own first success.
pub fn reset_stage_success_flags() {
    PARSE_SUCCESS_SEEN.with(|c| c.set(false));
    FILTER_STAGE_SUCCESS_BITS.with(|c| c.set(0));
}

// Gate counters live in the always-on internal tracker, so the exit-code signal
// is independent of `--stats` / `--no-diagnostics` collection, and parallel
// workers' values sum via the `count` merge op. The *gates* are:
//
//   * parse — kind-level: `__kelora_error_count_parse` (also feeds the error
//     summary) vs `__kelora_success_count_parse`.
//   * filter — per *stage*: `__kelora_gate_error_filter_<n>` vs
//     `__kelora_gate_success_filter_<n>`, keyed by the pipeline stage number.
//     Per-stage (not per-kind) so that a working first filter cannot mask a
//     second one that errors on every event it sees. The `gate` prefix keeps
//     these out of `extract_error_summary_from_tracking`, which treats every
//     `__kelora_error_count_*` key as a reportable error type.
//
// `exec` is deliberately not a gate: a transform is *best-effort*. A failing
// exec rolls back to the event as it was before that stage and emits it anyway
// (see `dev/v2-behavior-notes.md`), so the output stays valid even when the
// transform errors on every event. Use `--strict` to fail on the first exec
// error, or `--assert` for an explicit data-quality gate.

const FILTER_GATE_ERROR_PREFIX: &str = "__kelora_gate_error_filter_";
const FILTER_GATE_SUCCESS_PREFIX: &str = "__kelora_gate_success_filter_";

/// Insert `key = 1` with a `count` merge op into a tracking map.
fn insert_gate_count(state: &mut HashMap<String, Dynamic>, key: &str) {
    state.insert(format!("__op_{}", key), Dynamic::from("count"));
    state.insert(key.to_string(), Dynamic::from(1_i64));
}

/// Record that the parse gate produced one event without error. Paired with
/// `__kelora_error_count_parse`, this lets the exit-code model distinguish
/// "errored on some lines" (recovered) from "never once parsed" (wrong format /
/// unusable input → exit 1).
///
/// Writes both the thread-local tracker *and* `ctx_internal` (the pipeline's
/// durable per-run map): per-event engine calls reinstall `ctx.internal_tracker`
/// over the thread-local state, so a write that lands only in the thread state
/// is wiped by the next `--filter`/`--exec` evaluation.
pub fn record_parse_success(ctx_internal: &mut HashMap<String, Dynamic>) {
    // Fast path on the per-event hot path: after the first success this run, a
    // single Cell read. We only need "succeeded at least once"; parallel
    // workers' single records sum via the merge op.
    if PARSE_SUCCESS_SEEN.with(|c| c.replace(true)) {
        return;
    }
    with_internal_tracking(|state| insert_gate_count(state, "__kelora_success_count_parse"));
    insert_gate_count(ctx_internal, "__kelora_success_count_parse");
}

/// Record that filter stage `stage` evaluated one event without error (whether
/// or not it matched). Paired with [`record_filter_stage_error`], this lets the
/// exit-code model distinguish a filter that errored on *some* events
/// (recovered) from one that *never once* evaluated (a broken gate → exit 1).
///
/// Like [`record_parse_success`], writes both the thread-local tracker and
/// `ctx_internal` so the counter survives the per-event thread-state reinstall.
pub fn record_filter_stage_success(stage: usize, ctx_internal: &mut HashMap<String, Dynamic>) {
    // Fast path: one bit per filter stage, a Cell read + bit test per event.
    // Stages >= 64 (implausible, but cheap to keep correct) skip the once-flag
    // and re-insert the same `count = 1` every event instead.
    if stage < 64 {
        let bit = 1u64 << stage;
        let seen = FILTER_STAGE_SUCCESS_BITS.with(|c| {
            let bits = c.get();
            c.set(bits | bit);
            bits & bit != 0
        });
        if seen {
            return;
        }
    }
    let key = format!("{}{}", FILTER_GATE_SUCCESS_PREFIX, stage);
    with_internal_tracking(|state| insert_gate_count(state, &key));
    insert_gate_count(ctx_internal, &key);
}

/// Record that filter stage `stage` errored on one event. Cold path (errors
/// only). Writes the thread-local tracker; the filter error sites then persist
/// `__kelora_gate_*` keys into `ctx.internal_tracker` together with the
/// kind-level error counters (see `persist_error_tracking`).
pub fn record_filter_stage_error(stage: usize) {
    let key = format!("{}{}", FILTER_GATE_ERROR_PREFIX, stage);
    with_internal_tracking(|state| {
        let count = state.get(&key).and_then(|v| v.as_int().ok()).unwrap_or(0) + 1;
        state.insert(format!("__op_{}", key), Dynamic::from("count"));
        state.insert(key, Dynamic::from(count));
    });
}

fn internal_count(snapshot: &TrackingSnapshot, key: &str) -> i64 {
    snapshot
        .internal
        .get(key)
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0)
}

/// The per-record half of the v2 exit-code model: true when a *gate* — parse,
/// or an individual `--filter` stage — logged at least one error but never once
/// succeeded. Transforms (`exec`) are best-effort and excluded — see the gate
/// notes above.
///
/// This is checked in resilient (non-`--strict`) mode. It reads only the
/// always-on tracker, so it holds under `--no-diagnostics` and in
/// `--metrics`/`--drain`.
pub fn stage_failed_completely(snapshot: &TrackingSnapshot) -> bool {
    let parse_errors = internal_count(snapshot, "__kelora_error_count_parse");
    if parse_errors > 0 && internal_count(snapshot, "__kelora_success_count_parse") == 0 {
        return true;
    }
    // Each filter stage is its own gate: scan the per-stage error counters and
    // require a matching per-stage success. End-of-run only, so the scan cost
    // is irrelevant.
    snapshot.internal.iter().any(|(key, value)| {
        key.strip_prefix(FILTER_GATE_ERROR_PREFIX)
            .is_some_and(|stage| {
                value.as_int().unwrap_or(0) > 0
                    && internal_count(
                        snapshot,
                        &format!("{}{}", FILTER_GATE_SUCCESS_PREFIX, stage),
                    ) == 0
            })
    })
}

/// True if any stage returned a hard error result (`ScriptResult::Error`),
/// recorded under the `script` error type. In resilient mode this happens for
/// *forbidden operations* — notably mutating `conf` outside `--begin` — which the
/// pipeline deliberately surfaces as an error result rather than rolling back.
/// These are not best-effort like ordinary `--exec` errors, so they fail the run
/// in any mode (matching the pre-2.0 behavior, where the `script` type was always
/// counted toward the exit code).
pub fn has_unrecoverable_script_error(snapshot: &TrackingSnapshot) -> bool {
    internal_count(snapshot, "__kelora_error_count_script") > 0
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
                    // A filter that erred on every event fails the run; an exec that
                    // did rolls back and is recovered (exit 0). The coaching points
                    // at --strict either way (fail on the first error / immediately).
                    summary.push_str(", affecting every event");
                    // The follow-up coaching is advisory (a typo/script-bug tip), so
                    // it honors --no-diagnostics and the suppression implied by
                    // data-only modes. Re-enable with --diagnostics.
                    if config.is_some_and(|c| !c.processing.suppress_diagnostics) {
                        summary.push_str(
                            "\n  This usually means a script bug or field-name typo. Use --strict to fail immediately, or --verbose to inspect each error.",
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

    /// Build a snapshot from raw internal keys.
    fn snapshot(entries: &[(&str, i64)]) -> TrackingSnapshot {
        let mut internal = std::collections::HashMap::new();
        for (key, value) in entries {
            if *value != 0 {
                internal.insert(key.to_string(), Dynamic::from(*value));
            }
        }
        TrackingSnapshot::from_parts(std::collections::HashMap::new(), internal)
    }

    #[test]
    fn clean_run_is_not_a_failure() {
        assert!(!stage_failed_completely(&snapshot(&[
            ("__kelora_success_count_parse", 1),
            ("__kelora_gate_success_filter_0", 1),
        ])));
    }

    #[test]
    fn partial_gate_errors_are_recovered() {
        // A gate that errored on some events but succeeded on others is recovered.
        assert!(!stage_failed_completely(&snapshot(&[
            ("__kelora_gate_error_filter_0", 3),
            ("__kelora_gate_success_filter_0", 1),
        ])));
        assert!(!stage_failed_completely(&snapshot(&[
            ("__kelora_error_count_parse", 2),
            ("__kelora_success_count_parse", 1),
        ])));
    }

    #[test]
    fn gate_with_zero_success_and_errors_fails() {
        assert!(stage_failed_completely(&snapshot(&[(
            "__kelora_gate_error_filter_0",
            5
        )])));
        // Every line failed to parse (wrong format / unusable input).
        assert!(stage_failed_completely(&snapshot(&[(
            "__kelora_error_count_parse",
            4
        )])));
    }

    #[test]
    fn each_filter_stage_is_its_own_gate() {
        // A working first filter must not mask a second filter that errors on
        // every event it sees (the multi-filter form of #241).
        assert!(stage_failed_completely(&snapshot(&[
            ("__kelora_gate_success_filter_0", 1),
            ("__kelora_gate_error_filter_1", 4),
        ])));
        // ...but a second filter with partial errors is recovered.
        assert!(!stage_failed_completely(&snapshot(&[
            ("__kelora_gate_success_filter_0", 1),
            ("__kelora_gate_error_filter_1", 4),
            ("__kelora_gate_success_filter_1", 1),
        ])));
    }

    #[test]
    fn exec_is_best_effort_and_never_fails_the_run() {
        // exec is a transform, not a gate: even erroring on every event it saw
        // (zero successes) it rolls back and emits, so it must not fail the run.
        // --strict (handled elsewhere) is the way to fail on exec errors.
        // Exec errors only ever appear as the kind-level summary counter.
        assert!(!stage_failed_completely(&snapshot(&[(
            "__kelora_error_count_exec",
            9
        )])));
    }

    #[test]
    fn script_error_result_is_unrecoverable() {
        // A "script" error (a ScriptResult::Error, e.g. mutating conf outside
        // --begin) is a forbidden operation, not best-effort: it fails the run
        // even though it's not a gate.
        let snap = snapshot(&[("__kelora_error_count_script", 1)]);
        assert!(has_unrecoverable_script_error(&snap));
        // ...and it isn't a "gate failed completely" case.
        assert!(!stage_failed_completely(&snap));
        // No script error -> not flagged.
        assert!(!has_unrecoverable_script_error(&snapshot(&[(
            "__kelora_error_count_exec",
            3
        )])));
    }

    #[test]
    fn no_errors_no_successes_is_not_a_failure() {
        // Empty input / no events reached the stage: nothing errored, so nothing
        // failed.
        assert!(!stage_failed_completely(&snapshot(&[])));
    }

    #[test]
    fn filter_stage_success_is_once_per_run_and_registers_merge_op() {
        // Reset the thread-local internal tracker and the per-run flags so the
        // test is deterministic regardless of what ran before on this thread.
        with_internal_tracking(|state| state.clear());
        reset_stage_success_flags();
        let mut ctx_internal = HashMap::new();
        // Many successes, but we only need "succeeded at least once": the count is
        // recorded exactly once (cheap fast path on every later event).
        record_filter_stage_success(2, &mut ctx_internal);
        record_filter_stage_success(2, &mut ctx_internal);
        record_filter_stage_success(2, &mut ctx_internal);
        for state in [
            &with_internal_tracking(|state| state.clone()),
            &ctx_internal,
        ] {
            assert_eq!(
                state
                    .get("__kelora_gate_success_filter_2")
                    .and_then(|v| v.as_int().ok()),
                Some(1)
            );
            // The merge op must be registered so parallel workers' counts sum.
            assert_eq!(
                state
                    .get("__op___kelora_gate_success_filter_2")
                    .and_then(|v| v.clone().into_string().ok())
                    .as_deref(),
                Some("count")
            );
        }
        // Distinct stages record under distinct keys.
        record_filter_stage_success(5, &mut ctx_internal);
        assert!(ctx_internal.contains_key("__kelora_gate_success_filter_5"));
    }

    #[test]
    fn reset_clears_the_once_flags_for_a_new_run() {
        // Run 1 records a success; without a reset, run 2 (fresh tracker) would
        // never record its own, so a run-2 partial gate failure could look total.
        with_internal_tracking(|state| state.clear());
        reset_stage_success_flags();
        let mut ctx_internal = HashMap::new();
        record_parse_success(&mut ctx_internal);
        record_filter_stage_success(0, &mut ctx_internal);
        // New run: clear tracker, reset flags -> next success records again.
        with_internal_tracking(|state| state.clear());
        reset_stage_success_flags();
        let mut ctx_internal = HashMap::new();
        record_parse_success(&mut ctx_internal);
        record_filter_stage_success(0, &mut ctx_internal);
        with_internal_tracking(|state| {
            assert_eq!(
                state
                    .get("__kelora_success_count_parse")
                    .and_then(|v| v.as_int().ok()),
                Some(1)
            );
            assert_eq!(
                state
                    .get("__kelora_gate_success_filter_0")
                    .and_then(|v| v.as_int().ok()),
                Some(1)
            );
        });
    }

    #[test]
    fn filter_stage_error_increments_per_stage() {
        with_internal_tracking(|state| state.clear());
        record_filter_stage_error(1);
        record_filter_stage_error(1);
        with_internal_tracking(|state| {
            assert_eq!(
                state
                    .get("__kelora_gate_error_filter_1")
                    .and_then(|v| v.as_int().ok()),
                Some(2)
            );
            assert_eq!(
                state
                    .get("__op___kelora_gate_error_filter_1")
                    .and_then(|v| v.clone().into_string().ok())
                    .as_deref(),
                Some("count")
            );
        });
    }
}
