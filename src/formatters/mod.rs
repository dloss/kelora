//! Output formatters for log events
//!
//! This module contains various output formatters that convert events into
//! different textual representations for display or further processing.
//!
//! # Available Formatters
//!
//! - [`DefaultFormatter`] - Human-readable logfmt-style output with colors and wrapping
//! - [`JsonFormatter`] - JSON output, one object per line
//! - [`LogfmtFormatter`] - Strict logfmt-compliant output
//! - [`InspectFormatter`] - Detailed type-aware introspection output
//! - [`HideFormatter`] - Suppresses all output (null format)
//! - [`LevelmapFormatter`] - Dense single-character level visualization
//! - [`KeymapFormatter`] - Dense single-character field visualization
//! - [`TailmapFormatter`] - Percentile-based numeric distribution visualization
//! - [`CsvFormatter`] - CSV/TSV output with configurable columns
//!
//! # Helper Types
//!
//! - [`GapTracker`] - Tracks time gaps between events and inserts visual markers

mod compact_map;
mod csv;
mod default;
mod hide;
mod inspect;
mod json;
mod keymap;
mod levelmap;
mod logfmt;
mod tailmap;
pub mod utils;

#[cfg(test)]
mod tests;

// Re-export all public formatter types
pub use csv::CsvFormatter;
pub use default::{DefaultFormatter, GapTracker};
pub use hide::HideFormatter;
pub use inspect::InspectFormatter;
pub use json::JsonFormatter;
pub use keymap::KeymapFormatter;
pub use levelmap::LevelmapFormatter;
pub use logfmt::LogfmtFormatter;
pub use tailmap::TailmapFormatter;

// Re-export functions used in tests
#[cfg(test)]
pub use logfmt::sanitize_logfmt_key;
#[cfg(test)]
pub use utils::{
    escape_csv_value, escape_logfmt_string, format_dynamic_value, needs_csv_quoting,
    needs_logfmt_quoting,
};
