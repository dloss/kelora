//! Pipeline execution module
//!
//! This module handles running the log processing pipeline in both
//! sequential and parallel modes.

use anyhow::Result;
use crossbeam_channel::{bounded, select, Receiver, Sender};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{self, KeloraConfig};
use crate::decompression;
use crate::detection::{self, DetectedFormat};
use crate::engine::RhaiEngine;
use crate::parallel::{ParallelConfig, ParallelProcessor};
use crate::parsers;
use crate::parsers::type_conversion::TypeMap;
use crate::pipeline::{
    self, create_input_reader, create_pipeline_builder_from_config, create_pipeline_from_config,
    DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS,
};
use crate::platform::{Ctrl, SafeStderr};
use crate::readers;
use crate::rhai_functions::file_ops::{self, FileOpMode};
use crate::rhai_functions::tracking::{self, TrackingSnapshot};
use crate::stats::{
    get_thread_stats, set_collect_stats, stats_add_error, stats_add_line_filtered,
    stats_add_line_output, stats_add_line_read, stats_finish_processing, stats_start_timer,
    ProcessingStats,
};
use crate::{rhai_functions, stats};

const LINE_CHANNEL_BOUND: usize = 1024;

/// Result of pipeline processing
pub struct PipelineResult {
    pub stats: Option<ProcessingStats>,
    pub tracking_data: TrackingSnapshot,
    pub auto_detected_non_line: bool,
    pub field_discovery: Option<crate::field_discovery::FieldDiscovery>,
}

/// Core pipeline processing function using KeloraConfig
pub fn run_pipeline_with_kelora_config<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    crate::drain::reset();

    // Enable field discovery if requested
    if config.output.discover_fields.is_some() {
        crate::field_discovery::enable(config.output.discover_final);
    }

    // Enable/disable stats collection up front to avoid per-event overhead when diagnostics are off
    let collect_stats = config.output.stats.is_some()
        || config.output.discover_fields.is_some()
        || (!config.processing.silent && !config.processing.suppress_diagnostics);
    set_collect_stats(collect_stats);

    // Start statistics collection if enabled
    if collect_stats {
        stats_start_timer();
        // Set the initial format in stats (may be updated if auto-detected later)
        if !matches!(config.input.format, config::InputFormat::AutoPerFile) {
            stats::stats_set_detected_format(config.input.format.to_display_string());
        }
    }

    let use_parallel = config.should_use_parallel();

    if use_parallel && config.output.drain.is_some() {
        return Err(anyhow::anyhow!(
            "--drain summary is not supported with --parallel or thread overrides. Rerun without --parallel to use Drain template mining."
        ));
    }

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

    if use_parallel && matches!(config.output.format, config::OutputFormat::Tailmap) {
        return Err(anyhow::anyhow!(
            "tailmap output format is not supported with --parallel or thread overrides"
        ));
    }

    if use_parallel && config.output.discover_fields.is_some() {
        return Err(anyhow::anyhow!(
            "--discover is not supported with --parallel or thread overrides. Rerun without --parallel."
        ));
    }

    if use_parallel && config.input.merge_ts {
        return Err(anyhow::anyhow!(
            "--merge-sorted is not supported with --parallel or thread overrides. Rerun without --parallel."
        ));
    }

    if use_parallel && matches!(config.input.format, config::InputFormat::AutoPerFile) {
        return Err(anyhow::anyhow!(
            "-f auto-per-file is not supported with --parallel or thread overrides. Rerun without --parallel."
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

        let field_discovery = if crate::field_discovery::is_enabled() {
            Some(crate::field_discovery::take_thread_discovery())
        } else {
            None
        };

        Ok(PipelineResult {
            stats: final_stats,
            tracking_data,
            auto_detected_non_line,
            field_discovery,
        })
    }
}

/// Run pipeline in parallel mode using KeloraConfig
fn run_pipeline_parallel<W: Write + Send + 'static>(
    config: &KeloraConfig,
    output: W,
    ctrl_rx: &Receiver<Ctrl>,
) -> Result<PipelineResult> {
    crate::rhai_functions::inter_record::reset_state();
    let terminal_output = std::io::stderr().is_terminal();

    // Handle auto-detection for parallel mode
    let (final_config, auto_detected_non_line, detected_reader) =
        if matches!(config.input.format, config::InputFormat::Auto) {
            // For parallel mode, we need to detect format first
            let (detected_format, detected_reader) = detection::detect_format_for_parallel_mode(
                &config.input.files,
                config.input.no_input,
                config.processing.strict,
            )?;

            detection::emit_detected_format_notice(config, &detected_format, terminal_output);

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
        field_discovery: None, // Not supported in parallel mode
    })
}

enum SequentialInput {
    Stdin(Box<dyn BufRead + Send>),
    Files(Vec<String>),
    MergedFiles(MergedFileReader),
}

struct MergedFileReader {
    files: Vec<String>,
    format: config::InputFormat,
    strict: bool,
    cols_sep: Option<String>,
    extract_prefix: Option<String>,
    prefix_sep: String,
    ts_field: Option<String>,
    ts_format: Option<String>,
    default_timezone: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct MergeKey {
    timestamp: chrono::DateTime<chrono::Utc>,
    file_index: usize,
    line_number: usize,
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct MergeState {
    file_index: usize,
    line: String,
}

enum MergeTimestampParser {
    Generic(Box<dyn pipeline::EventParser>),
}

enum MergeTimestampResult {
    SkipLine,
    MissingTimestamp,
    Timestamp(chrono::DateTime<chrono::Utc>),
}

enum ReaderMessage {
    FormatDetected {
        detected: DetectedFormat,
    },
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

/// Run pipeline in sequential mode using KeloraConfig
fn run_pipeline_sequential<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<(config::InputFormat, bool)> {
    if matches!(config.input.format, config::InputFormat::Auto) {
        return run_pipeline_sequential_with_auto_detection(config, output, ctrl_rx);
    }
    if matches!(config.input.format, config::InputFormat::AutoPerFile)
        && (config.input.no_input || config.input.files.is_empty())
    {
        let mut auto_config = config.clone();
        auto_config.input.format = config::InputFormat::Auto;
        return run_pipeline_sequential_with_auto_detection(&auto_config, output, ctrl_rx);
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
        if config.input.merge_ts {
            SequentialInput::MergedFiles(MergedFileReader {
                files: sorted_files,
                format: config.input.format.clone(),
                strict: config.processing.strict,
                cols_sep: config.input.cols_sep.clone(),
                extract_prefix: config.input.extract_prefix.clone(),
                prefix_sep: config.input.prefix_sep.clone(),
                ts_field: config.input.ts_field.clone(),
                ts_format: config.input.ts_format.clone(),
                default_timezone: config.input.default_timezone.clone(),
            })
        } else {
            SequentialInput::Files(sorted_files)
        }
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

        let detected_format = detection::detect_format_from_peekable_reader(&mut peekable_reader)?;

        detection::emit_detected_format_notice(config, &detected_format, terminal_output);

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
                    detected_format = Some(detection::detect_format_from_peekable_reader(
                        &mut peekable_reader,
                    )?);
                    break;
                }
                Err(e) => {
                    if config.processing.strict {
                        return Err(anyhow::anyhow!(config::format_input_open_error(
                            file_path,
                            &e.to_string()
                        )));
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
                        crate::config::format_error_message_auto(&config::format_input_open_error(
                            &path, &err
                        ),)
                    );
                    stats::stats_file_open_failed(&path);
                }
                return Err(anyhow::anyhow!(
                    "Failed to open any input files for detection"
                ));
            }
        };

        detection::emit_detected_format_notice(config, &detected_format, terminal_output);

        let mut final_config = config.clone();
        final_config.input.format = detected_format.format.clone();

        // Set detected format in stats if stats are enabled
        if config.output.stats.is_some() {
            stats::stats_set_detected_format(final_config.input.format.to_display_string());
        }

        let input = if final_config.input.merge_ts {
            SequentialInput::MergedFiles(MergedFileReader {
                files: sorted_files,
                format: final_config.input.format.clone(),
                strict: final_config.processing.strict,
                cols_sep: final_config.input.cols_sep.clone(),
                extract_prefix: final_config.input.extract_prefix.clone(),
                prefix_sep: final_config.input.prefix_sep.clone(),
                ts_field: final_config.input.ts_field.clone(),
                ts_format: final_config.input.ts_format.clone(),
                default_timezone: final_config.input.default_timezone.clone(),
            })
        } else {
            SequentialInput::Files(sorted_files)
        };
        run_pipeline_sequential_internal(&final_config, output, ctrl_rx, input)?;

        Ok((
            final_config.input.format,
            detected_format.detected_non_line(),
        ))
    }
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

fn spawn_file_reader_auto_per_file(
    files: Vec<String>,
    strict: bool,
    config: KeloraConfig,
    sender: Sender<ReaderMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let terminal_output = std::io::stderr().is_terminal();
        for file_path in files {
            match ctrl_rx.try_recv() {
                Ok(Ctrl::Shutdown { immediate }) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    if immediate {
                        return Ok(());
                    }
                    break;
                }
                Ok(Ctrl::PrintStats) | Err(_) => {}
            }

            let Some(reader) = readers::open_input_reader(&file_path, 256 * 1024, strict)? else {
                continue;
            };

            let mut peekable_reader = readers::PeekableLineReader::new(reader);
            let detected = detection::detect_format_from_peekable_reader(&mut peekable_reader)?;

            detection::emit_detected_format_notice(&config, &detected, terminal_output);

            if sender
                .send(ReaderMessage::FormatDetected {
                    detected: detected.clone(),
                })
                .is_err()
            {
                return Ok(());
            }

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
                    Ok(Ctrl::PrintStats) | Err(_) => {}
                }

                buffer.clear();
                match peekable_reader.read_line(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = buffer.trim_end_matches(&['\n', '\r'][..]).to_string();
                        if sender
                            .send(ReaderMessage::Line {
                                line,
                                filename: Some(file_path.clone()),
                            })
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    Err(error) => {
                        if sender
                            .send(ReaderMessage::Error {
                                error,
                                filename: Some(file_path.clone()),
                            })
                            .is_err()
                        {
                            return Ok(());
                        }
                        break;
                    }
                }
            }
        }

        let _ = sender.send(ReaderMessage::Eof);
        Ok(())
    })
}

fn build_simple_merge_parser(
    format: &config::InputFormat,
    strict: bool,
    cols_sep: Option<String>,
) -> Result<Box<dyn pipeline::EventParser>> {
    let parser: Box<dyn pipeline::EventParser> = match format {
        config::InputFormat::Json => {
            Box::new(crate::parsers::JsonlParser::new().with_strict(strict))
        }
        config::InputFormat::Line => Box::new(crate::parsers::LineParser::new()),
        config::InputFormat::Raw => Box::new(crate::parsers::RawParser::new()),
        config::InputFormat::Logfmt => Box::new(crate::parsers::LogfmtParser::new()),
        config::InputFormat::Syslog => Box::new(crate::parsers::SyslogParser::new()?),
        config::InputFormat::Cef => Box::new(crate::parsers::CefParser::new().with_strict(strict)),
        config::InputFormat::Combined => Box::new(crate::parsers::CombinedParser::new()?),
        config::InputFormat::Cols(spec) => {
            Box::new(crate::parsers::ColsParser::new(spec.clone(), cols_sep).with_strict(strict))
        }
        config::InputFormat::Regex(pattern) => {
            Box::new(crate::parsers::RegexParser::new(pattern)?.with_strict(strict))
        }
        config::InputFormat::Cascade(formats) => {
            let mut entries: Vec<(String, Box<dyn pipeline::EventParser>)> = Vec::new();
            for fmt in formats {
                let inner = build_simple_merge_parser(fmt, strict, cols_sep.clone())?;
                entries.push((fmt.cascade_name().to_string(), inner));
            }
            Box::new(crate::parsers::CascadingParser::new(entries))
        }
        config::InputFormat::Auto => {
            return Err(anyhow::anyhow!(
                "--merge-sorted requires a concrete input format after auto-detection"
            ));
        }
        config::InputFormat::AutoPerFile => {
            return Err(anyhow::anyhow!(
                "--merge-sorted is not supported with -f auto-per-file"
            ));
        }
        config::InputFormat::Csv(_)
        | config::InputFormat::Tsv(_)
        | config::InputFormat::Csvnh
        | config::InputFormat::Tsvnh => {
            return Err(anyhow::anyhow!(
                "unexpected CSV family format in generic parser builder"
            ));
        }
    };
    Ok(parser)
}

fn build_merge_timestamp_parser(
    format: &config::InputFormat,
    strict: bool,
    cols_sep: Option<String>,
) -> Result<MergeTimestampParser> {
    let parser = match format {
        config::InputFormat::Csv(_)
        | config::InputFormat::Tsv(_)
        | config::InputFormat::Csvnh
        | config::InputFormat::Tsvnh => {
            return Err(anyhow::anyhow!(
                "--merge-sorted is not yet supported for CSV/TSV formats (header semantics across merged files)"
            ));
        }
        other => MergeTimestampParser::Generic(build_simple_merge_parser(other, strict, cols_sep)?),
    };
    Ok(parser)
}

fn preprocess_merge_line(line: &str, extract_prefix: Option<&str>, prefix_sep: &str) -> String {
    if extract_prefix.is_some() {
        if let Some((_, rest)) = line.split_once(prefix_sep) {
            return rest.to_string();
        }
    }
    line.to_string()
}

fn parse_merge_timestamp(
    parser: &mut MergeTimestampParser,
    line: &str,
    ts_config: &crate::timestamp::TsConfig,
) -> Result<MergeTimestampResult> {
    let mut event = match parser {
        MergeTimestampParser::Generic(parser) => parser.parse(line)?,
    };

    if event
        .fields
        .get("__skip__")
        .and_then(|v| v.clone().try_cast::<bool>())
        .unwrap_or(false)
    {
        return Ok(MergeTimestampResult::SkipLine);
    }

    // Force timestamp extraction with merge-time configuration.
    event.parsed_ts = None;
    event.extract_timestamp_with_config(None, ts_config);
    match event.parsed_ts {
        Some(timestamp) => Ok(MergeTimestampResult::Timestamp(timestamp)),
        None => Ok(MergeTimestampResult::MissingTimestamp),
    }
}

fn emit_merge_fatal_error(sender: &Sender<ReaderMessage>, message: String) -> bool {
    sender
        .send(ReaderMessage::Error {
            error: io::Error::other(message),
            filename: None,
        })
        .is_ok()
}

fn missing_merge_timestamp_message(filename: &str, line_number: usize) -> String {
    format!(
        "--merge-sorted requires a timestamp for every event in '{}' at line {}. Hint: Specify --ts-field <field> if your timestamps use a non-default field name.",
        filename, line_number
    )
}

fn parse_merge_failure_message(
    filename: &str,
    line_number: usize,
    error: &anyhow::Error,
) -> String {
    format!(
        "failed to parse line for --merge-sorted in '{}' at line {}: {}",
        filename, line_number, error
    )
}

fn empty_merge_file_message(filename: &str) -> String {
    format!(
        "--merge-sorted requires each input file to produce at least one timestamped event; '{}' did not.",
        filename
    )
}

fn spawn_merged_file_reader(
    reader: MergedFileReader,
    sender: Sender<ReaderMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let mut readers: Vec<(
            BufReader<decompression::DecompressionReader>,
            MergeTimestampParser,
            usize,
        )> = Vec::new();
        for file in &reader.files {
            let decompressed = decompression::DecompressionReader::new(file)?;
            readers.push((
                BufReader::new(decompressed),
                build_merge_timestamp_parser(
                    &reader.format,
                    reader.strict,
                    reader.cols_sep.clone(),
                )?,
                0,
            ));
        }

        let mut heap: BinaryHeap<Reverse<(MergeKey, MergeState)>> = BinaryHeap::new();
        let mut previous_timestamps: Vec<Option<chrono::DateTime<chrono::Utc>>> =
            vec![None; readers.len()];
        let ts_config = crate::timestamp::TsConfig {
            custom_field: reader.ts_field.clone(),
            custom_format: reader.ts_format.clone(),
            default_timezone: reader.default_timezone.clone(),
        };
        let extract_prefix = reader.extract_prefix.as_deref();

        for (file_index, (file_reader, parser, line_number)) in readers.iter_mut().enumerate() {
            loop {
                let mut line = String::new();
                let read = file_reader.read_line(&mut line)?;
                if read == 0 {
                    if previous_timestamps[file_index].is_none()
                        && !emit_merge_fatal_error(
                            &sender,
                            empty_merge_file_message(&reader.files[file_index]),
                        )
                    {
                        return Ok(());
                    }
                    break;
                }
                *line_number += 1;
                let line = line.trim_end_matches(&['\n', '\r'][..]).to_string();
                let parse_line = preprocess_merge_line(&line, extract_prefix, &reader.prefix_sep);
                match parse_merge_timestamp(parser, &parse_line, &ts_config) {
                    Ok(MergeTimestampResult::Timestamp(timestamp)) => {
                        let key = MergeKey {
                            timestamp,
                            file_index,
                            line_number: *line_number,
                        };
                        let state = MergeState { file_index, line };
                        previous_timestamps[file_index] = Some(timestamp);
                        heap.push(Reverse((key, state)));
                        break;
                    }
                    Ok(MergeTimestampResult::SkipLine) => {
                        // Header/skip line for this format; continue scanning.
                        continue;
                    }
                    Ok(MergeTimestampResult::MissingTimestamp) => {
                        if !emit_merge_fatal_error(
                            &sender,
                            missing_merge_timestamp_message(
                                &reader.files[file_index],
                                *line_number,
                            ),
                        ) {
                            return Ok(());
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        if !emit_merge_fatal_error(
                            &sender,
                            parse_merge_failure_message(
                                &reader.files[file_index],
                                *line_number,
                                &e,
                            ),
                        ) {
                            return Ok(());
                        }
                        return Ok(());
                    }
                }
            }
        }

        while let Some(Reverse((_key, state))) = heap.pop() {
            match ctrl_rx.try_recv() {
                Ok(Ctrl::Shutdown { immediate }) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    if immediate {
                        return Ok(());
                    }
                    break;
                }
                Ok(Ctrl::PrintStats) => {}
                Err(_) => {}
            }

            let filename = reader.files[state.file_index].clone();
            if sender
                .send(ReaderMessage::Line {
                    line: state.line,
                    filename: Some(filename.clone()),
                })
                .is_err()
            {
                break;
            }

            let (file_reader, parser, line_number) = &mut readers[state.file_index];
            loop {
                let mut next_line = String::new();
                let read = file_reader.read_line(&mut next_line)?;
                if read == 0 {
                    break;
                }
                *line_number += 1;
                let next_line = next_line.trim_end_matches(&['\n', '\r'][..]).to_string();
                let parse_line =
                    preprocess_merge_line(&next_line, extract_prefix, &reader.prefix_sep);
                match parse_merge_timestamp(parser, &parse_line, &ts_config) {
                    Ok(MergeTimestampResult::Timestamp(timestamp)) => {
                        if let Some(previous) = previous_timestamps[state.file_index] {
                            if timestamp < previous
                                && !emit_merge_fatal_error(
                                    &sender,
                                    format!(
                                        "input is not sorted by timestamp: event at '{}' line {} is earlier than the previous event ({} < {})",
                                        filename, *line_number, timestamp, previous
                                    ),
                                )
                            {
                                return Ok(());
                            }
                            if timestamp < previous {
                                return Ok(());
                            }
                        }
                        let key = MergeKey {
                            timestamp,
                            file_index: state.file_index,
                            line_number: *line_number,
                        };
                        let next_state = MergeState {
                            file_index: state.file_index,
                            line: next_line,
                        };
                        previous_timestamps[state.file_index] = Some(timestamp);
                        heap.push(Reverse((key, next_state)));
                        break;
                    }
                    Ok(MergeTimestampResult::SkipLine) => continue,
                    Ok(MergeTimestampResult::MissingTimestamp) => {
                        if !emit_merge_fatal_error(
                            &sender,
                            missing_merge_timestamp_message(&filename, *line_number),
                        ) {
                            return Ok(());
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        if !emit_merge_fatal_error(
                            &sender,
                            parse_merge_failure_message(&filename, *line_number, &e),
                        ) {
                            return Ok(());
                        }
                        return Ok(());
                    }
                }
            }
        }

        let _ = sender.send(ReaderMessage::Eof);
        Ok(())
    })
}

fn run_pipeline_sequential_internal<W: Write>(
    config: &KeloraConfig,
    output: &mut W,
    ctrl_rx: Receiver<Ctrl>,
    input: SequentialInput,
) -> Result<()> {
    crate::rhai_functions::inter_record::reset_state();
    let (mut pipeline, begin_stage, end_stage, mut ctx) = create_pipeline_from_config(config)?;

    file_ops::set_mode(FileOpMode::Sequential);

    if let Err(e) = begin_stage.execute(&mut ctx) {
        return Err(anyhow::anyhow!("Begin stage error: {}", e));
    }

    let (line_tx, line_rx) = bounded::<ReaderMessage>(LINE_CHANNEL_BOUND);
    let reader_ctrl = ctrl_rx.clone();
    let reader_handle = match input {
        SequentialInput::Stdin(reader) => spawn_stdin_reader(reader, line_tx, reader_ctrl),
        SequentialInput::Files(files) => {
            if matches!(config.input.format, config::InputFormat::AutoPerFile) {
                spawn_file_reader_auto_per_file(
                    files,
                    config.processing.strict,
                    config.clone(),
                    line_tx,
                    reader_ctrl,
                )
            } else {
                let reader = readers::MultiFileReader::new(files, config.processing.strict)?;
                spawn_file_reader(reader, line_tx, reader_ctrl)
            }
        }
        SequentialInput::MergedFiles(reader) => {
            spawn_merged_file_reader(reader, line_tx, reader_ctrl)
        }
    };

    let multiline_timeout = config
        .input
        .multiline
        .as_ref()
        .map(|_| Duration::from_millis(DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS));

    let mut current_csv_headers: Option<Vec<String>> = None;
    let mut current_csv_type_map: Option<TypeMap> = None;
    let mut last_filename: Option<String> = None;
    let mut current_input_format = config.input.format.clone();
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
    crate::rhai_functions::formatting::set_colors_enabled(gap_marker_use_colors);
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
                                    current_csv_type_map: &mut current_csv_type_map,
                                    last_filename: &mut last_filename,
                                    current_input_format: &mut current_input_format,
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
                                    current_csv_type_map: &mut current_csv_type_map,
                                    last_filename: &mut last_filename,
                                    current_input_format: &mut current_input_format,
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
    current_csv_type_map: &'a mut Option<TypeMap>,
    last_filename: &'a mut Option<String>,
    current_input_format: &'a mut config::InputFormat,
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
        current_csv_type_map,
        last_filename,
        current_input_format,
        gap_tracker,
    } = ctx;
    match message {
        ReaderMessage::FormatDetected { detected } => {
            let results = pipeline.flush(pipeline_ctx)?;
            for formatted in results {
                write_formatted_output(formatted, output, gap_tracker)?;
            }

            *current_csv_headers = None;
            *current_csv_type_map = None;
            *last_filename = None;
            *current_input_format = detected.format.clone();

            if config.output.stats.is_some() {
                stats::stats_add_detected_format_hit(&detected.format.to_display_string());
            }

            replace_pipeline_parser(pipeline, pipeline_ctx, config, &detected.format, None, None)?;

            Ok(false)
        }
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
                current_csv_type_map,
                last_filename,
                current_input_format,
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
                current_csv_type_map,
                last_filename,
                current_input_format,
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

fn replace_pipeline_parser(
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    config: &KeloraConfig,
    input_format: &config::InputFormat,
    csv_headers: Option<Vec<String>>,
    csv_type_map: Option<TypeMap>,
) -> Result<()> {
    let mut pipeline_builder =
        create_pipeline_builder_from_config(config).with_input_format(input_format.clone());
    if let Some(headers) = csv_headers {
        pipeline_builder = pipeline_builder.with_csv_headers(headers);
    }
    if let Some(type_map) = csv_type_map {
        pipeline_builder = pipeline_builder.with_csv_type_map(type_map);
    }

    pipeline.parser = pipeline_builder.build_parser()?;
    ctx.config.format_name = Some(input_format.to_display_string());
    Ok(())
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
    current_csv_type_map: &mut Option<TypeMap>,
    last_filename: &mut Option<String>,
    current_input_format: &mut config::InputFormat,
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

    let effective_input_format = current_input_format.clone();

    if line.trim().is_empty() {
        // Only skip empty lines for structured formats, not for line format
        if !matches!(effective_input_format, config::InputFormat::Line) {
            return Ok(ProcessingResult::Continue);
        }
        // For line format, continue processing the empty line
    }

    // For CSV formats, detect file changes and reinitialize parser, or handle first line for stdin
    if matches!(
        effective_input_format,
        config::InputFormat::Csv(_)
            | config::InputFormat::Tsv(_)
            | config::InputFormat::Csvnh
            | config::InputFormat::Tsvnh
    ) && (current_filename != *last_filename
        || (current_filename.is_none() && current_csv_headers.is_none()))
    {
        // File changed, reinitialize CSV parser for this file
        let mut temp_parser = match &effective_input_format {
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
        let type_map = temp_parser.get_type_map();
        *current_csv_headers = Some(headers.clone());
        *current_csv_type_map = if type_map.is_empty() {
            None
        } else {
            Some(type_map)
        };
        *last_filename = current_filename.clone();

        replace_pipeline_parser(
            pipeline,
            ctx,
            config,
            &effective_input_format,
            Some(headers),
            current_csv_type_map.clone(),
        )?;

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
