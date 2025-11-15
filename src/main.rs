use anyhow::Result;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender};
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

/// Detect format from a peekable reader
/// Returns the detected format without consuming the first line
fn detect_format_from_peekable_reader<R: std::io::BufRead>(
    reader: &mut readers::PeekableLineReader<R>,
) -> Result<config::InputFormat> {
    match reader.peek_first_line()? {
        None => {
            // Empty input, default to line format
            Ok(config::InputFormat::Line)
        }
        Some(line) => {
            // Remove newline for detection
            let trimmed_line = line.trim_end_matches(&['\r', '\n'][..]);
            let detected = parsers::detect_format(trimmed_line)?;
            Ok(detected)
        }
    }
}

/// Detect format for parallel mode processing
/// Returns the detected format
fn detect_format_for_parallel_mode(
    files: &[String],
    no_input: bool,
) -> Result<config::InputFormat> {
    use std::io;

    if no_input {
        // For --no-input mode, default to Line format
        return Ok(config::InputFormat::Line);
    }

    if files.is_empty() {
        // For stdin with potential gzip/zstd, handle decompression first
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        let mut peekable_reader =
            readers::PeekableLineReader::new(io::BufReader::new(processed_stdin));

        match detect_format_from_peekable_reader(&mut peekable_reader)? {
            config::InputFormat::Auto => Ok(config::InputFormat::Line), // Fallback
            format => Ok(format),
        }
    } else {
        // For files, read first line from first file
        let sorted_files = pipeline::builders::sort_files(files, &config::FileOrder::Cli)?;

        if sorted_files.is_empty() {
            return Ok(config::InputFormat::Line);
        }

        let first_file = &sorted_files[0];
        let decompressed = decompression::DecompressionReader::new(first_file)?;
        let mut peekable_reader = readers::PeekableLineReader::new(decompressed);

        match detect_format_from_peekable_reader(&mut peekable_reader)? {
            config::InputFormat::Auto => Ok(config::InputFormat::Line), // Fallback
            format => Ok(format),
        }
    }
}

use parallel::{ParallelConfig, ParallelProcessor};
use pipeline::DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS;
use pipeline::{
    create_input_reader, create_pipeline_builder_from_config, create_pipeline_from_config,
};
use stats::{
    get_thread_stats, stats_add_error, stats_add_line_filtered, stats_add_line_output,
    stats_add_line_read, stats_finish_processing, stats_start_timer, ProcessingStats,
};
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Result of pipeline processing
#[derive(Debug)]
struct PipelineResult {
    pub stats: Option<ProcessingStats>,
    pub tracking_data: TrackingSnapshot,
}

/// Core pipeline processing function using KeloraConfig  
fn run_pipeline_with_kelora_config<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    // Start statistics collection if enabled
    if config.output.stats {
        stats_start_timer();
    }

    let use_parallel = config.should_use_parallel();

    if use_parallel && matches!(config.output.format, config::OutputFormat::Levelmap) {
        return Err(anyhow::anyhow!(
            "levelmap output format is not supported with --parallel or thread overrides"
        ));
    }

    if use_parallel {
        run_pipeline_parallel(config, output, ctrl_rx)
    } else {
        let mut output = output;
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
        })
    }
}

/// Run pipeline in parallel mode using KeloraConfig
fn run_pipeline_parallel<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    // Handle auto-detection for parallel mode
    let final_config = if matches!(config.input.format, config::InputFormat::Auto) {
        // For parallel mode, we need to detect format first
        let detected_format =
            detect_format_for_parallel_mode(&config.input.files, config.input.no_input)?;

        // Report detected format
        if config.processing.quiet_level == 0 {
            let format_name = format!("{:?}", detected_format).to_lowercase();
            let message =
                config.format_error_message(&format!("auto-detected format: {}", format_name));
            eprintln!("{}", message);
        }

        // Create new config with detected format
        let mut new_config = config.clone();
        new_config.input.format = detected_format;
        new_config
    } else {
        config.clone()
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
    let reader = create_input_reader(config)?;

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
    })
}

/// Run pipeline in sequential mode using KeloraConfig
fn run_pipeline_sequential<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
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
        SequentialInput::Files(readers::MultiFileReader::new(sorted_files)?)
    };

    run_pipeline_sequential_internal(config, output, ctrl_rx, input)
}

/// Run pipeline in sequential mode with auto-detection support
fn run_pipeline_sequential_with_auto_detection<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    if config.input.no_input {
        // For --no-input mode, skip auto-detection and use empty input with Line format
        let mut final_config = config.clone();
        final_config.input.format = config::InputFormat::Line;
        let input =
            SequentialInput::Stdin(Box::new(io::BufReader::new(io::Cursor::new(Vec::new()))));
        return run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input);
    }

    if config.input.files.is_empty() {
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        let mut peekable_reader =
            readers::PeekableLineReader::new(io::BufReader::new(processed_stdin));

        let detected_format = detect_format_from_peekable_reader(&mut peekable_reader)?;

        if config.processing.quiet_level == 0 {
            let format_name = format!("{:?}", detected_format).to_lowercase();
            let message =
                config.format_error_message(&format!("auto-detected format: {}", format_name));
            eprintln!("{}", message);
        }

        let mut final_config = config.clone();
        final_config.input.format = detected_format;

        let input = SequentialInput::Stdin(Box::new(peekable_reader));
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)
    } else {
        let sorted_files =
            pipeline::builders::sort_files(&config.input.files, &config.input.file_order)?;

        if sorted_files.is_empty() {
            return Ok(());
        }

        let first_file = &sorted_files[0];
        let detected_format = {
            let decompressed = decompression::DecompressionReader::new(first_file)?;
            let mut peekable_reader = readers::PeekableLineReader::new(decompressed);
            detect_format_from_peekable_reader(&mut peekable_reader)?
        };

        if config.processing.quiet_level == 0 {
            let format_name = format!("{:?}", detected_format).to_lowercase();
            let message =
                config.format_error_message(&format!("auto-detected format: {}", format_name));
            eprintln!("{}", message);
        }

        let mut final_config = config.clone();
        final_config.input.format = detected_format;
        let input = SequentialInput::Files(readers::MultiFileReader::new(sorted_files)?);
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)
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
    let mut gap_tracker = if config.output.format == crate::config::OutputFormat::None {
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
                                &current_stats.format_stats_for_signal(config.input.multiline.is_some())
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
                                &current_stats.format_stats_for_signal(config.input.multiline.is_some())
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
                ProcessingResult::TakeLimitExhausted => Ok(true),
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
                ProcessingResult::TakeLimitExhausted => Ok(true),
            }
        }
        ReaderMessage::Eof => Ok(true),
    }
}

/// Processing result for sequential pipeline
enum ProcessingResult {
    Continue,
    TakeLimitExhausted,
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
    if config.output.stats {
        stats_add_line_read();
    }

    // Skip the first N lines if configured (applied before ignore-lines and parsing)
    if *skipped_lines < config.input.skip_lines {
        *skipped_lines += 1;
        // Count skipped line for stats
        if config.output.stats {
            stats_add_line_filtered();
        }
        return Ok(ProcessingResult::Continue);
    }

    // Apply section selection if configured (filters out lines outside selected sections)
    if let Some(selector) = section_selector {
        if !selector.should_include_line(&line) {
            // Count filtered line for stats
            if config.output.stats {
                stats_add_line_filtered();
            }
            return Ok(ProcessingResult::Continue);
        }
    }

    // Apply keep-lines filter if configured (early filtering before parsing)
    if let Some(ref keep_regex) = config.input.keep_lines {
        if !keep_regex.is_match(&line) {
            // Count filtered line for stats
            if config.output.stats {
                stats_add_line_filtered();
            }
            return Ok(ProcessingResult::Continue);
        }
    }

    // Apply ignore-lines filter if configured (early filtering before parsing)
    if let Some(ref ignore_regex) = config.input.ignore_lines {
        if ignore_regex.is_match(&line) {
            // Count filtered line for stats
            if config.output.stats {
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
            if config.output.stats && !results.is_empty() {
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
            if config.output.stats {
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
    let mut config = KeloraConfig::from_cli(&cli)?;
    // Set the ordered stages directly
    config.processing.stages = ordered_stages;

    if config.processing.span.is_some()
        && config.processing.quiet_level == 0
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
            if events_per_span > 100_000 && config.processing.quiet_level == 0 {
                let warning = config.format_error_message(
                    "span size above 100000 may require substantial memory; consider time-based spans",
                );
                stderr.writeln(&warning).unwrap_or(());
            }
        }
    }

    // Set processed begin/end scripts with includes applied
    let (processed_begin, processed_end) = cli.get_processed_begin_end(&matches)?;
    config.processing.begin = processed_begin;
    config.processing.end = processed_end;

    // Parse timestamp filter arguments if provided
    if cli.since.is_some() || cli.until.is_some() {
        // Use the same timezone logic as the main configuration
        let cli_timezone = config.input.default_timezone.as_deref();

        // Check for anchor dependencies
        let since_uses_end_anchor = cli
            .since
            .as_ref()
            .is_some_and(|s| s.starts_with("end+") || s.starts_with("end-"));
        let until_uses_start_anchor = cli
            .until
            .as_ref()
            .is_some_and(|u| u.starts_with("start+") || u.starts_with("start-"));

        // Detect circular dependency
        if since_uses_end_anchor && until_uses_start_anchor {
            stderr
                .writeln(&config.format_error_message(
                    "Cannot use both 'start' and 'end' anchors: --since uses 'end' anchor and --until uses 'start' anchor"
                ))
                .unwrap_or(());
            ExitCode::InvalidUsage.exit();
        }

        let (since, until) = if until_uses_start_anchor {
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
        } else if since_uses_end_anchor {
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
            // Print metrics if enabled (only if not terminated)
            if config.output.metrics > 0 && !SHOULD_TERMINATE.load(Ordering::Relaxed) {
                let metrics_output = crate::rhai_functions::tracking::format_metrics_output(
                    &pipeline_result.tracking_data.user,
                    config.output.metrics,
                );
                if !metrics_output.is_empty() && metrics_output != "No metrics tracked" {
                    stderr
                        .writeln(&config.format_metrics_message(&metrics_output))
                        .unwrap_or(());
                }
            }

            // Print metrics as JSON to stderr if --metrics-json enabled
            if config.output.metrics_json && !SHOULD_TERMINATE.load(Ordering::Relaxed) {
                if let Ok(json_output) = crate::rhai_functions::tracking::format_metrics_json(
                    &pipeline_result.tracking_data.user,
                ) {
                    stderr
                        .writeln(&config.format_metrics_message(&json_output))
                        .unwrap_or(());
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

            // Print output based on configuration (only if not terminated)
            if !SHOULD_TERMINATE.load(Ordering::Relaxed) {
                if let Some(ref s) = pipeline_result.stats {
                    if config.output.stats && config.processing.quiet_level == 0 {
                        // Full stats when --stats flag is used (unless quiet level > 0)
                        stderr
                            .writeln(&config.format_stats_message(
                                &s.format_stats(config.input.multiline.is_some()),
                            ))
                            .unwrap_or(());
                    } else if config.processing.quiet_level == 0 {
                        // Error summary by default when errors occur (unless quiet level > 0)
                        if let Some(error_summary) =
                            crate::rhai_functions::tracking::extract_error_summary_from_tracking(
                                &pipeline_result.tracking_data,
                                config.processing.verbose,
                                pipeline_result.stats.as_ref(),
                                Some(&config),
                            )
                        {
                            stderr
                                .writeln(&config.format_error_message(&error_summary))
                                .unwrap_or(());
                        }
                    }
                }
            }
            (pipeline_result.stats, Some(pipeline_result.tracking_data))
        }
        Err(e) => {
            stderr
                .writeln(&config.format_error_message(&format!("Pipeline error: {}", e)))
                .unwrap_or(());
            ExitCode::GeneralError.exit();
        }
    };

    // Check if we were terminated by a signal and print output
    if TERMINATED_BY_SIGNAL.load(Ordering::Relaxed) {
        if let Some(stats) = final_stats {
            if config.output.stats && config.processing.quiet_level == 0 {
                // Full stats when --stats flag is used (unless quiet level > 0)
                stderr
                    .writeln(&config.format_stats_message(
                        &stats.format_stats(config.input.multiline.is_some()),
                    ))
                    .unwrap_or(());
            } else if stats.has_errors() && config.processing.quiet_level == 0 {
                // Error summary by default when errors occur (unless quiet level > 0)
                stderr
                    .writeln(&config.format_error_message(&stats.format_error_summary()))
                    .unwrap_or(());
            }
        } else if config.output.stats && config.processing.quiet_level == 0 {
            stderr
                .writeln(&config.format_stats_message("Processing interrupted"))
                .unwrap_or(());
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
    let mut had_errors = if let Some(ref tracking) = tracking_data {
        // Check tracking data for errors from processing
        crate::rhai_functions::tracking::has_errors_in_tracking(tracking)
    } else if let Some(ref stats) = final_stats {
        // Check stats for errors from parallel processing or termination case
        stats.has_errors()
    } else {
        // No processing results available, assume no errors
        false
    };

    if config.processing.strict && override_failed {
        if config.processing.quiet_level == 0 && !config.output.stats {
            if let Some(message) = override_message.clone() {
                stderr
                    .writeln(&config.format_error_message(&message))
                    .unwrap_or(());
            }
        }
        had_errors = true;
    }

    if had_errors {
        ExitCode::GeneralError.exit();
    } else {
        ExitCode::Success.exit();
    }
}

/// Validate CLI arguments for early error detection
fn validate_cli_args(cli: &Cli) -> Result<()> {
    // Validate --no-input conflicts
    if cli.no_input && !cli.files.is_empty() {
        return Err(anyhow::anyhow!(
            "--no-input cannot be used with input files"
        ));
    }

    // Check if input files exist (if specified), skip "-" which represents stdin
    let mut stdin_count = 0;
    for file_path in &cli.files {
        if file_path == "-" {
            stdin_count += 1;
            if stdin_count > 1 {
                return Err(anyhow::anyhow!("stdin (\"-\") can only be specified once"));
            }
        } else if !std::path::Path::new(file_path).exists() {
            return Err(anyhow::anyhow!("File not found: {}", file_path));
        }
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

    if cli.span_close.is_some() && cli.span.is_none() {
        return Err(anyhow::anyhow!(
            "--span-close requires --span to be specified"
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

/// Validate configuration for consistency
#[allow(dead_code)]
fn validate_config(config: &KeloraConfig) -> Result<()> {
    // Check if files exist (if specified)
    for file_path in &config.input.files {
        if !std::path::Path::new(file_path).exists() {
            return Err(anyhow::anyhow!("File not found: {}", file_path));
        }
    }

    // Validate batch size
    if let Some(batch_size) = config.performance.batch_size {
        if batch_size == 0 {
            return Err(anyhow::anyhow!("Batch size must be greater than 0"));
        }
    }

    // Validate thread count
    if config.performance.threads > 1000 {
        return Err(anyhow::anyhow!("Thread count too high (max 1000)"));
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

/// Handle --save-alias command
fn handle_save_alias(raw_args: &[String], alias_name: &str, no_emoji: bool) {
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
        let prefix = if no_emoji { "kelora:" } else { "⚠️" };
        eprintln!("{} No command to save as alias '{}'", prefix, alias_name);
        std::process::exit(2);
    }

    // Join the arguments back into a single string
    let alias_value = shell_words::join(command_args);

    // Save the alias to the specified config file or auto-detect
    let target_path = config_file_path.as_ref().map(std::path::Path::new);
    match ConfigFile::save_alias(alias_name, &alias_value, target_path) {
        Ok((config_path, previous_value)) => {
            let success_prefix = if no_emoji { "kelora:" } else { "🔹" };
            println!(
                "{} Alias '{}' saved to {}",
                success_prefix,
                alias_name,
                config_path.display()
            );

            if let Some(prev) = previous_value {
                let info_prefix = if no_emoji { "kelora:" } else { "🔹" };
                println!("{} Replaced previous alias:", info_prefix);
                println!("    {} = {}", alias_name, prev);
            }
        }
        Err(e) => {
            let error_prefix = if no_emoji { "kelora:" } else { "⚠️" };
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

    // Check for --help-quick (short cheat sheet)
    if raw_args.iter().any(|arg| arg == "--help-quick") {
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

    // Check for --save-alias before other processing
    if let Some(alias_name) = extract_save_alias_arg(&raw_args) {
        let no_emoji =
            raw_args.iter().any(|arg| arg == "--no-emoji") || std::env::var("NO_EMOJI").is_ok();
        handle_save_alias(&raw_args, &alias_name, no_emoji);
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

    // Show usage if on TTY and no input files provided (unless --no-input is specified)
    if crate::tty::is_stdin_tty() && cli.files.is_empty() && !cli.no_input {
        // Print brief usage with description and help hint
        println!("{}", Cli::command().render_usage());
        println!("A command-line log analysis tool with embedded Rhai scripting");
        println!("Try 'kelora --help' for more information.");
        std::process::exit(0);
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

  Common timestamp field names are auto-detected: ts, timestamp, time, @timestamp
  Events without valid timestamps are filtered out in resilient mode (default)
  Use --strict to abort processing on missing/invalid timestamps
  Use --verbose to see detailed timestamp parsing errors

For the full chrono format reference, see:
https://docs.rs/chrono/latest/chrono/format/strftime/index.html
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
    let help_text = r#"Kelora Quick Help

Workflows:
  kelora -f logfmt --level error examples/simple_logfmt.log
  kelora -j examples/simple_json.jsonl --filter 'e.service == "database"' --exec 'e.duration_s = e.get_path("duration_ms", 0) / 1000' -k timestamp,message,duration_s
  kelora -f combined examples/web_access_large.log.gz --parallel --stats

High-frequency flags:
  -f, --input-format <FORMAT>   Choose parser (json, logfmt, combined, cols:<spec>)
  --filter <expr>               Rhai boolean guard (repeatable)
  --level <levels>              Comma-separated log levels to include
  -e, --exec <expr>             Transform events or emit metrics
  -k, --keys <fields>           Pick or reorder output fields
  -F, --output-format <FORMAT>  Switch formatter (default, json, logfmt, levelmap, none)
  --stats / --metrics           Show throughput stats or tracked counters

Docs & references:
  kelora --help          Full CLI reference grouped by stage
  kelora --help-rhai     Rhai language guide + stage semantics
  kelora --help-functions  Built-in function catalogue
  https://github.com/dloss/kelora/blob/main/examples/README.md (Examples index)

Need the timestamp catalogue?  kelora --help-time
Multiline strategies?          kelora --help-multiline
"#;
    println!("{}", help_text);
}

/// Print Rhai scripting guide
fn print_rhai_help() {
    let help_text = r###"
Rhai Language Guide for Kelora:

This guide covers Rhai language fundamentals for programmers familiar with Python, JavaScript, or Bash.
For practical examples: kelora --help-examples
For complete function reference: kelora --help-functions
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
  extract_re(e.line, "\d+")            Function call style
  e.line.extract_re("\d+")             Method call style (same thing!)

  Rhai allows calling any function as a method on its first argument.
  Use method style for chaining: e.url.extract_domain().lower().strip()

RHAI QUIRKS & GOTCHAS:
  • Strings use double quotes only: "hello" (not 'hello')
  • Semicolons recommended (optional at end of blocks, required for multiple statements)
  • No null/undefined: use unit type () to represent "nothing"
  • No implicit type conversion: "5" + 3 is error (use "5".to_int() + 3)
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
  ⚠️  Each --exec stage runs in ISOLATION. Local variables (let) do NOT persist:

  WRONG:  kelora -e 'let ctx = e.id' -e 'e.context = ctx'     # ERROR: ctx undefined!
  RIGHT:  kelora -e 'let ctx = e.id; e.context = ctx'         # Use semicolons for shared vars

  What persists:   e.field modifications, conf, metrics, window
  What doesn't:    let variables, function definitions (unless from --include)

RESILIENT MODE SNAPSHOTTING:
  Each successful stage creates a snapshot. On error, event reverts to last good state:

  kelora --resilient -e 'e.safe = "ok"' -e 'e.risky = parse(e.raw)' -e 'e.done = true'
  → If parse fails, event keeps 'safe' but not 'risky', continues with 'safe' field

  Why use multiple stages:
    ✓ Error isolation (failures don't corrupt earlier work)
    ✓ Progressive checkpoints (partial success possible)
  Why use semicolons in one stage:
    ✓ Share local variables
    ✓ All-or-nothing execution (no partial results)

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

QUIET MODE SIDE EFFECTS:
  print("msg")                         Levels -q/-qq: visible, -qqq: suppressed
  eprint("err")                        Levels -q/-qq: visible, -qqq: suppressed
  File ops (append_file, etc.)         Always work (needs --allow-fs-writes)

See also:
  kelora --help-examples   Practical log analysis patterns
  kelora --help-functions  Complete function catalogue
  kelora --help-time       Timestamp format reference
  https://rhai.rs          Full Rhai language documentation
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

For complete CLI reference: kelora --help
For Rhai scripting help: kelora --help-rhai
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

For complete CLI reference: kelora --help
For timestamp parsing help: kelora --help-time
For Rhai scripting help: kelora --help-rhai
"#;
    println!("{}", help_text);
}
