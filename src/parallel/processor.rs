//! Main parallel processor
//!
//! Contains the ParallelProcessor struct that orchestrates the parallel pipeline.

use anyhow::Result;
use crossbeam_channel::{bounded, unbounded};
use std::io::Read;
use std::thread;
use std::time::Duration;

use crate::pipeline::{PipelineBuilder, DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS};
use crate::platform::Ctrl;
use crate::rhai_functions::tracking::TrackingSnapshot;
use crate::stats::ProcessingStats;

use super::batching::{
    batcher_thread, file_aware_batcher_thread, file_aware_io_reader_thread, plain_io_reader_thread,
};
use super::sink::pipeline_result_sink_thread;
use super::tracker::GlobalTracker;
use super::types::{BatcherThreadConfig, ParallelConfig, WorkMessage};
use super::worker::{chunker_thread, worker_thread};

/// Main parallel processor
pub struct ParallelProcessor {
    config: ParallelConfig,
    global_tracker: GlobalTracker,
    take_limit: Option<usize>,
}

impl ParallelProcessor {
    pub fn new(config: ParallelConfig) -> Self {
        Self {
            config,
            global_tracker: GlobalTracker::new(),
            take_limit: None,
        }
    }

    pub fn with_take_limit(mut self, take_limit: Option<usize>) -> Self {
        self.take_limit = take_limit;
        self
    }

    /// Process input using the parallel pipeline
    pub fn process_with_pipeline<
        R: std::io::BufRead + Send + 'static,
        W: std::io::Write + Send + 'static,
    >(
        &self,
        reader: R,
        pipeline_builder: PipelineBuilder,
        stages: Vec<crate::config::ScriptStageType>,
        config: &crate::config::KeloraConfig,
        output: W,
        ctrl_rx: crossbeam_channel::Receiver<Ctrl>,
    ) -> Result<()> {
        // For file processing, try to use file-aware reader if available
        if !config.input.files.is_empty() {
            return self.process_with_file_aware_pipeline(
                pipeline_builder,
                stages,
                config,
                output,
                ctrl_rx,
            );
        }

        // Fallback to original implementation for stdin
        self.process_with_generic_pipeline(
            reader,
            pipeline_builder,
            stages,
            config,
            output,
            ctrl_rx,
        )
    }

    fn process_with_generic_pipeline<
        R: std::io::BufRead + Send + 'static,
        W: std::io::Write + Send + 'static,
    >(
        &self,
        reader: R,
        pipeline_builder: PipelineBuilder,
        stages: Vec<crate::config::ScriptStageType>,
        config: &crate::config::KeloraConfig,
        output: W,
        ctrl_rx: crossbeam_channel::Receiver<Ctrl>,
    ) -> Result<()> {
        // Create channels - conditionally use chunker thread for multiline mode
        let (batch_sender, batch_receiver) = if let Some(size) = self.config.buffer_size {
            bounded(size)
        } else {
            unbounded()
        };

        let (work_sender, work_receiver) = if let Some(size) = self.config.buffer_size {
            bounded(size)
        } else {
            unbounded()
        };

        let (result_sender, result_receiver) = if self.config.preserve_order {
            bounded(self.config.num_workers * 4) // Increased from 2x to 4x workers
        } else {
            unbounded()
        };

        // For CSV formats, we need to peek at the first line to initialize headers
        // We'll wrap the reader to handle this preprocessing
        let (reader, pipeline_builder, preprocessing_line_count) = if matches!(
            config.input.format,
            crate::config::InputFormat::Csv(_)
                | crate::config::InputFormat::Tsv(_)
                | crate::config::InputFormat::Csvnh
                | crate::config::InputFormat::Tsvnh
        ) {
            Self::preprocess_csv_with_reader(reader, pipeline_builder, config)?
        } else {
            (
                Box::new(reader) as Box<dyn std::io::BufRead + Send>,
                pipeline_builder,
                0,
            )
        };

        let batch_timeout = Duration::from_millis(self.config.batch_timeout_ms);
        let line_buffer_bound = self.config.buffer_size.unwrap_or(10000);
        let (line_sender, line_receiver) = bounded(line_buffer_bound);

        let io_handle = {
            let ctrl_for_io = ctrl_rx.clone();
            thread::spawn(move || plain_io_reader_thread(reader, line_sender, ctrl_for_io))
        };

        let batch_handle = {
            let batch_sender = batch_sender.clone();
            let batch_size = self.config.batch_size;
            let ignore_lines = config.input.ignore_lines.clone();
            let keep_lines = config.input.keep_lines.clone();
            let skip_lines = config.input.skip_lines;
            let head_lines = config.input.head_lines;
            let section_config = config.input.section.clone();
            let global_tracker_clone = self.global_tracker.clone();
            let input_format = config.input.format.clone();
            let ctrl_for_batcher = ctrl_rx.clone();

            thread::spawn(move || {
                batcher_thread(
                    line_receiver,
                    BatcherThreadConfig {
                        batch_sender,
                        batch_size,
                        batch_timeout,
                        global_tracker: global_tracker_clone,
                        ignore_lines,
                        keep_lines,
                        skip_lines,
                        head_lines,
                        section_config,
                        input_format,
                        preprocessing_line_count,
                    },
                    ctrl_for_batcher,
                )
            })
        };

        // Conditionally spawn chunker thread for multiline mode
        let chunker_handle = if let Some(multiline_config) = &config.input.multiline {
            let chunker_ctrl = ctrl_rx.clone();
            let chunker_multiline_config = multiline_config.clone();
            let chunker_input_format = config.input.format.clone();

            let handle = thread::spawn(move || {
                chunker_thread(
                    batch_receiver,
                    work_sender,
                    chunker_multiline_config,
                    chunker_input_format,
                    chunker_ctrl,
                )
            });
            Some(handle)
        } else {
            // For non-multiline mode, workers receive line batches directly
            // Convert line batches to work messages
            let converter_ctrl = ctrl_rx.clone();
            let handle = thread::spawn(move || -> Result<()> {
                while let Ok(batch) = batch_receiver.recv() {
                    if let Ok(Ctrl::Shutdown { .. }) = converter_ctrl.try_recv() {
                        break;
                    }
                    if work_sender.send(WorkMessage::LineBatch(batch)).is_err() {
                        break;
                    }
                }
                Ok(())
            });
            Some(handle)
        };

        // Start worker threads
        let mut worker_handles = Vec::with_capacity(self.config.num_workers);
        let worker_multiline_timeout = if config.input.multiline.is_some() {
            Some(Duration::from_millis(DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS))
        } else {
            None
        };

        for worker_id in 0..self.config.num_workers {
            let work_receiver = work_receiver.clone();
            let result_sender = result_sender.clone();
            let worker_pipeline_builder = pipeline_builder.clone();
            let worker_stages = stages.clone();
            let worker_ctrl = ctrl_rx.clone();
            let worker_timeout = worker_multiline_timeout;

            let handle = thread::spawn(move || {
                worker_thread(
                    worker_id,
                    work_receiver,
                    result_sender,
                    worker_pipeline_builder,
                    worker_stages,
                    worker_timeout,
                    worker_ctrl,
                )
            });
            worker_handles.push(handle);
        }

        // Drop senders to signal completion
        drop(batch_sender);
        drop(result_sender);

        // Start result sink thread
        let sink_handle = {
            let result_receiver = result_receiver;
            let preserve_order = self.config.preserve_order;
            let global_tracker = self.global_tracker.clone();
            let mut output = output;
            let config_clone = config.clone();
            let take_limit = self.take_limit;
            let ctrl_for_sink = ctrl_rx.clone();

            thread::spawn(move || {
                pipeline_result_sink_thread(
                    result_receiver,
                    preserve_order,
                    global_tracker,
                    &mut output,
                    &config_clone,
                    take_limit,
                    ctrl_for_sink,
                )
            })
        };

        // Wait for all threads to complete
        io_handle
            .join()
            .unwrap_or_else(|e| panic!("IO thread panicked: {:?}", e))?;
        batch_handle
            .join()
            .unwrap_or_else(|e| panic!("Batch processing thread panicked: {:?}", e))?;

        // Join chunker thread if it was spawned
        if let Some(handle) = chunker_handle {
            handle
                .join()
                .unwrap_or_else(|e| panic!("Chunker thread panicked: {:?}", e))?;
        }

        for (idx, handle) in worker_handles.into_iter().enumerate() {
            handle
                .join()
                .unwrap_or_else(|e| panic!("Worker thread {} panicked: {:?}", idx, e))?;
        }

        sink_handle
            .join()
            .unwrap_or_else(|e| panic!("Sink thread panicked: {:?}", e))?;

        Ok(())
    }

    fn process_with_file_aware_pipeline<W: std::io::Write + Send + 'static>(
        &self,
        pipeline_builder: PipelineBuilder,
        stages: Vec<crate::config::ScriptStageType>,
        config: &crate::config::KeloraConfig,
        output: W,
        ctrl_rx: crossbeam_channel::Receiver<Ctrl>,
    ) -> Result<()> {
        // Create file-aware reader
        let file_aware_reader = crate::pipeline::builders::create_file_aware_input_reader(config)?;

        // Create channels - conditionally use chunker thread for multiline mode
        let (batch_sender, batch_receiver) = if let Some(size) = self.config.buffer_size {
            bounded(size)
        } else {
            unbounded()
        };

        let (work_sender, work_receiver) = if let Some(size) = self.config.buffer_size {
            bounded(size)
        } else {
            unbounded()
        };

        let (result_sender, result_receiver) = if self.config.preserve_order {
            bounded(self.config.num_workers * 4)
        } else {
            unbounded()
        };

        // For CSV formats, we need to handle per-file preprocessing
        let file_aware_pipeline_builder = if matches!(
            config.input.format,
            crate::config::InputFormat::Csv(_)
                | crate::config::InputFormat::Tsv(_)
                | crate::config::InputFormat::Csvnh
                | crate::config::InputFormat::Tsvnh
        ) {
            // For now, we'll let the file-aware reader handle CSV initialization
            // This will be improved when we implement proper per-file schema detection
            pipeline_builder
        } else {
            pipeline_builder
        };

        let batch_timeout = Duration::from_millis(self.config.batch_timeout_ms);
        let line_buffer_bound = self.config.buffer_size.unwrap_or(10000);
        let (line_sender, line_receiver) = bounded(line_buffer_bound);

        let io_handle = {
            let ctrl_for_io = ctrl_rx.clone();
            thread::spawn(move || {
                file_aware_io_reader_thread(file_aware_reader, line_sender, ctrl_for_io)
            })
        };

        let batch_handle = {
            let batch_sender = batch_sender.clone();
            let batch_size = self.config.batch_size;
            let ignore_lines = config.input.ignore_lines.clone();
            let keep_lines = config.input.keep_lines.clone();
            let skip_lines = config.input.skip_lines;
            let head_lines = config.input.head_lines;
            let section_config = config.input.section.clone();
            let global_tracker_clone = self.global_tracker.clone();
            let input_format = config.input.format.clone();
            let strict = config.processing.strict;
            let ctrl_for_batcher = ctrl_rx.clone();

            thread::spawn(move || {
                file_aware_batcher_thread(
                    line_receiver,
                    batch_sender,
                    batch_size,
                    batch_timeout,
                    global_tracker_clone,
                    ignore_lines,
                    keep_lines,
                    skip_lines,
                    head_lines,
                    section_config,
                    input_format,
                    strict,
                    ctrl_for_batcher,
                )
            })
        };

        // Conditionally spawn chunker thread for multiline mode
        let chunker_handle = if let Some(multiline_config) = &config.input.multiline {
            let chunker_ctrl = ctrl_rx.clone();
            let chunker_multiline_config = multiline_config.clone();
            let chunker_input_format = config.input.format.clone();

            let handle = thread::spawn(move || {
                chunker_thread(
                    batch_receiver,
                    work_sender,
                    chunker_multiline_config,
                    chunker_input_format,
                    chunker_ctrl,
                )
            });
            Some(handle)
        } else {
            // For non-multiline mode, workers receive line batches directly
            // Convert line batches to work messages
            let converter_ctrl = ctrl_rx.clone();
            let handle = thread::spawn(move || -> Result<()> {
                while let Ok(batch) = batch_receiver.recv() {
                    if let Ok(Ctrl::Shutdown { .. }) = converter_ctrl.try_recv() {
                        break;
                    }
                    if work_sender.send(WorkMessage::LineBatch(batch)).is_err() {
                        break;
                    }
                }
                Ok(())
            });
            Some(handle)
        };

        // Start worker threads
        let mut worker_handles = Vec::with_capacity(self.config.num_workers);
        let worker_multiline_timeout = if config.input.multiline.is_some() {
            Some(Duration::from_millis(DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS))
        } else {
            None
        };

        for worker_id in 0..self.config.num_workers {
            let work_receiver = work_receiver.clone();
            let result_sender = result_sender.clone();
            let worker_pipeline_builder = file_aware_pipeline_builder.clone();
            let worker_stages = stages.clone();
            let worker_ctrl = ctrl_rx.clone();
            let worker_timeout = worker_multiline_timeout;

            let handle = thread::spawn(move || {
                worker_thread(
                    worker_id,
                    work_receiver,
                    result_sender,
                    worker_pipeline_builder,
                    worker_stages,
                    worker_timeout,
                    worker_ctrl,
                )
            });
            worker_handles.push(handle);
        }

        // Drop senders to signal completion
        drop(batch_sender);
        drop(result_sender);

        // Start result sink thread
        let sink_handle = {
            let result_receiver = result_receiver;
            let preserve_order = self.config.preserve_order;
            let global_tracker = self.global_tracker.clone();
            let mut output = output;
            let config_clone = config.clone();
            let take_limit = self.take_limit;
            let ctrl_for_sink = ctrl_rx.clone();

            thread::spawn(move || {
                pipeline_result_sink_thread(
                    result_receiver,
                    preserve_order,
                    global_tracker,
                    &mut output,
                    &config_clone,
                    take_limit,
                    ctrl_for_sink,
                )
            })
        };

        // Wait for all threads to complete
        io_handle
            .join()
            .unwrap_or_else(|e| panic!("IO thread panicked: {:?}", e))?;
        batch_handle
            .join()
            .unwrap_or_else(|e| panic!("Batch processing thread panicked: {:?}", e))?;

        // Join chunker thread if it was spawned
        if let Some(handle) = chunker_handle {
            handle
                .join()
                .unwrap_or_else(|e| panic!("Chunker thread panicked: {:?}", e))?;
        }

        for (idx, handle) in worker_handles.into_iter().enumerate() {
            handle
                .join()
                .unwrap_or_else(|e| panic!("Worker thread {} panicked: {:?}", idx, e))?;
        }

        sink_handle
            .join()
            .unwrap_or_else(|e| panic!("Sink thread panicked: {:?}", e))?;

        Ok(())
    }

    /// Get the final merged global state for use in --end stage
    /// Returns both user-visible metrics and internal stats snapshot
    pub fn get_final_tracked_state(&self) -> TrackingSnapshot {
        self.global_tracker.get_final_snapshot()
    }

    /// Get the final merged statistics from all workers
    pub fn get_final_stats(&self) -> ProcessingStats {
        self.global_tracker.get_final_stats()
    }

    /// Extract stats from tracking system into global stats
    pub fn extract_final_stats_from_tracking(
        &self,
        final_tracked: &TrackingSnapshot,
    ) -> Result<()> {
        self.global_tracker
            .extract_final_stats_from_tracking(&final_tracked.internal)
    }

    /// Preprocess CSV headers and return a reader that includes the first line if it's data
    fn preprocess_csv_with_reader<R: std::io::BufRead + Send + 'static>(
        mut reader: R,
        mut pipeline_builder: PipelineBuilder,
        config: &crate::config::KeloraConfig,
    ) -> Result<(Box<dyn std::io::BufRead + Send>, PipelineBuilder, usize)> {
        let mut first_line = String::new();
        reader.read_line(&mut first_line)?;

        if first_line.trim().is_empty() {
            return Ok((Box::new(reader), pipeline_builder, 0)); // Empty line will be processed normally
        }

        // Remove trailing newline for processing, but keep original for reinsertion
        let first_line_trimmed = first_line.trim_end().to_string();

        // Create a temporary parser to extract headers
        let mut temp_parser = match &config.input.format {
            crate::config::InputFormat::Csv(ref field_spec) => {
                let p = crate::parsers::CsvParser::new_csv();
                if let Some(ref spec) = field_spec {
                    p.with_field_spec(spec)?
                        .with_strict(config.processing.strict)
                } else {
                    p
                }
            }
            crate::config::InputFormat::Tsv(ref field_spec) => {
                let p = crate::parsers::CsvParser::new_tsv();
                if let Some(ref spec) = field_spec {
                    p.with_field_spec(spec)?
                        .with_strict(config.processing.strict)
                } else {
                    p
                }
            }
            crate::config::InputFormat::Csvnh => crate::parsers::CsvParser::new_csv_no_headers(),
            crate::config::InputFormat::Tsvnh => crate::parsers::CsvParser::new_tsv_no_headers(),
            _ => return Ok((Box::new(reader), pipeline_builder, 0)), // Not a CSV format
        };

        // Initialize headers from the first line
        let was_consumed = temp_parser.initialize_headers_from_line(&first_line_trimmed)?;

        // Get the initialized headers
        let headers = temp_parser.get_headers();

        // Add headers to pipeline builder
        pipeline_builder = pipeline_builder.with_csv_headers(headers);

        // Create a new reader that includes the first line if it should be processed as data
        let final_reader: Box<dyn std::io::BufRead + Send> = if was_consumed {
            // First line was a header, don't include it in processing
            Box::new(reader)
        } else {
            // First line is data, prepend it to the reader
            let first_line_bytes = first_line.into_bytes();
            let first_line_reader = std::io::Cursor::new(first_line_bytes);
            Box::new(first_line_reader.chain(reader))
        };

        // Only count the line as preprocessed if it was consumed (not re-inserted)
        let preprocessing_count = if was_consumed { 1 } else { 0 };
        Ok((final_reader, pipeline_builder, preprocessing_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_config_default() {
        let config = ParallelConfig::default();
        assert!(config.num_workers > 0);
        assert!(config.batch_size > 0);
        assert!(config.batch_timeout_ms > 0);
    }
}
