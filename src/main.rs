use anyhow::Result;
use crossbeam_channel::unbounded;
use std::collections::BTreeSet;

// Fast allocator: the streaming hot path is allocation-bound (per-event IndexMap
// inserts, Dynamic/Event clones). mimalloc cuts that churn versus the system malloc.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::io::IsTerminal;
use std::sync::atomic::Ordering;

#[cfg(unix)]
use signal_hook::consts::{SIGINT, SIGTERM};

mod args;
mod cli;
mod colors;
mod config;
mod config_file;
mod decompression;
mod detection;
mod drain;
mod engine;
mod event;
mod field_discovery;
mod formatters;
mod help;
mod interactive;
mod parallel;
mod parsers;
mod pipeline;
mod platform;
mod readers;
mod rhai_functions;
mod runner;
mod stats;
#[cfg(test)]
mod test_env;
mod timestamp;
mod tty;

// Re-export types at crate root for use by submodules
pub use cli::{FileOrder, InputFormat, OutputFormat};

use crate::rhai_functions::tracking::TrackingSnapshot;
use args::{process_args_with_config, validate_cli_args};
use cli::Cli;
use config::{
    KeloraConfig, MultilineConfig, ScriptStageType, SectionEnd, SectionStart, SpanMode,
    TimestampFilterConfig,
};
use platform::{
    install_broken_pipe_panic_hook, Ctrl, ExitCode, ProcessCleanup, SafeFileOut, SafeStderr,
    SafeStdout, SignalHandler, SHOULD_TERMINATE, TERMINATED_BY_SIGNAL, TERMINATION_SIGNAL,
};
use runner::{run_pipeline_with_kelora_config, PipelineResult};

fn main() -> Result<()> {
    install_broken_pipe_panic_hook();
    // Broadcast channel for shutdown requests from signal handler or other sources
    let (ctrl_tx, ctrl_rx) = unbounded::<Ctrl>();

    // Initialize signal handling early
    let _signal_handler = match SignalHandler::new(ctrl_tx.clone()) {
        Ok(handler) => handler,
        Err(e) => {
            eprintln!("Failed to initialize signal handling: {}", e);
            ExitCode::GeneralError.exit();
        }
    };

    // Initialize process cleanup
    let _cleanup = ProcessCleanup::new();

    // Initialize safe I/O wrappers
    let mut stderr = SafeStderr::new();
    let mut stdout = SafeStdout::new();

    // Process command line arguments with config file support
    let (matches, cli, config_expansion_info) = process_args_with_config(&mut stderr);

    // Validate CLI argument combinations
    if let Err(e) = validate_cli_args(&cli) {
        stderr
            .writeln(&config::format_error_message_auto(&format!("Error: {}", e)))
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    // Reject an invalid KELORA_SEED up front so reproducible runs fail fast
    // instead of silently falling back to a random seed.
    if let Err(raw) = crate::rhai_functions::random::parse_seed_env() {
        stderr
            .writeln(&config::format_error_message_auto(&format!(
                "Error: KELORA_SEED must be a non-negative integer, got '{}'",
                raw
            )))
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    // Extract ordered script stages
    let ordered_stages = match cli.get_ordered_script_stages(&matches) {
        Ok(stages) => stages,
        Err(e) => {
            stderr
                .writeln(&config::format_error_message_auto(&format!("Error: {}", e)))
                .unwrap_or(());
            ExitCode::InvalidUsage.exit();
        }
    };

    // Create configuration from CLI and set stages (using lib config directly)
    let mut config = match KeloraConfig::from_cli(&cli) {
        Ok(cfg) => cfg,
        Err(e) => {
            stderr
                .writeln(&config::format_error_message_auto(&format!(
                    "Error: {:#}",
                    e
                )))
                .unwrap_or(());
            std::process::exit(ExitCode::InvalidUsage as i32);
        }
    };

    // Display config expansion info (if diagnostics enabled)
    KeloraConfig::display_config_expansion(&config_expansion_info, &config, &mut stderr);

    // Set the ordered stages directly
    config.processing.stages = ordered_stages;
    let diagnostics_allowed = !config.processing.silent && !config.processing.suppress_diagnostics;
    // Runtime warnings emitted from inside tracking functions (e.g. the
    // track_unique size warning) honor the same gate as other diagnostics.
    crate::rhai_functions::tracking::set_tracking_warnings_enabled(diagnostics_allowed);

    let parallel_requested = config.performance.parallel
        || config.performance.threads > 0
        || config.performance.batch_size.is_some();

    if config.processing.span.is_some() && diagnostics_allowed && parallel_requested {
        let warning = config.format_error_message(
            "span aggregation requires sequential mode; ignoring --parallel settings. Rerun without --parallel if you need span aggregation.",
        );
        stderr.writeln(&warning).unwrap_or(());
    } else if (config.processing.window_size > 0 || config.processing.context.is_active())
        && diagnostics_allowed
        && parallel_requested
    {
        // Cross-event context (--window, -B/-C) is order-dependent and would be
        // silently corrupted by parallel batching (issue #281), so we force
        // sequential just like spans. Warn once, mirroring the span fallback.
        let warning = config.format_error_message(
            "cross-event context (--window or -B/-C) requires sequential mode; ignoring --parallel settings. Rerun without --parallel if you need cross-event context.",
        );
        stderr.writeln(&warning).unwrap_or(());
    }

    if let Some(span_cfg) = &config.processing.span {
        if let SpanMode::Count { events_per_span } = span_cfg.mode {
            if events_per_span > 100_000 && diagnostics_allowed {
                let warning = config.format_error_message(
                    "span size above 100000 may require substantial memory; consider time-based spans",
                );
                stderr.writeln(&warning).unwrap_or(());
            }
        }
    }

    // Set processed begin/end scripts with includes applied
    let (processed_begin, processed_end) = match cli.get_processed_begin_end(&matches) {
        Ok(scripts) => scripts,
        Err(e) => {
            stderr.writeln(&format!("kelora: {:#}", e)).unwrap_or(());
            std::process::exit(ExitCode::GeneralError as i32);
        }
    };
    config.processing.begin = processed_begin;
    config.processing.end = processed_end;

    // Parse timestamp filter arguments if provided
    if cli.since.is_some() || cli.until.is_some() {
        // Use the same timezone logic as the main configuration
        let cli_timezone = config.input.default_timezone.as_deref();
        let (since, until) = match crate::timestamp::resolve_time_range(
            cli.since.as_deref(),
            cli.until.as_deref(),
            cli_timezone,
        ) {
            Ok(range) => range,
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&e))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        };

        config.processing.timestamp_filter = Some(TimestampFilterConfig { since, until });
    }

    // Compile ignore-lines regex if provided
    if let Some(ignore_pattern) = &cli.ignore_lines {
        match regex::Regex::new(ignore_pattern) {
            Ok(regex) => {
                config.input.ignore_lines = Some(regex);
            }
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid ignore-lines regex pattern '{}': {}",
                        ignore_pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    }

    // Compile keep-lines regex if provided
    if let Some(keep_pattern) = &cli.keep_lines {
        match regex::Regex::new(keep_pattern) {
            Ok(regex) => {
                config.input.keep_lines = Some(regex);
            }
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid keep-lines regex pattern '{}': {}",
                        keep_pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    }

    // Compile section selection regexes if provided
    let section_start = if let Some(ref pattern) = cli.section_from {
        match regex::Regex::new(pattern) {
            Ok(regex) => Some(SectionStart::From(regex)),
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid --section-from regex pattern '{}': {}",
                        pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    } else if let Some(ref pattern) = cli.section_after {
        match regex::Regex::new(pattern) {
            Ok(regex) => Some(SectionStart::After(regex)),
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid --section-after regex pattern '{}': {}",
                        pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    } else {
        None
    };

    let section_end = if let Some(ref pattern) = cli.section_before {
        match regex::Regex::new(pattern) {
            Ok(regex) => Some(SectionEnd::Before(regex)),
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid --section-before regex pattern '{}': {}",
                        pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    } else if let Some(ref pattern) = cli.section_through {
        match regex::Regex::new(pattern) {
            Ok(regex) => Some(SectionEnd::Through(regex)),
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid --section-through regex pattern '{}': {}",
                        pattern, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    } else {
        None
    };

    if section_start.is_some() || section_end.is_some() {
        config.input.section = Some(crate::config::SectionConfig {
            start: section_start,
            end: section_end,
            max_sections: cli.max_sections,
        });
    }

    // Parse multiline configuration if provided
    if let Some(multiline_str) = &cli.multiline {
        match MultilineConfig::parse(multiline_str) {
            Ok(mut multiline_config) => {
                multiline_config.join = cli.multiline_join;
                config.input.multiline = Some(multiline_config);
            }
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid multiline configuration '{}': {}",
                        multiline_str, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    }

    if let Some(ref gap_str) = cli.mark_gaps {
        match crate::rhai_functions::datetime::to_duration(gap_str) {
            Ok(duration) => {
                if duration.inner.is_zero() {
                    stderr
                        .writeln(&config.format_error_message(
                            "--mark-gaps requires a duration greater than zero",
                        ))
                        .unwrap_or(());
                    ExitCode::InvalidUsage.exit();
                }
                config.output.mark_gaps = Some(duration.inner);
            }
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&format!(
                        "Invalid --mark-gaps duration '{}': {}",
                        gap_str, e
                    )))
                    .unwrap_or(());
                ExitCode::InvalidUsage.exit();
            }
        }
    }

    // Handle output destination and run pipeline
    let diagnostics_allowed_runtime =
        !config.processing.silent && !config.processing.suppress_diagnostics;
    let terminal_allowed = !config.processing.silent;

    let result = if let Some(ref output_file_path) = cli.output_file {
        // Guardrail: `-o`/`--output-file` takes a FILE, but it is easy to
        // mistake it for an output-FORMAT selector (which is `-F`). A bare
        // value that exactly matches a known format name (no path, no
        // extension) is almost always that mistake — e.g. `-o json` silently
        // writes a file literally named `json`. Warn, but still honor the
        // request so existing scripts are unaffected.
        if !config.processing.silent
            && !output_file_path.contains(std::path::is_separator)
            && !output_file_path.contains('.')
        {
            const FORMAT_NAMES: &[&str] = &[
                "default", "json", "logfmt", "inspect", "levelmap", "keymap", "tailmap", "csv",
                "tsv", "csvnh", "tsvnh", "line", "raw", "syslog", "cef", "combined",
            ];
            if FORMAT_NAMES.contains(&output_file_path.to_ascii_lowercase().as_str()) {
                stderr
                    .writeln(&config.format_warning_message(&format!(
                        "writing output to a file named '{}'; did you mean -F {} (--output-format)?",
                        output_file_path, output_file_path
                    )))
                    .unwrap_or(());
            }
        }
        // Use file output
        let file_output = match SafeFileOut::new(output_file_path) {
            Ok(file) => file,
            Err(e) => {
                stderr
                    .writeln(&config.format_error_message(&e.to_string()))
                    .unwrap_or(());
                ExitCode::GeneralError.exit();
            }
        };
        run_pipeline_with_kelora_config(&config, file_output, &ctrl_rx)
    } else {
        // Use stdout output
        let stdout_output = SafeStdout::new();
        run_pipeline_with_kelora_config(&config, stdout_output, &ctrl_rx)
    };

    let (final_stats, tracking_data) = match result {
        Ok(pipeline_result) => handle_pipeline_success(
            &config,
            pipeline_result,
            &mut stdout,
            &mut stderr,
            diagnostics_allowed_runtime,
            terminal_allowed,
        ),
        Err(e) => {
            // When every input failed to open, auto-detection already printed the
            // per-file reasons in detail; a generic "Pipeline error: …" line would
            // just repeat them. Any other error still prints normally. downcast_ref
            // walks the source chain, so this holds even if the error was wrapped
            // with context on the way up.
            if e.downcast_ref::<detection::AllInputsUnopenable>().is_none() {
                emit_fatal_line(&mut stderr, &config, &format!("Pipeline error: {}", e));
            }
            ExitCode::GeneralError.exit();
        }
    };

    // Determine if any events were output (to conditionally suppress leading newlines)
    let events_were_output = final_stats
        .as_ref()
        .is_some_and(|s| !config.processing.quiet_events && s.events_output > 0);

    // Check if we were terminated by a signal and print output
    if TERMINATED_BY_SIGNAL.load(Ordering::Relaxed) {
        handle_signal_termination(
            &config,
            final_stats.as_ref(),
            events_were_output,
            &mut stderr,
            terminal_allowed,
        );
    }

    let override_failed = final_stats
        .as_ref()
        .is_some_and(|stats| stats.timestamp_override_failed);
    let override_message = final_stats
        .as_ref()
        .and_then(|stats| stats.timestamp_override_warning.clone());

    // Determine the exit code from the two-axis v2 error model:
    //
    //   * Structural failures (can't open a named input, begin/end stage error)
    //     and explicit --assert violations fail the run in any mode. begin/end
    //     errors already abort as a fatal pipeline error above; the rest come
    //     from has_fatal_errors.
    //   * Per-record stages (parse / filter / exec) are recovered by default and
    //     fail the run only when one of them never once succeeded — a broken
    //     filter, a transform that errors on everything, or a whole input that
    //     won't parse. That signal lives in the always-on tracker, so it holds
    //     even under --no-diagnostics and in --metrics/--drain.
    //   * --strict escalates: any single parse/filter/exec error is fatal.
    //
    // has_errors() (any error worth *reporting*) is deliberately not used here:
    // a partial parse failure is reported but recovered.
    let strict = config.processing.strict;
    let mut had_errors = {
        let tracking_errors = tracking_data
            .as_ref()
            .map(|tracking| {
                if strict {
                    crate::rhai_functions::tracking::has_errors_in_tracking_with_policy(
                        tracking, true,
                    )
                } else {
                    // Resilient: a gate (parse/filter) that never succeeded, or a
                    // forbidden operation (conf mutation -> a "script" error result).
                    // Exec errors are best-effort and excluded.
                    crate::rhai_functions::tracking::stage_failed_completely(tracking)
                        || crate::rhai_functions::tracking::has_unrecoverable_script_error(tracking)
                }
            })
            .unwrap_or(false);
        let stats_errors = final_stats
            .as_ref()
            .map(|s| s.has_fatal_errors(strict))
            .unwrap_or(false);
        tracking_errors || stats_errors
    };

    if config.processing.strict && override_failed {
        if diagnostics_allowed_runtime && config.output.stats.is_none() {
            if let Some(message) = override_message.clone() {
                let formatted = config.format_error_message(&message);
                stderr.writeln(&formatted).unwrap_or(());
            }
        }
        had_errors = true;
    }

    // The terse fatal line is the *substitute* for the full error summary when
    // output is suppressed. The summary now surfaces whenever stderr is allowed
    // (everything except --silent), so the fatal line is only the fallback under
    // --silent; otherwise it would duplicate the summary.
    if had_errors && !terminal_allowed {
        let fatal_message = if let Some(ref tracking) = tracking_data {
            crate::rhai_functions::tracking::format_fatal_error_line(tracking)
        } else {
            "fatal error encountered".to_string()
        };
        emit_fatal_line(&mut stderr, &config, &fatal_message);
    }

    // Print assertion failure summary if any occurred
    if let Some(ref stats) = final_stats {
        if stats.assertion_failures > 0 {
            let failure_text = if stats.assertion_failures == 1 {
                "1 assertion failure".to_string()
            } else {
                format!("{} assertion failures", stats.assertion_failures)
            };
            eprintln!("{}", config.format_error_message(&failure_text));
        }
    }

    if had_errors {
        ExitCode::GeneralError.exit();
    } else {
        ExitCode::Success.exit();
    }
}

fn collect_filter_field_references(config: &KeloraConfig) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    let re = regex::Regex::new(r"\be\.([A-Za-z_][A-Za-z0-9_]*)").expect("valid filter regex");

    for stage in &config.processing.stages {
        if let ScriptStageType::Filter { script, .. } = stage {
            for captures in re.captures_iter(script) {
                if let Some(field) = captures.get(1) {
                    fields.insert(field.as_str().to_string());
                }
            }
        }
    }

    fields
}

/// Advisory diagnostic for ragged CSV/TSV rows. Resilient mode preserves the
/// data (overflow columns as cN fields, short rows with absent fields), so
/// this is a hint rather than an error; --strict rejects such rows instead.
fn maybe_print_csv_shape_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    if let Some(summary) = stats.format_ragged_rows_summary() {
        let mut message = format!("{}.", summary);
        // Over-wide rows usually mean an unescaped delimiter somewhere in the
        // row, so the *named* fields after it may hold the wrong values — the
        // extras are preserved, but don't take those rows at face value.
        if stats.csv_rows_extra_columns > 0 {
            if let Some(col) = stats.csv_overflow_start_column {
                message.push_str(&format!(
                    " Named fields on over-wide rows may be misaligned; inspect them with --filter '\"c{}\" in e'.",
                    col
                ));
            }
        }
        message.push_str(" Use --strict to reject ragged rows.");
        let formatted = config
            .format_hint_message(&message)
            .trim_start_matches('\n')
            .to_string();
        stderr.writeln(&formatted).unwrap_or(());
    }
}

fn maybe_print_zero_results_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    if stats.events_created == 0 || stats.events_output > 0 || stats.has_errors() {
        return;
    }

    // When every event is filtered out, the most actionable explanation is the
    // built-in filter whose keying field never appeared in the input. Check the
    // structural causes in pipeline order (level -> time -> --filter) and emit
    // the first that applies, so the hint stays focused on a single culprit.
    let hint = level_filter_zero_hint(config, stats)
        .or_else(|| timestamp_filter_zero_hint(config, stats))
        .or_else(|| filter_field_zero_hint(config, stats))
        .or_else(|| filter_numeric_string_hint(config, stats))
        .or_else(|| generic_filter_zero_hint(config, stats));

    if let Some(message) = hint {
        let formatted = config
            .format_hint_message(&message)
            .trim_start_matches('\n')
            .to_string();
        stderr.writeln(&formatted).unwrap_or(());
    }
}

/// Hint for the bare "no input" case. The quick help promises interactive mode
/// "with no arguments", but interactive mode only triggers on a real TTY (see
/// the gate in `args.rs`). When stdin is not a terminal (piped or redirected to
/// empty) and no files were given, kelora reads stdin, hits immediate EOF, and
/// otherwise exits 0 in silence — leaving the user wondering whether it
/// crashed. A one-line nudge points them at the actual options instead.
///
/// Fires only when truly nothing flowed: this lives behind the
/// `diagnostics_allowed_runtime` gate at the call site, which guarantees stats
/// were collected. Note that `lines_read` is only incremented under `-s/--stats`
/// (see `process_single_line` in runner.rs), so on the normal diagnostics path
/// it stays zero even when lines *were* read — the reliable "input arrived"
/// signals here are `events_created` (lines that parsed into events) and
/// `lines_errors` (lines that were read but failed to parse). Both are tracked
/// whenever diagnostics are on. Without the `lines_errors` check, unparseable
/// input (e.g. plain text fed with `-j`) produced both a "Parse errors" report
/// *and* a contradictory "stdin is empty" nudge. A legitimate empty pipe
/// (`grep nomatch f | kelora`) is no different from `kelora < /dev/null` here;
/// the nudge goes to stderr, so it never pollutes a downstream pipeline.
fn maybe_print_no_input_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    // --no-input is an explicit opt-out (begin/end-only scripting); respect it.
    if !config.input.files.is_empty()
        || config.input.no_input
        || crate::tty::is_stdin_tty()
        || stats.lines_read != 0
        || stats.events_created != 0
        || stats.lines_errors != 0
    {
        return;
    }

    let message = "No input: stdin is empty and no files were given. \
         Pass a file, pipe data in, or run kelora in a terminal for interactive mode. \
         See -h for a quick reference.";
    let formatted = config
        .format_hint_message(message)
        .trim_start_matches('\n')
        .to_string();
    stderr.writeln(&formatted).unwrap_or(());
}

/// Hint when input timestamps carry no zone offset and the UTC default was
/// assumed silently (#287). Naive timestamps (syslog, log4j, python-logging,
/// glog, apache-error, postgres, …) are resolved with `--input-tz` (default
/// UTC) everywhere; for a source that logs local time this shifts every
/// `parsed_ts` with no signal, quietly moving `--since`/`--until`/`--span`
/// boundaries and — under `--normalize-ts` — baking the wrong offset into the
/// output itself.
///
/// To avoid crying wolf on the common UTC cloud-log case, the hint fires only
/// when the run actually *depends on* or *materializes* the assumption: a time
/// filter, a span, or `--normalize-ts` is active. It stays silent when the user
/// chose a zone (`config.input.timezone_assumed` is false for an explicit
/// `--input-tz` or a non-empty `TZ`). Like the other one-time hints this lives
/// behind the `diagnostics_allowed_runtime` gate at the call site, so
/// `--no-diagnostics` / `-q` / `--silent` already suppress it.
fn maybe_print_naive_tz_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    if stats.naive_timestamps == 0 || !config.input.timezone_assumed {
        return;
    }

    let normalize = config.processing.normalize_timestamps;
    let time_op_active = normalize
        || config.processing.timestamp_filter.is_some()
        || config.processing.span.is_some();
    if !time_op_active {
        return;
    }

    let message = if normalize {
        "Timestamps carry no zone offset; assuming UTC, and --normalize-ts writes that offset into the output. Pass --input-tz <zone> if your source is not UTC."
    } else {
        "Timestamps carry no zone offset; assuming UTC. Pass --input-tz <zone> if your source is not UTC."
    };
    let formatted = config
        .format_hint_message(message)
        .trim_start_matches('\n')
        .to_string();
    stderr.writeln(&formatted).unwrap_or(());
}

/// Hint when `-l/--levels` dropped every event. Two structural causes are
/// distinguished, both far more actionable than a silent empty result:
///
/// 1. The input carries no level field at all (e.g. unstructured `line` input).
/// 2. A level field *is* present, but none of the requested levels appear among
///    the values actually seen — a vocabulary mismatch. This is the dangerous
///    case for an operator: glog logs `I/W/E/F`, syslog uses `CRIT` not
///    `CRITICAL`, so `-l ERROR` silently returns nothing even though errors
///    exist, which reads as "all clear". We list the levels actually present so
///    the empty result can't be misread. No semantics are applied — we just show
///    the operator the dialect and let them recognise it.
///
/// `--exclude-levels` alone keeps level-less events, so only an active include
/// list can zero the stream this way.
fn level_filter_zero_hint(config: &KeloraConfig, stats: &stats::ProcessingStats) -> Option<String> {
    if config.processing.levels.is_empty() {
        return None;
    }

    let has_level_field = crate::event::LEVEL_FIELD_NAMES
        .iter()
        .any(|name| stats.discovered_keys.contains(*name));
    if !has_level_field {
        let format_note = stats
            .detected_format
            .as_deref()
            .map(|format| format!(" (format: {format})"))
            .unwrap_or_default();
        return Some(format!(
            "0 events matched. -l/--levels is set, but no level field was found in the input{format_note} — it looks unstructured. Parse levels first (e.g. -f cols/regex), or match text with --filter 'e.line.contains(\"ERROR\")'."
        ));
    }

    // A level field exists but nothing matched. If a requested level is among
    // the observed values, the empty result is a genuine "none of those this
    // time" and needs no hint. Only warn when the requested vocabulary is
    // entirely absent from what was seen. (Both sides compare case-insensitively,
    // exactly as the filter itself does in LevelFilterStage.)
    if stats.discovered_levels.is_empty() {
        return None;
    }
    let requested_present = config.processing.levels.iter().any(|requested| {
        stats
            .discovered_levels
            .iter()
            .any(|seen| seen.eq_ignore_ascii_case(requested))
    });
    if requested_present {
        return None;
    }

    let levels_present: Vec<&str> = stats.discovered_levels.iter().map(String::as_str).collect();
    let example = levels_present.first().copied().unwrap_or("");
    Some(format!(
        "0 events matched. -l/--levels {} matched none of the levels present: {}. If those are the same level under a different name (e.g. 'E' vs 'ERROR'), match the value directly, e.g. --filter 'e.level == \"{}\"'.",
        config.processing.levels.join(","),
        levels_present.join(","),
        example
    ))
}

/// Hint when `--since/--until` dropped every event because no timestamp could
/// be parsed anywhere in the input. If at least one timestamp parsed, the empty
/// result is a legitimate out-of-range miss and gets no structural hint.
fn timestamp_filter_zero_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
) -> Option<String> {
    config.processing.timestamp_filter.as_ref()?;
    if stats.timestamp_parsed_events > 0 {
        return None;
    }
    Some(format!(
        "0 events matched. --since/--until is set, but no timestamps were parsed ({}/{} events). Set --ts-field/--ts-format; see --help-time.",
        stats.timestamp_parsed_events, stats.events_created
    ))
}

/// Hint when a `--filter` expression references a field name that never
/// appeared in any event — the original zero-results behavior.
fn filter_field_zero_hint(config: &KeloraConfig, stats: &stats::ProcessingStats) -> Option<String> {
    let referenced_fields = collect_filter_field_references(config);
    if referenced_fields.is_empty() {
        return None;
    }

    let unseen_fields: Vec<String> = referenced_fields
        .into_iter()
        .filter(|field| !stats.discovered_keys.contains(field))
        .collect();
    if unseen_fields.is_empty() {
        return None;
    }

    Some(format!(
        "0 events matched. Filter referenced unseen field{}: {}. This may be a typo; rerun with -s to inspect discovered keys.",
        if unseen_fields.len() == 1 { "" } else { "s" },
        unseen_fields.join(", ")
    ))
}

/// Hint when a `--filter` compares a *seen* field for equality against a
/// quoted, numeric-looking literal (`e.status == "404"`). In Rhai a number never
/// equals a string, so if the field holds numbers the quotes make the test
/// always false and the result is silently empty — the single most common
/// beginner mistake on typed (JSON/CSV-typed) data. We only reach here when
/// every event was dropped and the field *was* seen (so it isn't a typo), and we
/// phrase the fix conditionally because a genuine string field compared to a
/// numeric-looking value is legitimately empty too.
fn filter_numeric_string_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
) -> Option<String> {
    // `e.field == "123"` / `e.field == "12.5"` — a numeric-looking double-quoted
    // literal (Rhai strings are double-quoted). Captures the field and literal.
    let re = regex::Regex::new(r#"\be\.([A-Za-z_][A-Za-z0-9_]*)\s*==\s*"(-?\d[\d_]*(?:\.\d+)?)""#)
        .expect("valid numeric-string filter regex");

    for stage in &config.processing.stages {
        let ScriptStageType::Filter { script, .. } = stage else {
            continue;
        };
        for captures in re.captures_iter(script) {
            let field = captures.get(1)?.as_str();
            // Only when the field really exists; an unseen field is already
            // covered (more precisely) by filter_field_zero_hint.
            if !stats.discovered_keys.contains(field) {
                continue;
            }
            let literal = captures.get(2)?.as_str();
            return Some(format!(
                "0 events matched. Filter compares e.{field} to the string \"{literal}\". If e.{field} holds numbers, the quotes force a string-vs-number comparison that is always false — drop them: e.{field} == {literal}. Rerun with -s to check the field's type."
            ));
        }
    }
    None
}

/// Generic fallback when every event was dropped and none of the more specific
/// hints applied. Without it, `-l/--levels` zero-results get an explanatory note
/// (vocabulary mismatch, missing level field) while an ordinary `--filter` that
/// excludes everything — `--filter 'e.level == "NOPE"'` — exits in silence. This
/// restores parity: any active filtering criterion that zeroes the stream gets
/// at least an acknowledgement pointing at `-s`. Only fires when a filtering
/// criterion is actually present, so a plain conversion run never trips it.
fn generic_filter_zero_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
) -> Option<String> {
    let proc = &config.processing;
    let has_filter_criterion = proc
        .stages
        .iter()
        .any(|stage| matches!(stage, ScriptStageType::Filter { .. }))
        || !proc.levels.is_empty()
        || !proc.exclude_levels.is_empty()
        || proc.timestamp_filter.is_some();
    if !has_filter_criterion {
        return None;
    }

    Some(format!(
        "0 of {} events matched. Every event was excluded by the active criteria (--filter/-l/-L/--since/--until). Rerun with -s to inspect the data, or relax the criteria.",
        stats.events_created
    ))
}

/// Hint when `-k/--keys` or `--exclude-keys` names a field that never appeared
/// in any event across the whole stream. A field present in only some rows is
/// legitimate in heterogeneous logs; "never present anywhere" is the typo
/// signal. Unlike `maybe_print_zero_results_hint`, this fires regardless of how
/// many events were output: an include typo empties every event (loud, empty
/// output), but an exclude typo silently fails to drop the field (quiet) — the
/// redaction case we most want to surface.
fn maybe_print_key_typo_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    // We can only tell a typo from a legitimately-absent field once we know what
    // fields the input actually carries; an empty run is "no data", not a typo.
    if stats.events_created == 0 {
        return;
    }

    // A field "exists" if it was seen in the input (discovered_keys) OR produced
    // by a script stage (discovered_keys_output) — e.g. `--exec 'e.total = ...'`
    // followed by `-k total`. Checking only the input set falsely flags every
    // exec-created field as "never present", which both misleads users and
    // teaches them to ignore the genuinely useful typo hint.
    let known_keys: BTreeSet<String> = stats
        .discovered_keys
        .iter()
        .chain(stats.discovered_keys_output.iter())
        .cloned()
        .collect();

    let messages = [
        key_typo_message("-k/--keys", "field", "", &config.output.keys, &known_keys),
        key_typo_message(
            "--exclude-keys",
            "field",
            ", so it was not removed",
            &config.output.exclude_keys,
            &known_keys,
        ),
    ];

    for message in messages.into_iter().flatten() {
        let formatted = config
            .format_hint_message(&message)
            .trim_start_matches('\n')
            .to_string();
        stderr.writeln(&formatted).unwrap_or(());
    }
}

/// Build the typo hint for one key flag, or `None` when every requested key was
/// seen at least once. `consequence` is appended after the field name to explain
/// the effect (empty for `-k`, where empty output already speaks for itself).
fn key_typo_message(
    flag: &str,
    label: &str,
    consequence: &str,
    requested: &[String],
    discovered: &BTreeSet<String>,
) -> Option<String> {
    if requested.is_empty() {
        return None;
    }

    let unseen: Vec<&String> = requested
        .iter()
        .filter(|key| !discovered.contains(*key))
        .collect();

    match unseen.as_slice() {
        [] => None,
        [key] => Some(format!(
            "{flag} names {label} '{key}', which was never present in the input{consequence}. {}",
            unseen_key_suggestion(key, discovered)
        )),
        keys => {
            let names: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();
            Some(format!(
                "{flag} names {label}s never present in the input{consequence}: {}. {}",
                names.join(", "),
                present_fields_hint(discovered)
            ))
        }
    }
}

/// Inline "did you mean" for a single unseen key. Prefers the nearest discovered
/// field; otherwise surfaces the field names the run actually saw (the hint
/// already holds them), falling back to `--discover` only when that list is too
/// long to inline. Renamed fields (e.g. `timestamp` vs `ts`) are too lexically
/// distant for the nearest-field heuristic, so the present-fields list is what
/// surfaces the real name in that case.
fn unseen_key_suggestion(key: &str, discovered: &BTreeSet<String>) -> String {
    if let Some(nested) = nested_path_suggestion(key, discovered) {
        return nested;
    }
    if let Some(candidate) = nearest_field(key, discovered) {
        return format!("Did you mean '{candidate}'?");
    }
    present_fields_hint(discovered)
}

/// When an unseen key looks like a flattened nested path — it contains a `.` or
/// ends with `[]`, the way `--discover` prints nested fields (`api.queries`,
/// `tags[]`) — and its leading segment *is* a present top-level field, the user
/// almost certainly copied a nested name from `--discover`. `-k`/`--exclude-keys`
/// select or drop whole top-level fields and can't address nested values, so we
/// point at the `get_path` idiom instead of `nearest_field` guessing the bare
/// parent (which silently drops the nesting they asked for). This never blocks a
/// top-level field whose name literally contains a dot: such a field would be
/// present, so this "never present" hint wouldn't fire for it at all.
fn nested_path_suggestion(key: &str, discovered: &BTreeSet<String>) -> Option<String> {
    // Head of the path: text before the first `.` or `[`.
    let head_end = key.find(['.', '['])?;
    let head = &key[..head_end];
    if head.is_empty() || !discovered.contains(head) {
        return None;
    }

    // `field[]` is discover's notation for "elements of the array `field`", not
    // an addressable path. The array itself is a selectable top-level field, so
    // the fix there is simply `-k field`. Deeper paths (`a.b`, `a.b[]`) name a
    // value nested inside a map, reachable only via get_path on the container.
    let container = key.strip_suffix("[]").unwrap_or(key);
    if container == head {
        Some(format!(
            "Did you mean the top-level field '{head}'? -k/--keys selects whole fields, so -k {head} keeps its entire value (array and all)."
        ))
    } else {
        Some(format!(
            "'{head}' is present, but -k/--keys and --exclude-keys act on whole top-level fields and can't reach nested values. Flatten it first, e.g. --exec 'e.val = e.get_path(\"{container}\")' then -k val."
        ))
    }
}

/// List the fields the run actually saw when the set is small enough to read at
/// a glance; otherwise point at `--discover`, which is purpose-built for naming
/// fields (`-s` buries them in general stats).
fn present_fields_hint(discovered: &BTreeSet<String>) -> String {
    const MAX_INLINE: usize = 12;
    if !discovered.is_empty() && discovered.len() <= MAX_INLINE {
        let names: Vec<&str> = discovered.iter().map(String::as_str).collect();
        format!("Present fields: {}.", names.join(", "))
    } else {
        "Run --discover to list fields.".to_string()
    }
}

/// Closest discovered field to `key`, using the same similarity heuristic the
/// Rhai diagnostics use: >0.6 normalized similarity, substring containment, or a
/// shared 2-char prefix. Returns `None` when nothing is close enough so the
/// caller can fall back to listing present fields.
fn nearest_field(key: &str, discovered: &BTreeSet<String>) -> Option<String> {
    let key_lower = key.to_lowercase();
    let mut best: Option<(f64, &String)> = None;

    for field in discovered {
        let field_lower = field.to_lowercase();
        let similarity = normalized_similarity(&key_lower, &field_lower);
        let close = similarity > 0.6
            || field_lower.contains(&key_lower)
            || key_lower.contains(&field_lower)
            || shared_prefix(&key_lower, &field_lower);
        if close && best.is_none_or(|(best_sim, _)| similarity > best_sim) {
            best = Some((similarity, field));
        }
    }

    best.map(|(_, field)| field.clone())
}

/// Levenshtein similarity normalized to 0.0..=1.0 (1.0 == identical).
fn normalized_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        return 0.0;
    }
    1.0 - (levenshtein(a, b) as f64 / max_len as f64)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &ac) in a.iter().enumerate() {
        let mut curr = vec![i + 1];
        for (j, &bc) in b.iter().enumerate() {
            let cost = usize::from(ac != bc);
            curr.push((curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost));
        }
        prev = curr;
    }
    prev[b.len()]
}

/// Whether two strings share their first two bytes (a cheap "looks related"
/// check for short renamings the similarity score would otherwise miss).
fn shared_prefix(a: &str, b: &str) -> bool {
    a.len() >= 2 && b.len() >= 2 && a.as_bytes()[..2] == b.as_bytes()[..2]
}

/// Build the discover footer's format summary from the requested input format
/// and the run's processing stats. Returns `None` when no format is known
/// (e.g. stats disabled, or an empty/auto run that detected nothing).
fn build_discover_format_summary(
    format: &config::InputFormat,
    stats: Option<&stats::ProcessingStats>,
) -> Option<field_discovery::FormatSummary> {
    use field_discovery::FormatSummary;
    let stats = stats?;

    match format {
        config::InputFormat::Auto => {
            let name = stats.detected_format.clone()?;
            // Guard against the unresolved sentinel if detection never ran.
            if name == "auto" {
                return None;
            }
            Some(FormatSummary {
                format: name,
                detection: "auto",
                counts: Vec::new(),
                unit: "",
            })
        }
        config::InputFormat::AutoPerFile => {
            let counts: Vec<(String, usize)> = stats
                .detected_format_counts
                .iter()
                .map(|(name, count)| (name.clone(), *count))
                .collect();
            if counts.is_empty() {
                return None;
            }
            let format = if counts.len() == 1 {
                counts[0].0.clone()
            } else {
                "mixed".to_string()
            };
            Some(FormatSummary {
                format,
                detection: "per-file",
                counts,
                unit: "files",
            })
        }
        f if f.is_cascade() => {
            let counts: Vec<(String, usize)> = stats
                .cascade_format_counts
                .iter()
                .map(|(name, count)| (name.clone(), *count))
                .collect();
            if counts.is_empty() {
                return None;
            }
            let format = if counts.len() == 1 {
                counts[0].0.clone()
            } else {
                "mixed".to_string()
            };
            Some(FormatSummary {
                format,
                detection: "cascade",
                counts,
                unit: "events",
            })
        }
        other => Some(FormatSummary {
            format: other.to_display_string(),
            detection: "explicit",
            counts: Vec::new(),
            unit: "",
        }),
    }
}

/// Build the timestamp summary for the discover footer/row marker from
/// processing stats. Chooses the primary timestamp field the same way the
/// pipeline does: an explicit `--ts-field` wins; otherwise the
/// highest-priority candidate (per `TIMESTAMP_FIELD_NAMES`) that was actually
/// seen. Returns `None` when no timestamp field was identified, so the footer
/// stays quiet for timestamp-less inputs (the `--since`/`--until` hint covers
/// the failure case if a user tries to time-filter anyway).
fn build_discover_timestamp_summary(
    stats: Option<&stats::ProcessingStats>,
) -> Option<field_discovery::TimestampSummary> {
    use field_discovery::TimestampSummary;
    let stats = stats?;

    let (field, overridden) = if let Some(field) = &stats.timestamp_override_field {
        (field.clone(), true)
    } else {
        // Match identify_timestamp_field: first candidate (by priority) seen.
        let chosen = crate::event::TIMESTAMP_FIELD_NAMES
            .iter()
            .find(|name| stats.timestamp_fields.contains_key(**name))
            .map(|name| name.to_string())
            // Fall back to whichever field was recorded first (e.g. a named
            // format's `ts` that isn't in the generic candidate list).
            .or_else(|| stats.timestamp_fields.keys().next().cloned())?;
        (chosen, false)
    };

    let (detected, parsed) = stats
        .timestamp_fields
        .get(&field)
        .map(|s| (s.detected, s.parsed))
        .unwrap_or((
            stats.timestamp_detected_events,
            stats.timestamp_parsed_events,
        ));

    Some(TimestampSummary {
        field,
        overridden,
        detected,
        parsed,
    })
}

/// Handle successful pipeline execution - process metrics, stats, and warnings
fn handle_pipeline_success(
    config: &KeloraConfig,
    mut pipeline_result: PipelineResult,
    stdout: &mut SafeStdout,
    stderr: &mut SafeStderr,
    diagnostics_allowed_runtime: bool,
    terminal_allowed: bool,
) -> (Option<stats::ProcessingStats>, Option<TrackingSnapshot>) {
    let auto_detected_non_line = pipeline_result.auto_detected_non_line;
    // Determine if any events were output (to conditionally suppress leading newlines)
    let events_were_output = pipeline_result
        .stats
        .as_ref()
        .is_some_and(|s| !config.processing.quiet_events && s.events_output > 0);

    // Print metrics if enabled (only if not terminated)
    if let Some(ref metrics_format) = config.output.metrics {
        if terminal_allowed && !SHOULD_TERMINATE.load(Ordering::Relaxed) {
            use crate::cli::MetricsFormat;
            // Route to stdout in data-only mode, stderr when showing with events
            let use_stdout = !config.output.metrics_with_events;
            // Resolve the auto default like `ls`: the human table on a terminal,
            // the tsv record stream when stdout is piped or redirected. An
            // explicit --metrics=full forces the table through a pipe.
            let resolved_format = match metrics_format {
                MetricsFormat::Auto => {
                    if use_stdout && !std::io::stdout().is_terminal() {
                        MetricsFormat::Tsv
                    } else {
                        MetricsFormat::Full
                    }
                }
                other => other.clone(),
            };
            match &resolved_format {
                MetricsFormat::Auto => unreachable!("Auto is resolved above"),
                MetricsFormat::Tsv => {
                    let tsv_output = crate::rhai_functions::tracking::format_metrics_tsv(
                        &pipeline_result.tracking_data.user,
                        &pipeline_result.tracking_data.internal,
                    );
                    // A record stream is written verbatim: no header, no leading
                    // newline, so downstream head/tail/awk see only data rows.
                    if !tsv_output.is_empty() {
                        if use_stdout {
                            stdout.writeln(&tsv_output).unwrap_or(());
                        } else {
                            stderr.writeln(&tsv_output).unwrap_or(());
                        }
                    }
                }
                MetricsFormat::Short | MetricsFormat::Full => {
                    let metrics_level = match &resolved_format {
                        MetricsFormat::Short => 1,
                        MetricsFormat::Full => 2,
                        _ => 1,
                    };
                    let metrics_output = crate::rhai_functions::tracking::format_metrics_output(
                        &pipeline_result.tracking_data.user,
                        &pipeline_result.tracking_data.internal,
                        metrics_level,
                    );
                    if !metrics_output.is_empty() {
                        let mut formatted = config.format_metrics_message(
                            &metrics_output,
                            config.output.metrics_with_events, // Show header only for --with-metrics
                        );
                        if !events_were_output {
                            formatted = formatted.trim_start_matches('\n').to_string();
                        }
                        if use_stdout {
                            stdout.writeln(&formatted).unwrap_or(());
                        } else {
                            stderr.writeln(&formatted).unwrap_or(());
                        }
                    }
                }
                MetricsFormat::Json => {
                    if let Ok(json_output) = crate::rhai_functions::tracking::format_metrics_json(
                        &pipeline_result.tracking_data.user,
                        &pipeline_result.tracking_data.internal,
                    ) {
                        if use_stdout {
                            stdout.writeln(&json_output).unwrap_or(());
                        } else {
                            stderr.writeln(&json_output).unwrap_or(());
                        }
                    }
                }
            }
        }
    }

    if let Some(drain_format) = config.output.drain.clone() {
        if terminal_allowed && !SHOULD_TERMINATE.load(Ordering::Relaxed) {
            let templates = crate::drain::drain_templates();
            let output = match drain_format {
                crate::cli::DrainFormat::Table
                | crate::cli::DrainFormat::Full
                | crate::cli::DrainFormat::Id => {
                    crate::drain::format_templates_output(&templates, drain_format)
                }
                crate::cli::DrainFormat::Json => crate::drain::format_templates_json(&templates),
            };
            if !output.is_empty() && output != "No templates found" {
                stdout.writeln(&output).unwrap_or(());
            }
        }
    }

    // Write metrics to file if configured
    if let Some(ref metrics_file) = config.output.metrics_file {
        if let Ok(json_output) = crate::rhai_functions::tracking::format_metrics_json(
            &pipeline_result.tracking_data.user,
            &pipeline_result.tracking_data.internal,
        ) {
            if let Err(e) = std::fs::write(metrics_file, json_output) {
                stderr
                    .writeln(
                        &config
                            .format_error_message(&format!("Failed to write metrics file: {}", e)),
                    )
                    .unwrap_or(());
            }
        }
    }

    // Surface per-metric counts of skipped Unit () values (missing fields).
    // The track_* functions skip missing values silently; a metric whose field
    // is missing from *every* event usually means a field-name typo, so it
    // deserves a diagnostic line.
    //
    // This is a stuck-user signal — most acute under the `--freq`/`--describe`
    // sugar (and `--metrics`/`--drain`), where a typo'd field name yields
    // a bare "No metrics tracked" with no clue why. Those data-only modes imply
    // diagnostics suppression to keep stdout clean, which used to hide this hint
    // exactly where it's needed. So gate it like the script-error summary: survive
    // a mode's *implicit* suppression, but still obey an explicit --no-diagnostics
    // and --silent.
    //
    // Gating: a *partial* skip count is normal for varying-shape logs (a field
    // present in some events and absent in others), so a bare `count > 0` fired
    // the hint constantly. Only flag a metric that recorded a value on *no*
    // event — the strong typo signal — and only once the input is large enough
    // that an all-absent field is conclusive rather than a small-sample fluke.
    const SKIP_HINT_MIN_EVENTS: usize = 5;
    let total_events = pipeline_result
        .stats
        .as_ref()
        .map_or(0, |s| s.events_created);
    let skip_hint_allowed = !config.processing.silent
        && !config.processing.diagnostics_user_suppressed
        && total_events >= SKIP_HINT_MIN_EVENTS
        && !SHOULD_TERMINATE.load(Ordering::Relaxed);
    if skip_hint_allowed {
        let user = &pipeline_result.tracking_data.user;
        // A metric that recorded at least one value keys either directly on its
        // name (track_avg/track_freq/…) or on a derived sub-key (track_stats ->
        // `<name>_count`/`<name>_sum`/…, track_percentiles -> `<name>_p50`/…).
        // Treat any such key as "recorded", so a field present in even one event
        // is never flagged.
        let metric_recorded = |name: &str| {
            user.contains_key(name) || {
                let prefix = format!("{}_", name);
                user.keys().any(|k| k.starts_with(&prefix))
            }
        };
        let mut skips: Vec<(String, i64)> = pipeline_result
            .tracking_data
            .internal
            .iter()
            .filter_map(|(key, value)| {
                key.strip_prefix("__kelora_track_skipped_")
                    .map(|name| (name.to_string(), value.as_int().unwrap_or(0)))
            })
            .filter(|(name, count)| *count > 0 && !metric_recorded(name))
            .collect();
        if !skips.is_empty() {
            skips.sort();
            let detail = skips
                .iter()
                .map(|(name, count)| format!("{} ({})", name, count))
                .collect::<Vec<_>>()
                .join(", ");
            let mut hint = config.format_hint_message(&format!(
                "Tracking skipped events with missing values: {}. These metrics never recorded a value — likely a field-name typo.",
                detail
            ));
            if !events_were_output {
                hint = hint.trim_start_matches('\n').to_string();
            }
            stderr.writeln(&hint).unwrap_or(());
        }
    }

    // Hint when metrics were tracked but no metrics output option was requested.
    // An --end stage sees the `metrics` global and is the idiomatic way to
    // consume metrics into a custom report, so treat its presence as the metrics
    // already being handled — nudging "rerun with -m" there is just noise.
    let metrics_were_requested = config.output.metrics.is_some()
        || config.output.metrics_file.is_some()
        || config.processing.end.is_some();
    if !metrics_were_requested
        && !pipeline_result.tracking_data.user.is_empty()
        && diagnostics_allowed_runtime
        && !SHOULD_TERMINATE.load(Ordering::Relaxed)
    {
        let mut hint = config
            .format_hint_message("Metrics recorded; rerun with -m or --metrics=json to view them.");
        if !events_were_output {
            hint = hint.trim_start_matches('\n').to_string();
        }
        stderr.writeln(&hint).unwrap_or(());
    }

    // Print field discovery results if requested
    if !SHOULD_TERMINATE.load(Ordering::Relaxed) {
        let format_summary =
            build_discover_format_summary(&config.input.format, pipeline_result.stats.as_ref());
        let timestamp_summary = build_discover_timestamp_summary(pipeline_result.stats.as_ref());
        if let Some(discovery) = pipeline_result.field_discovery.as_mut() {
            discovery.format_summary = format_summary;
            discovery.timestamp_summary = timestamp_summary;
            // In plain --discover mode, nudge toward --discover-final whenever the
            // pipeline filters or transforms events, since the emitted field set can
            // then differ from the parsed input shown here. A bare probe (no stages,
            // no span, no time/take filters) stays uncluttered.
            let proc = &config.processing;
            discovery.suggest_discover_final = !config.output.discover_final
                && (!proc.stages.is_empty()
                    || proc.span.is_some()
                    || proc.timestamp_filter.is_some()
                    || proc.take_limit.is_some()
                    || !proc.levels.is_empty()
                    || !proc.exclude_levels.is_empty());
            let formatted = match config.output.discover_fields {
                Some(cli::DiscoverFieldsFormat::Json) => discovery.format_json(),
                _ => {
                    let use_unicode = crate::tty::should_use_emoji_with_mode(
                        &config.output.emoji,
                        &config.output.color,
                    );
                    discovery.format_table(use_unicode)
                }
            };
            stdout.writeln(&formatted).unwrap_or(());
        }
    }

    // Print output based on configuration (only if not terminated)
    if !SHOULD_TERMINATE.load(Ordering::Relaxed) {
        // Script/parse error summaries are correctness signals, not informational
        // diagnostics. They go to stderr (never polluting machine-readable stdout),
        // so they survive the data-only modes (--metrics/--drain/--discover) that
        // imply suppress_diagnostics. Only --silent (terminal_allowed == false)
        // hides them.
        let errors_allowed = terminal_allowed;
        let tracking_summary = if errors_allowed {
            crate::rhai_functions::tracking::extract_error_summary_from_tracking(
                &pipeline_result.tracking_data,
                config.processing.verbose,
                pipeline_result.stats.as_ref(),
                Some(config),
            )
        } else {
            None
        };

        if let Some(ref s) = pipeline_result.stats {
            if config.output.stats.is_some() && terminal_allowed {
                // Full stats when --stats flag is used (unless suppressed)
                // Route to stdout in data-only mode, stderr when showing with events
                let use_stdout = !config.output.stats_with_events;
                let json_stats = matches!(config.output.stats, Some(cli::StatsFormat::Json));
                let mut formatted = if json_stats {
                    s.format_stats_json()
                } else {
                    config.format_stats_message(
                        &s.format_stats(config.input.multiline.is_some()),
                        config.output.stats_with_events, // Show header only for --with-stats
                    )
                };
                if !events_were_output {
                    formatted = formatted.trim_start_matches('\n').to_string();
                }
                if use_stdout {
                    stdout.writeln(&formatted).unwrap_or(());
                } else {
                    stderr.writeln(&formatted).unwrap_or(());
                }
            } else if errors_allowed {
                // Error summary by default when errors occur (survives data-only modes;
                // only --silent suppresses it)
                let mut summaries = Vec::new();

                if let Some(tracking_summary) = tracking_summary.clone() {
                    summaries.push(tracking_summary);
                }

                let stats_summary = s.format_error_summary();
                let stats_summary_empty = stats_summary.is_empty();
                if !stats_summary_empty {
                    summaries.push(stats_summary);
                }

                if !summaries.is_empty() {
                    // The tracking summary can be multi-line (a "Parse errors: N
                    // total" header followed by indented per-line samples). Joining
                    // that with "; " glues the run recap onto the last sample line;
                    // fall back to a newline whenever any part spans multiple lines.
                    let separator = if summaries.iter().any(|s| s.contains('\n')) {
                        "\n"
                    } else {
                        "; "
                    };
                    let combined = summaries.join(separator);
                    let only_recovered_runtime_errors = tracking_summary.is_some()
                        && stats_summary_empty
                        && !config.processing.strict;
                    let mut formatted = if only_recovered_runtime_errors {
                        config.format_warning_message(&combined)
                    } else {
                        config.format_error_message(&combined)
                    };
                    if !events_were_output {
                        formatted = formatted.trim_start_matches('\n').to_string();
                    }
                    stderr.writeln(&formatted).unwrap_or(());
                }
            }

            // Lossy UTF-8 recoveries are surfaced even when no error summary
            // fires: it's a warning, not a failure (exit code stays 0), but the
            // user should see that invalid bytes were replaced rather than the
            // stream silently truncated (#239). With --stats it's already in the
            // stats block.
            if errors_allowed && config.output.stats.is_none() {
                if let Some(message) = s.format_decode_warning() {
                    let formatted = config.format_warning_message(&message);
                    stderr.writeln(&formatted).unwrap_or(());
                }
            }

            if diagnostics_allowed_runtime && terminal_allowed {
                // Fires before the zero-results hint, which returns early when
                // nothing was created — the empty-input case it can't explain.
                maybe_print_no_input_hint(config, s, stderr);
                maybe_print_zero_results_hint(config, s, stderr);
                // Surfaces the silent UTC assumption for naive timestamps when a
                // time filter, span, or --normalize-ts relies on it (#287).
                maybe_print_naive_tz_hint(config, s, stderr);
                // Fires independently of the zero-results hint: an exclude-key
                // typo leaves output intact but silently fails to drop the field.
                maybe_print_key_typo_hint(config, s, stderr);
                // With --stats the ragged-row count is already in the stats block.
                if config.output.stats.is_none() {
                    maybe_print_csv_shape_hint(config, s, stderr);
                }
            }
        } else if errors_allowed {
            if let Some(tracking_summary) = tracking_summary {
                let formatted = config.format_error_message(&tracking_summary);
                stderr
                    .writeln(formatted.trim_start_matches('\n'))
                    .unwrap_or(());
            }
        }
    }

    detection::emit_parse_failure_warning(
        config,
        pipeline_result.stats.as_ref(),
        auto_detected_non_line,
        events_were_output,
        std::io::stderr().is_terminal(),
    );
    (pipeline_result.stats, Some(pipeline_result.tracking_data))
}

/// Handle signal termination - print stats and exit with appropriate code
fn handle_signal_termination(
    config: &KeloraConfig,
    final_stats: Option<&stats::ProcessingStats>,
    events_were_output: bool,
    stderr: &mut SafeStderr,
    terminal_allowed: bool,
) -> ! {
    if let Some(stats) = final_stats {
        if config.output.stats.is_some() && terminal_allowed {
            // Full stats when --stats flag is used (unless suppressed)
            let mut formatted = config.format_stats_message(
                &stats.format_stats(config.input.multiline.is_some()),
                config.output.stats_with_events, // Show header only for --with-stats
            );
            if !events_were_output {
                formatted = formatted.trim_start_matches('\n').to_string();
            }
            stderr.writeln(&formatted).unwrap_or(());
        } else if stats.has_errors()
            && !config.processing.silent
            && !config.processing.suppress_diagnostics
        {
            // Error summary by default when errors occur (unless suppressed)
            let mut formatted = config.format_error_message(&stats.format_error_summary());
            if !events_were_output {
                formatted = formatted.trim_start_matches('\n').to_string();
            }
            stderr.writeln(&formatted).unwrap_or(());
        }
    } else if config.output.stats.is_some() && terminal_allowed {
        let mut formatted = config.format_stats_message(
            "Processing interrupted",
            config.output.stats_with_events, // Show header only for --with-stats
        );
        if !events_were_output {
            formatted = formatted.trim_start_matches('\n').to_string();
        }
        stderr.writeln(&formatted).unwrap_or(());
    }

    // Exit with the correct code based on which signal was received
    #[cfg(unix)]
    {
        let signal = TERMINATION_SIGNAL.load(Ordering::Relaxed);
        match signal {
            sig if sig == SIGTERM => ExitCode::SignalTerm.exit(),
            sig if sig == SIGINT => ExitCode::SignalInt.exit(),
            _ => ExitCode::SignalInt.exit(), // fallback for unknown signal
        }
    }
    #[cfg(not(unix))]
    {
        // Windows only supports SIGINT
        ExitCode::SignalInt.exit();
    }
}

fn emit_fatal_line(stderr: &mut SafeStderr, config: &KeloraConfig, message: &str) {
    stderr
        .writeln(&config.format_error_message(message))
        .unwrap_or(());
}
