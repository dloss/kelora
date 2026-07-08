//! Invariant property test for the timestamp fast-path pre-filter (spec §6).
//!
//! The whole optimization rests on one guarantee: for any line, a parser's
//! `extract_ts_only(line)` is either `None` ("unknown — do the full parse") or
//! exactly equal to the `parsed_ts` the full `parse(line)` would produce. If
//! the fast path ever returned a *different* `Some(ts)`, the pre-filter could
//! drop a line the real timestamp filter would have kept (or vice versa) — a
//! silent correctness bug. This test hammers that invariant with both
//! hand-written adversarial cases and randomly generated corpora.

use kelora::event::TIMESTAMP_FIELD_NAMES;
use kelora::parsers::{JsonlParser, LogfmtParser};
use kelora::pipeline::EventParser;
use proptest::prelude::*;

/// The full parse's resulting `parsed_ts` (the ground truth the fast path must
/// match), or `None` if the line does not parse or has no usable timestamp.
fn full_parse_ts<P: EventParser>(parser: &P, line: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    parser.parse(line).ok().and_then(|e| e.parsed_ts)
}

/// Assert the core invariant for one line and one parser.
fn assert_invariant<P: EventParser>(parser: &P, line: &str) {
    let fast = parser.extract_ts_only(line);
    if let Some(fast_ts) = fast {
        let full = full_parse_ts(parser, line);
        assert_eq!(
            Some(fast_ts),
            full,
            "extract_ts_only diverged from full parse for line: {line:?} \
             (fast={fast_ts:?}, full={full:?})"
        );
    }
    // fast == None is always acceptable — it just defers to the full parse.
}

// --- hand-written adversarial cases -----------------------------------------

#[test]
fn logfmt_hand_cases() {
    let parser = LogfmtParser::new();
    let cases = [
        // Plain ISO string timestamps under each field name.
        "ts=2026-01-01T12:00:00Z level=info msg=hi",
        "time=2026-01-01T12:00:00Z level=info",
        "timestamp=2026-01-01T12:00:00Z msg=x",
        // Precedence: `ts` wins over `time` regardless of line order.
        "time=2026-06-06T06:06:06Z ts=2026-01-01T00:00:00Z",
        "ts=2026-01-01T00:00:00Z time=2026-06-06T06:06:06Z",
        // Numeric unix timestamps (coerced to int/float by the full parser).
        "ts=1735689600 level=info",
        "ts=1735689600.5 level=info",
        // Non-convertible highest-precedence field falls through.
        "ts=true time=2026-01-01T00:00:00Z",
        // Unparseable timestamp string -> both yield None.
        "ts=not-a-timestamp level=info",
        // No timestamp field at all.
        "level=info msg=hello count=3",
        // Duplicate ts key: last value wins.
        "ts=2026-01-01T00:00:00Z ts=2026-02-02T00:00:00Z",
        // Quoted values.
        r#"ts="2026-01-01T12:00:00Z" msg="hello world""#,
        // Malformed logfmt -> extract_ts_only must return None.
        "this is not logfmt at all",
        "key with spaces=value",
        "=missing_key",
        "",
        "   ",
    ];
    for line in cases {
        assert_invariant(&parser, line);
    }
}

#[test]
fn json_hand_cases() {
    let parser = JsonlParser::new();
    let cases = [
        r#"{"ts":"2026-01-01T12:00:00Z","level":"info"}"#,
        r#"{"time":"2026-01-01T12:00:00Z"}"#,
        r#"{"timestamp":"2026-01-01T12:00:00Z","msg":"x"}"#,
        // Precedence by field-name list, not line order.
        r#"{"time":"2026-06-06T06:06:06Z","ts":"2026-01-01T00:00:00Z"}"#,
        r#"{"ts":"2026-01-01T00:00:00Z","time":"2026-06-06T06:06:06Z"}"#,
        // Numeric ts -> fast path bails (None), full parse handles it.
        r#"{"ts":1735689600}"#,
        r#"{"ts":1735689600.5}"#,
        // Non-string highest-precedence field -> fast path bails.
        r#"{"ts":true,"time":"2026-01-01T00:00:00Z"}"#,
        r#"{"ts":null,"time":"2026-01-01T00:00:00Z"}"#,
        // Nested object must NOT be treated as a top-level ts.
        r#"{"meta":{"ts":"2026-01-01T00:00:00Z"},"level":"info"}"#,
        r#"{"a":[1,2,{"ts":"2026-01-01T00:00:00Z"}],"level":"info"}"#,
        // ts appears inside another value string, not as a key.
        r#"{"msg":"\"ts\":\"2026-01-01\"","level":"info"}"#,
        // Escaped ts value -> bail.
        r#"{"ts":"2026-01-01T12:00:00ZZ"}"#,
        // Unparseable ts string -> both None.
        r#"{"ts":"not-a-timestamp"}"#,
        // No ts field.
        r#"{"level":"info","msg":"hello","count":3}"#,
        // Duplicate ts key: last wins (serde semantics).
        r#"{"ts":"2026-01-01T00:00:00Z","ts":"2026-02-02T00:00:00Z"}"#,
        // Trailing garbage / not an object -> full parse rejects; fast path None.
        r#"{"ts":"2026-01-01T00:00:00Z"} trailing"#,
        r#"[1,2,3]"#,
        r#"not json"#,
        r#"{"ts":"2026-01-01T00:00:00Z""#,
        r#"{}"#,
        "",
        // Whitespace around structure.
        r#"  {  "ts" : "2026-01-01T12:00:00Z" , "x" : 1 }  "#,
    ];
    for line in cases {
        assert_invariant(&parser, line);
    }
}

// --- randomized property tests ----------------------------------------------

/// A grab-bag of timestamp-shaped and junk value strings.
fn arb_value() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("2026-01-01T12:00:00Z".to_string()),
        Just("2026-01-01 12:00:00".to_string()),
        Just("1735689600".to_string()),
        Just("1735689600.25".to_string()),
        Just("2026-13-99T99:99:99Z".to_string()), // invalid
        Just("true".to_string()),
        Just("null".to_string()),
        Just("42".to_string()),
        "[a-zA-Z0-9:_./+-]{0,20}",
    ]
}

fn arb_key() -> impl Strategy<Value = String> {
    let mut keys: Vec<String> = TIMESTAMP_FIELD_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    keys.push("level".to_string());
    keys.push("msg".to_string());
    keys.push("count".to_string());
    proptest::sample::select(keys)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// Random logfmt lines: extract_ts_only must never diverge from full parse.
    #[test]
    fn prop_logfmt_invariant(pairs in prop::collection::vec((arb_key(), arb_value()), 0..8)) {
        let parser = LogfmtParser::new();
        let line = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_invariant(&parser, &line);
    }

    /// Random JSON objects with string values.
    #[test]
    fn prop_json_string_values_invariant(
        pairs in prop::collection::vec((arb_key(), arb_value()), 0..8)
    ) {
        let parser = JsonlParser::new();
        let body = pairs
            .iter()
            .map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), serde_json::to_string(v).unwrap()))
            .collect::<Vec<_>>()
            .join(",");
        let line = format!("{{{body}}}");
        assert_invariant(&parser, &line);
    }

    /// Random JSON objects with *mixed* value types (strings, numbers, bools,
    /// null, nested) — exercises the scanner's bail paths.
    #[test]
    fn prop_json_mixed_values_invariant(
        keys in prop::collection::vec(arb_key(), 0..8),
        seed in any::<u64>(),
    ) {
        let parser = JsonlParser::new();
        let mut s = seed;
        let mut next = || { s = s.wrapping_mul(6364136223846793005).wrapping_add(1); (s >> 33) as u32 };
        let body = keys
            .iter()
            .map(|k| {
                let kj = serde_json::to_string(k).unwrap();
                let v = match next() % 6 {
                    0 => "\"2026-01-01T12:00:00Z\"".to_string(),
                    1 => "1735689600".to_string(),
                    2 => "true".to_string(),
                    3 => "null".to_string(),
                    4 => "{\"nested\":\"2026-01-01T00:00:00Z\"}".to_string(),
                    _ => "\"junk\"".to_string(),
                };
                format!("{kj}:{v}")
            })
            .collect::<Vec<_>>()
            .join(",");
        let line = format!("{{{body}}}");
        assert_invariant(&parser, &line);
    }

    /// Arbitrary bytes as a "line" must never make the fast path diverge or panic.
    #[test]
    fn prop_json_arbitrary_text(line in "\\PC{0,60}") {
        let parser = JsonlParser::new();
        assert_invariant(&parser, &line);
    }

    #[test]
    fn prop_logfmt_arbitrary_text(line in "\\PC{0,60}") {
        let parser = LogfmtParser::new();
        assert_invariant(&parser, &line);
    }
}
