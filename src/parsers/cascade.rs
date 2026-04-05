//! Cascade parser: try multiple parsers in order, first success wins.
//!
//! When users pass a comma-separated format list like `--format json,logfmt,line`,
//! each line is tried against each parser in order. The first parser that
//! returns `Ok` handles the event, and the winning format name is written to
//! the `_format` field on the event for debugging and downstream filtering.
//!
//! Cascade is intentionally restricted to schema-less formats — CSV/TSV and
//! cols/regex formats are rejected at CLI parse time because their schemas
//! can't safely change mid-stream.

use crate::event::Event;
use crate::pipeline::EventParser;
use anyhow::Result;
use rhai::Dynamic;

/// Name of the field added to every event produced by cascade mode.
pub const FORMAT_FIELD: &str = "_format";

/// A parser that tries a list of inner parsers in order, returning the
/// event from the first one that succeeds.
pub struct CascadingParser {
    parsers: Vec<(String, Box<dyn EventParser>)>,
}

impl CascadingParser {
    /// Construct a new cascading parser from an ordered list of
    /// `(format-name, parser)` pairs. Names are used for the `_format` field
    /// and for per-format diagnostic counters.
    pub fn new(parsers: Vec<(String, Box<dyn EventParser>)>) -> Self {
        Self { parsers }
    }

    /// Names of the parsers in this cascade, in order.
    #[allow(dead_code)] // Exposed for potential future introspection/help text
    pub fn format_names(&self) -> Vec<&str> {
        self.parsers.iter().map(|(n, _)| n.as_str()).collect()
    }
}

impl EventParser for CascadingParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let mut last_err: Option<anyhow::Error> = None;
        for (name, parser) in &self.parsers {
            match parser.parse(line) {
                Ok(mut event) => {
                    event.set_field(FORMAT_FIELD.to_string(), Dynamic::from(name.clone()));
                    crate::stats::stats_add_cascade_format_hit(name);
                    return Ok(event);
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("cascade parser has no inner parsers configured")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::{JsonlParser, LineParser};

    #[test]
    fn cascade_prefers_first_success() {
        let cascade = CascadingParser::new(vec![
            ("json".to_string(), Box::new(JsonlParser::new())),
            ("line".to_string(), Box::new(LineParser::new())),
        ]);
        // Valid JSON should be parsed as json.
        let ev = cascade.parse(r#"{"msg":"hi"}"#).unwrap();
        assert_eq!(
            ev.fields
                .get(FORMAT_FIELD)
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "json"
        );
        assert!(ev.fields.contains_key("msg"));
    }

    #[test]
    fn cascade_falls_through_to_line() {
        let cascade = CascadingParser::new(vec![
            ("json".to_string(), Box::new(JsonlParser::new())),
            ("line".to_string(), Box::new(LineParser::new())),
        ]);
        // Non-JSON text should fall through to the line parser.
        let ev = cascade.parse("not json at all").unwrap();
        assert_eq!(
            ev.fields
                .get(FORMAT_FIELD)
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "line"
        );
        assert!(ev.fields.contains_key("line"));
    }
}
