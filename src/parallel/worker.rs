//! Worker thread for parallel processing
//!
//! Contains the worker thread that processes batches of lines or events.

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use rhai::Dynamic;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::pipeline::{self, PipelineBuilder};
use crate::platform::Ctrl;
use crate::rhai_functions::tracking;
use crate::stats::{get_thread_stats, stats_finish_processing, stats_start_timer};

use super::types::{Batch, BatchResult, EventBatch, ProcessedEvent, WorkMessage};

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
    match pipeline.flush(ctx) {
        Ok(mut flush_results) => {
            if final_flush {
                if let Some(trailing) = pipeline.finish_formatter() {
                    if !trailing.line.is_empty() {
                        flush_results.push(trailing);
                    }
                }
            }

            if flush_results.is_empty() {
                return Ok(());
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
                worker_stats: crate::stats::ProcessingStats::new(),
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
) -> Result<bool> {
    if batch.csv_headers.is_some() && batch.csv_headers != *current_csv_headers {
        *current_csv_headers = batch.csv_headers.clone();

        let new_pipeline_builder = pipeline_builder
            .clone()
            .with_csv_headers(current_csv_headers.clone().unwrap());
        let (new_pipeline, new_ctx) = new_pipeline_builder.build_worker(stages.to_vec())?;
        *pipeline = new_pipeline;
        ctx.rhai = new_ctx.rhai;
    }

    let before_internal = ctx.internal_tracker.clone();

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
                    ctx.internal_tracker
                        .entry("__kelora_stats_output".to_string())
                        .and_modify(|v| *v = Dynamic::from(v.as_int().unwrap_or(0) + 1))
                        .or_insert(Dynamic::from(1i64));
                    ctx.internal_tracker.insert(
                        "__op___kelora_stats_output".to_string(),
                        Dynamic::from("count"),
                    );
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

    let mut internal_deltas = std::collections::HashMap::new();
    for (key, value) in &ctx.internal_tracker {
        if key.starts_with("__op_") {
            // Operation metadata is added alongside the associated value when needed
            continue;
        }

        let op_key = format!("__op_{}", key);
        let operation = ctx
            .internal_tracker
            .get(&op_key)
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "replace".to_string());

        match operation.as_str() {
            "count" | "sum" => {
                let before_value = before_internal.get(key);
                let diff_dynamic =
                    if value.is_float() || before_value.map(|v| v.is_float()).unwrap_or(false) {
                        let current = if value.is_float() {
                            value.as_float().unwrap_or(0.0)
                        } else {
                            value.as_int().unwrap_or(0) as f64
                        };
                        let previous = before_value
                            .and_then(|v| {
                                if v.is_float() {
                                    v.as_float().ok()
                                } else {
                                    v.as_int().ok().map(|i| i as f64)
                                }
                            })
                            .unwrap_or(0.0);
                        Dynamic::from(current - previous)
                    } else {
                        let current = value.as_int().unwrap_or(0);
                        let previous = before_value.and_then(|v| v.as_int().ok()).unwrap_or(0);
                        Dynamic::from(current - previous)
                    };

                let is_zero = if diff_dynamic.is_float() {
                    diff_dynamic.as_float().unwrap_or(0.0).abs() < f64::EPSILON
                } else {
                    diff_dynamic.as_int().unwrap_or(0) == 0
                };

                if !is_zero {
                    internal_deltas.insert(key.clone(), diff_dynamic);
                    internal_deltas.insert(op_key.clone(), Dynamic::from(operation));
                }
            }
            _ => {
                internal_deltas.insert(key.clone(), value.clone());
                if let Some(op_value) = ctx.internal_tracker.get(&op_key) {
                    internal_deltas.insert(op_key.clone(), op_value.clone());
                }
            }
        }
    }

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
        worker_stats: get_thread_stats(),
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
) -> Result<bool> {
    if event_batch.csv_headers.is_some() && event_batch.csv_headers != *current_csv_headers {
        *current_csv_headers = event_batch.csv_headers.clone();

        let new_pipeline_builder = pipeline_builder
            .clone()
            .with_csv_headers(current_csv_headers.clone().unwrap());
        let (new_pipeline, new_ctx) = new_pipeline_builder.build_worker(stages.to_vec())?;
        *pipeline = new_pipeline;
        ctx.rhai = new_ctx.rhai;
    }

    let before_internal = ctx.internal_tracker.clone();

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
                    ctx.internal_tracker
                        .entry("__kelora_stats_output".to_string())
                        .and_modify(|v| *v = Dynamic::from(v.as_int().unwrap_or(0) + 1))
                        .or_insert(Dynamic::from(1i64));
                    ctx.internal_tracker.insert(
                        "__op___kelora_stats_output".to_string(),
                        Dynamic::from("count"),
                    );
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

    let mut internal_deltas = std::collections::HashMap::new();
    for (key, value) in &ctx.internal_tracker {
        if key.starts_with("__op_") {
            continue;
        }

        let op_key = format!("__op_{}", key);
        let operation = ctx
            .internal_tracker
            .get(&op_key)
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "replace".to_string());

        match operation.as_str() {
            "count" | "sum" => {
                let before_value = before_internal.get(key);
                let diff_dynamic =
                    if value.is_float() || before_value.map(|v| v.is_float()).unwrap_or(false) {
                        let current = if value.is_float() {
                            value.as_float().unwrap_or(0.0)
                        } else {
                            value.as_int().unwrap_or(0) as f64
                        };
                        let previous = before_value
                            .and_then(|v| {
                                if v.is_float() {
                                    v.as_float().ok()
                                } else {
                                    v.as_int().ok().map(|i| i as f64)
                                }
                            })
                            .unwrap_or(0.0);
                        Dynamic::from(current - previous)
                    } else {
                        let current = value.as_int().unwrap_or(0);
                        let previous = before_value.and_then(|v| v.as_int().ok()).unwrap_or(0);
                        Dynamic::from(current - previous)
                    };

                let is_zero = if diff_dynamic.is_float() {
                    diff_dynamic.as_float().unwrap_or(0.0).abs() < f64::EPSILON
                } else {
                    diff_dynamic.as_int().unwrap_or(0) == 0
                };

                if !is_zero {
                    internal_deltas.insert(key.clone(), diff_dynamic);
                    internal_deltas.insert(op_key.clone(), Dynamic::from(operation));
                }
            }
            _ => {
                internal_deltas.insert(key.clone(), value.clone());
                if let Some(op_value) = ctx.internal_tracker.get(&op_key) {
                    internal_deltas.insert(op_key.clone(), op_value.clone());
                }
            }
        }
    }

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
        worker_stats: get_thread_stats(),
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

    while let Ok(batch) = line_batch_receiver.recv() {
        // Check for shutdown
        if let Ok(Ctrl::Shutdown { .. }) = ctrl_rx.try_recv() {
            break;
        }

        last_start_line_num = batch.start_line_num;
        if batch.csv_headers.is_some() {
            last_csv_headers = batch.csv_headers.clone();
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
            };

            let _ = event_batch_sender.send(WorkMessage::EventBatch(event_batch));
        }
    }

    Ok(())
}
