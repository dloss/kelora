use anyhow::Result;
use crossbeam_channel::unbounded;
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
mod timestamp;
mod tty;

// Re-export types at crate root for use by submodules
pub use cli::{FileOrder, InputFormat, OutputFormat};

use crate::rhai_functions::tracking::TrackingSnapshot;
use args::{process_args_with_config, validate_cli_args};
use cli::Cli;
use config::{
    KeloraConfig, MultilineConfig, SectionEnd, SectionStart, SpanMode, TimestampFilterConfig,
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
    let (matches, cli) = process_args_with_config(&mut stderr);

    // Validate CLI argument combinations
    if let Err(e) = validate_cli_args(&cli) {
        stderr
            .writeln(&format!("kelora: Error: {}", e))
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    // Extract ordered script stages
    let ordered_stages = match cli.get_ordered_script_stages(&matches) {
        Ok(stages) => stages,
        Err(e) => {
            stderr
                .writeln(&format!("kelora: Error: {}", e))
                .unwrap_or(());
            ExitCode::InvalidUsage.exit();
        }
    };

    // Create configuration from CLI and set stages (using lib config directly)
    let mut config = match KeloraConfig::from_cli(&cli) {
        Ok(cfg) => cfg,
        Err(e) => {
            stderr.writeln(&format!("kelora: {:#}", e)).unwrap_or(());
            std::process::exit(ExitCode::InvalidUsage as i32);
        }
    };
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
            "span aggregation requires sequential mode; ignoring --parallel settings",
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

        // Check for anchor dependencies
        let since_uses_until_anchor = cli
            .since
            .as_ref()
            .is_some_and(|s| s.starts_with("until+") || s.starts_with("until-"));
        let until_uses_since_anchor = cli
            .until
            .as_ref()
            .is_some_and(|u| u.starts_with("since+") || u.starts_with("since-"));

        // Detect circular dependency
        if since_uses_until_anchor && until_uses_since_anchor {
            stderr
                .writeln(&config.format_error_message(
                    "Cannot use both 'since' and 'until' anchors: --since uses 'until' anchor and --until uses 'since' anchor"
                ))
                .unwrap_or(());
            ExitCode::InvalidUsage.exit();
        }

        let (since, until) = if until_uses_since_anchor {
            // Parse --since first, then use it as anchor for --until
            let since = if let Some(ref since_str) = cli.since {
                match crate::timestamp::parse_timestamp_arg_with_timezone(since_str, cli_timezone) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --since timestamp '{}': {}",
                                since_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            let until = if let Some(ref until_str) = cli.until {
                match crate::timestamp::parse_anchored_timestamp(
                    until_str,
                    since,
                    None,
                    cli_timezone,
                ) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --until timestamp '{}': {}",
                                until_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            (since, until)
        } else if since_uses_until_anchor {
            // Parse --until first, then use it as anchor for --since
            let until = if let Some(ref until_str) = cli.until {
                match crate::timestamp::parse_timestamp_arg_with_timezone(until_str, cli_timezone) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --until timestamp '{}': {}",
                                until_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            let since = if let Some(ref since_str) = cli.since {
                match crate::timestamp::parse_anchored_timestamp(
                    since_str,
                    None,
                    until,
                    cli_timezone,
                ) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --since timestamp '{}': {}",
                                since_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            (since, until)
        } else {
            // No anchors, parse independently (current behavior)
            let since = if let Some(ref since_str) = cli.since {
                match crate::timestamp::parse_timestamp_arg_with_timezone(since_str, cli_timezone) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --since timestamp '{}': {}",
                                since_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            let until = if let Some(ref until_str) = cli.until {
                match crate::timestamp::parse_timestamp_arg_with_timezone(until_str, cli_timezone) {
                    Ok(dt) => Some(dt),
                    Err(e) => {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Invalid --until timestamp '{}': {}",
                                until_str, e
                            )))
                            .unwrap_or(());
                        ExitCode::InvalidUsage.exit();
                    }
                }
            } else {
                None
            };

            (since, until)
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

    // Validate arguments early
    if let Err(e) = validate_cli_args(&cli) {
        stderr
            .writeln(&config.format_error_message(&format!("Error: {}", e)))
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
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

    // Determine exit code based on whether any errors occurred during processing
    let mut had_errors = {
        let tracking_errors = tracking_data
            .as_ref()
            .map(crate::rhai_functions::tracking::has_errors_in_tracking)
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

    if had_errors {
        ExitCode::GeneralError.exit();
    } else {
        ExitCode::Success.exit();
    }
}

/// Handle successful pipeline execution - process metrics, stats, and warnings
fn handle_pipeline_success(
    config: &KeloraConfig,
    pipeline_result: PipelineResult,
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
                MetricsFormat::Compact | MetricsFormat::Full => {
                    let metrics_level = match metrics_format {
                        MetricsFormat::Compact => 1,
                        MetricsFormat::Full => 2,
                        _ => 1,
                    };
                    let metrics_output = crate::rhai_functions::tracking::format_metrics_output(
                        &pipeline_result.tracking_data.user,
                        metrics_level,
                    );
                    if !metrics_output.is_empty() && metrics_output != "No metrics tracked" {
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

    if let Some(ref drain_format) = config.output.drain {
        if terminal_allowed && !SHOULD_TERMINATE.load(Ordering::Relaxed) {
            let templates = crate::drain::drain_templates();
            let output = match drain_format {
                crate::cli::DrainFormat::Table => {
                    crate::drain::format_templates_output(&templates, config.processing.verbose)
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

    // Print output based on configuration (only if not terminated)
    if !SHOULD_TERMINATE.load(Ordering::Relaxed) {
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
            } else if diagnostics_allowed_runtime {
                // Error summary by default when errors occur (unless diagnostics suppressed)
                let mut summaries = Vec::new();

                if let Some(tracking_summary) =
                    crate::rhai_functions::tracking::extract_error_summary_from_tracking(
                        &pipeline_result.tracking_data,
                        config.processing.verbose,
                        pipeline_result.stats.as_ref(),
                        Some(config),
                    )
                {
                    summaries.push(tracking_summary);
                }

                let stats_summary = s.format_error_summary();
                if !stats_summary.is_empty() {
                    summaries.push(stats_summary);
                }

                if !summaries.is_empty() {
                    let combined = summaries.join("; ");
                    let mut formatted = config.format_error_message(&combined);
                    if !events_were_output {
                        formatted = formatted.trim_start_matches('\n').to_string();
                    }
                    stderr.writeln(&formatted).unwrap_or(());
                }
            }
        }
    }

    detection::emit_parse_failure_warning(
        config,
        Some(&pipeline_result.tracking_data),
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
