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
    pub fn format_names(&self) -> Vec<&str> {
        self.parsers.iter().map(|(n, _)| n.as_str()).collect()
    }
}

impl CascadingParser {
    /// Error for a line no member accepted. The last member's own error (e.g.
    /// logfmt's "Key cannot contain spaces") would misattribute the failure to
    /// one arbitrary format, so name the whole cascade instead — the fix is
    /// almost always "add a catch-all member", not "appease the last member".
    fn no_member_matched(&self) -> anyhow::Error {
        if self.parsers.is_empty() {
            return anyhow::anyhow!("cascade parser has no inner parsers configured");
        }
        anyhow::anyhow!(
            "No format in cascade({}) matched this line; add a catch-all like 'line' as the last format",
            self.format_names().join(",")
        )
    }
}

impl EventParser for CascadingParser {
    fn parse(&self, line: &str) -> Result<Event> {
        for (name, parser) in &self.parsers {
            if let Ok(mut event) = parser.parse(line) {
                event.set_field(FORMAT_FIELD.to_string(), Dynamic::from(name.clone()));
                crate::stats::stats_add_cascade_format_hit(name);
                return Ok(event);
            }
        }
        Err(self.no_member_matched())
    }

    // Projection is honored only when *every* member supports it (spec §4): the
    // winning member is not known until parse time, so a single unsupported
    // member would silently ignore the projection for the lines it handles.
    fn supports_projection(&self) -> bool {
        !self.parsers.is_empty()
            && self
                .parsers
                .iter()
                .all(|(_, parser)| parser.supports_projection())
    }

    fn parse_projected(
        &self,
        line: &str,
        projection: &crate::projection::Projection,
    ) -> Result<Event> {
        for (name, parser) in &self.parsers {
            if let Ok(mut event) = parser.parse_projected(line, projection) {
                // `_format` is appended after parsing, so it is unaffected by
                // the projection dropping the member's own fields.
                event.set_field(FORMAT_FIELD.to_string(), Dynamic::from(name.clone()));
                crate::stats::stats_add_cascade_format_hit(name);
                return Ok(event);
            }
        }
        Err(self.no_member_matched())
    }

    // Any member of the cascade may be the one that parses a given line, so the
    // level appears verbatim only if *every* member guarantees it. A single
    // non-verbatim member (e.g. syslog) could produce a derived level, so the
    // whole cascade must be treated as non-verbatim (spec §4.3).
    fn level_appears_verbatim(&self) -> bool {
        !self.parsers.is_empty()
            && self
                .parsers
                .iter()
                .all(|(_, parser)| parser.level_appears_verbatim())
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
    fn cascade_error_names_the_cascade_not_the_last_member() {
        // The last member's own error ("Key cannot contain spaces" from
        // logfmt, say) misattributes the failure; the useful message names
        // the cascade and the fix.
        let cascade = CascadingParser::new(vec![(
            "json".to_string(),
            Box::new(JsonlParser::new()) as Box<dyn EventParser>,
        )]);
        let err = cascade
            .parse("definitely not json")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cascade(json)") && err.contains("catch-all"),
            "error should name the cascade and suggest a catch-all: {err}"
        );
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
