//! Result sink thread for parallel processing
//!
//! Handles output ordering and merges global state from workers.

use anyhow::{anyhow, Result};
use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::formatters::GapTracker;
use crate::platform::{Ctrl, SHOULD_TERMINATE};
use crate::rhai_functions::file_ops;

use super::tracker::GlobalTracker;
use super::types::{BatchResult, ProcessedEvent};

/// Write CSV header if the output format requires it
pub(crate) fn write_csv_header_if_needed<W: std::io::Write>(
    output: &mut W,
    config: &crate::config::KeloraConfig,
) -> Result<()> {
    // Only write headers for CSV formats that normally include headers
    match config.output.format {
        crate::config::OutputFormat::Csv | crate::config::OutputFormat::Tsv => {
            // Create a temporary formatter to generate the header
            let keys = config.output.get_effective_keys();
            if keys.is_empty() {
                return Err(anyhow::anyhow!(
                    "CSV output format requires --keys to specify field order"
                ));
            }

            let formatter = match config.output.format {
                crate::config::OutputFormat::Csv => crate::formatters::CsvFormatter::new(keys),
                crate::config::OutputFormat::Tsv => crate::formatters::CsvFormatter::new_tsv(keys),
                _ => unreachable!(),
            };

            // Generate and write the header
            let header = formatter.format_header();
            writeln!(output, "{}", header)?;
        }
        _ => {
            // Non-CSV formats don't need headers
        }
    }
    Ok(())
}

/// Pipeline result sink thread: handles output ordering and merges global state
/// Results are already formatted by the pipeline, so we just need to output them
pub(crate) fn pipeline_result_sink_thread<W: std::io::Write>(
    result_receiver: Receiver<BatchResult>,
    preserve_order: bool,
    global_tracker: GlobalTracker,
    output: &mut W,
    config: &crate::config::KeloraConfig,
    take_limit: Option<usize>,
    ctrl_rx: Receiver<Ctrl>,
) -> Result<()> {
    // Write CSV header if needed (before any worker results)
    write_csv_header_if_needed(output, config)?;

    let gap_marker_use_colors = crate::tty::should_use_colors_with_mode(&config.output.color);
    let mut gap_tracker = if config.processing.quiet_events {
        // Suppress gap markers when output is suppressed (stats-only, high quiet levels)
        None
    } else {
        config
            .output
            .mark_gaps
            .map(|threshold| GapTracker::new(threshold, gap_marker_use_colors))
    };

    if preserve_order {
        pipeline_ordered_result_sink(
            result_receiver,
            global_tracker,
            output,
            take_limit,
            &mut gap_tracker,
            ctrl_rx,
            config,
        )
    } else {
        pipeline_unordered_result_sink(
            result_receiver,
            global_tracker,
            output,
            take_limit,
            &mut gap_tracker,
            ctrl_rx,
            config,
        )
    }
}

/// Ordered result sink - maintains batch order for deterministic output
fn pipeline_ordered_result_sink<W: std::io::Write>(
    result_receiver: Receiver<BatchResult>,
    global_tracker: GlobalTracker,
    output: &mut W,
    take_limit: Option<usize>,
    gap_tracker: &mut Option<GapTracker>,
    _ctrl_rx: Receiver<Ctrl>,
    _config: &crate::config::KeloraConfig,
) -> Result<()> {
    let mut pending_batches: HashMap<u64, BatchResult> = HashMap::new();
    let mut next_expected_id = 0u64;
    let mut events_output = 0usize;

    let mut termination_detected = false;
    while let Ok(mut batch_result) = result_receiver.recv() {
        // Check for termination signal, but don't break immediately
        // Continue processing to collect final stats from workers
        if SHOULD_TERMINATE.load(Ordering::Relaxed) {
            termination_detected = true;
        }

        let batch_id = batch_result.batch_id;
        let user_tracked_updates = std::mem::take(&mut batch_result.user_tracked_updates);
        let internal_tracked_updates = std::mem::take(&mut batch_result.internal_tracked_updates);

        // Merge global state and stats
        global_tracker.merge_worker_state(user_tracked_updates, internal_tracked_updates)?;
        global_tracker.merge_worker_stats(&batch_result.worker_stats)?;

        // Handle special batches
        if batch_id == u64::MAX {
            // This is a final stats batch from a terminated worker
            // If we're terminating, we might want to exit soon after collecting these
            if termination_detected {
                // Continue processing a bit more to collect other final stats
                continue;
            }
            continue;
        } else if batch_id == u64::MAX - 1 {
            // This is a flush batch from a worker - process it immediately
            if !termination_detected {
                let remaining_limit = take_limit.map(|limit| limit.saturating_sub(events_output));
                let events_this_batch = pipeline_output_batch_results(
                    output,
                    &batch_result.results,
                    remaining_limit,
                    gap_tracker,
                )?;
                events_output += events_this_batch;

                // Check if we've reached the take limit
                if let Some(limit) = take_limit {
                    if events_output >= limit {
                        // Set termination signal to stop further processing
                        SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
            continue;
        }

        // If terminating, skip output processing but continue stats collection
        if termination_detected {
            continue;
        }

        // Store batch for ordering
        pending_batches.insert(batch_id, batch_result);

        // Output all consecutive batches starting from next_expected_id
        while let Some(batch) = pending_batches.remove(&next_expected_id) {
            let remaining_limit = take_limit.map(|limit| limit.saturating_sub(events_output));
            let events_this_batch = pipeline_output_batch_results(
                output,
                &batch.results,
                remaining_limit,
                gap_tracker,
            )?;
            events_output += events_this_batch;
            next_expected_id += 1;

            // Check if we've reached the take limit
            if let Some(limit) = take_limit {
                if events_output >= limit {
                    // Set termination signal to stop further processing
                    SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    // Output any remaining batches (shouldn't happen with proper shutdown)
    for (_, batch) in pending_batches {
        let remaining_limit = take_limit.map(|limit| limit.saturating_sub(events_output));
        events_output +=
            pipeline_output_batch_results(output, &batch.results, remaining_limit, gap_tracker)?;

        // Check if we've reached the take limit even in cleanup
        if let Some(limit) = take_limit {
            if events_output >= limit {
                break;
            }
        }
    }

    Ok(())
}

/// Unordered result sink - outputs batches as they arrive
fn pipeline_unordered_result_sink<W: std::io::Write>(
    result_receiver: Receiver<BatchResult>,
    global_tracker: GlobalTracker,
    output: &mut W,
    take_limit: Option<usize>,
    gap_tracker: &mut Option<GapTracker>,
    ctrl_rx: Receiver<Ctrl>,
    config: &crate::config::KeloraConfig,
) -> Result<()> {
    let mut termination_detected = false;
    let mut events_output = 0usize;

    loop {
        // Check for control messages first (non-blocking)
        match ctrl_rx.try_recv() {
            Ok(Ctrl::Shutdown { .. }) => {
                // Handle shutdown in termination detection below
            }
            Ok(Ctrl::PrintStats) => {
                // Print current parallel stats from coordinator
                let mut current_stats = global_tracker.get_final_stats();
                // Extract discovered keys/levels from current internal tracking
                let internal_tracking = global_tracker.lock_internal_tracked().clone();
                current_stats.extract_discovered_from_tracking(&internal_tracking);
                let stats_message = config.format_stats_message(
                    &current_stats.format_stats_for_signal(config.input.multiline.is_some(), false),
                    true, // Always show header for signal handler
                );
                let _ = crate::platform::SafeStderr::new().writeln(&stats_message);
            }
            Err(_) => {
                // No control message or channel closed, continue
            }
        }

        // Now handle result messages
        let mut batch_result = match result_receiver.recv() {
            Ok(result) => result,
            Err(_) => break, // Channel closed
        };

        // Check for termination signal, but don't break immediately
        // Continue processing to collect final stats from workers
        if SHOULD_TERMINATE.load(Ordering::Relaxed) {
            termination_detected = true;
        }

        // Merge global state and stats
        let user_updates = std::mem::take(&mut batch_result.user_tracked_updates);
        let internal_updates = std::mem::take(&mut batch_result.internal_tracked_updates);
        global_tracker.merge_worker_state(user_updates, internal_updates)?;
        global_tracker.merge_worker_stats(&batch_result.worker_stats)?;

        // Handle special batches
        if batch_result.batch_id == u64::MAX {
            // This is a final stats batch from a terminated worker
            if termination_detected {
                // Continue processing a bit more to collect other final stats
                continue;
            }
            continue;
        } else if batch_result.batch_id == u64::MAX - 1 {
            // This is a flush batch from a worker - process it immediately
            if !termination_detected {
                let remaining_limit = take_limit.map(|limit| limit.saturating_sub(events_output));
                let events_this_batch = pipeline_output_batch_results(
                    output,
                    &batch_result.results,
                    remaining_limit,
                    gap_tracker,
                )?;
                events_output += events_this_batch;

                // Check if we've reached the take limit
                if let Some(limit) = take_limit {
                    if events_output >= limit {
                        // Set termination signal to stop further processing
                        SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
            continue;
        }

        // If terminating, skip output processing but continue stats collection
        if termination_detected {
            continue;
        }

        // Output immediately
        let remaining_limit = take_limit.map(|limit| limit.saturating_sub(events_output));
        let events_this_batch = pipeline_output_batch_results(
            output,
            &batch_result.results,
            remaining_limit,
            gap_tracker,
        )?;
        events_output += events_this_batch;

        // Check if we've reached the take limit
        if let Some(limit) = take_limit {
            if events_output >= limit {
                // Set termination signal to stop further processing
                SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    Ok(())
}

/// Output batch results to the writer
fn pipeline_output_batch_results<W: std::io::Write>(
    output: &mut W,
    results: &[ProcessedEvent],
    remaining_limit: Option<usize>,
    gap_tracker: &mut Option<GapTracker>,
) -> Result<usize> {
    let mut events_output = 0usize;

    for processed in results {
        if let Err(err) = file_ops::execute_ops(&processed.file_ops) {
            return Err(anyhow!(err));
        }

        // Check if we've reached the limit
        if let Some(limit) = remaining_limit {
            if events_output >= limit {
                break;
            }
        }

        // Output captured messages in order, preserving stdout/stderr streams
        if !processed.captured_messages.is_empty() {
            // Use the new ordered message system
            for message in &processed.captured_messages {
                match message {
                    crate::rhai_functions::strings::CapturedMessage::Stdout(msg) => {
                        println!("{}", msg);
                    }
                    crate::rhai_functions::strings::CapturedMessage::Stderr(msg) => {
                        eprintln!("{}", msg);
                    }
                }
            }
        } else {
            // Fallback to old system for compatibility
            // First output any captured prints for this specific event (to stdout, not file)
            for print_msg in &processed.captured_prints {
                println!("{}", print_msg);
            }

            // Output any captured eprints for this specific event (to stderr)
            for eprint_msg in &processed.captured_eprints {
                eprintln!("{}", eprint_msg);
            }
        }

        // Then output the event itself to the designated output, skip empty strings
        if !processed.event.original_line.is_empty() {
            let marker = match gap_tracker.as_mut() {
                Some(tracker) => tracker.check(processed.timestamp),
                None => None,
            };

            if let Some(marker_line) = marker {
                writeln!(output, "{}", marker_line).unwrap_or(());
            }

            writeln!(output, "{}", &processed.event.original_line).unwrap_or(());
            events_output += 1;
        }
    }

    output.flush().unwrap_or(());
    Ok(events_output)
}
