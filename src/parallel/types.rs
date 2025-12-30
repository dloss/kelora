//! Type definitions for parallel processing
//!
//! Contains data structures for batches, messages, events, and configuration.

use chrono::{DateTime, Utc};
use crossbeam_channel::Sender;
use rhai::Dynamic;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::rhai_functions::file_ops::FileOp;
use crate::stats::ProcessingStats;

/// Context for processing plain lines (stdin or single file)
pub(crate) struct PlainLineContext<'a> {
    pub batch_sender: &'a Sender<Batch>,
    pub current_batch: &'a mut Vec<String>,
    pub batch_size: usize,
    pub batch_timeout: Duration,
    pub batch_id: &'a mut u64,
    pub batch_start_line: &'a mut usize,
    pub line_num: &'a mut usize,
    pub skipped_lines_count: &'a mut usize,
    pub filtered_lines: &'a mut usize,
    pub skip_lines: usize,
    pub head_lines: Option<usize>,
    pub section_selector: &'a mut Option<crate::pipeline::SectionSelector>,
    pub input_format: &'a crate::config::InputFormat,
    pub ignore_lines: &'a Option<regex::Regex>,
    pub keep_lines: &'a Option<regex::Regex>,
    pub pending_deadline: &'a mut Option<Instant>,
}

/// Context for processing file-aware lines (with filename tracking)
pub(crate) struct FileAwareLineContext<'a> {
    pub batch_sender: &'a Sender<Batch>,
    pub current_batch: &'a mut Vec<String>,
    pub current_filenames: &'a mut Vec<Option<String>>,
    pub batch_size: usize,
    pub batch_timeout: Duration,
    pub batch_id: &'a mut u64,
    pub batch_start_line: &'a mut usize,
    pub line_num: &'a mut usize,
    pub skipped_lines_count: &'a mut usize,
    pub filtered_lines: &'a mut usize,
    pub skip_lines: usize,
    pub head_lines: Option<usize>,
    pub section_selector: &'a mut Option<crate::pipeline::SectionSelector>,
    pub input_format: &'a crate::config::InputFormat,
    pub strict: bool,
    pub ignore_lines: &'a Option<regex::Regex>,
    pub keep_lines: &'a Option<regex::Regex>,
    pub pending_deadline: &'a mut Option<Instant>,
    pub current_headers: &'a mut Option<Vec<String>>,
    pub last_filename: &'a mut Option<String>,
}

/// Configuration for batcher thread - groups all configuration parameters
/// to reduce parameter count
pub(crate) struct BatcherThreadConfig {
    pub batch_sender: Sender<Batch>,
    pub batch_size: usize,
    pub batch_timeout: Duration,
    pub global_tracker: super::tracker::GlobalTracker,
    pub ignore_lines: Option<regex::Regex>,
    pub keep_lines: Option<regex::Regex>,
    pub skip_lines: usize,
    pub head_lines: Option<usize>,
    pub section_config: Option<crate::config::SectionConfig>,
    pub input_format: crate::config::InputFormat,
    pub preprocessing_line_count: usize,
}

/// Configuration for parallel processing
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    pub num_workers: usize,
    pub batch_size: usize,
    pub batch_timeout_ms: u64,
    pub preserve_order: bool,
    pub buffer_size: Option<usize>,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            num_workers: num_cpus::get(),
            batch_size: 1000,
            batch_timeout_ms: 200,
            preserve_order: true,
            buffer_size: Some(10000),
        }
    }
}

/// A batch of lines to be processed together
#[derive(Debug, Clone)]
pub struct Batch {
    pub id: u64,
    pub lines: Vec<String>,
    pub start_line_num: usize,
    pub filenames: Vec<Option<String>>,   // Filename for each line
    pub csv_headers: Option<Vec<String>>, // CSV headers for this batch (if applicable)
}

/// A batch of pre-chunked events (for multiline processing)
#[derive(Debug, Clone)]
pub struct EventBatch {
    pub id: u64,
    pub events: Vec<String>, // Complete event strings from chunker
    pub start_line_num: usize,
    pub filenames: Vec<Option<String>>, // Filename for each event
    pub csv_headers: Option<Vec<String>>,
}

/// Message type for distributing work to workers
#[derive(Debug)]
pub(crate) enum WorkMessage {
    LineBatch(Batch),       // Raw lines (non-multiline mode)
    EventBatch(EventBatch), // Pre-chunked events (multiline mode)
}

/// Message type for IO reader thread communication
#[derive(Debug)]
pub(crate) enum LineMessage {
    Line {
        line: String,
        filename: Option<String>,
    },
    Error {
        error: std::io::Error,
        filename: Option<String>,
    },
    Eof,
}

/// Result of processing a batch
#[derive(Debug)]
pub struct BatchResult {
    pub batch_id: u64,
    pub results: Vec<ProcessedEvent>,
    pub user_tracked_updates: HashMap<String, Dynamic>,
    pub internal_tracked_updates: HashMap<String, Dynamic>,
    pub worker_stats: ProcessingStats,
}

/// An event that has been processed and is ready for output
#[derive(Debug)]
pub struct ProcessedEvent {
    pub event: crate::event::Event,
    pub captured_prints: Vec<String>,
    pub captured_eprints: Vec<String>,
    pub captured_messages: Vec<crate::rhai_functions::strings::CapturedMessage>,
    pub timestamp: Option<DateTime<Utc>>,
    pub file_ops: Vec<FileOp>,
}
