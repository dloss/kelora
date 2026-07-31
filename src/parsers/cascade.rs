//! Cascade parser: try multiple parsers in order, first success wins.
//!
//! When users pass a comma-separated format list like `--format json,logfmt,line`,
//! each line is tried against each parser in order. The first parser that
//! returns `Ok` handles the event, and the winning format name is written to
//! the `_format` field on the event for debugging and downstream filtering —
//! unless the parsed record already has a field of that name, in which case the
//! log's own value is kept and the skipped tag is reported once per run.
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

/// Tag `event` with the winning format name, unless the parsed record already
/// carries a field literally named `_format`.
///
/// A blind `set_field` there would destroy the log's own value — an ECS record
/// with `"_format":"ecs-1.6"` would come out saying `"json"` — and that value is
/// unreconstructable, whereas the parser name is also available from `--stats`
/// (`Cascade formats: …`) and the `-v` detection notice. So data wins the
/// collision, and the skipped tag is counted for a once-per-run warning rather
/// than passing silently (#406).
///
/// The format hit is counted either way: it records which parser handled the
/// line, which is true regardless of whether the tag could be written.
///
/// A `Dynamic::UNIT` under the key is *not* a collision. The projected parsers
/// insert one for every key they chose not to materialize, specifically to keep
/// the field-name set identical to a full parse (see `json.rs`), so it holds no
/// value to protect — skipping the tag for it would lose the tag and preserve
/// nothing.
fn tag_format(event: &mut Event, name: &str) {
    let has_input_value = event
        .fields
        .get(FORMAT_FIELD)
        .is_some_and(|existing| !existing.is_unit());
    if has_input_value {
        crate::stats::stats_add_cascade_format_collision();
    } else {
        event.set_field(FORMAT_FIELD.to_string(), Dynamic::from(name.to_string()));
    }
    crate::stats::stats_add_cascade_format_hit(name);
}

impl EventParser for CascadingParser {
    fn parse(&self, line: &str) -> Result<Event> {
        for (name, parser) in &self.parsers {
            if let Ok(mut event) = parser.parse(line) {
                tag_format(&mut event, name);
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
                // the projection dropping the member's own fields. A collision
                // is therefore only possible when the record's own `_format`
                // survived the projection; if the projection dropped it, the
                // tag is written normally and nothing was lost silently — the
                // user asked for that field to go.
                tag_format(&mut event, name);
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

    /// A record carrying its own `_format` keeps it: overwriting would destroy
    /// input data that cannot be reconstructed, while the tag is still available
    /// from `--stats` and `-v` (#406).
    #[test]
    fn cascade_keeps_an_existing_format_field() {
        let cascade = CascadingParser::new(vec![
            ("json".to_string(), Box::new(JsonlParser::new())),
            ("line".to_string(), Box::new(LineParser::new())),
        ]);
        let ev = cascade
            .parse(r#"{"_format":"ecs-1.6","level":"info"}"#)
            .unwrap();
        assert_eq!(
            ev.fields
                .get(FORMAT_FIELD)
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "ecs-1.6",
            "the log's own _format value must survive the cascade tag"
        );
        assert!(ev.fields.contains_key("level"));
    }

    /// Same protection on the projected path, which has its own tag call site.
    #[test]
    fn cascade_projected_keeps_an_existing_format_field() {
        use crate::projection::Projection;

        let cascade = CascadingParser::new(vec![
            ("json".to_string(), Box::new(JsonlParser::new())),
            ("line".to_string(), Box::new(LineParser::new())),
        ]);
        let projection = Projection::Only(
            [FORMAT_FIELD.to_string(), "level".to_string()]
                .into_iter()
                .collect(),
        );
        let ev = cascade
            .parse_projected(r#"{"_format":"ecs-1.6","level":"info"}"#, &projection)
            .unwrap();
        assert_eq!(
            ev.fields
                .get(FORMAT_FIELD)
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "ecs-1.6"
        );
    }

    /// When the projection does not materialize the record's own `_format`, the
    /// key holds only a `Dynamic::UNIT` placeholder — no value to protect, so the
    /// tag is written as usual. Skipping it here would lose the tag and preserve
    /// nothing.
    #[test]
    fn cascade_projected_tags_over_a_placeholder_value() {
        use crate::projection::Projection;

        let cascade = CascadingParser::new(vec![
            ("json".to_string(), Box::new(JsonlParser::new())),
            ("line".to_string(), Box::new(LineParser::new())),
        ]);
        let projection = Projection::Only(["level".to_string()].into_iter().collect());
        let ev = cascade
            .parse_projected(r#"{"_format":"ecs-1.6","level":"info"}"#, &projection)
            .unwrap();
        assert_eq!(
            ev.fields
                .get(FORMAT_FIELD)
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "json"
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
