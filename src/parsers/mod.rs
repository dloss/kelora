pub mod auto_detect;
pub mod cascade;
pub mod cef;
pub mod cols;
pub mod combined;
pub mod csv;
pub mod json;
pub mod line;
pub mod logfmt;
pub mod raw;
pub mod regex;
pub mod syslog;
pub mod type_conversion;

#[allow(unused_imports)] // Used by lib.rs for format auto-detection
pub use auto_detect::detect_format;
#[allow(unused_imports)] // FORMAT_FIELD re-exported for external access
pub use cascade::{CascadingParser, FORMAT_FIELD};
pub use cef::CefParser;
pub use cols::ColsParser;
pub use combined::CombinedParser;
pub use csv::CsvParser;
pub use json::JsonlParser;
pub use line::LineParser;
pub use logfmt::LogfmtParser;
pub use raw::RawParser;
pub use regex::RegexParser;
pub use syslog::SyslogParser;
