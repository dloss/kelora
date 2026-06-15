use super::*;
use crate::event::{ContextType, Event};
use crate::pipeline::Formatter;
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use rhai::{Array, Dynamic, Map};

fn parts(line: &str) -> Vec<String> {
    line.split('|')
        .map(|segment| segment.trim().to_string())
        .collect()
}

#[test]
fn test_json_formatter_empty_event() {
    let event = Event::default();
    let formatter = JsonFormatter::new();
    let result = formatter.format(&event);
    assert!(result.starts_with('{') && result.ends_with('}'));
}

#[test]
fn test_json_formatter_with_fields() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
    event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
    event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
    event.set_field("status".to_string(), Dynamic::from(200i64));

    let formatter = JsonFormatter::new();
    let result = formatter.format(&event);

    assert!(result.contains("\"level\":\"INFO\""));
    assert!(result.contains("\"msg\":\"Test message\""));
    assert!(result.contains("\"user\":\"alice\""));
    assert!(result.contains("\"status\":200"));
}

#[test]
fn test_inspect_formatter_basic() {
    let mut event = Event::default();
    event.set_field("message".to_string(), Dynamic::from("hello"));
    event.set_field("code".to_string(), Dynamic::from(42_i64));
    event.set_field("active".to_string(), Dynamic::from(true));

    let formatter = InspectFormatter::new(0);
    let output = formatter.format(&event);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines.len(), 4);
    assert_eq!(
        parts(lines[1]),
        vec![
            "message".to_string(),
            "string".to_string(),
            "\"hello\"".to_string()
        ]
    );
    assert_eq!(
        parts(lines[2]),
        vec!["code".to_string(), "int".to_string(), "42".to_string()]
    );
    assert_eq!(
        parts(lines[3]),
        vec!["active".to_string(), "bool".to_string(), "true".to_string()]
    );
}

#[test]
fn test_inspect_formatter_surfaces_meta() {
    let mut event = Event::default();
    event.set_field("message".to_string(), Dynamic::from("hello"));
    event.parsed_ts = Some(Utc.with_ymd_and_hms(2025, 10, 4, 8, 0, 0).unwrap());
    event.line_num = Some(7);
    event.filename = Some("app.log".to_string());

    let formatter = InspectFormatter::new(0);
    let output = formatter.format(&event);

    // meta.* values are advertised so users discover the global `meta` object
    // instead of re-parsing the string timestamp with to_datetime().
    assert!(
        output.contains("meta.parsed_ts"),
        "should surface parsed_ts: {output}"
    );
    assert!(
        output.contains("2025-10-04T08:00:00Z"),
        "parsed_ts rendered as UTC ISO: {output}"
    );
    assert!(output.contains("meta.line_num"), "{output}");
    assert!(output.contains("meta.filename"), "{output}");
}

#[test]
fn test_inspect_formatter_omits_absent_meta() {
    // Default event has no parsed_ts/line_num/filename (e.g. stdin, no
    // timestamp): the meta block stays quiet rather than printing empty rows.
    let mut event = Event::default();
    event.set_field("message".to_string(), Dynamic::from("hello"));

    let formatter = InspectFormatter::new(0);
    let output = formatter.format(&event);
    assert!(!output.contains("meta."), "{output}");
}

#[test]
fn test_inspect_formatter_nested_structure() {
    let mut inner = Map::new();
    inner.insert("id".into(), Dynamic::from(7_i64));
    inner.insert("name".into(), Dynamic::from("alpha"));

    let mut event = Event::default();
    event.set_field("meta".to_string(), Dynamic::from(inner));

    let formatter = InspectFormatter::new(0);
    let output = formatter.format(&event);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines.len(), 5);
    assert_eq!(
        parts(lines[1]),
        vec!["meta".to_string(), "map(2)".to_string(), "{".to_string()]
    );
    assert_eq!(
        parts(lines[2]),
        vec!["id".to_string(), "int".to_string(), "7".to_string()]
    );
    assert_eq!(
        parts(lines[3]),
        vec![
            "name".to_string(),
            "string".to_string(),
            "\"alpha\"".to_string()
        ]
    );
    assert_eq!(lines[4], "}");
}

#[test]
fn test_inspect_formatter_truncates_long_values() {
    let long_value = "a".repeat(120);
    let mut event = Event::default();
    event.set_field("payload".to_string(), Dynamic::from(long_value.clone()));

    let formatter = InspectFormatter::new(0);
    let output = formatter.format(&event);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines.len(), 2);
    let expected_truncated = format!("\"{}\"...", "a".repeat(80));
    assert_eq!(
        parts(lines[1]),
        vec![
            "payload".to_string(),
            "string".to_string(),
            expected_truncated.clone()
        ]
    );

    let verbose_formatter = InspectFormatter::new(2);
    let verbose_output = verbose_formatter.format(&event);
    let verbose_lines: Vec<&str> = verbose_output.lines().collect();
    assert_eq!(verbose_lines[0], "---");
    assert_eq!(verbose_lines.len(), 2);
    let expected_full = format!("\"{}\"", long_value);
    assert_eq!(
        parts(verbose_lines[1]),
        vec!["payload".to_string(), "string".to_string(), expected_full]
    );
    assert!(verbose_output.len() > output.len());
}

#[test]
fn test_default_formatter() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
    event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
    event.set_field("count".to_string(), Dynamic::from(42i64));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        false, // Disable wrapping for this test
        false,
        0, // No quiet mode
    ); // No colors, no brief mode, no wrapping
    let result = formatter.format(&event);

    // Check that all fields are present with proper formatting
    // Strings should be quoted with single quotes, numbers should not be
    assert!(result.contains("level='INFO'"));
    assert!(result.contains("user='alice'"));
    assert!(result.contains("count=42"));
    // Fields should be space-separated
    assert!(result.contains(" "));
}

#[test]
fn test_default_formatter_uses_ts_format_hint() {
    let mut event = Event::default();
    event.set_field("ts".to_string(), Dynamic::from("2000/01/01 17.59.55,210"));
    event.set_field("msg".to_string(), Dynamic::from("hello"));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig {
            format_fields: Vec::new(),
            auto_format_all: true,
            format_as_utc: true,
            parse_format_hint: Some("%Y/%m/%d %H.%M.%S,%f".to_string()),
            parse_timezone_hint: Some("UTC".to_string()),
        },
        false,
        false,
        0,
    );

    let result = formatter.format(&event);
    assert!(result.contains("ts='2000-01-01T17:59:55.210+00:00'"));
}

#[test]
fn test_default_formatter_nested_values_render_as_json() {
    let mut meta = Map::new();
    meta.insert("id".into(), Dynamic::from(7_i64));
    meta.insert("name".into(), Dynamic::from("alpha"));

    let tags: Array = vec![Dynamic::from("blue"), Dynamic::from("green")];

    let mut event = Event::default();
    event.set_field("meta".to_string(), Dynamic::from(meta));
    event.set_field("tags".to_string(), Dynamic::from(tags));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        false,
        false,
        0, // No quiet mode
    );
    let result = formatter.format(&event);

    assert!(result.contains("meta={\"id\":7,\"name\":\"alpha\"}"));
    assert!(result.contains("tags=[\"blue\",\"green\"]"));
}

#[test]
fn test_default_formatter_pretty_nested_output() {
    let mut meta = Map::new();
    meta.insert("id".into(), Dynamic::from(7_i64));
    meta.insert("name".into(), Dynamic::from("alpha"));

    let tags: Array = vec![Dynamic::from("blue"), Dynamic::from("green")];

    let mut event = Event::default();
    event.set_field("meta".to_string(), Dynamic::from(meta));
    event.set_field("tags".to_string(), Dynamic::from(tags));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        false,
        true,
        0, // No quiet mode
    );
    let result = formatter.format(&event);

    assert!(result.contains("meta={\n    \"id\": 7,\n    \"name\": \"alpha\"\n  }"));
    assert!(result.contains("tags=[\n    \"blue\",\n    \"green\"\n  ]"));
}

#[test]
fn test_default_formatter_brief_mode() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("info".to_string()));
    event.set_field("msg".to_string(), Dynamic::from("test message".to_string()));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        true,
        crate::config::TimestampFormatConfig::default(),
        false, // Disable wrapping for this test
        false,
        0, // No quiet mode
    ); // No colors, brief mode, no wrapping
    let result = formatter.format(&event);

    // Brief mode should output only values, space-separated
    assert_eq!(result, "info test message");
}

#[test]
fn test_context_markers_use_emoji_when_enabled() {
    let formatter = DefaultFormatter::new_with_wrapping(
        true,
        true,
        false,
        crate::config::TimestampFormatConfig::default(),
        false,
        false,
        0, // No quiet mode
    );

    let mut before_event = Event {
        context_type: ContextType::Before,
        ..Default::default()
    };
    before_event.set_field("msg".to_string(), Dynamic::from("before".to_string()));
    let before_line = formatter.format(&before_event);
    assert!(before_line.starts_with("\x1b[34m/\x1b[0m "));

    let mut match_event = Event {
        context_type: ContextType::Match,
        ..Default::default()
    };
    match_event.set_field("msg".to_string(), Dynamic::from("match".to_string()));
    let match_line = formatter.format(&match_event);
    assert!(match_line.starts_with("\x1b[95m◉\x1b[0m "));

    let mut after_event = Event {
        context_type: ContextType::After,
        ..Default::default()
    };
    after_event.set_field("msg".to_string(), Dynamic::from("after".to_string()));
    let after_line = formatter.format(&after_event);
    assert!(after_line.starts_with("\x1b[34m\\\x1b[0m "));

    let mut overlap_event = Event {
        context_type: ContextType::Both,
        ..Default::default()
    };
    overlap_event.set_field("msg".to_string(), Dynamic::from("overlap".to_string()));
    let overlap_line = formatter.format(&overlap_event);
    assert!(overlap_line.starts_with("\x1b[36m|\x1b[0m "));
}

#[test]
fn test_logfmt_formatter_basic() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
    event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
    event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
    event.set_field("status".to_string(), Dynamic::from(200i64));

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    // Should properly quote strings with spaces, leave numbers unquoted
    assert!(result.contains("level=INFO"));
    assert!(result.contains("msg=\"Test message\""));
    assert!(result.contains("user=alice"));
    assert!(result.contains("status=200"));
    // Fields should be space-separated
    assert!(result.contains(" "));
}

#[test]
fn test_logfmt_formatter_quoting() {
    let mut event = Event::default();
    event.set_field("simple".to_string(), Dynamic::from("value".to_string()));
    event.set_field(
        "spaced".to_string(),
        Dynamic::from("has spaces".to_string()),
    );
    event.set_field("empty".to_string(), Dynamic::from("".to_string()));
    event.set_field(
        "quoted".to_string(),
        Dynamic::from("has\"quotes".to_string()),
    );
    event.set_field("equals".to_string(), Dynamic::from("has=sign".to_string()));

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    assert!(result.contains("simple=value")); // No quotes needed
    assert!(result.contains("spaced=\"has spaces\"")); // Quotes due to space
    assert!(result.contains("empty=\"\"")); // Quotes due to empty
    assert!(result.contains("quoted=\"has\\\"quotes\"")); // Escaped quotes
    assert!(result.contains("equals=\"has=sign\"")); // Quotes due to equals sign
}

#[test]
fn test_logfmt_formatter_types() {
    let mut event = Event::default();
    event.set_field("string".to_string(), Dynamic::from("hello".to_string()));
    event.set_field("integer".to_string(), Dynamic::from(42i64));
    event.set_field("float".to_string(), Dynamic::from(2.5f64));
    event.set_field("bool_true".to_string(), Dynamic::from(true));
    event.set_field("bool_false".to_string(), Dynamic::from(false));

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    // Numbers and booleans should not be quoted
    assert!(result.contains("string=hello"));
    assert!(result.contains("integer=42"));
    assert!(result.contains("float=2.5"));
    assert!(result.contains("bool_true=true"));
    assert!(result.contains("bool_false=false"));
}

#[test]
fn test_logfmt_formatter_nested_values_are_quoted_and_parseable() {
    // A nested map flattens into a compact "k=v,k=v" string. That string
    // contains '=' and ',' and MUST be quoted, otherwise the emitted logfmt
    // is ambiguous and breaks round-tripping back through the logfmt parser.
    let mut meta = Map::new();
    meta.insert("a".into(), Dynamic::from("x"));
    meta.insert("b".into(), Dynamic::from("y"));

    let tags: Array = vec![Dynamic::from("blue"), Dynamic::from("green")];

    let mut event = Event::default();
    event.set_field("id".to_string(), Dynamic::from(1_i64));
    event.set_field("meta".to_string(), Dynamic::from(meta));
    event.set_field("tags".to_string(), Dynamic::from(tags));

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    // Scalars stay unquoted; the compact nested cells must be quoted.
    assert!(result.contains("id=1"), "got: {}", result);
    assert!(result.contains("meta=\"a=x,b=y\""), "got: {}", result);
    assert!(
        result.contains("tags=\"0=blue,1=green\""),
        "got: {}",
        result
    );

    // And the output must survive a round-trip through the logfmt parser
    // (no stray unquoted '=' splitting the value into bogus keys).
    let parser = crate::parsers::logfmt::LogfmtParser::new();
    let parsed = crate::pipeline::EventParser::parse(&parser, &result);
    assert!(parsed.is_ok(), "should parse: {}", result);
    let parsed = parsed.unwrap();
    assert_eq!(parsed.fields.get("meta").unwrap().to_string(), "a=x,b=y");
    assert_eq!(
        parsed.fields.get("tags").unwrap().to_string(),
        "0=blue,1=green"
    );
}

#[test]
fn test_logfmt_formatter_single_key_nested_preserves_key() {
    // Regression: a nested structure that flattens to a single entry must keep
    // its key (e.g. meta="b=1"), not collapse to just the leaf value (meta=1),
    // which silently dropped the field name and broke round-tripping.
    let mut single = Map::new();
    single.insert("b".into(), Dynamic::from(1_i64));

    let deep_inner = {
        let mut m = Map::new();
        m.insert("c".into(), Dynamic::from(1_i64));
        m
    };
    let mut deep = Map::new();
    deep.insert("b".into(), Dynamic::from(deep_inner));

    let single_elem: Array = vec![Dynamic::from(42_i64)];

    let mut event = Event::default();
    event.set_field("single".to_string(), Dynamic::from(single));
    event.set_field("deep".to_string(), Dynamic::from(deep));
    event.set_field("arr".to_string(), Dynamic::from(single_elem));

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    assert!(result.contains("single=\"b=1\""), "got: {}", result);
    assert!(result.contains("deep=\"b_c=1\""), "got: {}", result);
    // Single-element arrays keep their index, matching the multi-element form.
    assert!(result.contains("arr=\"0=42\""), "got: {}", result);

    // And it must still round-trip cleanly through the logfmt parser.
    let parser = crate::parsers::logfmt::LogfmtParser::new();
    let parsed = crate::pipeline::EventParser::parse(&parser, &result).unwrap();
    assert_eq!(parsed.fields.get("single").unwrap().to_string(), "b=1");
    assert_eq!(parsed.fields.get("deep").unwrap().to_string(), "b_c=1");
}

#[test]
fn test_logfmt_formatter_empty_event() {
    let event = Event::default();
    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);
    assert_eq!(result, "");
}

#[test]
fn test_logfmt_formatter_key_sanitization() {
    let mut event = Event::default();
    // Test various problematic key characters
    event.set_field(
        "field with spaces".to_string(),
        Dynamic::from("value1".to_string()),
    );
    event.set_field(
        "field=with=equals".to_string(),
        Dynamic::from("value2".to_string()),
    );
    event.set_field(
        "field\twith\ttabs".to_string(),
        Dynamic::from("value3".to_string()),
    );
    event.set_field(
        "field\nwith\nnewlines".to_string(),
        Dynamic::from("value4".to_string()),
    );
    event.set_field(
        "field\rwith\rcarriage".to_string(),
        Dynamic::from("value5".to_string()),
    );
    event.set_field(
        "normal_field".to_string(),
        Dynamic::from("value6".to_string()),
    );
    event.set_field(
        "field-with-dashes".to_string(),
        Dynamic::from("value7".to_string()),
    );
    event.set_field(
        "field.with.dots".to_string(),
        Dynamic::from("value8".to_string()),
    );

    let formatter = LogfmtFormatter::new();
    let result = formatter.format(&event);

    // Keys should be sanitized by replacing problematic characters with underscores
    assert!(result.contains("field_with_spaces=value1"));
    assert!(result.contains("field_with_equals=value2"));
    assert!(result.contains("field_with_tabs=value3"));
    assert!(result.contains("field_with_newlines=value4"));
    assert!(result.contains("field_with_carriage=value5"));
    assert!(result.contains("normal_field=value6"));

    // Non-problematic characters should be preserved
    assert!(result.contains("field-with-dashes=value7"));
    assert!(result.contains("field.with.dots=value8"));

    // Ensure the result can be parsed by the logfmt parser
    let parser = crate::parsers::logfmt::LogfmtParser::new();
    let parsed = crate::pipeline::EventParser::parse(&parser, &result);
    assert!(
        parsed.is_ok(),
        "Sanitized logfmt output should be parseable: {}",
        result
    );

    let parsed_event = parsed.unwrap();
    // Verify that sanitized keys preserve the data
    assert_eq!(
        parsed_event
            .fields
            .get("field_with_spaces")
            .unwrap()
            .to_string(),
        "value1"
    );
    assert_eq!(
        parsed_event
            .fields
            .get("field_with_equals")
            .unwrap()
            .to_string(),
        "value2"
    );
    assert_eq!(
        parsed_event.fields.get("normal_field").unwrap().to_string(),
        "value6"
    );
}

#[test]
fn test_sanitize_logfmt_key_function() {
    // Test the sanitize_logfmt_key function directly
    assert_eq!(sanitize_logfmt_key("normal_field"), "normal_field");
    assert_eq!(
        sanitize_logfmt_key("field with spaces"),
        "field_with_spaces"
    );
    assert_eq!(
        sanitize_logfmt_key("field=with=equals"),
        "field_with_equals"
    );
    assert_eq!(sanitize_logfmt_key("field\twith\ttabs"), "field_with_tabs");
    assert_eq!(
        sanitize_logfmt_key("field\nwith\nnewlines"),
        "field_with_newlines"
    );
    assert_eq!(
        sanitize_logfmt_key("field\rwith\rcarriage"),
        "field_with_carriage"
    );
    assert_eq!(
        sanitize_logfmt_key("field-with-dashes"),
        "field-with-dashes"
    );
    assert_eq!(sanitize_logfmt_key("field.with.dots"), "field.with.dots");
    assert_eq!(
        sanitize_logfmt_key("field_with_underscores"),
        "field_with_underscores"
    );
    assert_eq!(sanitize_logfmt_key(""), "");

    // Test edge cases
    assert_eq!(sanitize_logfmt_key("==="), "___");
    assert_eq!(sanitize_logfmt_key("   "), "___");
    assert_eq!(sanitize_logfmt_key(" = \t = \n = \r "), "_____________");
}

#[test]
fn test_levelmap_formatter_emits_full_line() {
    let formatter = LevelmapFormatter::with_width(3);
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("level".to_string(), Dynamic::from("info"));
    assert!(formatter.format(&event1).is_empty());

    let mut event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event2.set_field("level".to_string(), Dynamic::from("debug"));
    assert!(formatter.format(&event2).is_empty());

    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("level".to_string(), Dynamic::from("trace"));
    let line = formatter.format(&event3);
    assert_eq!(line, "1970-01-01T00:00:00.000Z idt");

    assert!(formatter.finish().is_none());

    let ts2 = Utc.timestamp_millis_opt(1_000).unwrap();
    let mut event4 = Event {
        parsed_ts: Some(ts2),
        ..Event::default()
    };
    event4.set_field("level".to_string(), Dynamic::from("warn"));
    assert!(formatter.format(&event4).is_empty());

    let trailing = formatter
        .finish()
        .expect("should flush trailing levelmap line");
    assert_eq!(trailing, "1970-01-01T00:00:01.000Z w");
}

#[test]
fn test_levelmap_formatter_unknown_level() {
    let formatter = LevelmapFormatter::with_width(1);
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let event = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };

    let line = formatter.format(&event);
    assert_eq!(line, "1970-01-01T00:00:00.000Z ?");
}

#[test]
fn test_keymap_formatter_emits_full_line() {
    let formatter = KeymapFormatter::with_width(3, Some("status".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("status".to_string(), Dynamic::from("ok"));
    assert!(formatter.format(&event1).is_empty());

    let mut event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event2.set_field("status".to_string(), Dynamic::from("error"));
    assert!(formatter.format(&event2).is_empty());

    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("status".to_string(), Dynamic::from("warn"));
    let line = formatter.format(&event3);
    assert_eq!(line, "1970-01-01T00:00:00.000Z oew");

    assert!(formatter.finish().is_none());

    let ts2 = Utc.timestamp_millis_opt(1_000).unwrap();
    let mut event4 = Event {
        parsed_ts: Some(ts2),
        ..Event::default()
    };
    event4.set_field("status".to_string(), Dynamic::from("pending"));
    assert!(formatter.format(&event4).is_empty());

    let trailing = formatter
        .finish()
        .expect("should flush trailing keymap line");
    assert_eq!(trailing, "1970-01-01T00:00:01.000Z p");
}

#[test]
fn test_keymap_formatter_empty_field() {
    let formatter = KeymapFormatter::with_width(1, Some("status".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let event = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };

    let line = formatter.format(&event);
    assert_eq!(line, "1970-01-01T00:00:00.000Z .");
}

#[test]
fn test_keymap_formatter_custom_field() {
    let formatter = KeymapFormatter::with_width(4, Some("method".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("method".to_string(), Dynamic::from("GET"));
    assert!(formatter.format(&event1).is_empty());

    let mut event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event2.set_field("method".to_string(), Dynamic::from("POST"));
    assert!(formatter.format(&event2).is_empty());

    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("method".to_string(), Dynamic::from("PUT"));
    assert!(formatter.format(&event3).is_empty());

    let mut event4 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event4.set_field("method".to_string(), Dynamic::from("DELETE"));
    let line = formatter.format(&event4);
    assert_eq!(line, "1970-01-01T00:00:00.000Z GPPD");
}

#[test]
fn test_tailmap_formatter_percentile_bucketing() {
    let formatter = TailmapFormatter::with_width(20, Some("value".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    // Create a dataset with known percentile distribution
    // Values: 1-100, so p90=90, p95=95, p99=99
    for i in 1..=100 {
        let mut event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event.set_field("value".to_string(), Dynamic::from(i as f64));
        assert!(formatter.format(&event).is_empty());
    }

    let output = formatter.finish().expect("should have output");
    assert!(output.contains("1970-01-01T00:00:00.000Z"));

    // Collect all characters from data lines only (skip legend)
    let mut all_chars = String::new();
    for line in output.lines() {
        // Only process lines that contain timestamp (data lines)
        if line.contains("1970-01-01") {
            if let Some(chars) = line.split_whitespace().nth(1) {
                all_chars.push_str(chars);
            }
        }
    }

    // Should have exactly 100 characters (one per value)
    assert_eq!(all_chars.len(), 100);

    // Expected distribution for values 1-100:
    // '_' for < p90 (values 1-89): ~89 characters
    // '1' for p90-p95 (values 90-94): ~5 characters
    // '2' for p95-p99 (values 95-98): ~4 characters
    // '3' for >= p99 (values 99-100): ~2 characters
    let underscore_count = all_chars.chars().filter(|&c| c == '_').count();
    let one_count = all_chars.chars().filter(|&c| c == '1').count();
    let two_count = all_chars.chars().filter(|&c| c == '2').count();
    let three_count = all_chars.chars().filter(|&c| c == '3').count();

    // Allow some tolerance for percentile estimation
    assert!(
        (85..=92).contains(&underscore_count),
        "Expected ~89 underscores for bottom 90%, got {}",
        underscore_count
    );
    assert!(
        (3..=7).contains(&one_count),
        "Expected ~5 ones for p90-p95, got {}",
        one_count
    );
    assert!(
        (2..=6).contains(&two_count),
        "Expected ~4 twos for p95-p99, got {}",
        two_count
    );
    assert!(
        (1..=4).contains(&three_count),
        "Expected ~2 threes for >= p99, got {}",
        three_count
    );
}

#[test]
fn test_tailmap_formatter_basic() {
    let formatter = TailmapFormatter::with_width(5, Some("value".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    // Add events with values 0, 25, 50, 75, 100
    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("value".to_string(), Dynamic::from(0.0));
    assert!(formatter.format(&event1).is_empty());

    let mut event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event2.set_field("value".to_string(), Dynamic::from(25.0));
    assert!(formatter.format(&event2).is_empty());

    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("value".to_string(), Dynamic::from(50.0));
    assert!(formatter.format(&event3).is_empty());

    let mut event4 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event4.set_field("value".to_string(), Dynamic::from(75.0));
    assert!(formatter.format(&event4).is_empty());

    let mut event5 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event5.set_field("value".to_string(), Dynamic::from(100.0));
    assert!(formatter.format(&event5).is_empty());

    // Tailmap outputs all at once in finish()
    let output = formatter.finish().expect("should have output");
    assert!(output.contains("1970-01-01T00:00:00.000Z"));
    // Should have 1 data line + 2 legend lines (with blank line separator)
    let data_lines: Vec<_> = output
        .lines()
        .filter(|line| line.contains("1970-01-01"))
        .collect();
    assert_eq!(data_lines.len(), 1);
}

#[test]
fn test_tailmap_formatter_tail_distribution() {
    // Test that tailmap correctly identifies tail values
    let formatter = TailmapFormatter::with_width(10, Some("value".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    // Create values where we know the percentiles:
    // 1-9 are bottom 90% (should be '_')
    // Value 10 is exactly p90 (should be '1')
    for i in 1..=10 {
        let mut event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event.set_field("value".to_string(), Dynamic::from(i as f64));
        formatter.format(&event);
    }

    let output = formatter.finish().expect("should have output");
    // Only collect characters from data lines (skip legend)
    let all_chars: String = output
        .lines()
        .filter(|line| line.contains("1970-01-01"))
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();

    // Most should be underscore (bottom 90%)
    let underscore_count = all_chars.chars().filter(|&c| c == '_').count();
    assert!(
        underscore_count >= 8,
        "Expected at least 8 underscores, got {}",
        underscore_count
    );
}

#[test]
fn test_tailmap_formatter_missing_values() {
    let formatter = TailmapFormatter::with_width(3, Some("value".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("value".to_string(), Dynamic::from(10.0));
    formatter.format(&event1);

    // Event without value field
    let event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    formatter.format(&event2);

    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("value".to_string(), Dynamic::from(20.0));
    formatter.format(&event3);

    let output = formatter.finish().expect("should have output");
    // Middle character should be '.' for missing value
    assert!(output.contains('.'));
}

#[test]
fn test_keymap_formatter_non_string_fields() {
    let formatter = KeymapFormatter::with_width(5, Some("value".to_string()));
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    // Test with integer
    let mut event1 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event1.set_field("value".to_string(), Dynamic::from(42_i64));
    assert!(formatter.format(&event1).is_empty());

    // Test with float
    let mut event2 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event2.set_field("value".to_string(), Dynamic::from(9.87));
    assert!(formatter.format(&event2).is_empty());

    // Test with boolean true
    let mut event3 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event3.set_field("value".to_string(), Dynamic::from(true));
    assert!(formatter.format(&event3).is_empty());

    // Test with boolean false
    let mut event4 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event4.set_field("value".to_string(), Dynamic::from(false));
    assert!(formatter.format(&event4).is_empty());

    // Test with negative number
    let mut event5 = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event5.set_field("value".to_string(), Dynamic::from(-99_i64));
    let line = formatter.format(&event5);
    // Should show: 4, 9, t, f, - (first chars of "42", "9.87", "true", "false", "-99")
    assert_eq!(line, "1970-01-01T00:00:00.000Z 49tf-");
}

#[test]
fn test_levelmap_legend_lists_observed_levels() {
    // Wide buffer so nothing flushes mid-stream; legend comes from finish().
    let formatter = LevelmapFormatter::with_width_and_legend(80, false, true);
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    for level in ["info", "error", "warn", "info", "error"] {
        let mut event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event.set_field("level".to_string(), Dynamic::from(level));
        formatter.format(&event);
    }
    // Event with no level field -> '?' glyph, "(none)" label.
    formatter.format(&Event {
        parsed_ts: Some(ts),
        ..Event::default()
    });

    let output = formatter.finish().expect("legend output");
    // Data line, blank separator, then a data-driven legend.
    assert!(output.contains("iewie?"));
    assert!(output.contains("\n\n"));
    assert!(output.contains("e = error"));
    assert!(output.contains("i = info"));
    assert!(output.contains("w = warn"));
    assert!(output.contains("? = (none)"));
}

#[test]
fn test_levelmap_legend_suppressed_by_default() {
    let formatter = LevelmapFormatter::with_width_and_legend(80, false, false);
    let ts = Utc.timestamp_millis_opt(0).unwrap();
    let mut event = Event {
        parsed_ts: Some(ts),
        ..Event::default()
    };
    event.set_field("level".to_string(), Dynamic::from("info"));
    formatter.format(&event);

    let output = formatter.finish().expect("trailing line");
    assert!(!output.contains('='));
    assert!(!output.contains("\n\n"));
}

#[test]
fn test_keymap_legend_groups_colliding_values() {
    let formatter = KeymapFormatter::with_width_and_legend(80, Some("status".to_string()), true);
    let ts = Utc.timestamp_millis_opt(0).unwrap();

    for status in ["200", "204", "200", "404", "500"] {
        let mut event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event.set_field("status".to_string(), Dynamic::from(status));
        formatter.format(&event);
    }
    // Missing field -> '.' glyph, "(missing)" label.
    formatter.format(&Event {
        parsed_ts: Some(ts),
        ..Event::default()
    });

    let output = formatter.finish().expect("legend output");
    // First chars collide: 200 and 204 both render as '2'.
    assert!(output.contains("2 = 200,204"));
    assert!(output.contains("4 = 404"));
    assert!(output.contains("5 = 500"));
    assert!(output.contains(". = (missing)"));
}

#[test]
fn test_tailmap_legend_can_be_suppressed() {
    let formatter = TailmapFormatter::with_width_and_legend(20, Some("value".to_string()), false);
    let ts = Utc.timestamp_millis_opt(0).unwrap();
    for i in 1..=50 {
        let mut event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event.set_field("value".to_string(), Dynamic::from(i as f64));
        formatter.format(&event);
    }

    let output = formatter.finish().expect("data output");
    assert!(output.contains("1970-01-01"));
    // No legend/threshold lines when suppressed.
    assert!(!output.contains("below p90"));
    assert!(!output.contains("events, range"));
}

#[test]
fn test_hide_formatter() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
    event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
    event.set_field("user".to_string(), Dynamic::from("alice".to_string()));

    let formatter = HideFormatter::new();
    let result = formatter.format(&event);
    assert_eq!(result, "");
}

#[test]
fn test_hide_formatter_empty_event() {
    let event = Event::default();
    let formatter = HideFormatter::new();
    let result = formatter.format(&event);
    assert_eq!(result, "");
}

#[test]
fn test_null_formatter_behavior() {
    // Null format uses HideFormatter, so test that it produces empty strings
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("ERROR".to_string()));
    event.set_field(
        "msg".to_string(),
        Dynamic::from("Critical error".to_string()),
    );

    let formatter = HideFormatter::new(); // Null format uses HideFormatter
    let result = formatter.format(&event);
    assert_eq!(result, ""); // Should be empty for null format
}

#[test]
fn test_shared_escaping_utilities() {
    // Test escape_logfmt_string
    assert_eq!(escape_logfmt_string("simple"), "simple");
    assert_eq!(escape_logfmt_string("with\"quotes"), "with\\\"quotes");
    assert_eq!(escape_logfmt_string("with\nnewline"), "with\\nnewline");
    assert_eq!(escape_logfmt_string("with\ttab"), "with\\ttab");
    assert_eq!(escape_logfmt_string("with\\backslash"), "with\\\\backslash");

    // Test needs_logfmt_quoting
    assert!(!needs_logfmt_quoting("simple"));
    assert!(needs_logfmt_quoting("with spaces"));
    assert!(needs_logfmt_quoting(""));
    assert!(needs_logfmt_quoting("with=equals"));
    assert!(needs_logfmt_quoting("with\"quotes"));
    assert!(needs_logfmt_quoting("with\ttab"));

    // Test format_dynamic_value
    assert_eq!(
        format_dynamic_value(&Dynamic::from("test")),
        ("test".to_string(), true)
    );
    assert_eq!(
        format_dynamic_value(&Dynamic::from(42i64)),
        ("42".to_string(), false)
    );
    assert_eq!(
        format_dynamic_value(&Dynamic::from(true)),
        ("true".to_string(), false)
    );
}

#[test]
fn test_csv_formatter_basic() {
    let keys = vec!["name".to_string(), "age".to_string(), "city".to_string()];
    let formatter = CsvFormatter::new(keys);

    let mut event = Event::default();
    event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
    event.set_field("age".to_string(), Dynamic::from(25i64));
    event.set_field("city".to_string(), Dynamic::from("New York".to_string()));

    let result = formatter.format(&event);

    // Should include header and data
    assert!(result.contains("name,age,city"));
    assert!(result.contains("Alice,25,New York"));
}

#[test]
fn test_csv_formatter_single_key_nested_preserves_key() {
    // Regression: a single-key nested object must keep its key in the compact
    // cell (obj cell = "b:1"), not collapse to just the leaf value ("1").
    let keys = vec!["id".to_string(), "obj".to_string()];
    let formatter = CsvFormatter::new(keys);

    let mut inner = Map::new();
    inner.insert("b".into(), Dynamic::from(1i64));

    let mut event = Event::default();
    event.set_field("id".to_string(), Dynamic::from(7i64));
    event.set_field("obj".to_string(), Dynamic::from(inner));

    let result = formatter.format(&event);
    assert!(result.contains("7,b:1"), "got: {}", result);
}

#[test]
fn test_csv_formatter_with_quoting() {
    let keys = vec!["name".to_string(), "msg".to_string()];
    let formatter = CsvFormatter::new(keys);

    let mut event = Event::default();
    event.set_field("name".to_string(), Dynamic::from("Smith, John".to_string()));
    event.set_field(
        "msg".to_string(),
        Dynamic::from("He said \"hello\"".to_string()),
    );

    let result = formatter.format(&event);

    // Should properly quote values with commas and quotes
    assert!(result.contains("\"Smith, John\""));
    assert!(result.contains("\"He said \"\"hello\"\"\""));
}

#[test]
fn test_tsv_formatter_basic() {
    let keys = vec!["name".to_string(), "age".to_string()];
    let formatter = CsvFormatter::new_tsv(keys);

    let mut event = Event::default();
    event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
    event.set_field("age".to_string(), Dynamic::from(25i64));

    let result = formatter.format(&event);

    // Should use tab separator
    assert!(result.contains("name\tage"));
    assert!(result.contains("Alice\t25"));
}

#[test]
fn test_csv_formatter_no_header() {
    let keys = vec!["name".to_string(), "age".to_string()];
    let formatter = CsvFormatter::new_csv_no_header(keys);

    let mut event = Event::default();
    event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
    event.set_field("age".to_string(), Dynamic::from(25i64));

    let result = formatter.format(&event);

    // Should not include header
    assert!(!result.contains("name,age"));
    assert_eq!(result, "Alice,25");
}

#[test]
fn test_csv_formatter_missing_fields() {
    let keys = vec!["name".to_string(), "age".to_string(), "city".to_string()];
    let formatter = CsvFormatter::new_csv_no_header(keys);

    let mut event = Event::default();
    event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
    // age is missing
    event.set_field("city".to_string(), Dynamic::from("Boston".to_string()));

    let result = formatter.format(&event);

    // Should have empty field for missing age
    assert_eq!(result, "Alice,,Boston");
}

#[test]
fn test_csv_escaping_utilities() {
    // Test needs_csv_quoting
    assert!(!needs_csv_quoting("simple", ','));
    assert!(needs_csv_quoting("with,comma", ','));
    assert!(needs_csv_quoting("with\"quote", ','));
    assert!(needs_csv_quoting("with\nnewline", ','));
    assert!(needs_csv_quoting("", ','));
    assert!(needs_csv_quoting(" leading", ','));
    assert!(needs_csv_quoting("trailing ", ','));

    // Test with tab delimiter
    assert!(!needs_csv_quoting("with,comma", '\t'));
    assert!(needs_csv_quoting("with\ttab", '\t'));

    // Test escape_csv_value
    assert_eq!(escape_csv_value("simple", ','), "simple");
    assert_eq!(escape_csv_value("with,comma", ','), "\"with,comma\"");
    assert_eq!(escape_csv_value("with\"quote", ','), "\"with\"\"quote\"");
    assert_eq!(escape_csv_value("", ','), "\"\"");
}

#[test]
fn test_default_formatter_wrapping_disabled() {
    let mut event = Event::default();
    event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
    event.set_field(
        "message".to_string(),
        Dynamic::from("This is a very long message that would normally wrap".to_string()),
    );
    event.set_field("user".to_string(), Dynamic::from("alice".to_string()));

    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        false, // wrapping disabled
        false,
        0, // No quiet mode
    );
    let result = formatter.format(&event);

    // Should be single line when wrapping is disabled
    assert!(!result.contains('\n'));
    assert!(result.contains("level='INFO'"));
    assert!(result.contains("message='This is a very long message that would normally wrap'"));
    assert!(result.contains("user='alice'"));
}

#[test]
fn test_default_formatter_wrapping_enabled() {
    let mut event = Event::default();
    event.set_field("field1".to_string(), Dynamic::from("value1".to_string()));
    event.set_field("field2".to_string(), Dynamic::from("value2".to_string()));
    event.set_field(
        "very_long_field_name".to_string(),
        Dynamic::from("a very long field value that will definitely cause wrapping".to_string()),
    );
    event.set_field("field4".to_string(), Dynamic::from("value4".to_string()));

    // Override terminal width for consistent testing
    let mut formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        true,
        false,
        0, // No quiet mode
    );
    formatter.set_terminal_width_for_test(50); // Small width to force wrapping

    let result = formatter.format(&event);

    // Should wrap when width is exceeded
    assert!(result.contains('\n'));
    assert!(result.contains("  ")); // Should have indentation

    // All fields should still be present
    assert!(result.contains("field1='value1'"));
    assert!(result.contains("field2='value2'"));
    assert!(result.contains(
        "very_long_field_name='a very long field value that will definitely cause wrapping'"
    ));
    assert!(result.contains("field4='value4'"));
}

#[test]
fn test_default_formatter_wrapping_brief_mode() {
    let mut event = Event::default();
    event.set_field("field1".to_string(), Dynamic::from("short".to_string()));
    event.set_field(
        "field2".to_string(),
        Dynamic::from("this is a much longer value that should cause wrapping".to_string()),
    );
    event.set_field("field3".to_string(), Dynamic::from("end".to_string()));

    let mut formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        true,
        crate::config::TimestampFormatConfig::default(),
        true,
        false,
        0, // No quiet mode
    );
    formatter.set_terminal_width_for_test(30); // Very small width

    let result = formatter.format(&event);

    // Brief mode should still wrap properly
    assert!(result.contains('\n'));
    assert!(result.contains("  ")); // Should have indentation

    // In brief mode, only values are shown (no key= parts)
    assert!(result.contains("short"));
    assert!(result.contains("this is a much longer value that should cause wrapping"));
    assert!(result.contains("end"));
    assert!(!result.contains("field1="));
    assert!(!result.contains("field2="));
    assert!(!result.contains("field3="));
}

#[test]
fn test_display_length_ignores_ansi_codes() {
    let formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        true,
        false,
        0, // No quiet mode
    );

    // Test string with ANSI color codes
    let colored_text = "\x1b[31mred text\x1b[0m";
    assert_eq!(formatter.display_length_for_test(colored_text), 8); // "red text" = 8 chars

    let plain_text = "red text";
    assert_eq!(formatter.display_length_for_test(plain_text), 8);

    // Empty string
    assert_eq!(formatter.display_length_for_test(""), 0);

    // Only ANSI codes
    assert_eq!(formatter.display_length_for_test("\x1b[31m\x1b[0m"), 0);
}

#[test]
fn test_wrapping_preserves_field_boundaries() {
    let mut event = Event::default();
    event.set_field("a".to_string(), Dynamic::from("value".to_string()));
    event.set_field("b".to_string(), Dynamic::from("value".to_string()));
    event.set_field("c".to_string(), Dynamic::from("value".to_string()));

    let mut formatter = DefaultFormatter::new_with_wrapping(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        true,
        false,
        0, // No quiet mode
    );
    formatter.set_terminal_width_for_test(20); // Force wrapping

    let result = formatter.format(&event);

    // Should never break within a field, only between fields
    assert!(!result.contains("a='val\n  ue'")); // Would be bad
    assert!(result.contains("a='value'")); // Should be complete

    // Should have proper line structure
    let lines: Vec<&str> = result.split('\n').collect();
    assert!(lines.len() > 1); // Should have multiple lines

    // Continuation lines should be indented
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && !line.is_empty() {
            assert!(
                line.starts_with("  "),
                "Line {} should be indented: '{}'",
                i,
                line
            );
        }
    }
}

#[test]
fn test_default_formatter_new_constructor_enables_wrapping_by_default() {
    let mut event = Event::default();
    event.set_field("field1".to_string(), Dynamic::from("value1".to_string()));
    event.set_field(
        "very_long_field_name_that_exceeds_width".to_string(),
        Dynamic::from(
            "a very long field value that should definitely cause wrapping in most terminals"
                .to_string(),
        ),
    );
    event.set_field("field3".to_string(), Dynamic::from("value3".to_string()));

    // Use the basic constructor (should enable wrapping by default now)
    let mut formatter = DefaultFormatter::new(
        false,
        false,
        false,
        crate::config::TimestampFormatConfig::default(),
        false,
        0, // No quiet mode
    );

    // Default constructor should have wrapping enabled
    assert!(formatter.is_wrapping_enabled_for_test());

    // Force a small terminal width to make wrapping deterministic in tests
    formatter.set_terminal_width_for_test(80);

    let result = formatter.format(&event);

    // Should wrap by default now
    assert!(
        result.contains('\n'),
        "Default constructor should enable wrapping"
    );
    assert!(
        result.contains("  "),
        "Should have indentation when wrapping"
    );

    // All fields should still be present
    assert!(result.contains("field1='value1'"));
    assert!(result.contains("very_long_field_name_that_exceeds_width="));
    assert!(result.contains("field3='value3'"));
}

#[test]
fn test_gap_tracker_inserts_marker_for_large_delta() {
    let mut tracker = GapTracker::new(ChronoDuration::minutes(30), false);

    let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
    let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());

    assert!(tracker.check(first).is_none());
    let marker = tracker.check(second).expect("marker line");
    assert!(marker.starts_with('_'));
    assert!(marker.ends_with('_'));
    assert!(marker.contains("time gap: 2 hours"));
}

#[test]
fn test_gap_tracker_skips_small_delta() {
    let mut tracker = GapTracker::new(ChronoDuration::hours(2), false);

    let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
    let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 12, 0, 0).unwrap());

    assert!(tracker.check(first).is_none());
    assert!(tracker.check(second).is_none());
}

#[test]
fn test_gap_tracker_handles_missing_timestamp() {
    let mut tracker = GapTracker::new(ChronoDuration::minutes(45), false);

    assert!(tracker.check(None).is_none());

    let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 12, 0, 0).unwrap());
    assert!(tracker.check(second).is_none());

    let third = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());
    let marker = tracker.check(third).expect("marker line");
    assert!(marker.contains("time gap: 1 hour"));
    assert!(marker.starts_with('_'));
}

#[test]
fn test_gap_tracker_handles_reverse_order() {
    let mut tracker = GapTracker::new(ChronoDuration::milliseconds(1), false);

    let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
    let earlier = Some(Utc.with_ymd_and_hms(2024, 2, 5, 10, 59, 59).unwrap());

    assert!(tracker.check(first).is_none());
    let marker = tracker.check(earlier).expect("marker for backwards jump");
    assert!(marker.contains("time gap"));
}

#[test]
fn test_gap_tracker_colors_marker_when_enabled() {
    let mut tracker = GapTracker::new(ChronoDuration::minutes(30), true);

    let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
    let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());

    assert!(tracker.check(first).is_none());
    let marker = tracker.check(second).expect("colored marker");
    assert!(marker.contains("\x1b[34m"));
    assert!(marker.contains("\x1b[0m"));
    assert!(marker.starts_with("\x1b[34m_"));
    let reset_index = marker.rfind("\x1b[0m").expect("reset sequence");
    assert!(marker[..reset_index].ends_with('_'));
    assert!(marker.contains("time gap: 2 hours"));
}

#[test]
fn test_gap_tracker_formats_fractional_microseconds_compactly() {
    let mut tracker = GapTracker::new(ChronoDuration::milliseconds(1), false);

    let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
    let second = first.map(|ts| ts + ChronoDuration::microseconds(1_230_000));

    assert!(tracker.check(first).is_none());
    let marker = tracker.check(second).expect("fractional marker");
    assert!(marker.contains("time gap: 1.23 seconds"));
}
