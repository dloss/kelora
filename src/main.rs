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

    if config.processing.span.is_some()
        && diagnostics_allowed
        && (config.performance.parallel
            || config.performance.threads > 0
            || config.performance.batch_size.is_some())
    {
        let warning = config.format_error_message(
            "span aggregation requires sequential mode; ignoring --parallel settings. Rerun without --parallel if you need span aggregation.",
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
            emit_fatal_line(&mut stderr, &config, &format!("Pipeline error: {}", e));
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

    // Determine exit code based on unrecovered processing failures. In default
    // resilient mode, filter/exec runtime errors are diagnostics, not failures.
    let mut had_errors = {
        let tracking_errors = tracking_data
            .as_ref()
            .map(|tracking| {
                crate::rhai_functions::tracking::has_errors_in_tracking_with_policy(
                    tracking,
                    config.processing.strict,
                )
            })
            .unwrap_or(false);
        let stats_errors = final_stats
            .as_ref()
            .map(|s| s.has_errors())
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

    if had_errors && !diagnostics_allowed_runtime {
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

fn maybe_print_zero_results_hint(
    config: &KeloraConfig,
    stats: &stats::ProcessingStats,
    stderr: &mut SafeStderr,
) {
    if stats.events_created == 0 || stats.events_output > 0 || stats.has_errors() {
        return;
    }

    let referenced_fields = collect_filter_field_references(config);
    if referenced_fields.is_empty() {
        return;
    }

    let unseen_fields: Vec<String> = referenced_fields
        .into_iter()
        .filter(|field| !stats.discovered_keys.contains(field))
        .collect();

    if unseen_fields.is_empty() {
        return;
    }

    let mut hint = config.format_hint_message(&format!(
        "0 events matched. Filter referenced unseen field{}: {}. This may be a typo; rerun with -s to inspect discovered keys.",
        if unseen_fields.len() == 1 { "" } else { "s" },
        unseen_fields.join(", ")
    ));
    hint = hint.trim_start_matches('\n').to_string();
    stderr.writeln(&hint).unwrap_or(());
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
            match metrics_format {
                MetricsFormat::Short | MetricsFormat::Full => {
                    let metrics_level = match metrics_format {
                        MetricsFormat::Short => 1,
                        MetricsFormat::Full => 2,
                        _ => 1,
                    };
                    let metrics_output = crate::rhai_functions::tracking::format_metrics_output(
                        &pipeline_result.tracking_data.user,
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

    // Hint when metrics were tracked but no metrics output option was requested
    let metrics_were_requested =
        config.output.metrics.is_some() || config.output.metrics_file.is_some();
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
        if let Some(discovery) = pipeline_result.field_discovery.as_mut() {
            discovery.format_summary = format_summary;
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
                let mut formatted = config.format_stats_message(
                    &s.format_stats(config.input.multiline.is_some()),
                    config.output.stats_with_events, // Show header only for --with-stats
                );
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
                    let combined = summaries.join("; ");
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

            if diagnostics_allowed_runtime && terminal_allowed {
                maybe_print_zero_results_hint(config, s, stderr);
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
