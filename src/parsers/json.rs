use crate::event::{Event, FieldMap};
use crate::pipeline::EventParser;
use crate::projection::Projection;
use anyhow::Result;
use rhai::Dynamic;
use serde::de::{
    Deserialize, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
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
        // Build the ImmutableString straight from the borrowed span: one
        // allocation, not the String-then-ImmutableString pair of
        // `from(v.to_string())`. serde_json calls this for string values with no
        // escapes (the common case); escaped values go through visit_string.
        Ok(Dynamic::from(rhai::ImmutableString::from(v)))
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

/// Projection-aware top-level object deserializer. Keys the projection wants are
/// materialized into `Dynamic`s exactly as the plain path does. For unwanted
/// keys the value is parsed straight into `IgnoredAny` — skipping the (possibly
/// deep) `Dynamic` allocation, which is the bulk of the per-field cost — and a
/// cheap `Dynamic::UNIT` placeholder is inserted under the real key.
///
/// The placeholder (rather than skipping the insert outright) keeps the *set of
/// field names* identical to a full parse, so the always-on field-name
/// discovery that powers the `-k` typo hint stays byte-identical. Insertion
/// order is preserved too, so nothing downstream can observe the difference —
/// `KeyFilterStage` drops every unwanted (placeholder) field before output, and
/// projection is only ever active when such a stage is present. The JSON is
/// still fully tokenized, so a syntax error anywhere (even inside a skipped
/// value) surfaces identically to the plain path.
struct ProjectedFieldMapVisitor<'p> {
    projection: &'p Projection,
}

impl<'de> Visitor<'de> for ProjectedFieldMapVisitor<'_> {
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
            if self.projection.wants(&k) {
                let DynamicValue(v) = map.next_value()?;
                fields.insert(k, v);
            } else {
                map.next_value::<IgnoredAny>()?;
                fields.insert(k, Dynamic::UNIT);
            }
        }
        Ok(fields)
    }
}

/// Carries the projection into the serde deserialization without a
/// `serde_json::Value` intermediate.
struct EventFieldsSeed<'p> {
    projection: &'p Projection,
}

impl<'de> DeserializeSeed<'de> for EventFieldsSeed<'_> {
    type Value = FieldMap;

    fn deserialize<D>(self, deserializer: D) -> Result<FieldMap, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(ProjectedFieldMapVisitor {
            projection: self.projection,
        })
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

impl EventParser for JsonlParser {
    // The level is read straight from the JSON string value, so it appears
    // verbatim in the line text (the only theoretical exception — a `\uXXXX`
    // escaped level value like "error" — never occurs in practice and is
    // documented as a known limitation of the pre-filter).
    fn level_appears_verbatim(&self) -> bool {
        true
    }

    fn supports_projection(&self) -> bool {
        true
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

    fn parse_projected(&self, line: &str, projection: &Projection) -> Result<Event> {
        // No pushdown, or a non-object line (whose error path lives in `parse`):
        // defer to the plain path so behavior is byte-identical.
        if projection.is_all() {
            return self.parse(line);
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        if line.trim_start().as_bytes().first() != Some(&b'{') {
            return self.parse(line);
        }

        // Drive the deserializer with a seed so unwanted values become
        // `IgnoredAny` (no `Dynamic` alloc). `end()` reproduces the trailing-
        // data check that `from_str` performs, keeping errors identical.
        let mut de = serde_json::Deserializer::from_str(line);
        let fields = EventFieldsSeed { projection }
            .deserialize(&mut de)
            .and_then(|f| de.end().map(|()| f))
            .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", clean_json_error(&e)))?;
        let mut event = Event::with_fields(line.to_string(), fields);
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
        let parser = JsonlParser::new();
        let line = r#"{"level":"error","msg":"boom","nested":{"a":[1,2,3]},"n":42}"#;
        let ev = parser.parse_projected(line, &only(&["level"])).unwrap();

        // Wanted field keeps its real value.
        assert_eq!(
            ev.fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "error"
        );
        // Unwanted fields keep their NAME (so field discovery is unchanged) but
        // are collapsed to UNIT — no Dynamic value was built.
        for k in ["msg", "nested", "n"] {
            assert!(ev.fields.contains_key(k), "name {k} must be preserved");
            assert!(ev.fields.get(k).unwrap().is_unit(), "{k} must be UNIT");
        }
        // Field-name set matches a full parse (order preserved).
        let full = parser.parse(line).unwrap();
        assert_eq!(
            ev.fields.keys().collect::<Vec<_>>(),
            full.fields.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn projected_all_equals_plain_parse() {
        let parser = JsonlParser::new();
        let line = r#"{"level":"info","msg":"x","k":1}"#;
        let projected = parser.parse_projected(line, &Projection::All).unwrap();
        let plain = parser.parse(line).unwrap();
        assert_eq!(
            projected.fields.get("k").unwrap().as_int().unwrap(),
            plain.fields.get("k").unwrap().as_int().unwrap()
        );
    }

    #[test]
    fn projected_invalid_json_errors_like_plain() {
        let parser = JsonlParser::new();
        let bad = r#"{"level":"error","broken":}"#;
        let projected = parser.parse_projected(bad, &only(&["level"]));
        let plain = parser.parse(bad);
        assert!(projected.is_err() && plain.is_err());
        assert_eq!(
            projected.unwrap_err().to_string(),
            plain.unwrap_err().to_string()
        );
    }

    #[test]
    fn projected_preserves_timestamp_extraction() {
        // `ts` is a projection-wanted field here; parsed_ts must still populate.
        let parser = JsonlParser::new();
        let line = r#"{"ts":"2026-01-01T00:00:00Z","level":"info","drop":"me"}"#;
        let ev = parser
            .parse_projected(line, &only(&["ts", "level"]))
            .unwrap();
        assert!(ev.parsed_ts.is_some());
    }

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
