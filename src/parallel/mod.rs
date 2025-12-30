//! Parallel processing module for kelora
//!
//! This module provides parallel processing capabilities for log analysis,
//! splitting the work across multiple threads for improved performance.
//!
//! # Module Structure
//!
//! - `types`: Data structures for batches, messages, and configuration
//! - `tracker`: Thread-safe state tracking and merge logic
//! - `batching`: Line batching and I/O reader threads
//! - `worker`: Worker thread for processing batches
//! - `sink`: Result sink thread for ordered output
//! - `processor`: Main ParallelProcessor orchestration

mod batching;
mod processor;
mod sink;
mod tracker;
mod types;
mod worker;

// Re-export public types
pub use processor::ParallelProcessor;
pub use types::ParallelConfig;
