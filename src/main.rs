use anyhow::Result;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender};
use std::fs;
use std::io::IsTerminal;
use std::sync::atomic::Ordering;

#[cfg(unix)]
use signal_hook::consts::{SIGINT, SIGTERM};

use crate::engine::RhaiEngine;
use crate::rhai_functions::tracking::{self, TrackingSnapshot};

mod cli;
mod colors;
mod config;
mod config_file;
mod decompression;
mod engine;
mod event;
mod formatters;
mod interactive;
mod parallel;
mod parsers;
mod pipeline;
mod platform;
mod readers;
mod rhai_functions;

use crate::rhai_functions::file_ops::{self, FileOpMode};
mod stats;
mod timestamp;
mod tty;

use config::KeloraConfig;
use config_file::ConfigFile;
use platform::{
    install_broken_pipe_panic_hook, Ctrl, ExitCode, ProcessCleanup, SafeFileOut, SafeStderr,
    SafeStdout, SignalHandler, SHOULD_TERMINATE, TERMINATED_BY_SIGNAL, TERMINATION_SIGNAL,
};

// Internal CLI imports
use cli::{Cli, FileOrder, InputFormat, OutputFormat};
use config::{MultilineConfig, SectionEnd, SectionStart, SpanMode, TimestampFilterConfig};

#[derive(Debug, Clone)]
struct DetectedFormat {
    format: config::InputFormat,
    had_input: bool,
}

impl DetectedFormat {
    fn detected_non_line(&self) -> bool {
        self.had_input && !matches!(self.format, config::InputFormat::Line)
    }

    fn fell_back_to_line(&self) -> bool {
        self.had_input && matches!(self.format, config::InputFormat::Line)
    }
}

/// Detect format from a peekable reader
/// Returns the detected format without consuming the first line
fn detect_format_from_peekable_reader<R: std::io::BufRead>(
    reader: &mut readers::PeekableLineReader<R>,
) -> Result<DetectedFormat> {
    match reader.peek_first_line()? {
        None => Ok(DetectedFormat {
            format: config::InputFormat::Line,
            had_input: false,
        }),
        Some(line) => {
            // Remove newline for detection
            let trimmed_line = line.trim_end_matches(&['\r', '\n'][..]);
            let detected = parsers::detect_format(trimmed_line)?;
            Ok(DetectedFormat {
                format: detected,
                had_input: true,
            })
        }
    }
}

/// Detect format for parallel mode processing
/// Returns the detected format
fn detect_format_for_parallel_mode(
    files: &[String],
    no_input: bool,
    strict: bool,
) -> Result<(DetectedFormat, Option<Box<dyn BufRead + Send>>)> {
    use std::io;

    if no_input {
        // For --no-input mode, default to Line format
        return Ok((
            DetectedFormat {
                format: config::InputFormat::Line,
                had_input: false,
            },
            None,
        ));
    }

    if files.is_empty() {
        // For stdin with potential gzip/zstd, handle decompression first
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        let mut peekable_reader =
            readers::PeekableLineReader::new(io::BufReader::new(processed_stdin));

        let detected = detect_format_from_peekable_reader(&mut peekable_reader)?;

        // Reuse the peekable reader so we don't consume stdin twice
        Ok((detected, Some(Box::new(peekable_reader))))
    } else {
        // For files, read first line from first file
        let sorted_files = pipeline::builders::sort_files(files, &config::FileOrder::Cli)?;

        let mut failed_opens: Vec<(String, String)> = Vec::new();
        let mut failed_dirs: Vec<String> = Vec::new();
        let mut detected: Option<DetectedFormat> = None;

        for file_path in &sorted_files {
            if let Ok(metadata) = fs::metadata(file_path) {
                if metadata.is_dir() {
                    if strict {
                        return Err(anyhow::anyhow!(
                            "Input path '{}' is a directory; only files are supported",
                            file_path
                        ));
                    }
                    failed_dirs.push(file_path.clone());
                    continue;
                }
            }

            match decompression::DecompressionReader::new(file_path) {
                Ok(decompressed) => {
                    let mut peekable_reader = readers::PeekableLineReader::new(decompressed);
                    detected = Some(detect_format_from_peekable_reader(&mut peekable_reader)?);
                    break;
                }
                Err(e) => {
                    if strict {
                        return Err(anyhow::anyhow!(
                            "Failed to open file '{}': {}",
                            file_path,
                            e
                        ));
                    }
                    failed_opens.push((file_path.clone(), e.to_string()));
                }
            }
        }

        let detected = match detected {
            Some(detected) => detected,
            None => {
                for path in failed_dirs {
                    eprintln!(
                        "{}",
                        crate::config::format_error_message_auto(&format!(
                            "Input path '{}' is a directory; skipping (input files only)",
                            path
                        ))
                    );
                    stats::stats_file_open_failed(&path);
                }
                for (path, err) in failed_opens {
                    eprintln!(
                        "{}",
                        crate::config::format_error_message_auto(&format!(
                            "Failed to open file '{}': {}",
                            path, err
                        ))
                    );
                    stats::stats_file_open_failed(&path);
                }
                return Err(anyhow::anyhow!(
                    "Failed to open any input files for detection"
                ));
            }
        };

        // For files we can reopen them later, so we don't need to keep this reader
        Ok((detected, None))
    }
}

fn detection_notices_allowed(config: &KeloraConfig, terminal_output: bool) -> bool {
    if config.processing.silent
        || config.processing.suppress_diagnostics
        || config.processing.quiet_events
        || std::env::var("KELORA_NO_TIPS").is_ok()
    {
        return false;
    }

    terminal_output
}

fn format_detected_format_notice(
    config: &KeloraConfig,
    detected: &DetectedFormat,
    terminal_output: bool,
) -> Option<String> {
    if !detection_notices_allowed(config, terminal_output) {
        return None;
    }

    if detected.detected_non_line() {
        let format_name = detected.format.to_display_string();
        let message = config.format_info_message(&format!("Auto-detected format: {}", format_name));
        Some(message)
    } else if detected.fell_back_to_line() {
        let message = config
            .format_hint_message("No input format detected; using line. Override with -f <fmt>.");
        Some(message)
    } else {
        None
    }
}

fn emit_detected_format_notice(
    config: &KeloraConfig,
    detected: &DetectedFormat,
    terminal_output: bool,
) {
    if let Some(message) = format_detected_format_notice(config, detected, terminal_output) {
        eprintln!("{}", message);
    }
}

fn extract_counter_from_tracking(tracking: &TrackingSnapshot, key: &str) -> i64 {
    tracking
        .internal
        .get(key)
        .or_else(|| tracking.user.get(key))
        .and_then(|value| {
            if value.is_int() {
                value.as_int().ok()
            } else if value.is_float() {
                value.as_float().ok().map(|v| v as i64)
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn parse_failure_warning_message(
    config: &KeloraConfig,
    tracking: Option<&TrackingSnapshot>,
    auto_detected_non_line: bool,
    events_were_output: bool,
    terminal_output: bool,
) -> Option<String> {
    if !auto_detected_non_line || !detection_notices_allowed(config, terminal_output) {
        return None;
    }

    let tracking = tracking?;

    let parse_errors = extract_counter_from_tracking(tracking, "__kelora_error_count_parse");
    let events_created = extract_counter_from_tracking(tracking, "__kelora_stats_events_created");

    let seen = std::cmp::max(1, events_created + parse_errors);
    let should_warn = (parse_errors >= 10 && parse_errors * 3 >= seen)
        || (events_created == 0 && parse_errors >= 3);

    if should_warn {
        let mut message = config
            .format_error_message("Parsing mostly failed; rerun with -f line or specify -f <fmt>.");
        if !events_were_output {
            message = message.trim_start_matches('\n').to_string();
        }
        Some(message)
    } else {
        None
    }
}

fn emit_parse_failure_warning(
    config: &KeloraConfig,
    tracking: Option<&TrackingSnapshot>,
    auto_detected_non_line: bool,
    events_were_output: bool,
    terminal_output: bool,
) {
    if let Some(message) = parse_failure_warning_message(
        config,
        tracking,
        auto_detected_non_line,
        events_were_output,
        terminal_output,
    ) {
        eprintln!("{}", message);
    }
}

use parallel::{ParallelConfig, ParallelProcessor};
use pipeline::DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS;
use pipeline::{
    create_input_reader, create_pipeline_builder_from_config, create_pipeline_from_config,
};
use stats::{
    get_thread_stats, set_collect_stats, stats_add_error, stats_add_line_filtered,
    stats_add_line_output, stats_add_line_read, stats_finish_processing, stats_start_timer,
    ProcessingStats,
};
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Result of pipeline processing
#[derive(Debug)]
struct PipelineResult {
    pub stats: Option<ProcessingStats>,
    pub tracking_data: TrackingSnapshot,
    pub auto_detected_non_line: bool,
}

/// Core pipeline processing function using KeloraConfig  
fn run_pipeline_with_kelora_config<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    // Enable/disable stats collection up front to avoid per-event overhead when diagnostics are off
    let collect_stats = config.output.stats.is_some()
        || (!config.processing.silent && !config.processing.suppress_diagnostics);
    set_collect_stats(collect_stats);

    // Start statistics collection if enabled
    if collect_stats {
        stats_start_timer();
        // Set the initial format in stats (may be updated if auto-detected later)
        stats::stats_set_detected_format(config.input.format.to_display_string());
    }

    let use_parallel = config.should_use_parallel();

    if use_parallel && matches!(config.output.format, config::OutputFormat::Levelmap) {
        return Err(anyhow::anyhow!(
            "levelmap output format is not supported with --parallel or thread overrides"
        ));
    }

    if use_parallel && matches!(config.output.format, config::OutputFormat::Keymap) {
        return Err(anyhow::anyhow!(
            "keymap output format is not supported with --parallel or thread overrides"
        ));
    }

    if use_parallel {
        run_pipeline_parallel(config, output, ctrl_rx)
    } else {
        let mut output = output;
        let (_final_input_format, auto_detected_non_line) =
            run_pipeline_sequential(config, &mut output, ctrl_rx.clone())?;
        let tracking_user = tracking::get_thread_tracking_state();
        let tracking_internal = tracking::get_thread_internal_state();
        let tracking_data = TrackingSnapshot::from_parts(tracking_user, tracking_internal);
        // Always collect stats for error reporting, even if --stats not used
        stats_finish_processing();
        let mut stats = get_thread_stats();
        stats.extract_discovered_from_tracking(&tracking_data.internal);
        let final_stats = Some(stats);

        Ok(PipelineResult {
            stats: final_stats,
            tracking_data,
            auto_detected_non_line,
        })
    }
}

/// Run pipeline in parallel mode using KeloraConfig
fn run_pipeline_parallel<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    let terminal_output = std::io::stderr().is_terminal();

    // Handle auto-detection for parallel mode
    let (final_config, auto_detected_non_line, detected_reader) =
        if matches!(config.input.format, config::InputFormat::Auto) {
            // For parallel mode, we need to detect format first
            let (detected_format, detected_reader) = detect_format_for_parallel_mode(
                &config.input.files,
                config.input.no_input,
                config.processing.strict,
            )?;

            emit_detected_format_notice(config, &detected_format, terminal_output);

            // Create new config with detected format
            let mut new_config = config.clone();
            new_config.input.format = detected_format.format.clone();

            // Update detected format in stats if stats are enabled
            if config.output.stats.is_some() {
                stats::stats_set_detected_format(new_config.input.format.to_display_string());
            }

            let was_auto_detected_non_line = detected_format.detected_non_line();

            (new_config, was_auto_detected_non_line, detected_reader)
        } else {
            (config.clone(), false, None)
        };

    let config = &final_config;
    let batch_size = config.effective_batch_size();

    let preserve_order = !config.performance.no_preserve_order;
    let parallel_config = ParallelConfig {
        num_workers: config.effective_threads(),
        batch_size,
        batch_timeout_ms: config.performance.batch_timeout,
        preserve_order,
        buffer_size: Some(10000),
    };

    let processor =
        ParallelProcessor::new(parallel_config).with_take_limit(config.processing.take_limit);

    // Create pipeline builder and components for begin/end stages
    let pipeline_builder = create_pipeline_builder_from_config(config);
    let (_pipeline, begin_stage, end_stage, mut ctx) = pipeline_builder
        .clone()
        .build(config.processing.stages.clone())?;

    file_ops::set_mode(FileOpMode::Sequential);

    // Execute begin stage sequentially if provided
    if let Err(e) = begin_stage.execute(&mut ctx) {
        return Err(anyhow::anyhow!("Begin stage error: {}", e));
    }

    // Get reader using pipeline builder
    let reader: Box<dyn BufRead + Send> = if let Some(reader) = detected_reader {
        reader
    } else {
        create_input_reader(config)?
    };

    // Process stages in parallel
    if preserve_order {
        file_ops::set_mode(FileOpMode::ParallelOrdered);
    } else {
        file_ops::set_mode(FileOpMode::ParallelUnordered);
    }

    processor.process_with_pipeline(
        reader,
        pipeline_builder,
        config.processing.stages.clone(),
        config,
        output,
        ctrl_rx.clone(),
    )?;

    // Merge the parallel metrics state with our pipeline context
    let parallel_snapshot = processor.get_final_tracked_state();

    // Extract internal stats from tracking system before merging
    // This is needed for error reporting, not just when --stats is enabled
    processor
        .extract_final_stats_from_tracking(&parallel_snapshot)
        .unwrap_or(());

    // Filter out stats and errors from user-visible context and merge the rest
    for (key, dynamic_value) in &parallel_snapshot.user {
        if !key.starts_with("__internal_")
            && !key.starts_with("__kelora_stats_")
            && !key.starts_with("__op___kelora_stats_")
            && !key.starts_with("__kelora_error_")
            && !key.starts_with("__op___kelora_error_")
        {
            ctx.tracker.insert(key.clone(), dynamic_value.clone());
        }
    }

    // Execute end stage sequentially with merged state
    file_ops::set_mode(FileOpMode::Sequential);
    if let Err(e) = end_stage.execute(&ctx) {
        return Err(anyhow::anyhow!("End stage error: {}", e));
    }

    // Return both stats and tracking data
    // Always collect stats for error reporting, even if --stats not used
    Ok(PipelineResult {
        stats: Some(processor.get_final_stats()),
        tracking_data: parallel_snapshot,
        auto_detected_non_line,
    })
}

/// Run pipeline in sequential mode using KeloraConfig
fn run_pipeline_sequential<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<(config::InputFormat, bool)> {
    if matches!(config.input.format, config::InputFormat::Auto) {
        return run_pipeline_sequential_with_auto_detection(config, output, ctrl_rx);
    }

    let input = if config.input.no_input {
        // Create empty input for --no-input mode
        SequentialInput::Stdin(Box::new(io::BufReader::new(io::Cursor::new(Vec::new()))))
    } else if config.input.files.is_empty() {
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        SequentialInput::Stdin(Box::new(io::BufReader::new(processed_stdin)))
    } else {
        let sorted_files =
            pipeline::builders::sort_files(&config.input.files, &config.input.file_order)?;
        SequentialInput::Files(readers::MultiFileReader::new(
            sorted_files,
            config.processing.strict,
        )?)
    };

    run_pipeline_sequential_internal(config, output, ctrl_rx, input)?;

    Ok((config.input.format.clone(), false))
}

/// Run pipeline in sequential mode with auto-detection support
fn run_pipeline_sequential_with_auto_detection<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<(config::InputFormat, bool)> {
    let terminal_output = std::io::stderr().is_terminal();

    if config.input.no_input {
        // For --no-input mode, skip auto-detection and use empty input with Line format
        let mut final_config = config.clone();
        final_config.input.format = config::InputFormat::Line;
        let input =
            SequentialInput::Stdin(Box::new(io::BufReader::new(io::Cursor::new(Vec::new()))));
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)?;
        return Ok((final_config.input.format, false));
    }

    if config.input.files.is_empty() {
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        let mut peekable_reader =
            readers::PeekableLineReader::new(io::BufReader::new(processed_stdin));

        let detected_format = detect_format_from_peekable_reader(&mut peekable_reader)?;

        emit_detected_format_notice(config, &detected_format, terminal_output);

        let mut final_config = config.clone();
        final_config.input.format = detected_format.format.clone();

        // Set detected format in stats if stats are enabled
        if config.output.stats.is_some() {
            stats::stats_set_detected_format(final_config.input.format.to_display_string());
        }

        let input = SequentialInput::Stdin(Box::new(peekable_reader));
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)?;

        Ok((
            final_config.input.format,
            detected_format.detected_non_line(),
        ))
    } else {
        let sorted_files =
            pipeline::builders::sort_files(&config.input.files, &config.input.file_order)?;

        if sorted_files.is_empty() {
            return Ok((config::InputFormat::Line, false));
        }

        let mut failed_opens: Vec<(String, String)> = Vec::new();
        let mut failed_dirs: Vec<String> = Vec::new();
        let mut detected_format: Option<DetectedFormat> = None;
        for file_path in &sorted_files {
            if let Ok(metadata) = fs::metadata(file_path) {
                if metadata.is_dir() {
                    if config.processing.strict {
                        return Err(anyhow::anyhow!(
                            "Input path '{}' is a directory; only files are supported",
                            file_path
                        ));
                    }
                    failed_dirs.push(file_path.clone());
                    continue;
                }
            }

            match decompression::DecompressionReader::new(file_path) {
                Ok(decompressed) => {
                    let mut peekable_reader = readers::PeekableLineReader::new(decompressed);
                    detected_format =
                        Some(detect_format_from_peekable_reader(&mut peekable_reader)?);
                    break;
                }
                Err(e) => {
                    if config.processing.strict {
                        return Err(anyhow::anyhow!(
                            "Failed to open file '{}': {}",
                            file_path,
                            e
                        ));
                    }
                    failed_opens.push((file_path.clone(), e.to_string()));
                }
            }
        }

        let detected_format = match detected_format {
            Some(detected) => detected,
            None => {
                for path in failed_dirs {
                    eprintln!(
                        "{}",
                        crate::config::format_error_message_auto(&format!(
                            "Input path '{}' is a directory; skipping (input files only)",
                            path
                        ))
                    );
                    stats::stats_file_open_failed(&path);
                }
                for (path, err) in failed_opens {
                    eprintln!(
                        "{}",
                        crate::config::format_error_message_auto(&format!(
                            "Failed to open file '{}': {}",
                            path, err
                        ))
                    );
                    stats::stats_file_open_failed(&path);
                }
                return Err(anyhow::anyhow!(
                    "Failed to open any input files for detection"
                ));
            }
        };

        emit_detected_format_notice(config, &detected_format, terminal_output);

        let mut final_config = config.clone();
        final_config.input.format = detected_format.format.clone();

        // Set detected format in stats if stats are enabled
        if config.output.stats.is_some() {
            stats::stats_set_detected_format(final_config.input.format.to_display_string());
        }

        let input = SequentialInput::Files(readers::MultiFileReader::new(
            sorted_files,
            final_config.processing.strict,
        )?);
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)?;

        Ok((
            final_config.input.format,
            detected_format.detected_non_line(),
        ))
    }
}

const LINE_CHANNEL_BOUND: usize = 1024;

enum SequentialInput {
    Stdin(Box<dyn BufRead + Send>),
    Files(readers::MultiFileReader),
}

enum ReaderMessage {
    Line {
        line: String,
        filename: Option<String>,
    },
    Error {
        error: io::Error,
        filename: Option<String>,
    },
    Eof,
}

fn spawn_stdin_reader(
    mut reader: Box<dyn BufRead + Send>,
    sender: Sender<ReaderMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let mut buffer = String::new();
        loop {
            match ctrl_rx.try_recv() {
                Ok(Ctrl::Shutdown { immediate }) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    if immediate {
                        return Ok(());
                    }
                    break;
                }
                Ok(Ctrl::PrintStats) => {
                    // Reader thread doesn't have stats to print, ignore
                }
                Err(_) => {
                    // No message, continue
                }
            }

            buffer.clear();
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    break;
                }
                Ok(_) => {
                    let line = buffer.trim_end_matches(&['\n', '\r'][..]).to_string();
                    if sender
                        .send(ReaderMessage::Line {
                            line,
                            filename: None,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    if sender
                        .send(ReaderMessage::Error {
                            error: e,
                            filename: None,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        Ok(())
    })
}

fn spawn_file_reader(
    mut reader: readers::MultiFileReader,
    sender: Sender<ReaderMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let mut buffer = String::new();
        loop {
            match ctrl_rx.try_recv() {
                Ok(Ctrl::Shutdown { immediate }) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    if immediate {
                        return Ok(());
                    }
                    break;
                }
                Ok(Ctrl::PrintStats) => {
                    // Reader thread doesn't have stats to print, ignore
                }
                Err(_) => {
                    // No message, continue
                }
            }

            buffer.clear();
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    break;
                }
                Ok(_) => {
                    let filename = reader.current_filename().map(|s| s.to_string());
                    let line = buffer.trim_end_matches(&['\n', '\r'][..]).to_string();
                    if sender.send(ReaderMessage::Line { line, filename }).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let filename = reader.current_filename().map(|s| s.to_string());
                    if sender
                        .send(ReaderMessage::Error { error: e, filename })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        Ok(())
    })
}

fn run_pipeline_sequential_internal<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
    input: SequentialInput,
) -> Result<()> {
    let (mut pipeline, begin_stage, end_stage, mut ctx) = create_pipeline_from_config(config)?;

    file_ops::set_mode(FileOpMode::Sequential);

    if let Err(e) = begin_stage.execute(&mut ctx) {
        return Err(anyhow::anyhow!("Begin stage error: {}", e));
    }

    let (line_tx, line_rx) = bounded::<ReaderMessage>(LINE_CHANNEL_BOUND);
    let reader_ctrl = ctrl_rx.clone();
    let reader_handle = match input {
        SequentialInput::Stdin(reader) => spawn_stdin_reader(reader, line_tx, reader_ctrl),
        SequentialInput::Files(reader) => spawn_file_reader(reader, line_tx, reader_ctrl),
    };

    let multiline_timeout = config
        .input
        .multiline
        .as_ref()
        .map(|_| Duration::from_millis(DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS));

    let mut current_csv_headers: Option<Vec<String>> = None;
    let mut last_filename: Option<String> = None;
    let mut line_num = 0usize;
    let mut skipped_lines = 0usize;
    let mut section_selector = config
        .input
        .section
        .as_ref()
        .map(|section_config| pipeline::SectionSelector::new(section_config.clone()));
    let mut pending_deadline: Option<Instant> = None;
    let mut shutdown_requested = false;
    let mut immediate_shutdown = false;
    let gap_marker_use_colors = crate::tty::should_use_colors_with_mode(&config.output.color);
    let mut gap_tracker = if config.processing.quiet_events {
        // Suppress gap markers when output is suppressed (stats-only, high quiet levels)
        None
    } else {
        config
            .output
            .mark_gaps
            .map(|threshold| crate::formatters::GapTracker::new(threshold, gap_marker_use_colors))
    };

    let ctrl_rx = ctrl_rx;
    let line_rx = line_rx;

    loop {
        if immediate_shutdown || shutdown_requested {
            break;
        }

        let deadline_duration = pending_deadline.map(|deadline| {
            let now = Instant::now();
            if deadline <= now {
                Duration::from_millis(0)
            } else {
                deadline.saturating_duration_since(now)
            }
        });

        if let Some(duration) = deadline_duration {
            if duration.is_zero() {
                let results = pipeline.flush(&mut ctx)?;
                for formatted in results {
                    write_formatted_output(formatted, output, &mut gap_tracker)?;
                }
                pending_deadline = None;
                continue;
            }

            let timeout = crossbeam_channel::after(duration);
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if immediate {
                                immediate_shutdown = true;
                            } else {
                                shutdown_requested = true;
                            }
                        }
                        Ok(Ctrl::PrintStats) => {
                            // Print current stats to stderr (sequential mode)
                            let mut current_stats = get_thread_stats();
                            let internal_tracking = RhaiEngine::get_thread_internal_state();
                            current_stats.extract_discovered_from_tracking(&internal_tracking);
                            let stats_message = config.format_stats_message(
                                &current_stats.format_stats_for_signal(
                                    config.input.multiline.is_some(),
                                    true,
                                ),
                                true, // Always show header for signal handler
                            );
                            let _ = SafeStderr::new().writeln(&stats_message);
                        }
                        Err(_) => {
                            shutdown_requested = true;
                        }
                    }
                }
                recv(line_rx) -> msg => {
                    match msg {
                        Ok(message) => {
                            if handle_reader_message(
                                message,
                                ReaderContext {
                                    pipeline: &mut pipeline,
                                    ctx: &mut ctx,
                                    config,
                                    output,
                                    line_num: &mut line_num,
                                    skipped_lines: &mut skipped_lines,
                                    section_selector: &mut section_selector,
                                    current_csv_headers: &mut current_csv_headers,
                                    last_filename: &mut last_filename,
                                    gap_tracker: &mut gap_tracker,
                                },
                            )? {
                                shutdown_requested = true;
                            }
                            pending_deadline = multiline_timeout
                                .and_then(|timeout| pipeline
                                    .has_pending_chunk()
                                    .then(|| Instant::now() + timeout));
                        }
                        Err(_) => {
                            shutdown_requested = true;
                        }
                    }
                }
                recv(timeout) -> _ => {
                    let results = pipeline.flush(&mut ctx)?;
                    for formatted in results {
                        write_formatted_output(formatted, output, &mut gap_tracker)?;
                    }
                    pending_deadline = None;
                }
            }
        } else {
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if immediate {
                                immediate_shutdown = true;
                            } else {
                                shutdown_requested = true;
                            }
                        }
                        Ok(Ctrl::PrintStats) => {
                            // Print current stats to stderr (sequential mode)
                            let mut current_stats = get_thread_stats();
                            let internal_tracking = RhaiEngine::get_thread_internal_state();
                            current_stats.extract_discovered_from_tracking(&internal_tracking);
                            let stats_message = config.format_stats_message(
                                &current_stats.format_stats_for_signal(
                                    config.input.multiline.is_some(),
                                    true,
                                ),
                                true, // Always show header for signal handler
                            );
                            let _ = SafeStderr::new().writeln(&stats_message);
                        }
                        Err(_) => {
                            shutdown_requested = true;
                        }
                    }
                }
                recv(line_rx) -> msg => {
                    match msg {
                        Ok(message) => {
                            if handle_reader_message(
                                message,
                                ReaderContext {
                                    pipeline: &mut pipeline,
                                    ctx: &mut ctx,
                                    config,
                                    output,
                                    line_num: &mut line_num,
                                    skipped_lines: &mut skipped_lines,
                                    section_selector: &mut section_selector,
                                    current_csv_headers: &mut current_csv_headers,
                                    last_filename: &mut last_filename,
                                    gap_tracker: &mut gap_tracker,
                                },
                            )? {
                                shutdown_requested = true;
                            }
                            pending_deadline = multiline_timeout
                                .and_then(|timeout| pipeline
                                    .has_pending_chunk()
                                    .then(|| Instant::now() + timeout));
                        }
                        Err(_) => {
                            shutdown_requested = true;
                        }
                    }
                }
            }
        }

        if rhai_functions::process::is_exit_requested() {
            let exit_code = rhai_functions::process::get_exit_code();
            std::process::exit(exit_code);
        }
    }

    drop(line_rx);

    match reader_handle.join() {
        Ok(result) => result?,
        Err(_) => return Err(anyhow::anyhow!("Reader thread panicked")),
    }

    if immediate_shutdown {
        return Ok(());
    }

    let results = pipeline.flush(&mut ctx)?;
    for formatted in results {
        write_formatted_output(formatted, output, &mut gap_tracker)?;
    }

    pipeline.finish_spans(&mut ctx)?;

    if let Some(result) = pipeline.finish_formatter() {
        write_formatted_output(result, output, &mut gap_tracker)?;
    }

    if let Err(e) = end_stage.execute(&ctx) {
        return Err(anyhow::anyhow!("End stage error: {}", e));
    }

    rhai_functions::tracking::merge_thread_tracking_to_context(&mut ctx);

    Ok(())
}

struct ReaderContext<'a, W: Write> {
    pipeline: &'a mut pipeline::Pipeline,
    ctx: &'a mut pipeline::PipelineContext,
    config: &'a KeloraConfig,
    output: &'a mut W,
    line_num: &'a mut usize,
    skipped_lines: &'a mut usize,
    section_selector: &'a mut Option<pipeline::SectionSelector>,
    current_csv_headers: &'a mut Option<Vec<String>>,
    last_filename: &'a mut Option<String>,
    gap_tracker: &'a mut Option<crate::formatters::GapTracker>,
}

fn handle_reader_message<W: Write>(
    message: ReaderMessage,
    ctx: ReaderContext<'_, W>,
) -> Result<bool> {
    let ReaderContext {
        pipeline,
        ctx: pipeline_ctx,
        config,
        output,
        line_num,
        skipped_lines,
        section_selector,
        current_csv_headers,
        last_filename,
        gap_tracker,
    } = ctx;
    match message {
        ReaderMessage::Line { line, filename } => {
            match process_line_sequential(
                Ok(line),
                line_num,
                skipped_lines,
                section_selector,
                pipeline,
                pipeline_ctx,
                config,
                output,
                filename,
                current_csv_headers,
                last_filename,
                gap_tracker,
            )? {
                ProcessingResult::Continue => Ok(false),
                ProcessingResult::TakeLimitExhausted | ProcessingResult::Stop => Ok(true),
            }
        }
        ReaderMessage::Error { error, filename } => {
            match process_line_sequential(
                Err(error),
                line_num,
                skipped_lines,
                section_selector,
                pipeline,
                pipeline_ctx,
                config,
                output,
                filename,
                current_csv_headers,
                last_filename,
                gap_tracker,
            )? {
                ProcessingResult::Continue => Ok(false),
                ProcessingResult::TakeLimitExhausted | ProcessingResult::Stop => Ok(true),
            }
        }
        ReaderMessage::Eof => Ok(true),
    }
}

/// Processing result for sequential pipeline
enum ProcessingResult {
    Continue,
    TakeLimitExhausted,
    Stop,
}

/// Process a single line in sequential mode with filename tracking and CSV schema detection
#[allow(clippy::too_many_arguments)]
fn process_line_sequential<W: Write>(
    line_result: io::Result<String>,
    line_num: &mut usize,
    skipped_lines: &mut usize,
    section_selector: &mut Option<pipeline::SectionSelector>,
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    config: &KeloraConfig,
    output: &mut W,
    current_filename: Option<String>,
    current_csv_headers: &mut Option<Vec<String>>,
    last_filename: &mut Option<String>,
    gap_tracker: &mut Option<crate::formatters::GapTracker>,
) -> Result<ProcessingResult> {
    let line = line_result?;
    *line_num += 1;

    // Count line read for stats
    if config.output.stats.is_some() {
        stats_add_line_read();
    }

    // Check if we've hit the head limit (stops I/O early)
    if let Some(head_limit) = config.input.head_lines {
        if *line_num > head_limit {
            return Ok(ProcessingResult::Stop);
        }
    }

    // Skip the first N lines if configured (applied before ignore-lines and parsing)
    if *skipped_lines < config.input.skip_lines {
        *skipped_lines += 1;
        // Count skipped line for stats
        if config.output.stats.is_some() {
            stats_add_line_filtered();
        }
        return Ok(ProcessingResult::Continue);
    }

    // Apply section selection if configured (filters out lines outside selected sections)
    if let Some(selector) = section_selector {
        if !selector.should_include_line(&line) {
            // Count filtered line for stats
            if config.output.stats.is_some() {
                stats_add_line_filtered();
            }
            return Ok(ProcessingResult::Continue);
        }
    }

    // Apply keep-lines filter if configured (early filtering before parsing)
    if let Some(ref keep_regex) = config.input.keep_lines {
        if !keep_regex.is_match(&line) {
            // Count filtered line for stats
            if config.output.stats.is_some() {
                stats_add_line_filtered();
            }
            return Ok(ProcessingResult::Continue);
        }
    }

    // Apply ignore-lines filter if configured (early filtering before parsing)
    if let Some(ref ignore_regex) = config.input.ignore_lines {
        if ignore_regex.is_match(&line) {
            // Count filtered line for stats
            if config.output.stats.is_some() {
                stats_add_line_filtered();
            }
            return Ok(ProcessingResult::Continue);
        }
    }

    if line.trim().is_empty() {
        // Only skip empty lines for structured formats, not for line format
        if !matches!(config.input.format, config::InputFormat::Line) {
            return Ok(ProcessingResult::Continue);
        }
        // For line format, continue processing the empty line
    }

    // For CSV formats, detect file changes and reinitialize parser, or handle first line for stdin
    if matches!(
        config.input.format,
        config::InputFormat::Csv(_)
            | config::InputFormat::Tsv(_)
            | config::InputFormat::Csvnh
            | config::InputFormat::Tsvnh
    ) && (current_filename != *last_filename
        || (current_filename.is_none() && current_csv_headers.is_none()))
    {
        // File changed, reinitialize CSV parser for this file
        let mut temp_parser = match &config.input.format {
            config::InputFormat::Csv(ref field_spec) => {
                let p = parsers::CsvParser::new_csv();
                if let Some(ref spec) = field_spec {
                    p.with_field_spec(spec)?
                        .with_strict(config.processing.strict)
                } else {
                    p
                }
            }
            config::InputFormat::Tsv(ref field_spec) => {
                let p = parsers::CsvParser::new_tsv();
                if let Some(ref spec) = field_spec {
                    p.with_field_spec(spec)?
                        .with_strict(config.processing.strict)
                } else {
                    p
                }
            }
            config::InputFormat::Csvnh => parsers::CsvParser::new_csv_no_headers(),
            config::InputFormat::Tsvnh => parsers::CsvParser::new_tsv_no_headers(),
            _ => unreachable!(),
        };

        // Initialize headers from the first line
        let was_consumed = temp_parser.initialize_headers_from_line(&line)?;

        // Get the initialized headers
        let headers = temp_parser.get_headers();
        *current_csv_headers = Some(headers.clone());
        *last_filename = current_filename.clone();

        // Rebuild the pipeline with new headers
        let mut pipeline_builder = create_pipeline_builder_from_config(config);
        pipeline_builder = pipeline_builder.with_csv_headers(headers);

        // Note: We rebuild the entire pipeline including begin/end stages, but only use
        // the pipeline and context. The begin stage was already executed at session start
        // and the end stage will be executed when the original session completes.
        let (new_pipeline, _unused_begin_stage, _unused_end_stage, new_ctx) =
            pipeline_builder.build(config.processing.stages.clone())?;

        *pipeline = new_pipeline;
        // Keep the existing context's tracking state but update the Rhai engine
        ctx.rhai = new_ctx.rhai;

        // If the first line was consumed as a header, don't process it as data
        if was_consumed {
            return Ok(ProcessingResult::Continue);
        }
    }

    // Update metadata with filename tracking
    ctx.meta.line_num = Some(*line_num);
    ctx.meta.filename = current_filename;

    // Process line through pipeline
    match pipeline.process_line(line, ctx) {
        Ok(results) => {
            // Count output lines for stats
            if config.output.stats.is_some() && !results.is_empty() {
                stats_add_line_output();
            }
            // Note: Empty results are now counted as either:
            // 1. Parsing errors (counted by stats_add_line_error() in pipeline)
            // 2. Filter rejections (counted by stats_add_event_filtered() in pipeline)
            // So we don't need to count empty results as filtered here anymore

            // Output all results (usually just one), skip empty strings
            for formatted in results {
                write_formatted_output(formatted, output, gap_tracker)?;
            }

            // Check if take limit is exhausted after processing
            if pipeline.is_take_limit_exhausted() {
                return Ok(ProcessingResult::TakeLimitExhausted);
            }
        }
        Err(e) => {
            // Count errors for stats
            if config.output.stats.is_some() {
                stats_add_error();
            }

            // Handle error based on new resiliency model
            if config.processing.strict {
                return Err(e);
            }
            // Default resilient mode: continue processing
        }
    }

    Ok(ProcessingResult::Continue)
}

fn write_formatted_output<W: Write>(
    formatted: pipeline::FormattedOutput,
    output: &mut W,
    gap_tracker: &mut Option<crate::formatters::GapTracker>,
) -> io::Result<()> {
    if let Err(err) = file_ops::execute_ops(&formatted.file_ops) {
        return Err(io::Error::other(err));
    }

    let marker = match gap_tracker.as_mut() {
        Some(tracker) => tracker.check(formatted.timestamp),
        None => None,
    };

    if let Some(marker_line) = marker {
        writeln!(output, "{}", marker_line)?;
    }

    if !formatted.line.is_empty() {
        writeln!(output, "{}", formatted.line)?;
    }

    Ok(())
}

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
            Ok(multiline_config) => {
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
        Ok(pipeline_result) => {
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
                            let metrics_output =
                                crate::rhai_functions::tracking::format_metrics_output(
                                    &pipeline_result.tracking_data.user,
                                    metrics_level,
                                );
                            if !metrics_output.is_empty() && metrics_output != "No metrics tracked"
                            {
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
                            if let Ok(json_output) =
                                crate::rhai_functions::tracking::format_metrics_json(
                                    &pipeline_result.tracking_data.user,
                                )
                            {
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

            // Write metrics to file if configured
            if let Some(ref metrics_file) = config.output.metrics_file {
                if let Ok(json_output) = crate::rhai_functions::tracking::format_metrics_json(
                    &pipeline_result.tracking_data.user,
                ) {
                    if let Err(e) = std::fs::write(metrics_file, json_output) {
                        stderr
                            .writeln(&config.format_error_message(&format!(
                                "Failed to write metrics file: {}",
                                e
                            )))
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
                let mut hint = config.format_hint_message(
                    "Metrics recorded; rerun with -m or --metrics=json to view them.",
                );
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
                                Some(&config),
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

            emit_parse_failure_warning(
                &config,
                Some(&pipeline_result.tracking_data),
                auto_detected_non_line,
                events_were_output,
                std::io::stderr().is_terminal(),
            );
            (pipeline_result.stats, Some(pipeline_result.tracking_data))
        }
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
            } else if stats.has_errors() && diagnostics_allowed_runtime {
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

fn emit_fatal_line(stderr: &mut SafeStderr, config: &KeloraConfig, message: &str) {
    stderr
        .writeln(&config.format_error_message(message))
        .unwrap_or(());
}

/// Validate CLI arguments for early error detection
fn validate_cli_args(cli: &Cli) -> Result<()> {
    // Validate --no-input conflicts
    if cli.no_input && !cli.files.is_empty() {
        return Err(anyhow::anyhow!(
            "--no-input cannot be used with input files"
        ));
    }

    // Check stdin usage
    let mut stdin_count = 0;
    for file_path in &cli.files {
        if file_path == "-" {
            stdin_count += 1;
            if stdin_count > 1 {
                return Err(anyhow::anyhow!("stdin (\"-\") can only be specified once"));
            }
        }
        // Note: File existence is checked at runtime during processing (exit 1),
        // not during CLI validation (exit 2)
    }

    // Check if exec files exist (if specified)
    for exec_file in &cli.exec_files {
        if !std::path::Path::new(exec_file).exists() {
            return Err(anyhow::anyhow!("Exec file not found: {}", exec_file));
        }
    }

    // Validate batch size
    if let Some(batch_size) = cli.batch_size {
        if batch_size == 0 {
            return Err(anyhow::anyhow!("Batch size must be greater than 0"));
        }
    }

    // Validate thread count
    if cli.threads > 1000 {
        return Err(anyhow::anyhow!("Thread count too high (max 1000)"));
    }

    if cli.span_close.is_some() && cli.span.is_none() && cli.span_idle.is_none() {
        return Err(anyhow::anyhow!(
            "--span-close requires --span or --span-idle to be specified"
        ));
    }

    // Check for --core with CSV/TSV formats (not allowed with these formats)
    if cli.core {
        match cli.output_format {
            OutputFormat::Csv => {
                return Err(anyhow::anyhow!(
                    "csv output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Tsv => {
                return Err(anyhow::anyhow!(
                    "tsv output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Csvnh => {
                return Err(anyhow::anyhow!(
                    "csvnh output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Tsvnh => {
                return Err(anyhow::anyhow!(
                    "tsvnh output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            _ => {
                // Other formats are fine with --core
            }
        }
    }

    Ok(())
}

/// Extract --config-file argument from raw args
fn extract_config_file_arg(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "--config-file" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

/// Extract --save-alias argument from raw args
fn extract_save_alias_arg(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "--save-alias" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

/// Check if the given alias_name appears in any `-a` or `--alias` reference in the args
fn should_resolve_alias_references(args: &[String], alias_name: &str) -> bool {
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-a" || args[i] == "--alias") && i + 1 < args.len() {
            if args[i + 1] == alias_name {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// Handle --save-alias command
fn handle_save_alias(raw_args: &[String], alias_name: &str, use_emoji: bool) {
    use crate::config_file::ConfigFile;

    // Extract --config-file if specified
    let mut config_file_path: Option<String> = None;
    let mut command_args = Vec::new();
    let mut i = 0;
    while i < raw_args.len() {
        if raw_args[i] == "--save-alias" {
            // Skip --save-alias and its argument
            i += 2;
        } else if raw_args[i] == "--config-file" && i + 1 < raw_args.len() {
            // Extract --config-file for saving
            config_file_path = Some(raw_args[i + 1].clone());
            i += 2;
        } else {
            command_args.push(raw_args[i].clone());
            i += 1;
        }
    }

    // Skip the program name (first argument)
    if !command_args.is_empty() {
        command_args.remove(0);
    }

    // Check if we have any command left to save
    if command_args.is_empty() {
        let prefix = if use_emoji { "⚠️" } else { "kelora:" };
        eprintln!("{} No command to save as alias '{}'", prefix, alias_name);
        std::process::exit(2);
    }

    // Check if we should resolve alias references (when updating self-referencing alias)
    let should_resolve = should_resolve_alias_references(&command_args, alias_name);

    // If we need to resolve OR validate, load the config file
    let alias_value = if command_args
        .iter()
        .any(|arg| arg == "-a" || arg == "--alias")
    {
        // Command contains alias references - need to load config
        let config_result = match config_file_path.as_ref() {
            Some(path) => ConfigFile::load_with_custom_path(Some(path)),
            None => ConfigFile::load_with_custom_path(None),
        };

        match config_result {
            Ok(config) => {
                if should_resolve {
                    // Resolution mode: flatten all aliases
                    match config.resolve_args_only(&command_args) {
                        Ok(resolved_args) => {
                            if resolved_args.is_empty() {
                                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                                eprintln!(
                                    "{} Resolved command is empty for alias '{}'",
                                    prefix, alias_name
                                );
                                std::process::exit(2);
                            }
                            shell_words::join(resolved_args)
                        }
                        Err(e) => {
                            let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                            eprintln!("{} Failed to resolve aliases in command: {}", prefix, e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    // Preservation mode: validate references exist but keep them
                    if let Err(e) = config.validate_alias_references(&command_args) {
                        let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                        eprintln!("{} {}", prefix, e);
                        eprintln!(
                            "{} Cannot save alias '{}' with reference to non-existent alias",
                            prefix, alias_name
                        );
                        std::process::exit(1);
                    }
                    shell_words::join(command_args)
                }
            }
            Err(_) if should_resolve => {
                // Trying to update non-existent alias
                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                eprintln!(
                    "{} Cannot update alias '{}' - no config file found",
                    prefix, alias_name
                );
                eprintln!(
                    "{} To create a new alias, use a command without referencing itself",
                    prefix
                );
                std::process::exit(1);
            }
            Err(_) => {
                // Preservation mode but config doesn't exist - that's an error
                // because we're referencing other aliases that don't exist
                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                eprintln!(
                    "{} Cannot save alias '{}' with alias references - no config file found",
                    prefix, alias_name
                );
                eprintln!(
                    "{} Create the referenced aliases first, or use a command without alias references",
                    prefix
                );
                std::process::exit(1);
            }
        }
    } else {
        // No alias references - just join and save
        shell_words::join(command_args)
    };

    // Save the alias to the specified config file or auto-detect
    let target_path = config_file_path.as_ref().map(std::path::Path::new);
    match ConfigFile::save_alias(alias_name, &alias_value, target_path) {
        Ok((config_path, previous_value)) => {
            let success_prefix = if use_emoji { "🔹" } else { "kelora:" };
            println!(
                "{} Alias '{}' saved to {}",
                success_prefix,
                alias_name,
                config_path.display()
            );

            if let Some(prev) = previous_value {
                let info_prefix = if use_emoji { "🔹" } else { "kelora:" };
                println!("{} Replaced previous alias:", info_prefix);
                println!("    {} = {}", alias_name, prev);
            }
        }
        Err(e) => {
            let error_prefix = if use_emoji { "⚠️" } else { "kelora:" };
            eprintln!(
                "{} Failed to save alias '{}': {}",
                error_prefix, alias_name, e
            );
            std::process::exit(1);
        }
    }
}

/// Process command line arguments with config file support
fn process_args_with_config(stderr: &mut SafeStderr) -> (ArgMatches, Cli) {
    // Get raw command line arguments
    let raw_args: Vec<String> = std::env::args().collect();

    // Extract --config-file argument early for use by config commands
    let config_file_path = extract_config_file_arg(&raw_args);

    // Check for config-related option conflicts
    let has_show_config = raw_args.iter().any(|arg| arg == "--show-config");
    let has_edit_config = raw_args.iter().any(|arg| arg == "--edit-config");
    let has_ignore_config = raw_args.iter().any(|arg| arg == "--ignore-config");

    if has_show_config && has_edit_config {
        stderr
            .writeln("kelora: Error: --show-config and --edit-config are mutually exclusive")
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    if has_ignore_config && has_edit_config {
        stderr
            .writeln("kelora: Error: --ignore-config and --edit-config are mutually exclusive")
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    // Check for --show-config first, before any other processing
    if has_show_config {
        ConfigFile::show_config();
        std::process::exit(0);
    }

    // Check for --edit-config
    if has_edit_config {
        ConfigFile::edit_config(config_file_path.as_deref());
        std::process::exit(0);
    }

    // Check for --help-time
    if raw_args.iter().any(|arg| arg == "--help-time") {
        print_time_format_help();
        std::process::exit(0);
    }

    // Check for --help-functions
    if raw_args.iter().any(|arg| arg == "--help-functions") {
        print_functions_help();
        std::process::exit(0);
    }

    // Check for -h (brief help)
    if raw_args.iter().any(|arg| arg == "-h") {
        print_quick_help();
        std::process::exit(0);
    }

    // Check for --help-examples
    if raw_args.iter().any(|arg| arg == "--help-examples") {
        print_examples_help();
        std::process::exit(0);
    }

    // Check for --help-rhai
    if raw_args.iter().any(|arg| arg == "--help-rhai") {
        print_rhai_help();
        std::process::exit(0);
    }

    // Check for --help-multiline
    if raw_args.iter().any(|arg| arg == "--help-multiline") {
        print_multiline_help();
        std::process::exit(0);
    }

    // Check for --help-regex
    if raw_args.iter().any(|arg| arg == "--help-regex") {
        print_regex_help();
        std::process::exit(0);
    }

    // Check for --help-formats
    if raw_args.iter().any(|arg| arg == "--help-formats") {
        print_formats_help();
        std::process::exit(0);
    }

    // Check for --save-alias before other processing
    if let Some(alias_name) = extract_save_alias_arg(&raw_args) {
        let use_emoji = tty::should_use_emoji_for_stderr();
        handle_save_alias(&raw_args, &alias_name, use_emoji);
        std::process::exit(0);
    }

    // Check for --ignore-config
    let ignore_config = has_ignore_config;

    let processed_args = if ignore_config {
        // Skip config file processing
        raw_args
    } else {
        // Load config file and process aliases
        match ConfigFile::load_with_custom_path(config_file_path.as_deref()) {
            Ok(config_file) => match config_file.process_args(raw_args) {
                Ok(processed) => processed,
                Err(e) => {
                    stderr
                        .writeln(&format!("kelora: Config error: {}", e))
                        .unwrap_or(());
                    std::process::exit(1);
                }
            },
            Err(e) => {
                stderr
                    .writeln(&format!("kelora: Config file error: {}", e))
                    .unwrap_or(());
                std::process::exit(1);
            }
        }
    };

    // Parse with potentially modified arguments
    let matches = Cli::command().get_matches_from(processed_args);
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| {
        stderr
            .writeln(&format!("kelora: Error: {}", e))
            .unwrap_or(());
        std::process::exit(1);
    });

    // Resolve inverted boolean flags
    cli.resolve_boolean_flags();

    // Config file defaults and aliases are already applied in process_args above

    // Check if we should enter interactive mode
    // Interactive mode is activated when:
    // - stdin is a TTY (not piped input)
    // - no input files are provided
    // - --no-input is not specified
    // - no other arguments are provided (just the program name)
    if crate::tty::is_stdin_tty() && cli.files.is_empty() && !cli.no_input {
        // Check if this is truly no arguments (interactive mode) or just missing input files
        let raw_args: Vec<String> = std::env::args().collect();

        // If only program name, enter interactive mode
        if raw_args.len() == 1 {
            // Enter interactive mode
            if let Err(e) = crate::interactive::run_interactive_mode() {
                eprintln!("Interactive mode error: {}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }

        // Otherwise, show error (user provided flags but no input files)
        eprintln!("error: no input files or stdin provided");
        eprintln!();
        eprintln!("{}", Cli::command().render_usage());
        eprintln!();
        eprintln!("For more information, try '-h'.");
        std::process::exit(2);
    }

    (matches, cli)
}

/// Print time format help message adapted for Rust/chrono
fn print_time_format_help() {
    let help_text = r#"
Time Format Reference for --ts-format:

Use with:
  --ts-format <FMT>     Describe how timestamps are parsed
  --input-tz <TZ>       Supply a timezone for inputs without offsets (e.g., --input-tz UTC)
  --multiline timestamp:format=FMT  Use the same chrono format for header detection

Basic date/time components:
%Y  Year with century (e.g., 2024)
%y  Year without century (00-99)
%m  Month as zero-padded decimal (01-12)
%b  Month as abbreviated name (Jan, Feb, ..., Dec)
%B  Month as full name (January, February, ..., December)
%d  Day of month as zero-padded decimal (01-31)
%j  Day of year as zero-padded decimal (001-366)
%H  Hour (24-hour) as zero-padded decimal (00-23)
%I  Hour (12-hour) as zero-padded decimal (01-12)
%p  AM/PM indicator
%M  Minute as zero-padded decimal (00-59)
%S  Second as zero-padded decimal (00-59)

Subsecond precision cheatsheet:
%f   Microseconds (000000-999999)
%3f  Milliseconds (000-999)
%6f  Microseconds (000000-999999)
%9f  Nanoseconds (000000000-999999999)
%.f  Auto-match subseconds with flexible precision

Time zone tokens:
%z  UTC offset (+HHMM or -HHMM)
%Z  Time zone name (if available)
%:z UTC offset with colon (+HH:MM or -HH:MM)

Weekday helpers:
%w  Weekday as decimal (0=Sunday, 6=Saturday)
%a  Weekday as abbreviated name (Sun, Mon, ..., Sat)
%A  Weekday as full name (Sunday, Monday, ..., Saturday)

Week numbers:
%W  Week number (Monday as first day of week)
%U  Week number (Sunday as first day of week)

Common examples:
%Y-%m-%d %H:%M:%S           2024-01-15 14:30:45
%Y-%m-%dT%H:%M:%S%z         2024-01-15T14:30:45+0000
%Y-%m-%d %H:%M:%S%.f        2024-01-15 14:30:45.123456
%b %d %H:%M:%S              Jan 15 14:30:45 (syslog format)
%d/%b/%Y:%H:%M:%S %z        15/Jan/2024:14:30:45 +0000 (Apache access log)
%Y-%m-%d %H:%M:%S,%3f       2024-01-15 14:30:45,123 (Python logging)

Naive timestamp + timezone example:
  kelora app.log --ts-format "%Y-%m-%d %H:%M:%S" --input-tz Europe/Berlin
  (parses local timestamps and normalises them internally)

Shell tip: wrap the entire format in single quotes or escape % symbols to keep
  your shell from expanding them.

Timestamp filtering with --since and --until:
  kelora --since "2024-01-15T10:00:00Z" app.log   # Events after timestamp
  kelora --until "yesterday" app.log              # Events before yesterday
  kelora --since 1h app.log                       # Last hour (1h, 30m, 2d, etc.)
  kelora --since +1h app.log                      # Future events (+ means ahead)

  Anchored timestamps (relative to the other boundary):
  kelora --since 10:00 --until start+30m app.log  # 30 minutes starting at 10:00
  kelora --since end-1h --until 11:00 app.log     # 1 hour ending at 11:00
  kelora --since -2h --until start+1h app.log     # 1 hour starting 2 hours ago

  'start' anchors to --since, 'end' anchors to --until
  Cannot use both anchors in the same command (e.g., --since end-1h --until start+1h)

  Common timestamp field names are auto-detected:
    ts, _ts, timestamp, at, time, @timestamp, log_timestamp, event_time,
    datetime, date_time, created_at, logged_at, _t, @t, t
  Events without valid timestamps are filtered out in resilient mode (default)
  Use --strict to abort processing on missing/invalid timestamps
  Use --verbose to see detailed timestamp parsing errors

For the full chrono format reference, see:
https://docs.rs/chrono/latest/chrono/format/strftime/index.html

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}

/// Print available Rhai functions help
fn print_functions_help() {
    let help_text = rhai_functions::docs::generate_help_text();
    println!("{}", help_text);
}

/// Print practical Rhai examples
fn print_examples_help() {
    let help_text = rhai_functions::docs::generate_examples_text();
    println!("{}", help_text);
}

fn print_quick_help() {
    let help_text = r#"Kelora - Scriptable log processor for the command line

Usage:
  kelora [OPTIONS] [FILES]...
  kelora [OPTIONS] < input.log
  kelora            # Run without arguments to enter interactive mode
  kelora --help     # Full CLI reference (all options)

Quick Examples:
  kelora -f logfmt -l error simple_logfmt.log
  kelora web_access_large.log.gz --stats
  kelora simple_json.jsonl --filter 'e.service == "database"' --exec 'e.duration_s = e.get_path("duration_ms", 0) / 1000' -k timestamp,message,duration_s
  kelora simple_json.jsonl --since 2024-01-15T10:01:00Z --until levels warn,error --stats
  kelora audit.jsonl --exec 'track_count(e.action)' --metrics
  kelora payments_latency.jsonl --parallel --filter 'e.duration_ms > 500' -k order_id,duration_ms,status
  tail -f app.log | kelora -j -l error,warn

Common Options:
  -f, --input-format <FORMAT>   Choose parser (auto, json, line, raw, logfmt, syslog, cef, csv, tsv, csvnh, tsvnh, combined, cols:<spec>, regex:<pattern>)
  -j                            Shortcut for -f json
  --filter <expr>               Keep events where expression is true (can repeat; run in the order given)
  -l, --levels <levels>         Keep only these log levels (comma-separated)
  -e, --exec <expr>             Transform events or emit metrics (can repeat; run in the order given)
  -k, --keys <fields>           Pick or reorder output fields
  -F, --output-format <FORMAT>  Output format (default/json/logfmt/inspect/levelmap/keymap/csv/tsv/csvnh/tsvnh)
  -q, --quiet                   Suppress event output (-s/--stats and -m/--metrics imply this)
  -n, --take <N>                Limit output to first N events
  -s, --stats                   Show only the statistics, with discovered fields
  -m, --metrics                 Show only the tracked metrics

Interactive Mode:
  kelora                     Run without arguments to enter interactive mode
                             (readline-based REPL with history, glob expansion,
                             and proper quoting - especially helpful on Windows)

More Help:
  kelora --help              Full CLI reference (all 100+ options grouped by category)
  kelora --help-rhai         Rhai language guide + stage semantics
  kelora --help-functions    Complete built-in function catalogue (150+ functions)
  kelora --help-examples     Common patterns and example walkthroughs
  kelora --help-formats      Format reference with extracted fields
  kelora --help-time         Timestamp format reference
  kelora --help-multiline    Multiline event strategies
  kelora --help-regex        Regex format parsing guide
"#;
    println!("{}", help_text);
}

/// Print Rhai scripting guide
fn print_rhai_help() {
    let help_text = r###"
Rhai Language Guide:

This guide covers Rhai language fundamentals for programmers familiar with Python, JavaScript, or Bash.
For Rhai language details: https://rhai.rs

VARIABLES & TYPES:
  let x = 42;                          Variable declaration (required for new vars)
  let name = "alice";                  String (double quotes only)
  let active = true;                   Boolean (true/false)
  let tags = [1, 2, 3];                Array (dynamic, mixed types ok)
  let user = #{name: "bob", age: 30};  Map/object literal
  let empty = ();                      Unit type (Rhai's "nothing", not null/undefined)

  type_of(x)                           Returns type as string: "i64", "string", "array", "map", "()"
  x = "hello";                         Dynamic typing: variables can change type

OPERATORS:
  Arithmetic:  +  -  *  /  %  **       (power: 2**3 == 8)
  Comparison:  ==  !=  <  >  <=  >=
  Logical:     &&  ||  !
  Bitwise:     &  |  ^  <<  >>
  Assignment:  =  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=
  Range:       1..5  1..=5            (exclusive/inclusive, for loops only)
  Membership:  "key" in map            (check map key existence)

STRING INTERPOLATION:
  Rhai supports string interpolation using ${...} syntax within backtick strings:

  let name = "Alice";
  let age = 30;
  let msg = `Hello, ${name}! You are ${age} years old.`;

  Complex expressions:
  let x = 10, y = 20;
  let result = `Sum: ${x + y}, Product: ${x * y}`;

  Nested interpolations allowed:
  let status = "active";
  let msg = `User ${name} is ${`currently ${status}`}`;

  Note: Interpolation only works with backtick strings (`text`), not double quotes ("text")

RAW STRINGS:
  Wrap strings with #"..."# to disable escape sequences (perfect for regexes):

  let regex = #"\d{3}-\d{2}-\d{4}"#;       No escaping needed (vs "\\d{3}-\\d{2}-\\d{4}")
  let path = #"C:\Users\data"#;            Windows paths work naturally
  let s = ##"Contains "quotes""##;         Use multiple # to include " inside

CONTROL FLOW:
  if x > 10 {                          If-else (braces required)
      print("big");
  } else if x > 5 {
      print("medium");
  } else {
      print("small");
  }

  switch x {                           Switch expression (returns value)
      1 => "one",
      2 | 3 => "two or three",
      4..=6 => "four to six",
      _ => "other"                     (underscore = default)
  }

LOOPS:
  for i in 0..10 { print(i); }         Range loop (0..10 excludes 10, 0..=10 includes)
  for item in array { print(item); }   Array iteration
  for (key, value) in map { ... }      Map iteration

  while condition { ... }              While loop
  loop { if done { break; } }          Infinite loop (use break/continue)

FUNCTIONS & CLOSURES:
  fn add(a, b) { a + b }               Function definition (last expr is return value)
  fn greet(name) {                     Explicit return
      return "Hello, " + name;
  }

  let double = |x| x * 2;              Closure syntax
  [1,2,3].map(|x| x * 2)               Common in array methods
  [1,2,3].filter(|x| x > 1)            Predicate closures

FUNCTION-AS-METHOD SYNTAX (Rhai special feature):
  extract_regex(e.line, "\d+")            Function call style
  e.line.extract_regex("\d+")             Method call style (same thing!)

  Rhai allows calling any function as a method on its first argument.
  Use method style for chaining: e.url.extract_domain().lower().strip()

RHAI QUIRKS & GOTCHAS:
  • Strings use double quotes only: "hello" (not 'hello')
  • Semicolons recommended (optional at end of blocks, required for multiple statements)
  • No null/undefined: use unit type () to represent "nothing"
  • No implicit type conversion: "5" + 3 is error (use "5".to_int() + 3)
  • try/catch available: try { ... } catch (err) { ... } catches runtime errors (type/type-mismatch, missing fields); compile errors still abort; prefer guards/to_int_or over exceptions for speed
  • let required for new variables (x = 1 errors if x not declared)
  • Arrays/maps are reference types: modifying copies affects original
  • Last expression in block is return value (no return needed)
  • Single-line comments: // ... (multi-line: /* ... */)
  • Function calls without parens ok if no args: e.len (same as e.len())

KELORA PIPELINE STAGES:
  --begin         Pre-run once before parsing; populate global `conf` map (becomes read-only)
  --filter        Boolean gate per event (true keeps, false drops); repeatable, ordered
  --exec / -e     Transform per event; repeatable, ordered
  --exec-file     Same as --exec, reads script from file
  --end           Post-run once after processing; access global `metrics` map for reports

Prerequisites: --allow-fs-writes (file I/O), --window N (windowing), --metrics (tracking)

VARIABLE SCOPE BETWEEN STAGES:
  Each --exec stage runs in ISOLATION. Local variables (let) do NOT persist:

  WRONG:  kelora -e 'let ctx = e.id' -e 'e.context = ctx'     # ERROR: ctx undefined!
  RIGHT:  kelora -e 'let ctx = e.id; e.context = ctx'         # Use semicolons for shared vars

  What persists:   e.field modifications, conf, metrics, window
  What doesn't:    let variables, function definitions (unless from --include)

RESILIENT MODE SNAPSHOTTING:
  Each successful stage creates a snapshot. On error, event reverts to last good state:

  kelora --resilient -e 'e.safe = "ok"' -e 'e.risky = parse(e.raw)' -e 'e.done = true'
  → If parse fails, event keeps 'safe' but not 'risky', continues with 'safe' field

  Why use multiple stages:
    - Error isolation (failures don't corrupt earlier work)
    - Progressive checkpoints (partial success possible)
  Why use semicolons in one stage:
    - Share local variables
    - All-or-nothing execution (no partial results)

KELORA EVENT ACCESS:
  e                                    Current event (global variable in --filter/--exec)
  e.field                              Direct field access
  e.nested.field                       Nested field traversal (maps)
  e.scores[1]                          Array indexing (0-based, negative ok: -1 = last)
  e.headers["user-agent"]              Bracket notation for special chars in keys

  "field" in e                         Check top-level field exists
  e.has_path("user.role")              Check nested path exists (safe)
  e.get_path("user.role", "guest")     Get nested with default fallback

  e.field = ()                         Remove field (unit assignment)
  e = ()                               Remove entire event (becomes empty, filtered out)

EVENT METADATA:
  meta                                 Event metadata (global variable in --filter/--exec)
  meta.line                            Original raw line from input (always available)
  meta.line_num                        Line number (1-based, available with files)
  meta.filename                        Source filename (available when processing multiple files)
  meta.parsed_ts                       Parsed UTC timestamp before scripts (or () if missing)

  # Example: Track errors by filename
  --exec 'if e.level == "ERROR" { track_count(meta.filename) }'

  # Example: Debug with line numbers
  --filter 'e.status >= 500' --exec 'eprint("Error at line " + meta.line_num)'

ARRAY & MAP OPERATIONS:
  JSON arrays → native Rhai arrays (full functionality)
  sorted(e.scores)                     Sort numerically/lexicographically
  reversed(e.items)                    Reverse order
  unique(e.tags)                       Remove duplicates
  sorted_by(e.users, "age")            Sort objects by field
  e.tags.join(", ")                    Join to string

  emit_each(e.items)                   Fan out: each array element → separate event
  emit_each(e.items, #{ctx: "x"})      Fan out with base fields added to each

COMMON PATTERNS:
  # Safe nested access
  let role = e.get_path("user.role", "guest");

  # Type conversion with fallback
  let port = to_int_or(e.port, 8080);

  # Array safety
  if e.items.len() > 0 { e.first = e.items[0]; }

  # Conditional field removal
  if e.level != "DEBUG" { e.stack_trace = (); }

  # Method chaining
  e.domain = e.url.extract_domain().to_lower().strip();

  # Map iteration
  for (key, val) in e { print(key + " = " + val); }

GLOBAL CONTEXT:
  state                                Mutable global map for complex state tracking (sequential mode only)
                                       Use for: deduplication, storing complex objects, cross-event logic
                                       For simple counting/metrics, prefer track_*() (works in parallel too)
                                       Supports: state["key"], contains(), get(), set(), len(), is_empty(),
                                       keys(), values(), clear(), remove(), +=, mixin(), fill_with()
                                       Use state.to_map() to convert to regular map for other operations
                                       (e.g., state.to_map().to_logfmt(), state.to_map().to_kv())
                                       Note: Accessing state in --parallel mode will cause a runtime error
  conf                                 Global config map (read-only after --begin)
  metrics                              Global metrics map (from track_* calls, read in --end)
  get_env("VAR", "default")            Environment variable access

ERROR HANDLING MODES:
  Default (resilient):
    • Parse errors → skip line, continue
    • Filter errors → treat as false, drop event
    • Exec errors → rollback, keep original event
  --strict mode:
    • Any error → abort with exit code 1

SCRIPT OUTPUT (print/eprint):
  print("msg")                        Write to stdout (visible by default)
  eprint("err")                       Write to stderr (visible by default)

  Suppression: --no-script-output, -s, -m suppress print/eprint
               --silent does NOT suppress print/eprint (they still work)

  File operations (always work, requires --allow-fs-writes):
    append_file(path, content), write_file(path, content), --metrics-file

For other help topics: kelora -h
"###;
    println!("{}", help_text);
}

/// Print multiline strategy help
fn print_multiline_help() {
    let help_text = r#"
Multiline Strategy Reference for --multiline:

Quick usage:
  kelora access.log --multiline timestamp
  kelora stack.log --multiline indent
  kelora trace.log --multiline regex:match=^TRACE
  kelora payload.json --multiline all

MODES:

timestamp
  Detect leading timestamps with Kelora's adaptive parser.
  Optional hint: --multiline timestamp:format='%b %e %H-%M-%S'

indent
  Treat any line that begins with indentation as a continuation.

regex:match=REGEX[:end=REGEX]
  Define record headers (and optional terminators) yourself.
  Example: --multiline regex:match=^BEGIN:end=^END

all
  Buffer the entire input as a single event.

NOTES:
- Multiline stays off unless you set -M/--multiline.
- Detection runs before parsing; pick -f raw/json/etc. as needed.
- Buffering continues until the next detected start or end arrives.
- With --parallel, tune --batch-size/--batch-timeout to keep memory bounded.
- Literal ':' characters are not supported inside the value today. Encode them in regex patterns (e.g. '\x3A') or normalise timestamp headers before parsing.

TROUBLESHOOTING:
- Use --stats or --metrics to watch buffered event counts.
- If buffers grow unbounded, tighten the regex or disable multiline temporarily.
- Remember that `--multiline all` reads the entire stream into memory.

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}

fn print_regex_help() {
    let help_text = r#"
Regex Format Parsing Reference for -f regex:PATTERN:

QUICK START:
  kelora app.log -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'
  kelora access.log -f 'regex:(?P<ip>\S+) - (?P<user>\S+) \[(?P<ts>[^\]]+)\]'
  kelora metrics.log -f 'regex:(?P<code:int>\d+) (?P<latency:float>[\d.]+)ms (?P<msg>.*)'

SYNTAX:

Pattern format:
  -f 'regex:PATTERN'

Named capture groups (REQUIRED):
  (?P<field_name>...)       Capture as string
  (?P<field:int>...)        Capture and convert to integer
  (?P<field:float>...)      Capture and convert to float
  (?P<field:bool>...)       Capture and convert to boolean

IMPORTANT NOTES:

Automatic anchoring:
  Kelora automatically adds ^ and $ anchors to your pattern.
  DON'T write:  -f 'regex:^pattern$'    (anchors will be doubled!)
  DO write:     -f 'regex:pattern'      (anchors added automatically)

Named groups required:
  All capture groups must be named with (?P<name>...).
  Regular unnamed groups (\d+) won't create fields.

Field names:
  Must contain only letters, numbers, and underscores.
  Reserved names: original_line, parsed_ts, fields

EXAMPLES:

Simple syslog-style log:
  kelora app.log -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'
  # Matches: Jan 15 10:00:00 INFO Application started

Apache combined log format:
  kelora access.log -f 'regex:(?P<ip>\S+) - (?P<user>\S+) \[(?P<ts>[^\]]+)\] "(?P<request>[^"]+)" (?P<status:int>\d+) (?P<bytes:int>\d+)'
  # Matches: 192.168.1.1 - alice [15/Jan/2025:10:00:00 +0000] "GET /api HTTP/1.1" 200 1234

Custom format with typed fields:
  kelora metrics.log -f 'regex:(?P<ts>\S+) \[(?P<level>\w+)\] (?P<code:int>\d+) (?P<duration:float>[\d.]+)ms (?P<msg>.+)'
  # Matches: 2025-01-15T10:00:00Z [ERROR] 500 123.45ms Internal error

Greedy vs. non-greedy matching:
  kelora data.log -f 'regex:(?P<date>\d{4}-\d{2}-\d{2}) (?P<msg>.*)'   # .* is greedy (matches to end)
  kelora data.log -f 'regex:(?P<key>\w+)=(?P<val>[^ ]+) (?P<rest>.*)'  # [^ ]+ stops at space

COMMON MISTAKES:

✗ Adding your own anchors:
  -f 'regex:^pattern$'                    # WRONG: Anchors doubled!
  -f 'regex:pattern'                      # CORRECT: Anchors added automatically

✗ Using unnamed groups:
  -f 'regex:(\d+) (\w+)'                  # WRONG: Groups must be named!
  -f 'regex:(?P<num>\d+) (?P<word>\w+)'   # CORRECT: Named groups required

✗ Wrong type annotation:
  -f 'regex:(?P<status:integer>\d+)'      # WRONG: Unknown type 'integer'
  -f 'regex:(?P<status:int>\d+)'          # CORRECT: Use 'int', 'float', or 'bool'

✗ Forgetting to escape special characters:
  -f 'regex:(?P<ip>\S+) [(?P<ts>.*)]'     # WRONG: [ needs escaping
  -f 'regex:(?P<ip>\S+) \[(?P<ts>.*)\]'   # CORRECT: Escape [ and ]

ALTERNATIVE: Use -f cols for simpler patterns!

For whitespace-delimited logs, cols: is often easier than regex:

Instead of regex:
  -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'

Use cols:
  -f 'cols:month day time level *msg'

The cols: format:
  - Splits on whitespace automatically
  - *field captures remaining line (like .* in regex)
  - Supports custom separators: --cols-sep=','
  - No need to worry about escaping special characters

Learn more: kelora --help (see --input-format examples)

DEBUGGING:

When patterns don't match:
  1. Use -vv to see detailed error messages
  2. Check for trailing newlines in error output
  3. Test pattern incrementally (start simple, add complexity)
  4. Verify pattern works in a regex tester (remember Kelora adds ^$)
  5. Consider using -f cols for simpler whitespace-delimited logs

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}

fn print_formats_help() {
    let help_text = r#"
Format Reference:

INPUT FORMATS:

Specify with -f, --input-format <format>

json (-j)
  JSON Lines format, one object per line
  Fields: All JSON keys preserved with types

line
  Plain text, one line per event
  Fields: line

logfmt
  Heroku-style key=value pairs
  Fields: All parsed keys

syslog
  RFC5424/RFC3164 system logs
  Fields: pri, facility, severity, level, ts, host, prog, pid, msg
          [msgid, version - RFC5424 only]

combined
  Apache/Nginx access logs (CLF, Combined, Nginx+request_time)
  Fields: ip, ts, method, path, protocol, status
          [identity, user, bytes, referer, agent, request_time]
  Note: Fields in brackets are optional (omitted if value is "-")

cef
  ArcSight Common Event Format
  Fields: cefver, vendor, product, version, eventid, event, severity
          [ts, host - from optional syslog prefix]
          + all extension key=value pairs become top-level fields

csv / tsv / csvnh / tsvnh
  Comma/tab-separated values, with/without headers
  Fields: Header names or c1, c2, c3...
  Type annotations: 'csv status:int bytes:int response_time:float'
  Supported types: int, float, bool

cols:<spec>
  Custom column-based parsing with whitespace or custom separator
  Fields: User-defined via spec
  Examples: 'cols:ts level *msg'
            'cols:ts(2) level *msg'  (ts consumes 2 tokens)
            'cols:name age:int city' --cols-sep '|'
  Tokens: field       - consume one column
          field(N)    - consume N columns and join
          -           - skip one column
          -(N)        - skip N columns
          *field      - capture rest of line (must be last)
          field:type  - apply type (int, float, bool, string)

regex:<pattern>
  Regular expression with named capture groups
  Fields: Named groups (?P<name>...) with optional type annotations
  Examples: 'regex:(?P<code:int>\d+) (?P<msg>.*)'
            'regex:(?P<ip>\S+) - - \[(?P<ts>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+)'
  Types: (?P<name:int>...), (?P<name:float>...), (?P<name:bool>...)
  Note: Pattern automatically anchored with ^...$

auto (default)
  Auto-detect format from first non-empty line
  Detection order: json → syslog → cef → combined → logfmt → csv → line
  Note: Detects once and applies to all lines

OUTPUT FORMATS:

Specify with -F, --output-format <format>

default   - Colored key-value format
json      - JSON Lines (one object per line)
logfmt    - Key-value pairs
inspect   - Debug format with type information
levelmap  - Compact visual with timestamps and level indicators
keymap    - Compact visual showing first character of specified field (-k/--keys required, exactly one field)
csv       - Comma-separated with header row
tsv       - Tab-separated with header row
csvnh     - CSV without header
tsvnh     - TSV without header

Use -q/--quiet to suppress output (implied by -s/--stats and -m/--metrics).

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorMode, EmojiMode};
    use rhai::Dynamic;

    fn base_config() -> KeloraConfig {
        let mut cfg = KeloraConfig::default();
        cfg.output.emoji = EmojiMode::Never;
        cfg.output.color = ColorMode::Never;
        cfg.processing.quiet_events = false;
        cfg.processing.silent = false;
        cfg.processing.suppress_diagnostics = false;
        cfg
    }

    #[test]
    fn detected_format_notice_for_non_line_format() {
        let cfg = base_config();
        let detected = DetectedFormat {
            format: config::InputFormat::Json,
            had_input: true,
        };

        let message =
            format_detected_format_notice(&cfg, &detected, true).expect("expected info notice");

        assert!(
            message.contains("Auto-detected format: json"),
            "message was {message}"
        );
    }

    #[test]
    fn parse_failure_warning_triggers_on_heavy_errors() {
        let cfg = base_config();
        let mut tracking = TrackingSnapshot::default();
        tracking.internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(10_i64),
        );
        tracking.internal.insert(
            "__kelora_stats_events_created".to_string(),
            Dynamic::from(0_i64),
        );

        let message = parse_failure_warning_message(&cfg, Some(&tracking), true, false, true)
            .expect("expected warning");

        assert!(
            message.contains("Parsing mostly failed"),
            "message was {message}"
        );
    }

    #[test]
    fn parse_failure_warning_skips_light_error_rates() {
        let cfg = base_config();
        let mut tracking = TrackingSnapshot::default();
        tracking.internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(2_i64),
        );
        tracking.internal.insert(
            "__kelora_stats_events_created".to_string(),
            Dynamic::from(10_i64),
        );

        assert!(
            parse_failure_warning_message(&cfg, Some(&tracking), true, false, true).is_none(),
            "should not warn on low error rate"
        );
    }
}
