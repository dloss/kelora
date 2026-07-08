use crate::event::{Event, FieldMap};
use crate::pipeline::EventParser;
use anyhow::Result;
use rhai::Dynamic;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// Tidy up a `serde_json` parse error for display. Each input line is parsed
/// independently, so serde always reports `line 1`, which collides confusingly
/// with kelora's own line counter in the surrounding diagnostic (`line 3:
/// Invalid JSON: ... at line 1 column 1`). Drop the redundant `line 1` while
/// keeping the column; multi-line events (line > 1) keep their full location.
fn clean_json_error(e: &serde_json::Error) -> String {
    e.to_string().replace(" at line 1 column ", " at column ")
}

/// A `rhai::Dynamic` deserialized directly from JSON, skipping the
/// `serde_json::Value` intermediate tree. Number/nesting semantics mirror
/// [`crate::event::json_to_dynamic_owned`] exactly.
struct DynamicValue(Dynamic);

struct DynVisitor;

impl<'de> Visitor<'de> for DynVisitor {
    type Value = Dynamic;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Dynamic, E> {
        Ok(Dynamic::from(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Dynamic, E> {
        Ok(Dynamic::from(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Dynamic, E> {
        // Match json_to_dynamic_owned: prefer i64, fall back to u64 to avoid precision loss.
        Ok(if v <= i64::MAX as u64 {
            Dynamic::from(v as i64)
        } else {
            Dynamic::from(v)
        })
    }

    fn visit_f64<E>(self, v: f64) -> Result<Dynamic, E> {
        Ok(Dynamic::from(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Dynamic, E> {
        Ok(Dynamic::from(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Dynamic, E> {
        Ok(Dynamic::from(v))
    }

    fn visit_unit<E>(self) -> Result<Dynamic, E> {
        Ok(Dynamic::UNIT)
    }

    fn visit_none<E>(self) -> Result<Dynamic, E> {
        Ok(Dynamic::UNIT)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Dynamic, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Dynamic, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut arr = rhai::Array::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(DynamicValue(v)) = seq.next_element()? {
            arr.push(v);
        }
        Ok(Dynamic::from(arr))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Dynamic, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut m = rhai::Map::new();
        while let Some(k) = map.next_key::<String>()? {
            let DynamicValue(v) = map.next_value()?;
            m.insert(k.into(), v);
        }
        Ok(Dynamic::from(m))
    }
}

impl<'de> Deserialize<'de> for DynamicValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DynVisitor).map(DynamicValue)
    }
}

/// Top-level object: deserialized straight into a `FieldMap`, avoiding both the
/// `serde_json::Value::Object` indexmap and a second pass to build our map.
struct EventFields(FieldMap);

struct FieldMapVisitor;

impl<'de> Visitor<'de> for FieldMapVisitor {
    type Value = FieldMap;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<FieldMap, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = FieldMap::with_capacity_and_hasher(
            map.size_hint().unwrap_or(0),
            ahash::RandomState::default(),
        );
        while let Some(k) = map.next_key::<String>()? {
            let DynamicValue(v) = map.next_value()?;
            fields.insert(k, v);
        }
        Ok(fields)
    }
}

impl<'de> Deserialize<'de> for EventFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer
            .deserialize_map(FieldMapVisitor)
            .map(EventFields)
    }
}

pub struct JsonlParser {
    auto_timestamp: bool,
    strict: bool,
}

impl JsonlParser {
    pub fn new() -> Self {
        Self {
            auto_timestamp: true,
            strict: false,
        }
    }

    pub fn new_without_auto_timestamp() -> Self {
        Self {
            auto_timestamp: false,
            strict: false,
        }
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// Return the byte index of `key` in [`TIMESTAMP_FIELD_NAMES`], or `None`.
/// Lower index = higher precedence, matching `identify_timestamp_field`.
fn ts_key_precedence(key: &[u8]) -> Option<usize> {
    crate::event::TIMESTAMP_FIELD_NAMES
        .iter()
        .position(|name| name.as_bytes() == key)
}

/// Skip ASCII JSON whitespace starting at `i`.
#[inline]
fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Scan a JSON string beginning at `bytes[i] == b'"'`. Returns
/// `(content_start, content_end, has_escape, index_after_closing_quote)`, or
/// `None` if unterminated. Does not validate escape sequences — it only tracks
/// whether any backslash escape is present so the caller can bail.
fn scan_string(bytes: &[u8], i: usize) -> Option<(usize, usize, bool, usize)> {
    let start = i + 1;
    let mut j = start;
    let mut has_escape = false;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => {
                has_escape = true;
                j += 2; // skip the escaped byte; loop guard catches overrun
            }
            b'"' => return Some((start, j, has_escape, j + 1)),
            _ => j += 1,
        }
    }
    None
}

/// Skip a balanced object/array beginning at `bytes[i]` (`{` or `[`), honoring
/// strings so braces inside string values don't miscount. Returns the index
/// just past the matching close, or `None` if unbalanced.
fn skip_balanced(bytes: &[u8], i: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'"' => {
                let (_, _, _, next) = scan_string(bytes, j)?;
                j = next;
            }
            b'{' | b'[' => {
                depth += 1;
                j += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                j += 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => j += 1,
        }
    }
    None
}

/// Skip a scalar value (number/`true`/`false`/`null`) up to the next
/// delimiter. Returns the delimiter index, or `None` on a surprising byte.
fn skip_scalar(bytes: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r' => return Some(j),
            b'"' | b'{' | b'[' => return None, // structure where a scalar was expected
            _ => j += 1,
        }
    }
    None
}

/// Timestamp fast path for JSON: a single-pass, allocation-free scan of the
/// top-level object that extracts only the timestamp needed by
/// `--since`/`--until`. It walks the object structure (respecting strings and
/// nesting) and records, among the top-level keys, the highest-precedence
/// timestamp field name that is present. It fires only when that field's value
/// is a plain (escape-free) JSON string, exactly the shape whose decoded value
/// equals its raw bytes — so the string it parses is byte-for-byte what the
/// full parse would hand to the timestamp parser. On anything ambiguous
/// (escapes in a key, a non-string or escaped top-level ts value, trailing
/// garbage, or malformed structure) it returns `None`, deferring to the full
/// parse. `None` is always safe (see [`EventParser::extract_ts_only`]).
fn json_extract_ts(line: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let line = line.trim_end_matches('\n').trim_end_matches('\r');
    let bytes = line.trim().as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let len = bytes.len();
    let mut i = 1usize;

    // Highest-precedence top-level ts key seen so far, and whether its value was
    // a clean (escape-free) string plus that value's byte range.
    let mut best_idx = usize::MAX;
    let mut best_clean_string: Option<(usize, usize)> = None;

    let close;
    loop {
        i = skip_ws(bytes, i);
        if i >= len {
            return None; // unterminated object
        }
        if bytes[i] == b'}' {
            close = i + 1;
            break;
        }
        if bytes[i] != b'"' {
            return None; // expected a string key
        }

        // Key.
        let (ks, ke, key_has_escape, next) = scan_string(bytes, i)?;
        if key_has_escape {
            return None; // escapes in the key region — bail (spec §3.1)
        }
        i = next;
        let key = &bytes[ks..ke];
        let ts_idx = ts_key_precedence(key);

        i = skip_ws(bytes, i);
        if i >= len || bytes[i] != b':' {
            return None;
        }
        i = skip_ws(bytes, i + 1);
        if i >= len {
            return None;
        }

        // Value.
        match bytes[i] {
            b'"' => {
                let (vs, ve, v_has_escape, next) = scan_string(bytes, i)?;
                i = next;
                if let Some(idx) = ts_idx {
                    if idx <= best_idx {
                        best_idx = idx;
                        best_clean_string = if v_has_escape { None } else { Some((vs, ve)) };
                    }
                }
            }
            b'{' | b'[' => {
                i = skip_balanced(bytes, i)?;
                if let Some(idx) = ts_idx {
                    if idx <= best_idx {
                        best_idx = idx;
                        best_clean_string = None; // non-string top-level ts value
                    }
                }
            }
            _ => {
                i = skip_scalar(bytes, i)?;
                if let Some(idx) = ts_idx {
                    if idx <= best_idx {
                        best_idx = idx;
                        best_clean_string = None; // number/bool/null ts value
                    }
                }
            }
        }

        i = skip_ws(bytes, i);
        if i >= len {
            return None;
        }
        match bytes[i] {
            b',' => i += 1,
            b'}' => {
                close = i + 1;
                break;
            }
            _ => return None,
        }
    }

    // The object must be the entire input; trailing non-whitespace means serde
    // would reject the line, so defer to the full parse rather than act on it.
    if skip_ws(bytes, close) != len {
        return None;
    }

    // Fire only when the winning top-level ts field is a clean string.
    let (vs, ve) = best_clean_string?;
    let ts_str = std::str::from_utf8(&bytes[vs..ve]).ok()?;
    crate::timestamp::with_thread_local_parser(|parser| {
        parser.parse_ts_with_config(ts_str, None, None)
    })
}

impl EventParser for JsonlParser {
    // The level is read straight from the JSON string value, so it appears
    // verbatim in the line text (the only theoretical exception — a `\uXXXX`
    // escaped level value like "error" — never occurs in practice and is
    // documented as a known limitation of the pre-filter).
    fn level_appears_verbatim(&self) -> bool {
        true
    }

    // Consistent with the full parse only under the default timestamp config
    // (see `LogfmtParser::supports_ts_fast_path`).
    fn supports_ts_fast_path(&self) -> bool {
        self.auto_timestamp
    }

    fn extract_ts_only(&self, line: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        if !self.auto_timestamp {
            return None;
        }
        json_extract_ts(line)
    }

    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

        // Fast path: objects (the overwhelmingly common case) deserialize
        // straight into the FieldMap, skipping the serde_json::Value tree.
        // Non-objects fall through to the slow path purely to reproduce the
        // exact "Expected JSON object" error.
        if line.trim_start().as_bytes().first() == Some(&b'{') {
            let EventFields(fields) = serde_json::from_str(line)
                .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", clean_json_error(&e)))?;
            let mut event = Event::with_fields(line.to_string(), fields);
            if self.auto_timestamp {
                event.extract_timestamp();
            }
            return Ok(event);
        }

        let json_value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", clean_json_error(&e)))?;
        Err(anyhow::anyhow!("Expected JSON object, got: {}", json_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::EventParser;

    #[test]
    fn test_json_parser_basic() {
        let parser = JsonlParser::new();
        let result =
            EventParser::parse(&parser, r#"{"level":"info","message":"test","count":42}"#).unwrap();

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
            "test"
        );
        assert!(result.fields.get("count").is_some());
        assert_eq!(result.fields.get("count").unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_json_parser_complex() {
        let parser = JsonlParser::new();
        let result = EventParser::parse(
            &parser,
            r#"{"timestamp":"2023-01-01T12:00:00Z","level":"error","user":"alice","status":404}"#,
        )
        .unwrap();

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
        assert!(result.fields.get("user").is_some());
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
        assert!(result.fields.get("status").is_some());
        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 404);
    }
}
