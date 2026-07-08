use crate::event::Event;
use crate::parsers::type_conversion::looks_like_json_number;
use crate::pipeline::EventParser;
use crate::projection::Projection;
use anyhow::Result;
use rhai::Dynamic;
use std::borrow::Cow;

pub struct LogfmtParser {
    auto_timestamp: bool,
}

impl LogfmtParser {
    pub fn new() -> Self {
        Self {
            auto_timestamp: true,
        }
    }

    pub fn new_without_auto_timestamp() -> Self {
        Self {
            auto_timestamp: false,
        }
    }

    /// Parse a logfmt line directly into `event`, borrowing key and value spans
    /// from `line`. Scans bytes over ASCII delimiters (`=`, space, tab, `"`,
    /// `\`), which never occur inside a multi-byte UTF-8 sequence, so the spans
    /// are always valid `&str`. The only allocations are the owned key, the one
    /// `ImmutableString`/`String` backing each string value, and — for a quoted
    /// value containing escapes — a single unescape buffer. In particular the
    /// per-line `Vec<(String, String)>` of the old parser is gone, and unquoted
    /// and escape-free quoted values never allocate an intermediate `String`.
    ///
    /// When `projection` is `Some(Only(..))`, the value span of a key the
    /// projection does not want is still scanned (so quoting/escapes advance the
    /// cursor correctly and every key-level error is identical) but never
    /// unescaped, coerced, or allocated: a cheap `Dynamic::UNIT` placeholder is
    /// inserted under the real key instead. Keeping the *name* leaves the
    /// field-name set — and the always-on typo-hint discovery that reads it —
    /// byte-identical to a full parse; `KeyFilterStage` drops the placeholders
    /// before output. This is the "nearly free in the span parser" projection
    /// the spec anticipated.
    ///
    /// Semantics mirror the previous char-by-char parser exactly (verified by the
    /// unit tests and the differential matrix): key errors, `""`/`\x` escape
    /// handling, unterminated quotes running to end of line, and numeric/boolean
    /// coercion are all preserved.
    fn parse_into_event(
        &self,
        line: &str,
        event: &mut Event,
        projection: Option<&Projection>,
    ) -> Result<(), String> {
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Skip inter-pair whitespace.
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i >= len {
                break;
            }

            // Key: bytes up to '='. A space/tab before '=' is an error, matching
            // the original parser.
            let key_start = i;
            while i < len {
                match bytes[i] {
                    b'=' => break,
                    b' ' | b'\t' => return Err("Key cannot contain spaces".to_string()),
                    _ => i += 1,
                }
            }
            if i == key_start {
                return Err("Empty key found".to_string());
            }
            let key = &line[key_start..i];

            if i >= len || bytes[i] != b'=' {
                return Err(format!("Expected '=' after key '{}'", key));
            }
            i += 1; // consume '='

            // Whether this key's value must be materialized. Unwanted values are
            // still scanned (the cursor must advance past them correctly) but not
            // unescaped/coerced/allocated.
            let keep = projection.is_none_or(|p| p.wants(key));

            // Value.
            if i < len && bytes[i] == b'"' {
                i += 1; // opening quote
                let content_start = i;
                let mut needs_unescape = false;
                loop {
                    if i >= len {
                        break; // unterminated quote: value runs to end of line
                    }
                    match bytes[i] {
                        // A `""` pair is an escaped quote; a lone `"` closes.
                        b'"' => {
                            if i + 1 < len && bytes[i + 1] == b'"' {
                                needs_unescape = true;
                                i += 2;
                            } else {
                                break;
                            }
                        }
                        b'\\' => {
                            needs_unescape = true;
                            i += 2; // skip the escaped byte (clamped below)
                        }
                        _ => i += 1,
                    }
                }
                let raw = &line[content_start..i.min(len)];
                if i < len {
                    i += 1; // consume closing quote
                }
                if keep {
                    let value = if needs_unescape {
                        Cow::Owned(unescape_quoted_value(raw))
                    } else {
                        Cow::Borrowed(raw)
                    };
                    event.set_field(key, self.parse_value_to_dynamic(value));
                } else {
                    event.set_field(key, Dynamic::UNIT);
                }
            } else {
                // Unquoted value: read until whitespace or end of line.
                let value_start = i;
                while i < len && bytes[i] != b' ' && bytes[i] != b'\t' {
                    i += 1;
                }
                if keep {
                    event.set_field(
                        key,
                        self.parse_value_to_dynamic(Cow::Borrowed(&line[value_start..i])),
                    );
                } else {
                    event.set_field(key, Dynamic::UNIT);
                }
            }
        }

        Ok(())
    }

    /// Coerce a logfmt value into a Dynamic, borrowing where possible.
    fn parse_value_to_dynamic(&self, value: Cow<str>) -> Dynamic {
        let v: &str = value.as_ref();
        // Only coerce values that are syntactically valid JSON numbers. This
        // keeps zero-padded IDs ("007"), signed values ("+1555..."), and
        // inf/nan as strings rather than silently rewriting them.
        if looks_like_json_number(v) {
            // Try integer first
            if let Ok(i) = v.parse::<i64>() {
                return Dynamic::from(i);
            }
            // Try float (e.g. fractional, exponent, or integers beyond i64)
            if let Ok(f) = v.parse::<f64>() {
                return Dynamic::from(f);
            }
        }

        // Booleans, case-insensitive. Only ASCII case variants can lower-case to
        // "true"/"false", so this matches the original `to_lowercase()` compare
        // without allocating.
        if v.eq_ignore_ascii_case("true") {
            return Dynamic::from(true);
        }
        if v.eq_ignore_ascii_case("false") {
            return Dynamic::from(false);
        }

        // String value: reuse an owned (already-unescaped) buffer, or build one
        // ImmutableString from the borrowed span. A single allocation either way,
        // never the String-then-ImmutableString double allocation of before.
        match value {
            Cow::Owned(s) => Dynamic::from(s),
            Cow::Borrowed(s) => Dynamic::from(rhai::ImmutableString::from(s)),
        }
    }
}

/// Un-escape the raw content of a quoted logfmt value — the bytes between the
/// surrounding quotes, still containing `\x` escapes and `""` quote pairs.
/// Mirrors the original char-by-char decoder exactly, including a trailing lone
/// backslash decoding to nothing.
fn unescape_quoted_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            // Every `"` in `raw` is the first half of a `""` pair: drop the
            // second half and emit one quote.
            '"' => {
                let _ = chars.next();
                out.push('"');
            }
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => {} // trailing backslash emits nothing
            },
            other => out.push(other),
        }
    }
    out
}

impl EventParser for LogfmtParser {
    // The level is the raw text of a `level=...` value, so it appears verbatim
    // in the line (a quoted value like `level="error"` still contains the token
    // as a substring).
    fn level_appears_verbatim(&self) -> bool {
        true
    }

    fn supports_projection(&self) -> bool {
        true
    }

    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let content = line.trim();

        // Pre-size the field map by counting '=' delimiters (a cheap SIMD scan).
        // A '=' inside a quoted value slightly over-counts, which only over-
        // reserves capacity — it never forces a re-hash mid-parse, and it avoids
        // the per-line map growth that inserting into an unsized map would cause.
        let capacity = memchr::memchr_iter(b'=', content.as_bytes()).count();
        let mut event = Event::with_capacity(line.to_string(), capacity);

        self.parse_into_event(content, &mut event, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Extract timestamp from the parsed data
        if self.auto_timestamp {
            event.extract_timestamp();
        }
        Ok(event)
    }

    fn parse_projected(&self, line: &str, projection: &Projection) -> Result<Event> {
        if projection.is_all() {
            return self.parse(line);
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let content = line.trim();

        let capacity = memchr::memchr_iter(b'=', content.as_bytes()).count();
        let mut event = Event::with_capacity(line.to_string(), capacity);

        // Wanted keys get their real (parsed) value; unwanted keys keep only
        // their name with a `UNIT` placeholder (see `parse_into_event`).
        self.parse_into_event(content, &mut event, Some(projection))
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        if self.auto_timestamp {
            event.extract_timestamp();
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::EventParser;
    use crate::projection::Projection;

    fn only(fields: &[&str]) -> Projection {
        Projection::Only(fields.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn projected_materializes_wanted_and_placeholders_unwanted() {
        let parser = LogfmtParser::new();
        let line = r#"level=error msg="boom bang" count=42 flag=true q="a=b c""#;
        let ev = parser
            .parse_projected(line, &only(&["level", "count"]))
            .unwrap();

        assert_eq!(
            ev.fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "error"
        );
        // Wanted numeric value is still type-coerced.
        assert_eq!(ev.fields.get("count").unwrap().as_int().unwrap(), 42);
        // Unwanted keys keep their name but collapse to UNIT (quoted value with
        // embedded '=' is skipped correctly, so parse position is preserved).
        for k in ["msg", "flag", "q"] {
            assert!(ev.fields.contains_key(k), "name {k} preserved");
            assert!(ev.fields.get(k).unwrap().is_unit(), "{k} must be UNIT");
        }
        // Same field-name set (and order) as a full parse.
        let full = parser.parse(line).unwrap();
        assert_eq!(
            ev.fields.keys().collect::<Vec<_>>(),
            full.fields.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn projected_key_errors_match_plain() {
        let parser = LogfmtParser::new();
        for bad in ["key value", "=value", "key with spaces=value"] {
            let projected = parser.parse_projected(bad, &only(&["key"]));
            let plain = parser.parse(bad);
            assert_eq!(projected.is_err(), plain.is_err(), "input: {bad}");
        }
    }

    #[test]
    fn projected_unwanted_quoted_with_escapes_advances_correctly() {
        // A skipped quoted value containing escapes and an embedded '=' must not
        // desync the cursor; the following wanted key must still parse.
        let parser = LogfmtParser::new();
        let line = r#"skip="a=\"b\" c" level=warn"#;
        let ev = parser.parse_projected(line, &only(&["level"])).unwrap();
        assert!(ev.fields.get("skip").unwrap().is_unit());
        assert_eq!(
            ev.fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "warn"
        );
    }

    #[test]
    fn test_logfmt_parser_basic() {
        let parser = LogfmtParser::new();
        let result =
            EventParser::parse(&parser, r#"level=info message="test message" count=42"#).unwrap();

        assert_eq!(
            result
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "info"
        );
        assert_eq!(
            result
                .fields
                .get("message")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "test message"
        );
        assert!(result.fields.get("count").is_some());
        assert_eq!(result.fields.get("count").unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_logfmt_parser_types() {
        let parser = LogfmtParser::new();
        let result = EventParser::parse(
            &parser,
            r#"str="hello" int=123 float=2.5 bool_true=true bool_false=false"#,
        )
        .unwrap();

        assert_eq!(
            result
                .fields
                .get("str")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "hello"
        );
        assert_eq!(result.fields.get("int").unwrap().as_int().unwrap(), 123);
        assert_eq!(result.fields.get("float").unwrap().as_float().unwrap(), 2.5);
        assert!(result.fields.get("bool_true").unwrap().as_bool().unwrap());
        assert!(!result.fields.get("bool_false").unwrap().as_bool().unwrap());
    }

    #[test]
    fn test_logfmt_parser_quoted_values() {
        let parser = LogfmtParser::new();
        let result = EventParser::parse(
            &parser,
            r#"key1="value with spaces" key2="value with \"quotes\"" key3=simple"#,
        )
        .unwrap();

        assert_eq!(
            result
                .fields
                .get("key1")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "value with spaces"
        );
        assert_eq!(
            result
                .fields
                .get("key2")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "value with \"quotes\""
        );
        assert_eq!(
            result
                .fields
                .get("key3")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "simple"
        );
    }

    #[test]
    fn test_logfmt_parser_escape_sequences() {
        let parser = LogfmtParser::new();
        let result = EventParser::parse(
            &parser,
            r#"newline="line1\nline2" tab="col1\tcol2" backslash="back\\slash""#,
        )
        .unwrap();

        assert_eq!(
            result
                .fields
                .get("newline")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "line1\nline2"
        );
        assert_eq!(
            result
                .fields
                .get("tab")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "col1\tcol2"
        );
        assert_eq!(
            result
                .fields
                .get("backslash")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "back\\slash"
        );
    }

    #[test]
    fn test_logfmt_parser_empty_values() {
        let parser = LogfmtParser::new();
        let result =
            EventParser::parse(&parser, r#"empty="" quoted_empty="" unquoted_value=value"#)
                .unwrap();

        assert_eq!(
            result
                .fields
                .get("empty")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            ""
        );
        assert_eq!(
            result
                .fields
                .get("quoted_empty")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            ""
        );
        assert_eq!(
            result
                .fields
                .get("unquoted_value")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "value"
        );
    }

    #[test]
    fn test_logfmt_parser_core_fields() {
        let parser = LogfmtParser::new();
        let result = EventParser::parse(
            &parser,
            r#"timestamp=2023-01-01T12:00:00Z level=error message="Connection failed" user=alice"#,
        )
        .unwrap();

        // Core fields should be accessible through fields map
        assert_eq!(
            result
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "error"
        );
        assert_eq!(
            result
                .fields
                .get("message")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Connection failed"
        );
        assert!(result.parsed_ts.is_some());

        // Other fields should be available
        assert_eq!(
            result
                .fields
                .get("user")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice"
        );
    }

    #[test]
    fn test_logfmt_parser_errors() {
        let parser = LogfmtParser::new();

        // Missing equals sign
        assert!(EventParser::parse(&parser, "key value").is_err());

        // Empty key
        assert!(EventParser::parse(&parser, "=value").is_err());

        // Key with spaces
        assert!(EventParser::parse(&parser, "key with spaces=value").is_err());
    }
}
