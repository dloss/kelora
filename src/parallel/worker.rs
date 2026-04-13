//! Worker thread for parallel processing
//!
//! Contains the worker thread that processes batches of lines or events.

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::parsers::type_conversion::TypeMap;
use crate::pipeline::{self, PipelineBuilder};
use crate::platform::Ctrl;
use crate::rhai_functions::tracking;
use crate::stats::{get_thread_stats, stats_finish_processing, stats_start_timer};

use super::types::{Batch, BatchResult, EventBatch, ProcessedEvent, WorkMessage};

fn processing_stats_delta(
    before: &crate::stats::ProcessingStats,
    after: &crate::stats::ProcessingStats,
) -> crate::stats::ProcessingStats {
    let mut delta = crate::stats::ProcessingStats {
        lines_errors: after.lines_errors.saturating_sub(before.lines_errors),
        errors: after.errors.saturating_sub(before.errors),
        assertion_failures: after
            .assertion_failures
            .saturating_sub(before.assertion_failures),
        files_processed: after.files_processed.saturating_sub(before.files_processed),
        script_executions: after
            .script_executions
            .saturating_sub(before.script_executions),
        timestamp_detected_events: after
            .timestamp_detected_events
            .saturating_sub(before.timestamp_detected_events),
        timestamp_parsed_events: after
            .timestamp_parsed_events
            .saturating_sub(before.timestamp_parsed_events),
        timestamp_absent_events: after
            .timestamp_absent_events
            .saturating_sub(before.timestamp_absent_events),
        yearless_timestamps: after
            .yearless_timestamps
            .saturating_sub(before.yearless_timestamps),
        timestamp_override_failed: after.timestamp_override_failed,
        timestamp_override_field: after.timestamp_override_field.clone(),
        timestamp_override_format: after.timestamp_override_format.clone(),
        timestamp_override_warning: after.timestamp_override_warning.clone(),
        ..Default::default()
    };

    for (expr, count) in &after.assertion_failures_by_expr {
        let before_count = before
            .assertion_failures_by_expr
            .get(expr)
            .copied()
            .unwrap_or(0);
        let delta_count = count.saturating_sub(before_count);
        if delta_count > 0 {
            delta
                .assertion_failures_by_expr
                .insert(expr.clone(), delta_count);
        }
    }

    for (field, stat) in &after.timestamp_fields {
        let before_stat = before.timestamp_fields.get(field);
        let detected = stat
            .detected
            .saturating_sub(before_stat.map_or(0, |s| s.detected));
        let parsed = stat
            .parsed
            .saturating_sub(before_stat.map_or(0, |s| s.parsed));
        if detected > 0 || parsed > 0 {
            delta.timestamp_fields.insert(
                field.clone(),
                crate::stats::TimestampFieldStat { detected, parsed },
            );
        }
    }

    for (name, count) in &after.cascade_format_counts {
        let before_count = before.cascade_format_counts.get(name).copied().unwrap_or(0);
        let delta_count = count.saturating_sub(before_count);
        if delta_count > 0 {
            delta
                .cascade_format_counts
                .insert(name.clone(), delta_count);
        }
    }

    delta
}

fn processing_stats_is_empty(stats: &crate::stats::ProcessingStats) -> bool {
    stats.lines_errors == 0
        && stats.errors == 0
        && stats.assertion_failures == 0
        && stats.assertion_failures_by_expr.is_empty()
        && stats.files_processed == 0
        && stats.script_executions == 0
        && stats.timestamp_detected_events == 0
        && stats.timestamp_parsed_events == 0
        && stats.timestamp_absent_events == 0
        && stats.timestamp_fields.is_empty()
        && stats.timestamp_override_field.is_none()
        && stats.timestamp_override_format.is_none()
        && !stats.timestamp_override_failed
        && stats.timestamp_override_warning.is_none()
        && stats.yearless_timestamps == 0
        && stats.cascade_format_counts.is_empty()
}

fn internal_stats_is_empty(stats: &pipeline::InternalStats) -> bool {
    stats.lines_output == 0
        && stats.lines_errors == 0
        && stats.events_created == 0
        && stats.events_output == 0
        && stats.events_filtered == 0
        && stats.discovered_levels.is_empty()
        && stats.discovered_keys.is_empty()
        && stats.discovered_levels_output.is_empty()
        && stats.discovered_keys_output.is_empty()
}

/// Worker thread: processes batches in parallel
pub(crate) fn worker_thread(
    _worker_id: usize,
    work_receiver: Receiver<WorkMessage>,
    result_sender: Sender<BatchResult>,
    pipeline_builder: PipelineBuilder,
    stages: Vec<crate::config::ScriptStageType>,
    multiline_timeout: Option<Duration>,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    crate::rhai_functions::strings::set_parallel_mode(true);

    stats_start_timer();

    let (mut pipeline, mut ctx) = pipeline_builder.clone().build_worker(stages.clone())?;

    let mut current_csv_headers: Option<Vec<String>> = None;
    let mut current_csv_type_map: Option<TypeMap> = None;
    let mut immediate_shutdown = false;

    let ctrl_rx = ctrl_rx;
    let work_receiver = work_receiver;

    'worker_loop: loop {
        if immediate_shutdown {
            break;
        }

        let flush_deadline = multiline_timeout
            .filter(|_| pipeline.has_pending_chunk())
            .map(|timeout| Instant::now() + timeout);

        if let Some(deadline) = flush_deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            let timeout = crossbeam_channel::after(wait);
            select! {
                recv(ctrl_rx) -> msg => {
                    if handle_worker_ctrl(
                        msg,
                        &mut immediate_shutdown,
                        &mut pipeline,
                        &mut ctx,
                        &result_sender,
                    )? {
                        break 'worker_loop;
                    }
                }
                recv(work_receiver) -> msg => {
                    match msg {
                        Ok(work_msg) => {
                            let continue_processing = match work_msg {
                                WorkMessage::LineBatch(batch) => {
                                    worker_process_batch(
                                        batch,
                                        &mut pipeline,
                                        &mut ctx,
                                        &pipeline_builder,
                                        &stages,
                                        &result_sender,
                                        &mut current_csv_headers,
                                        &mut current_csv_type_map,
                                    )?
                                }
                                WorkMessage::EventBatch(event_batch) => {
                                    worker_process_event_batch(
                                        event_batch,
                                        &mut pipeline,
                                        &mut ctx,
                                        &pipeline_builder,
                                        &stages,
                                        &result_sender,
                                        &mut current_csv_headers,
                                        &mut current_csv_type_map,
                                    )?
                                }
                            };
                            if !continue_processing {
                                break 'worker_loop;
                            }
                        }
                        Err(_) => break 'worker_loop,
                    }
                }
                recv(timeout) -> _ => {
                    worker_flush_pipeline(
                        &mut pipeline,
                        &mut ctx,
                        &result_sender,
                        false,
                    )?;
                }
            }
        } else {
            select! {
                recv(ctrl_rx) -> msg => {
                    if handle_worker_ctrl(
                        msg,
                        &mut immediate_shutdown,
                        &mut pipeline,
                        &mut ctx,
                        &result_sender,
                    )? {
                        break 'worker_loop;
                    }
                }
                recv(work_receiver) -> msg => {
                    match msg {
                        Ok(work_msg) => {
                            let continue_processing = match work_msg {
                                WorkMessage::LineBatch(batch) => {
                                    worker_process_batch(
                                        batch,
                                        &mut pipeline,
                                        &mut ctx,
                                        &pipeline_builder,
                                        &stages,
                                        &result_sender,
                                        &mut current_csv_headers,
                                        &mut current_csv_type_map,
                                    )?
                                }
                                WorkMessage::EventBatch(event_batch) => {
                                    worker_process_event_batch(
                                        event_batch,
                                        &mut pipeline,
                                        &mut ctx,
                                        &pipeline_builder,
                                        &stages,
                                        &result_sender,
                                        &mut current_csv_headers,
                                        &mut current_csv_type_map,
                                    )?
                                }
                            };
                            if !continue_processing {
                                break 'worker_loop;
                            }
                        }
                        Err(_) => break 'worker_loop,
                    }
                }
            }
        }
    }

    if !immediate_shutdown {
        worker_flush_pipeline(&mut pipeline, &mut ctx, &result_sender, true)?;
    }

    stats_finish_processing();

    Ok(())
}

/// Handle control messages for worker thread
fn handle_worker_ctrl(
    msg: Result<Ctrl, crossbeam_channel::RecvError>,
    immediate_shutdown: &mut bool,
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    result_sender: &Sender<BatchResult>,
) -> Result<bool> {
    match msg {
        Ok(Ctrl::Shutdown { immediate }) => {
            if immediate {
                *immediate_shutdown = true;
                return Ok(true);
            }

            worker_flush_pipeline(pipeline, ctx, result_sender, false)?;
            Ok(false)
        }
        Ok(Ctrl::PrintStats) => {
            // Worker threads don't print stats directly - ignore
            Ok(false)
        }
        Err(_) => {
            // Treat channel closure as graceful shutdown request
            worker_flush_pipeline(pipeline, ctx, result_sender, false)?;
            Ok(true)
        }
    }
}

/// Flush the worker pipeline and send any pending results
fn worker_flush_pipeline(
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    result_sender: &Sender<BatchResult>,
    final_flush: bool,
) -> Result<()> {
    ctx.internal_stats = pipeline::InternalStats::default();
    let before_worker_stats = get_thread_stats();
    match pipeline.flush(ctx) {
        Ok(mut flush_results) => {
            if final_flush {
                if let Some(trailing) = pipeline.finish_formatter() {
                    if !trailing.line.is_empty() {
                        flush_results.push(trailing);
                    }
                }
            }

            let mut flush_batch_results = Vec::with_capacity(flush_results.len());
            for formatted_result in flush_results {
                let pipeline::FormattedOutput {
                    line,
                    timestamp,
                    file_ops,
                } = formatted_result;
                let mut dummy_event = Event::default_with_line(line);
                dummy_event.set_metadata(0, None);

                flush_batch_results.push(ProcessedEvent {
                    event: dummy_event,
                    captured_prints: Vec::new(),
                    captured_eprints: Vec::new(),
                    captured_messages: Vec::new(),
                    timestamp,
                    file_ops,
                });
            }

            let flush_internal_stats = std::mem::take(&mut ctx.internal_stats);
            let flush_worker_stats =
                processing_stats_delta(&before_worker_stats, &get_thread_stats());

            if flush_batch_results.is_empty()
                && internal_stats_is_empty(&flush_internal_stats)
                && processing_stats_is_empty(&flush_worker_stats)
            {
                return Ok(());
            }

            let mut flush_user_updates = HashMap::new();
            let mut flush_internal_updates = HashMap::new();

            for (key, value) in &ctx.internal_tracker {
                flush_internal_updates.insert(key.clone(), value.clone());
            }

            for (key, value) in &ctx.tracker {
                flush_user_updates.insert(key.clone(), value.clone());
            }

            for (key, value) in ctx
                .internal_tracker
                .iter()
                .filter(|(k, _)| k.starts_with("__op_"))
            {
                flush_user_updates.insert(key.clone(), value.clone());
            }

            let thread_user = tracking::get_thread_tracking_state();
            for (key, value) in thread_user {
                flush_user_updates.insert(key, value);
            }

            let thread_internal = tracking::get_thread_internal_state();
            for (key, value) in thread_internal
                .iter()
                .filter(|(k, _)| k.starts_with("__op_"))
            {
                flush_user_updates.insert(key.clone(), value.clone());
            }
            for (key, value) in thread_internal {
                flush_internal_updates.insert(key, value);
            }

            let flush_batch_result = BatchResult {
                batch_id: u64::MAX - 1,
                results: flush_batch_results,
                user_tracked_updates: flush_user_updates,
                internal_tracked_updates: flush_internal_updates,
                internal_stats: flush_internal_stats,
                worker_stats: flush_worker_stats,
            };

            let _ = result_sender.send(flush_batch_result);
            Ok(())
        }
        Err(e) => {
            if ctx.config.strict {
                return Err(e);
            }
            eprintln!("Warning: Failed to flush worker pipeline: {}", e);
            Ok(())
        }
    }
}

/// Process a batch of lines
#[allow(clippy::too_many_arguments)]
fn worker_process_batch(
    batch: Batch,
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    pipeline_builder: &PipelineBuilder,
    stages: &[crate::config::ScriptStageType],
    result_sender: &Sender<BatchResult>,
    current_csv_headers: &mut Option<Vec<String>>,
    current_csv_type_map: &mut Option<TypeMap>,
) -> Result<bool> {
    if (batch.csv_headers.is_some() && batch.csv_headers != *current_csv_headers)
        || (batch.csv_type_map.is_some() && batch.csv_type_map != *current_csv_type_map)
    {
        if batch.csv_headers.is_some() {
            *current_csv_headers = batch.csv_headers.clone();
        }
        if batch.csv_type_map.is_some() {
            *current_csv_type_map = batch.csv_type_map.clone();
        }

        let mut new_pipeline_builder = pipeline_builder.clone();
        if let Some(ref headers) = current_csv_headers {
            new_pipeline_builder = new_pipeline_builder.with_csv_headers(headers.clone());
        }
        if let Some(ref type_map) = current_csv_type_map {
            new_pipeline_builder = new_pipeline_builder.with_csv_type_map(type_map.clone());
        }
        let (new_pipeline, new_ctx) = new_pipeline_builder.build_worker(stages.to_vec())?;
        *pipeline = new_pipeline;
        ctx.rhai = new_ctx.rhai;
    }

    ctx.internal_stats = pipeline::InternalStats::default();
    let before_worker_stats = get_thread_stats();

    let mut batch_results = Vec::with_capacity(batch.lines.len());

    for (line_idx, line) in batch.lines.iter().enumerate() {
        let current_line_num = batch.start_line_num + line_idx;
        ctx.meta.line_num = Some(current_line_num);
        ctx.meta.filename = batch.filenames.get(line_idx).cloned().flatten();

        crate::rhai_functions::strings::clear_captured_prints();
        crate::rhai_functions::strings::clear_captured_eprints();

        match pipeline.process_line(line.clone(), ctx) {
            Ok(formatted_results) => {
                if !formatted_results.is_empty() {
                    ctx.internal_stats.lines_output += 1;
                }

                let captured_prints = crate::rhai_functions::strings::take_captured_prints();
                let captured_eprints = crate::rhai_functions::strings::take_captured_eprints();
                let captured_messages = crate::rhai_functions::strings::take_captured_messages();

                if formatted_results.is_empty()
                    && (!captured_prints.is_empty()
                        || !captured_eprints.is_empty()
                        || !captured_messages.is_empty())
                {
                    let dummy_event = Event::default_with_line(String::new());
                    batch_results.push(ProcessedEvent {
                        event: dummy_event,
                        captured_prints,
                        captured_eprints,
                        captured_messages,
                        timestamp: None,
                        file_ops: Vec::new(),
                    });
                } else {
                    for formatted_result in formatted_results {
                        let pipeline::FormattedOutput {
                            line,
                            timestamp,
                            file_ops,
                        } = formatted_result;
                        let mut dummy_event = Event::default_with_line(line);
                        dummy_event.set_metadata(current_line_num, None);

                        batch_results.push(ProcessedEvent {
                            event: dummy_event,
                            captured_prints: captured_prints.clone(),
                            captured_eprints: captured_eprints.clone(),
                            captured_messages: captured_messages.clone(),
                            timestamp,
                            file_ops,
                        });
                    }
                }
            }
            Err(e) => {
                let captured_eprints = crate::rhai_functions::strings::take_captured_eprints();
                let captured_messages = crate::rhai_functions::strings::take_captured_messages();

                if !captured_eprints.is_empty() || !captured_messages.is_empty() {
                    let dummy_event = Event::default_with_line(String::new());
                    batch_results.push(ProcessedEvent {
                        event: dummy_event,
                        captured_prints: Vec::new(),
                        captured_eprints,
                        captured_messages,
                        timestamp: None,
                        file_ops: Vec::new(),
                    });
                }

                if ctx.config.strict {
                    return Err(e);
                } else {
                    continue;
                }
            }
        }

        if crate::rhai_functions::process::is_exit_requested() {
            let exit_code = crate::rhai_functions::process::get_exit_code();
            std::process::exit(exit_code);
        }
    }

    let internal_deltas = std::collections::HashMap::new();

    let mut user_deltas = std::collections::HashMap::new();
    let thread_user = tracking::get_thread_tracking_state();
    for (key, value) in thread_user {
        user_deltas.insert(key, value);
    }

    let thread_internal_meta = tracking::get_thread_internal_state();
    for (key, value) in thread_internal_meta
        .iter()
        .filter(|(k, _)| k.starts_with("__op_"))
    {
        user_deltas.insert(key.clone(), value.clone());
    }

    let batch_result = BatchResult {
        batch_id: batch.id,
        results: batch_results,
        user_tracked_updates: user_deltas,
        internal_tracked_updates: internal_deltas,
        internal_stats: std::mem::take(&mut ctx.internal_stats),
        worker_stats: processing_stats_delta(&before_worker_stats, &get_thread_stats()),
    };

    if result_sender.send(batch_result).is_err() {
        return Ok(false);
    }

    ctx.tracker.clear();

    Ok(true)
}

/// Process a batch of pre-chunked events (for multiline processing)
#[allow(clippy::too_many_arguments)]
fn worker_process_event_batch(
    event_batch: EventBatch,
    pipeline: &mut pipeline::Pipeline,
    ctx: &mut pipeline::PipelineContext,
    pipeline_builder: &PipelineBuilder,
    stages: &[crate::config::ScriptStageType],
    result_sender: &Sender<BatchResult>,
    current_csv_headers: &mut Option<Vec<String>>,
    current_csv_type_map: &mut Option<TypeMap>,
) -> Result<bool> {
    if (event_batch.csv_headers.is_some() && event_batch.csv_headers != *current_csv_headers)
        || (event_batch.csv_type_map.is_some() && event_batch.csv_type_map != *current_csv_type_map)
    {
        if event_batch.csv_headers.is_some() {
            *current_csv_headers = event_batch.csv_headers.clone();
        }
        if event_batch.csv_type_map.is_some() {
            *current_csv_type_map = event_batch.csv_type_map.clone();
        }

        let mut new_pipeline_builder = pipeline_builder.clone();
        if let Some(ref headers) = current_csv_headers {
            new_pipeline_builder = new_pipeline_builder.with_csv_headers(headers.clone());
        }
        if let Some(ref type_map) = current_csv_type_map {
            new_pipeline_builder = new_pipeline_builder.with_csv_type_map(type_map.clone());
        }
        let (new_pipeline, new_ctx) = new_pipeline_builder.build_worker(stages.to_vec())?;
        *pipeline = new_pipeline;
        ctx.rhai = new_ctx.rhai;
    }

    ctx.internal_stats = pipeline::InternalStats::default();
    let before_worker_stats = get_thread_stats();

    let mut batch_results = Vec::with_capacity(event_batch.events.len());

    for (event_idx, event_string) in event_batch.events.iter().enumerate() {
        let current_line_num = event_batch.start_line_num + event_idx;
        ctx.meta.line_num = Some(current_line_num);
        ctx.meta.filename = event_batch.filenames.get(event_idx).cloned().flatten();

        crate::rhai_functions::strings::clear_captured_prints();
        crate::rhai_functions::strings::clear_captured_eprints();

        // For event batches, skip chunking and go directly to parsing
        match pipeline.process_event_string(event_string.clone(), ctx) {
            Ok(formatted_results) => {
                if !formatted_results.is_empty() {
                    ctx.internal_stats.lines_output += 1;
                }

                let captured_prints = crate::rhai_functions::strings::take_captured_prints();
                let captured_eprints = crate::rhai_functions::strings::take_captured_eprints();
                let captured_messages = crate::rhai_functions::strings::take_captured_messages();

                if formatted_results.is_empty()
                    && (!captured_prints.is_empty()
                        || !captured_eprints.is_empty()
                        || !captured_messages.is_empty())
                {
                    let dummy_event = Event::default_with_line(String::new());
                    batch_results.push(ProcessedEvent {
                        event: dummy_event,
                        captured_prints,
                        captured_eprints,
                        captured_messages,
                        timestamp: None,
                        file_ops: Vec::new(),
                    });
                } else {
                    for formatted_result in formatted_results {
                        let pipeline::FormattedOutput {
                            line,
                            timestamp,
                            file_ops,
                        } = formatted_result;
                        let mut dummy_event = Event::default_with_line(line);
                        dummy_event.set_metadata(current_line_num, None);

                        batch_results.push(ProcessedEvent {
                            event: dummy_event,
                            captured_prints: captured_prints.clone(),
                            captured_eprints: captured_eprints.clone(),
                            captured_messages: captured_messages.clone(),
                            timestamp,
                            file_ops,
                        });
                    }
                }
            }
            Err(e) => {
                let captured_eprints = crate::rhai_functions::strings::take_captured_eprints();
                let captured_messages = crate::rhai_functions::strings::take_captured_messages();

                if !captured_eprints.is_empty() || !captured_messages.is_empty() {
                    let dummy_event = Event::default_with_line(String::new());
                    batch_results.push(ProcessedEvent {
                        event: dummy_event,
                        captured_prints: Vec::new(),
                        captured_eprints,
                        captured_messages,
                        timestamp: None,
                        file_ops: Vec::new(),
                    });
                }

                if ctx.config.strict {
                    return Err(e);
                } else {
                    continue;
                }
            }
        }

        if crate::rhai_functions::process::is_exit_requested() {
            let exit_code = crate::rhai_functions::process::get_exit_code();
            std::process::exit(exit_code);
        }
    }

    let internal_deltas = std::collections::HashMap::new();

    let mut user_deltas = std::collections::HashMap::new();
    let thread_user = tracking::get_thread_tracking_state();
    for (key, value) in thread_user {
        user_deltas.insert(key, value);
    }

    let batch_result = BatchResult {
        batch_id: event_batch.id,
        results: batch_results,
        user_tracked_updates: user_deltas,
        internal_tracked_updates: internal_deltas,
        internal_stats: std::mem::take(&mut ctx.internal_stats),
        worker_stats: processing_stats_delta(&before_worker_stats, &get_thread_stats()),
    };

    if result_sender.send(batch_result).is_err() {
        return Ok(false);
    }

    ctx.tracker.clear();

    Ok(true)
}

/// Chunker thread: converts line batches to event batches for multiline processing
pub(crate) fn chunker_thread(
    line_batch_receiver: Receiver<super::types::Batch>,
    event_batch_sender: Sender<WorkMessage>,
    multiline_config: crate::config::MultilineConfig,
    input_format: crate::config::InputFormat,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    // Create a chunker for multiline processing
    let mut chunker =
        crate::pipeline::multiline::create_multiline_chunker(&multiline_config, input_format)
            .map_err(|e| anyhow::anyhow!("Failed to create chunker: {}", e))?;

    // Track metadata required to emit accurate event batches across line boundaries
    let mut pending_event_filename: Option<Option<String>> = None;
    let mut next_event_batch_id = 0u64;
    let mut last_start_line_num: usize = 0;
    let mut last_csv_headers: Option<Vec<String>> = None;
    let mut last_csv_type_map: Option<TypeMap> = None;

    while let Ok(batch) = line_batch_receiver.recv() {
        // Check for shutdown
        if let Ok(Ctrl::Shutdown { .. }) = ctrl_rx.try_recv() {
            break;
        }

        last_start_line_num = batch.start_line_num;
        if batch.csv_headers.is_some() {
            last_csv_headers = batch.csv_headers.clone();
        }
        if batch.csv_type_map.is_some() {
            last_csv_type_map = batch.csv_type_map.clone();
        }

        let mut events = Vec::new();
        let mut event_filenames = Vec::new();

        // Process each line through chunker
        for (line_idx, line) in batch.lines.iter().enumerate() {
            let line_filename = batch.filenames.get(line_idx).cloned().flatten();

            if pending_event_filename.is_none() || !chunker.has_pending() {
                pending_event_filename = Some(line_filename.clone());
            }

            // Feed line to chunker and collect complete events
            if let Some(chunk) = chunker.feed_line(line.clone()) {
                let event_filename = pending_event_filename
                    .take()
                    .unwrap_or_else(|| line_filename.clone());

                events.push(chunk);
                event_filenames.push(event_filename);

                // Current line becomes the first line of the next buffered event
                pending_event_filename = Some(line_filename.clone());
            }
        }

        // Send event batch to workers if we have events
        if !events.is_empty() {
            let event_batch = EventBatch {
                id: next_event_batch_id,
                events,
                start_line_num: batch.start_line_num,
                filenames: event_filenames,
                csv_headers: batch.csv_headers,
                csv_type_map: batch.csv_type_map,
            };

            next_event_batch_id = next_event_batch_id.wrapping_add(1);

            if event_batch_sender
                .send(WorkMessage::EventBatch(event_batch))
                .is_err()
            {
                break;
            }
        }
    }

    // Flush any remaining buffered event after input closes or shutdown
    if chunker.has_pending() {
        if let Some(chunk) = chunker.flush() {
            let flushed_filename = pending_event_filename.take().unwrap_or(None);

            let event_batch = EventBatch {
                id: next_event_batch_id,
                events: vec![chunk],
                start_line_num: last_start_line_num,
                filenames: vec![flushed_filename],
                csv_headers: last_csv_headers,
                csv_type_map: last_csv_type_map,
            };

            let _ = event_batch_sender.send(WorkMessage::EventBatch(event_batch));
        }
    }

    Ok(())
}
