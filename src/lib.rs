#![allow(clippy::new_without_default, clippy::should_implement_trait)]

pub mod byte_size;
pub mod cli;
pub mod colors;
pub mod config;
pub mod config_file;
pub mod decompression;
pub mod drain;
pub mod engine;
pub mod event;
pub mod field_discovery;
pub mod formatters;
pub mod parallel;
pub mod parsers;
pub mod pipeline;
pub mod platform;
pub mod readers;
pub mod rhai_functions;
pub mod stats;
pub mod timestamp;
pub mod tty;

#[cfg(test)]
mod test_env;

pub use cli::{Cli, FileOrder, InputFormat, OutputFormat};
