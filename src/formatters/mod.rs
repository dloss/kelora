mod compact_map;
mod csv;
mod default;
mod gap;
mod hide;
mod inspect;
mod json;
mod logfmt;
mod tailmap;
mod utils;

pub use compact_map::{KeymapFormatter, LevelmapFormatter};
pub use csv::CsvFormatter;
pub use default::DefaultFormatter;
pub use gap::GapTracker;
pub use hide::HideFormatter;
pub use inspect::InspectFormatter;
pub use json::JsonFormatter;
pub use logfmt::LogfmtFormatter;
pub use tailmap::TailmapFormatter;
pub(crate) use utils::escape_for_display;

#[cfg(test)]
pub(crate) use csv::{escape_csv_value, needs_csv_quoting};
#[cfg(test)]
pub(crate) use logfmt::{escape_logfmt_string, needs_logfmt_quoting, sanitize_logfmt_key};
#[cfg(test)]
pub(crate) use utils::format_dynamic_value;

#[cfg(test)]
mod tests;
