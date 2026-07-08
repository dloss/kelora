use crate::event::Event;
use crate::parsers::type_conversion::looks_like_json_number;
use crate::pipeline::EventParser;
use anyhow::Result;
use rhai::Dynamic;

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

    /// Parse logfmt line: key1=value1 key2="value with spaces" key3=value3
    /// Adapted from Stelp but converted to work with Kelora's Dynamic system
    fn parse_logfmt_pairs(&self, line: &str) -> Result<Vec<(String, String)>, String> {
        let mut pairs = Vec::new();
        let mut chars = line.chars().peekable();

        while chars.peek().is_some() {
            // Skip whitespace
            while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                chars.next();
            }

            if chars.peek().is_none() {
                break;
            }

            // Parse key
            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' {
                    break;
                } else if ch == ' ' || ch == '\t' {
                    return Err("Key cannot contain spaces".to_string());
                } else {
                    key.push(chars.next().unwrap());
                }
            }

            if key.is_empty() {
                return Err("Empty key found".to_string());
            }

            // Expect '='
            if chars.next() != Some('=') {
                return Err(format!("Expected '=' after key '{}'", key));
            }

            // Parse value
            let mut value = String::new();
            if chars.peek() == Some(&'"') {
                // Quoted value
                chars.next(); // consume opening quote
                while let Some(ch) = chars.next() {
                    if ch == '"' {
                        // Check for escaped quote
                        if chars.peek() == Some(&'"') {
                            chars.next(); // consume escaped quote
                            value.push('"');
                        } else {
                            break; // end of quoted value
                        }
                    } else if ch == '\\' {
                        // Handle escape sequences
                        if let Some(escaped_ch) = chars.next() {
                            match escaped_ch {
                                'n' => value.push('\n'),
                                't' => value.push('\t'),
                                'r' => value.push('\r'),
                                '\\' => value.push('\\'),
                                '"' => value.push('"'),
                                _ => {
                                    value.push('\\');
                                    value.push(escaped_ch);
                                }
                            }
                        }
                    } else {
                        value.push(ch);
                    }
                }
            } else {
                // Unquoted value - read until space or end
                while let Some(&ch) = chars.peek() {
                    if ch == ' ' || ch == '\t' {
                        break;
                    } else {
                        value.push(chars.next().unwrap());
                    }
                }
            }

            pairs.push((key, value));
        }

        Ok(pairs)
    }

    /// Try to convert string to a numeric Dynamic value, falling back to string
    fn parse_value_to_dynamic(&self, value: String) -> Dynamic {
        // Only coerce values that are syntactically valid JSON numbers. This
        // keeps zero-padded IDs ("007"), signed values ("+1555..."), and
        // inf/nan as strings rather than silently rewriting them.
        if looks_like_json_number(&value) {
            // Try integer first
            if let Ok(i) = value.parse::<i64>() {
                return Dynamic::from(i);
            }

            // Try float (e.g. fractional, exponent, or integers beyond i64)
            if let Ok(f) = value.parse::<f64>() {
                return Dynamic::from(f);
            }
        }

        // Try boolean
        match value.to_lowercase().as_str() {
            "true" => return Dynamic::from(true),
            "false" => return Dynamic::from(false),
            _ => {}
        }

        // Default to string
        Dynamic::from(value)
    }
}

/// Convert a raw (unquoted) logfmt value into the exact timestamp string the
/// full path would hand to the parser, without allocating for the common
/// string case. This inlines `LogfmtParser::parse_value_to_dynamic` followed by
/// `timestamp::dynamic_to_timestamp_string`:
///  - a JSON-number-shaped value becomes its `i64`/`f64` rendering (as the full
///    path's numeric `Dynamic` would stringify),
///  - a case-insensitive `true`/`false` becomes a bool, which is *not*
///    convertible to a timestamp string (`None` → fall through to next field),
///  - everything else is a string whose timestamp form is itself (borrowed).
fn logfmt_value_to_ts_str(value: &str) -> Option<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;
    if looks_like_json_number(value) {
        if let Ok(i) = value.parse::<i64>() {
            return Some(Cow::Owned(i.to_string()));
        }
        if let Ok(f) = value.parse::<f64>() {
            return Some(Cow::Owned(format!("{:.9}", f)));
        }
    }
    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
        return None; // bool value -> not convertible to a timestamp
    }
    Some(Cow::Borrowed(value))
}

impl EventParser for LogfmtParser {
    // The level is the raw text of a `level=...` value, so it appears verbatim
    // in the line (a quoted value like `level="error"` still contains the token
    // as a substring).
    fn level_appears_verbatim(&self) -> bool {
        true
    }

    // The fast path is provably consistent with the full parse only when this
    // parser auto-extracts the timestamp with the default config. When
    // `--ts-field`/`--ts-format`/`--default-timezone` is set the parser is
    // built without auto-timestamp and wrapped in `TimestampConfiguredParser`,
    // which does not enable the fast path (spec §3.1/§4).
    fn supports_ts_fast_path(&self) -> bool {
        self.auto_timestamp
    }

    // Extract only the timestamp with an allocation-free scan of the same
    // key=value structure the full parse tokenizes, then run the winning field
    // through the *identical* conversion + parsing path
    // (`parse_value_to_dynamic` → `dynamic_to_timestamp_string` → the
    // thread-local adaptive parser with the default config). This mirrors
    // `Event::extract_timestamp` / `identify_timestamp_field` exactly: the first
    // timestamp field name (in `TIMESTAMP_FIELD_NAMES` precedence) that is
    // present and convertible wins, and its parse result — `Some` or `None` —
    // is returned verbatim. Only the single winning value is ever allocated.
    //
    // Bails to `None` (deferring to the full parse) on: any structure the full
    // tokenizer would reject as malformed (so the full parse reproduces the
    // identical error), or a *quoted* timestamp value (whose decoded form can
    // differ from its raw bytes) — quoted timestamps are rare and defer safely.
    fn extract_ts_only(&self, line: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        if !self.auto_timestamp {
            return None;
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let bytes = line.trim().as_bytes();
        let len = bytes.len();
        let mut i = 0usize;

        // Unquoted value slices of the timestamp fields we saw, indexed by
        // precedence in TIMESTAMP_FIELD_NAMES (last write wins). A stack array
        // of `Option<&str>` — no heap allocation.
        const N: usize = crate::event::TIMESTAMP_FIELD_NAMES.len();
        let mut hits: [Option<&str>; N] = [None; N];

        while i < len {
            // Skip inter-pair whitespace.
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i >= len {
                break;
            }

            // Key: up to '='. A space/tab in the key or a missing '=' is a
            // tokenizer error in the full parse, so bail.
            let key_start = i;
            while i < len && bytes[i] != b'=' {
                if bytes[i] == b' ' || bytes[i] == b'\t' {
                    return None;
                }
                i += 1;
            }
            if i >= len {
                return None; // no '=' after key
            }
            let key = &bytes[key_start..i];
            if key.is_empty() {
                return None; // empty key
            }
            i += 1; // consume '='
            let ts_idx = crate::event::TIMESTAMP_FIELD_NAMES
                .iter()
                .position(|name| name.as_bytes() == key);

            // Value.
            if i < len && bytes[i] == b'"' {
                // Quoted: find the closing quote honoring `\x` and `""` escapes
                // exactly as the full tokenizer does, so non-ts pairs are skipped
                // correctly. A quoted *timestamp* value bails (see above).
                i += 1;
                loop {
                    if i >= len {
                        break; // unterminated quote: full parser takes to EOL
                    }
                    match bytes[i] {
                        b'"' => {
                            if i + 1 < len && bytes[i + 1] == b'"' {
                                i += 2;
                            } else {
                                i += 1;
                                break;
                            }
                        }
                        b'\\' => i += 2,
                        _ => i += 1,
                    }
                }
                if ts_idx.is_some() {
                    return None; // quoted timestamp value -> defer to full parse
                }
            } else {
                // Unquoted: read until whitespace or end.
                let value_start = i;
                while i < len && bytes[i] != b' ' && bytes[i] != b'\t' {
                    i += 1;
                }
                if let Some(idx) = ts_idx {
                    hits[idx] = Some(std::str::from_utf8(&bytes[value_start..i]).ok()?);
                }
            }
        }

        // Resolve by field-name precedence, replicating `identify_timestamp_field`
        // (first present + convertible field wins; its parse result is returned).
        for value in hits.iter().flatten() {
            if let Some(ts_str) = logfmt_value_to_ts_str(value) {
                return crate::timestamp::with_thread_local_parser(|parser| {
                    parser.parse_ts_with_config(&ts_str, None, None)
                });
            }
            // Present but not convertible: fall through to the next field.
        }
        None
    }

    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let pairs = self
            .parse_logfmt_pairs(line.trim())
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Pre-allocate Event with capacity based on number of pairs
        let mut event = Event::with_capacity(line.to_string(), pairs.len());

        for (key, value) in pairs {
            // Convert string values to appropriate Dynamic types
            let dynamic_value = self.parse_value_to_dynamic(value);
            event.set_field(key, dynamic_value);
        }

        // Extract timestamp from the parsed data
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
