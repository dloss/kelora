//! Batcher thread logic for parallel processing
//!
//! Contains batch creation, line filtering, and batch sending functions.

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::platform::Ctrl;

use super::tracker::GlobalTracker;
use super::types::{
    Batch, BatcherThreadConfig, FileAwareLineContext, LineMessage, PlainLineContext,
};

/// Plain IO reader thread - reads from stdin or a single reader
pub(crate) fn plain_io_reader_thread<R: std::io::BufRead>(
    mut reader: R,
    line_sender: Sender<LineMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    let mut buffer = String::new();
    loop {
        if let Ok(Ctrl::Shutdown { immediate }) = ctrl_rx.try_recv() {
            let _ = line_sender.send(LineMessage::Eof);
            if immediate {
                return Ok(());
            }
            break;
        }

        buffer.clear();
        match reader.read_line(&mut buffer) {
            Ok(0) => {
                let _ = line_sender.send(LineMessage::Eof);
                break;
            }
            Ok(_) => {
                let line = buffer.trim_end().to_string();
                if line_sender
                    .send(LineMessage::Line {
                        line,
                        filename: None,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                let _ = line_sender.send(LineMessage::Error {
                    error: e,
                    filename: None,
                });
                break;
            }
        }
    }
    Ok(())
}

/// File-aware IO reader thread - reads from multiple files with filename tracking
pub(crate) fn file_aware_io_reader_thread(
    mut reader: Box<dyn crate::readers::FileAwareRead>,
    line_sender: Sender<LineMessage>,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    let mut buffer = String::new();
    loop {
        match ctrl_rx.try_recv() {
            Ok(Ctrl::Shutdown { immediate }) => {
                let _ = line_sender.send(LineMessage::Eof);
                if immediate {
                    return Ok(());
                }
                break;
            }
            Ok(Ctrl::PrintStats) => {
                // File reader thread doesn't have stats to print, ignore
            }
            Err(_) => {
                // No message, continue
            }
        }

        buffer.clear();
        match reader.read_line(&mut buffer) {
            Ok(0) => {
                let _ = line_sender.send(LineMessage::Eof);
                break;
            }
            Ok(_) => {
                let line = buffer.trim_end().to_string();
                let filename = reader.current_filename().map(|s| s.to_string());
                if line_sender
                    .send(LineMessage::Line { line, filename })
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                let filename = reader.current_filename().map(|s| s.to_string());
                let _ = line_sender.send(LineMessage::Error { error: e, filename });
                break;
            }
        }
    }
    Ok(())
}

/// Batcher thread - collects lines into batches for parallel processing
pub(crate) fn batcher_thread(
    line_receiver: Receiver<LineMessage>,
    config: BatcherThreadConfig,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    let mut batch_id = 0u64;
    let mut current_batch = Vec::with_capacity(config.batch_size);
    let mut line_num = config.preprocessing_line_count;
    let mut batch_start_line = 1usize;
    let mut pending_deadline: Option<Instant> = None;
    let mut skipped_lines_count = 0usize;
    let mut filtered_lines = 0usize;
    let mut section_selector = config
        .section_config
        .map(crate::pipeline::SectionSelector::new);

    let ctrl_rx = ctrl_rx;

    'outer: loop {
        if let Some(deadline) = pending_deadline {
            let now = Instant::now();
            if deadline <= now {
                if !current_batch.is_empty() {
                    send_batch(
                        &config.batch_sender,
                        &mut current_batch,
                        batch_id,
                        batch_start_line,
                    )?;
                    batch_id += 1;
                    batch_start_line = line_num + 1;
                }
                pending_deadline = None;
                continue;
            }

            let wait = deadline - now;
            let timeout = crossbeam_channel::after(wait);
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if !current_batch.is_empty() && !immediate {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                        Ok(Ctrl::PrintStats) => {
                            // Batcher thread doesn't have stats to print, ignore
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(line_receiver) -> msg => {
                    match msg {
                        Ok(LineMessage::Line { line, .. }) => {
                            handle_plain_line(
                                line,
                                PlainLineContext {
                                    batch_sender: &config.batch_sender,
                                    current_batch: &mut current_batch,
                                    batch_size: config.batch_size,
                                    batch_timeout: config.batch_timeout,
                                    batch_id: &mut batch_id,
                                    batch_start_line: &mut batch_start_line,
                                    line_num: &mut line_num,
                                    skipped_lines_count: &mut skipped_lines_count,
                                    filtered_lines: &mut filtered_lines,
                                    skip_lines: config.skip_lines,
                                    head_lines: config.head_lines,
                                    section_selector: &mut section_selector,
                                    input_format: &config.input_format,
                                    ignore_lines: &config.ignore_lines,
                                    keep_lines: &config.keep_lines,
                                    pending_deadline: &mut pending_deadline,
                                },
                            )?;

                            // Check if we've reached the head limit after processing this line
                            if let Some(head_limit) = config.head_lines {
                                if line_num >= head_limit {
                                    // Flush remaining batch and stop
                                    if !current_batch.is_empty() {
                                        send_batch(
                                            &config.batch_sender,
                                            &mut current_batch,
                                            batch_id,
                                            batch_start_line,
                                        )?;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                        Ok(LineMessage::Error { error, filename }) => {
                            let context = filename
                                .as_deref()
                                .map(|f| format!("while reading {}", f))
                                .unwrap_or_else(|| "while reading stdin".to_string());
                            return Err(anyhow::Error::from(error).context(context));
                        }
                        Ok(LineMessage::Eof) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(timeout) -> _ => {
                    if !current_batch.is_empty() {
                        send_batch(
                            &config.batch_sender,
                            &mut current_batch,
                            batch_id,
                            batch_start_line,
                        )?;
                        batch_id += 1;
                        batch_start_line = line_num + 1;
                    }
                    pending_deadline = None;
                }
            }
        } else {
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if !current_batch.is_empty() && !immediate {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                        Ok(Ctrl::PrintStats) => {
                            // Batcher thread doesn't have stats to print, ignore
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(line_receiver) -> msg => {
                    match msg {
                        Ok(LineMessage::Line { line, .. }) => {
                            handle_plain_line(
                                line,
                                PlainLineContext {
                                    batch_sender: &config.batch_sender,
                                    current_batch: &mut current_batch,
                                    batch_size: config.batch_size,
                                    batch_timeout: config.batch_timeout,
                                    batch_id: &mut batch_id,
                                    batch_start_line: &mut batch_start_line,
                                    line_num: &mut line_num,
                                    skipped_lines_count: &mut skipped_lines_count,
                                    filtered_lines: &mut filtered_lines,
                                    skip_lines: config.skip_lines,
                                    head_lines: config.head_lines,
                                    section_selector: &mut section_selector,
                                    input_format: &config.input_format,
                                    ignore_lines: &config.ignore_lines,
                                    keep_lines: &config.keep_lines,
                                    pending_deadline: &mut pending_deadline,
                                },
                            )?;

                            // Check if we've reached the head limit after processing this line
                            if let Some(head_limit) = config.head_lines {
                                if line_num >= head_limit {
                                    // Flush remaining batch and stop
                                    if !current_batch.is_empty() {
                                        send_batch(
                                            &config.batch_sender,
                                            &mut current_batch,
                                            batch_id,
                                            batch_start_line,
                                        )?;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                        Ok(LineMessage::Error { error, filename }) => {
                            let context = filename
                                .as_deref()
                                .map(|f| format!("while reading {}", f))
                                .unwrap_or_else(|| "while reading stdin".to_string());
                            return Err(anyhow::Error::from(error).context(context));
                        }
                        Ok(LineMessage::Eof) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch(
                                    &config.batch_sender,
                                    &mut current_batch,
                                    batch_id,
                                    batch_start_line,
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    config.global_tracker.set_total_lines_read(line_num)?;
    config.global_tracker.add_lines_filtered(filtered_lines)?;

    Ok(())
}

/// File-aware batcher thread - handles multiple files with per-file CSV headers
#[allow(clippy::too_many_arguments)]
pub(crate) fn file_aware_batcher_thread(
    line_receiver: Receiver<LineMessage>,
    batch_sender: Sender<Batch>,
    batch_size: usize,
    batch_timeout: Duration,
    global_tracker: GlobalTracker,
    ignore_lines: Option<regex::Regex>,
    keep_lines: Option<regex::Regex>,
    skip_lines: usize,
    head_lines: Option<usize>,
    section_config: Option<crate::config::SectionConfig>,
    input_format: crate::config::InputFormat,
    strict: bool,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    let mut batch_id = 0u64;
    let mut current_batch = Vec::with_capacity(batch_size);
    let mut current_filenames = Vec::with_capacity(batch_size);
    let mut line_num = 0usize;
    let mut batch_start_line = 1usize;
    let mut pending_deadline: Option<Instant> = None;
    let mut skipped_lines_count = 0usize;
    let mut filtered_lines = 0usize;
    let mut last_filename: Option<String> = None;
    let mut current_headers: Option<Vec<String>> = None;
    let mut section_selector = section_config.map(crate::pipeline::SectionSelector::new);

    let ctrl_rx = ctrl_rx;

    'outer: loop {
        if let Some(deadline) = pending_deadline {
            let now = Instant::now();
            if deadline <= now {
                if !current_batch.is_empty() {
                    send_batch_with_filenames_and_headers(
                        &batch_sender,
                        &mut current_batch,
                        &mut current_filenames,
                        batch_id,
                        batch_start_line,
                        current_headers.clone(),
                    )?;
                    batch_id += 1;
                    batch_start_line = line_num + 1;
                }
                pending_deadline = None;
                continue;
            }

            let wait = deadline - now;
            let timeout = crossbeam_channel::after(wait);
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if !current_batch.is_empty() && !immediate {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                        Ok(Ctrl::PrintStats) => {
                            // File-aware batcher thread doesn't have stats to print, ignore
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(line_receiver) -> msg => {
                    match msg {
                        Ok(LineMessage::Line { line, filename }) => {
                            let ctx = FileAwareLineContext {
                                batch_sender: &batch_sender,
                                current_batch: &mut current_batch,
                                current_filenames: &mut current_filenames,
                                batch_size,
                                batch_timeout,
                                batch_id: &mut batch_id,
                                batch_start_line: &mut batch_start_line,
                                line_num: &mut line_num,
                                skipped_lines_count: &mut skipped_lines_count,
                                filtered_lines: &mut filtered_lines,
                                skip_lines,
                                head_lines,
                                section_selector: &mut section_selector,
                                input_format: &input_format,
                                strict,
                                ignore_lines: &ignore_lines,
                                keep_lines: &keep_lines,
                                pending_deadline: &mut pending_deadline,
                                current_headers: &mut current_headers,
                                last_filename: &mut last_filename,
                            };
                            handle_file_aware_line(line, filename, ctx)?;

                            // Check if we've reached the head limit after processing this line
                            if let Some(head_limit) = head_lines {
                                if line_num >= head_limit {
                                    // Flush remaining batch and stop
                                    if !current_batch.is_empty() {
                                        send_batch_with_filenames_and_headers(
                                            &batch_sender,
                                            &mut current_batch,
                                            &mut current_filenames,
                                            batch_id,
                                            batch_start_line,
                                            current_headers.clone(),
                                        )?;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                        Ok(LineMessage::Error { error, filename }) => {
                            let context = filename
                                .as_deref()
                                .map(|f| format!("while reading {}", f))
                                .unwrap_or_else(|| "while reading stdin".to_string());
                            return Err(anyhow::Error::from(error).context(context));
                        }
                        Ok(LineMessage::Eof) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(timeout) -> _ => {
                    if !current_batch.is_empty() {
                        send_batch_with_filenames_and_headers(
                            &batch_sender,
                            &mut current_batch,
                            &mut current_filenames,
                            batch_id,
                            batch_start_line,
                            current_headers.clone(),
                        )?;
                        batch_id += 1;
                        batch_start_line = line_num + 1;
                    }
                    pending_deadline = None;
                }
            }
        } else {
            select! {
                recv(ctrl_rx) -> msg => {
                    match msg {
                        Ok(Ctrl::Shutdown { immediate }) => {
                            if !current_batch.is_empty() && !immediate {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                        Ok(Ctrl::PrintStats) => {
                            // File-aware batcher thread doesn't have stats to print, ignore
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
                recv(line_receiver) -> msg => {
                    match msg {
                        Ok(LineMessage::Line { line, filename }) => {
                            let ctx = FileAwareLineContext {
                                batch_sender: &batch_sender,
                                current_batch: &mut current_batch,
                                current_filenames: &mut current_filenames,
                                batch_size,
                                batch_timeout,
                                batch_id: &mut batch_id,
                                batch_start_line: &mut batch_start_line,
                                line_num: &mut line_num,
                                skipped_lines_count: &mut skipped_lines_count,
                                filtered_lines: &mut filtered_lines,
                                skip_lines,
                                head_lines,
                                section_selector: &mut section_selector,
                                input_format: &input_format,
                                strict,
                                ignore_lines: &ignore_lines,
                                keep_lines: &keep_lines,
                                pending_deadline: &mut pending_deadline,
                                current_headers: &mut current_headers,
                                last_filename: &mut last_filename,
                            };
                            handle_file_aware_line(line, filename, ctx)?;

                            // Check if we've reached the head limit after processing this line
                            if let Some(head_limit) = head_lines {
                                if line_num >= head_limit {
                                    // Flush remaining batch and stop
                                    if !current_batch.is_empty() {
                                        send_batch_with_filenames_and_headers(
                                            &batch_sender,
                                            &mut current_batch,
                                            &mut current_filenames,
                                            batch_id,
                                            batch_start_line,
                                            current_headers.clone(),
                                        )?;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                    Ok(LineMessage::Error { error, filename }) => {
                        let context = filename
                            .as_deref()
                            .map(|f| format!("while reading {}", f))
                            .unwrap_or_else(|| "while reading stdin".to_string());
                        return Err(anyhow::Error::from(error).context(context));
                    }
                        Ok(LineMessage::Eof) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                        Err(_) => {
                            if !current_batch.is_empty() {
                                send_batch_with_filenames_and_headers(
                                    &batch_sender,
                                    &mut current_batch,
                                    &mut current_filenames,
                                    batch_id,
                                    batch_start_line,
                                    current_headers.clone(),
                                )?;
                            }
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    global_tracker.set_total_lines_read(line_num)?;
    global_tracker.add_lines_filtered(filtered_lines)?;

    Ok(())
}

/// Handle a plain line (no filename tracking)
pub(crate) fn handle_plain_line(line: String, ctx: PlainLineContext<'_>) -> Result<()> {
    *ctx.line_num += 1;

    // Check if we've hit the head limit (stops processing early)
    if let Some(head_limit) = ctx.head_lines {
        if *ctx.line_num > head_limit {
            // Signal that we should stop processing by returning early
            // The batcher will flush remaining batch and stop
            return Ok(());
        }
    }

    if *ctx.skipped_lines_count < ctx.skip_lines {
        *ctx.skipped_lines_count += 1;
        *ctx.filtered_lines += 1;
        return Ok(());
    }

    // Apply section selection if configured
    if let Some(selector) = ctx.section_selector {
        if !selector.should_include_line(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    if line.is_empty() && !matches!(ctx.input_format, crate::config::InputFormat::Line) {
        return Ok(());
    }

    if let Some(keep_regex) = ctx.keep_lines.as_ref() {
        if !keep_regex.is_match(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    if let Some(ignore_regex) = ctx.ignore_lines.as_ref() {
        if ignore_regex.is_match(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    ctx.current_batch.push(line);

    if ctx.current_batch.len() >= ctx.batch_size || ctx.batch_timeout.is_zero() {
        send_batch(
            ctx.batch_sender,
            ctx.current_batch,
            *ctx.batch_id,
            *ctx.batch_start_line,
        )?;
        *ctx.batch_id += 1;
        *ctx.batch_start_line = *ctx.line_num + 1;
        *ctx.pending_deadline = None;
    } else {
        *ctx.pending_deadline = Some(Instant::now() + ctx.batch_timeout);
    }

    Ok(())
}

/// Handle a file-aware line (with filename and CSV header tracking)
pub(crate) fn handle_file_aware_line(
    line: String,
    filename: Option<String>,
    ctx: FileAwareLineContext<'_>,
) -> Result<()> {
    *ctx.line_num += 1;

    // Check if we've hit the head limit (stops processing early)
    if let Some(head_limit) = ctx.head_lines {
        if *ctx.line_num > head_limit {
            // Signal that we should stop processing by returning early
            return Ok(());
        }
    }

    if *ctx.skipped_lines_count < ctx.skip_lines {
        *ctx.skipped_lines_count += 1;
        *ctx.filtered_lines += 1;
        return Ok(());
    }

    // Apply section selection if configured
    if let Some(selector) = ctx.section_selector {
        if !selector.should_include_line(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    if line.is_empty() && !matches!(ctx.input_format, crate::config::InputFormat::Line) {
        return Ok(());
    }

    if let Some(ref keep_regex) = ctx.keep_lines {
        if !keep_regex.is_match(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    if let Some(ref ignore_regex) = ctx.ignore_lines {
        if ignore_regex.is_match(&line) {
            *ctx.filtered_lines += 1;
            return Ok(());
        }
    }

    let filename_changed = match (&filename, &*ctx.last_filename) {
        (Some(new), Some(prev)) => new != prev,
        (None, None) => false,
        _ => true,
    };

    if matches!(
        ctx.input_format,
        crate::config::InputFormat::Csv(_)
            | crate::config::InputFormat::Tsv(_)
            | crate::config::InputFormat::Csvnh
            | crate::config::InputFormat::Tsvnh
    ) && filename_changed
    {
        if !ctx.current_batch.is_empty() {
            send_batch_with_filenames_and_headers(
                ctx.batch_sender,
                ctx.current_batch,
                ctx.current_filenames,
                *ctx.batch_id,
                *ctx.batch_start_line,
                ctx.current_headers.clone(),
            )?;
            *ctx.batch_id += 1;
            *ctx.batch_start_line = *ctx.line_num + 1;
            *ctx.pending_deadline = None;
        }

        *ctx.current_headers = create_csv_parser_for_file(ctx.input_format, &line, ctx.strict)
            .map(|parser| parser.get_headers());
        *ctx.last_filename = filename.clone();

        if matches!(
            ctx.input_format,
            crate::config::InputFormat::Csv(_) | crate::config::InputFormat::Tsv(_)
        ) {
            return Ok(());
        }
    } else if filename_changed {
        *ctx.last_filename = filename.clone();
    }

    ctx.current_batch.push(line);
    ctx.current_filenames.push(filename);

    if ctx.current_batch.len() >= ctx.batch_size || ctx.batch_timeout.is_zero() {
        send_batch_with_filenames_and_headers(
            ctx.batch_sender,
            ctx.current_batch,
            ctx.current_filenames,
            *ctx.batch_id,
            *ctx.batch_start_line,
            ctx.current_headers.clone(),
        )?;
        *ctx.batch_id += 1;
        *ctx.batch_start_line = *ctx.line_num + 1;
        *ctx.pending_deadline = None;
    } else {
        *ctx.pending_deadline = Some(Instant::now() + ctx.batch_timeout);
    }

    Ok(())
}

/// Create a CSV parser for a specific file based on its first line
pub(crate) fn create_csv_parser_for_file(
    input_format: &crate::config::InputFormat,
    first_line: &str,
    strict: bool,
) -> Option<crate::parsers::CsvParser> {
    let mut parser = match input_format {
        crate::config::InputFormat::Csv(ref field_spec) => {
            let p = crate::parsers::CsvParser::new_csv();
            if let Some(ref spec) = field_spec {
                p.with_field_spec(spec).ok()?.with_strict(strict)
            } else {
                p
            }
        }
        crate::config::InputFormat::Tsv(ref field_spec) => {
            let p = crate::parsers::CsvParser::new_tsv();
            if let Some(ref spec) = field_spec {
                p.with_field_spec(spec).ok()?.with_strict(strict)
            } else {
                p
            }
        }
        crate::config::InputFormat::Csvnh => crate::parsers::CsvParser::new_csv_no_headers(),
        crate::config::InputFormat::Tsvnh => crate::parsers::CsvParser::new_tsv_no_headers(),
        _ => return None,
    };

    if parser.initialize_headers_from_line(first_line).is_ok() {
        Some(parser)
    } else {
        None
    }
}

/// Send a batch of lines to the batch receiver
pub(crate) fn send_batch(
    batch_sender: &Sender<Batch>,
    current_batch: &mut Vec<String>,
    batch_id: u64,
    batch_start_line: usize,
) -> Result<()> {
    if current_batch.is_empty() {
        return Ok(());
    }

    let batch_len = current_batch.len();
    let batch = Batch {
        id: batch_id,
        lines: std::mem::take(current_batch),
        start_line_num: batch_start_line,
        filenames: vec![None; batch_len], // No filename tracking for regular batches
        csv_headers: None,                // No CSV headers for regular batches
    };

    if batch_sender.send(batch).is_err() {
        return Err(anyhow::anyhow!("Channel closed"));
    }

    Ok(())
}

/// Send a batch with filename tracking and optional CSV headers
pub(crate) fn send_batch_with_filenames_and_headers(
    batch_sender: &Sender<Batch>,
    current_batch: &mut Vec<String>,
    current_filenames: &mut Vec<Option<String>>,
    batch_id: u64,
    batch_start_line: usize,
    csv_headers: Option<Vec<String>>,
) -> Result<()> {
    if current_batch.is_empty() {
        return Ok(());
    }

    let batch = Batch {
        id: batch_id,
        lines: std::mem::take(current_batch),
        start_line_num: batch_start_line,
        filenames: std::mem::take(current_filenames),
        csv_headers,
    };

    if batch_sender.send(batch).is_err() {
        return Err(anyhow::anyhow!("Channel closed"));
    }

    Ok(())
}
