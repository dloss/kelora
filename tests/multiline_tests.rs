mod common;
use common::*;

#[test]
fn test_multiline_real_world_scenario() {
    let input = r#"{"timestamp": "2023-07-18T15:04:23.456Z", "user": "alice", "status": 200, "message": "login successful", "response_time": 45}
{"timestamp": "2023-07-18T15:04:25.789Z", "user": "bob", "status": 404, "message": "page not found", "response_time": 12}
{"timestamp": "2023-07-18T15:06:41.210Z", "user": "charlie", "status": 500, "message": "internal error", "response_time": 234}
{"timestamp": "2023-07-18T15:07:12.345Z", "user": "alice", "status": 403, "message": "forbidden", "response_time": 18}
{"timestamp": "2023-07-18T15:08:30.678Z", "user": "dave", "status": 200, "message": "success", "response_time": 67}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "-F", "json",
        "--filter", "e.status >= 400",
        "--exec", "e.alert_level = if e.status >= 500 { \"critical\" } else { \"warning\" }; track_sum(\"total_errors\", 1);",
        "--end", "print(`Total errors processed: ${metrics[\"total_errors\"]}`);"
    ], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout
        .trim()
        .lines()
        .filter(|line| line.starts_with('{'))
        .collect();
    assert_eq!(lines.len(), 3, "Should filter to 3 error lines");

    assert!(
        stdout.contains("Total errors processed: 3"),
        "Should count all error lines"
    );

    // Verify alert levels are correctly assigned
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        let status = parsed["status"].as_i64().unwrap();
        let alert_level = parsed["alert_level"].as_str().unwrap();

        if status >= 500 {
            assert_eq!(alert_level, "critical");
        } else {
            assert_eq!(alert_level, "warning");
        }
    }
}

#[test]
fn test_multiline_all_strategy_json() {
    // Test reading entire JSON file as single event
    let input = r#"{"users": [
  {"name": "alice", "age": 30, "status": "active"},
  {"name": "bob", "age": 25, "status": "inactive"},
  {"name": "charlie", "age": 35, "status": "active"}
], "total": 3, "timestamp": "2023-07-18T15:00:00Z"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "-M", "all",
        "-F", "json",
        "--exec", "e.user_count = e.users.len(); e.active_users = e.users.filter(|user| user.status == \"active\").len();"
    ], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully with -M all");

    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Output should be valid JSON");

    // Verify the original data is preserved
    assert_eq!(parsed["total"].as_i64().unwrap(), 3);
    assert_eq!(parsed["users"].as_array().unwrap().len(), 3);

    // Verify our transformations worked
    assert_eq!(parsed["user_count"].as_i64().unwrap(), 3);
    assert_eq!(parsed["active_users"].as_i64().unwrap(), 2);
}

#[test]
fn test_multiline_all_strategy_text() {
    // Test reading entire text content as single event
    let input = r#"Line 1 with some content
Line 2 with more content
Line 3 with even more content
Final line of the document"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "raw",
        "-M", "all",
        "--exec", "let lines = e.raw.split(\"\\n\"); e.line_count = lines.len(); e.word_count = e.raw.split(\" \").len();"
    ], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with -M all on text"
    );

    // The output may be wrapped across multiple lines due to the long line content
    // The important thing is that we have exactly one event processed

    // The output should contain our transformations
    assert!(stdout.contains("line_count=4"), "Should count 4 lines");
    assert!(stdout.contains("word_count=18"), "Should count 18 words");

    // Verify the content is there (the long line with newlines)
    assert!(
        stdout.contains("Line 1 with some content\\nLine 2"),
        "Should contain the joined content with newlines"
    );
}

#[test]
fn test_multiline_all_strategy_empty_input() {
    // Test -M all with empty input
    let input = "";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "all",
            "--exec",
            "e.is_empty = e.line.len() == 0;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should handle empty input with -M all");

    // With empty input, there should be no output events
    assert_eq!(
        stdout.trim(),
        "",
        "Should produce no output for empty input"
    );
}

#[test]
fn test_multiline_all_strategy_with_stats() {
    // Test -M all with stats enabled - using line format with shorter content
    let input = r#"Log 1
Log 2  
Log 3"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "all",
            "--with-stats",
            "--exec",
            "e.line_count = e.line.split(\"\\n\").len();",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with -M all and stats"
    );

    // Should create exactly 1 event (entire input as single event)
    assert!(
        stderr.contains("Events created: 1"),
        "Should create exactly 1 event"
    );
    assert!(stderr.contains("1 output"), "Should output exactly 1 event");
}

#[test]
fn test_multiline_indent_with_filters_and_stats() {
    let input = r#"ERROR connection failed
    at module.rs:42
    caused by network reset
WARN degraded performance
    while contacting replica
INFO recovered cleanly
"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "indent",
            "-F",
            "json",
            "--with-stats",
            "--filter",
            "e.line.contains(\"ERROR\") || e.line.contains(\"WARN\")",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with -M indent"
    );

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        2,
        "Filter should keep only ERROR and WARN events"
    );

    let first = events
        .first()
        .and_then(|event| event["line"].as_str())
        .expect("First event should contain a line field");
    assert!(
        first.contains("connection failed") && first.contains("module.rs:42"),
        "First event should contain the stack trace content"
    );

    let second = events
        .get(1)
        .and_then(|event| event["line"].as_str())
        .expect("Second event should contain a line field");
    assert!(
        second.contains("degraded performance") && second.contains("contacting replica"),
        "Second event should retain continuation lines"
    );

    let stats = extract_stats_lines(&stderr);
    assert!(
        !stats.is_empty(),
        "Stats output should be present when --stats is enabled"
    );
    assert_eq!(
        extract_events_created_from_stats(&stderr),
        3,
        "Three multiline events should be created before filtering"
    );
    assert_eq!(
        extract_events_filtered_from_stats(&stderr),
        1,
        "One event should be filtered out"
    );
}

#[test]
fn test_multiline_timestamp_with_format_hint_parallel_batches() {
    let input = r#"2023|07|18_15*04*23 INFO primary event
    stack line one
2023|07|18_15*04*24 INFO secondary event
    stack line two
2023|07|18_15*04*25 WARN final event
    last detail
"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "timestamp:format=%Y|%m|%d_%H*%M*%S",
            "--parallel",
            "--batch-size",
            "1",
            "--batch-timeout",
            "1",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with timestamp strategy"
    );

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        3,
        "Parallel batches should not split multiline events"
    );

    let first_line = events[0]["line"]
        .as_str()
        .expect("First event should contain aggregated text");
    assert!(
        first_line.contains("primary event") && first_line.contains("stack line one"),
        "First event should include both header and continuation text"
    );

    let second_line = events[1]["line"]
        .as_str()
        .expect("Second event should contain aggregated text");
    assert!(
        second_line.contains("secondary event") && second_line.contains("stack line two"),
        "Second event should keep its continuation line"
    );

    let third_line = events[2]["line"]
        .as_str()
        .expect("Third event should contain aggregated text");
    assert!(
        third_line.contains("final event") && third_line.contains("last detail"),
        "Third event should retain trailing detail lines"
    );
}

#[test]
fn test_multiline_regex_with_start_and_end_patterns() {
    let input = r#"START request 1
payload line a
payload line b
END
START request 2
payload line c
END
"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "raw",
            "-M",
            "regex:match=^START:end=^END",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with regex mode"
    );

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(events.len(), 2, "Expected two regex-delimited events");

    let first = events[0]["raw"]
        .as_str()
        .expect("Regex event should retain raw text");
    assert!(
        first.contains("START request 1")
            && first.contains("payload line b")
            && first.contains("END"),
        "Regex end pattern should keep the terminating line in the event"
    );

    let second = events[1]["raw"]
        .as_str()
        .expect("Regex event should retain raw text");
    assert!(
        second.contains("START request 2")
            && second.contains("payload line c")
            && second.ends_with("END"),
        "Second regex section should flush cleanly at END"
    );
}

#[test]
fn test_multiline_regex_invalid_pattern_surfaces_error() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "regex:match=[", "-F", "json"], "");

    assert_eq!(
        exit_code, 1,
        "Invalid regex configuration should propagate as an error"
    );
    assert!(
        stderr.contains("Invalid regex start pattern"),
        "Error output should mention the invalid regex start pattern"
    );
}

// Edge cases for -M indent

#[test]
fn test_multiline_indent_empty_lines_between_events() {
    let input = r#"ERROR first error
    continuation line

ERROR second error
    another continuation

INFO normal line"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Should handle empty lines in indent mode");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // Empty lines should break multiline events
    assert!(events.len() >= 3, "Should create multiple events");
}

#[test]
fn test_multiline_indent_mixed_indentation() {
    // Test with mix of spaces and tabs
    let input =
        "START line\n\tcontinuation with tab\n  continuation with spaces\n    deeper indentation";

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Should handle mixed indentation");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        1,
        "Mixed indentation should be treated as one event"
    );
    let event_text = events[0]["line"].as_str().unwrap();
    assert!(
        event_text.contains("START line"),
        "Should contain start line"
    );
    assert!(
        event_text.contains("tab") && event_text.contains("spaces"),
        "Should contain continuations"
    );
}

#[test]
fn test_multiline_indent_all_indented() {
    // If all lines are indented, what happens?
    let input = "    line 1\n    line 2\n    line 3";

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Should handle all-indented input");
    assert!(!stdout.trim().is_empty(), "Should produce output");
}

// Edge cases for -M timestamp

#[test]
fn test_multiline_timestamp_missing_timestamp() {
    let input = r#"2023-04-15T10:00:00 First event
continuation without timestamp
2023-04-15T10:00:01 Second event
another continuation"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "timestamp", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Should handle missing timestamps");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        2,
        "Lines without timestamps should be grouped with previous event"
    );
}

#[test]
fn test_multiline_timestamp_nonmatching_lines() {
    // Test lines that don't match the timestamp pattern get grouped with previous event
    let input = r#"2023-04-15T10:00:00 Event 1
Not a timestamp line
2023-04-15T10:00:01 Event 2
Also not a timestamp"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "timestamp", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "Should handle lines that don't match timestamp pattern"
    );

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // Should create 2 events, each with non-timestamped lines grouped with them
    assert_eq!(
        events.len(),
        2,
        "Should group non-matching lines with previous event"
    );
    let first_event = events[0]["line"].as_str().unwrap();
    assert!(
        first_event.contains("Not a timestamp line"),
        "First event should include non-matching line"
    );
}

#[test]
fn test_multiline_timestamp_no_timestamp_at_start() {
    // What if first line has no timestamp?
    let input = r#"Random text without timestamp
2023-04-15T10:00:00 First timestamped event
continuation
2023-04-15T10:00:01 Second event"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "timestamp", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "Should handle input starting without timestamp"
    );
    assert!(!stdout.trim().is_empty(), "Should produce output");
}

// Edge cases for -M regex

#[test]
fn test_multiline_regex_start_only() {
    let input = r#"START event 1
continuation 1
START event 2
continuation 2"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", "regex:match=^START", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "Should handle regex with start pattern only");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        2,
        "Should create events based on start pattern"
    );
}

#[test]
fn test_multiline_regex_end_without_match_error() {
    // Test that end-only pattern requires match pattern
    let input = "line 1\nEND\n";

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "regex:end=^END", "-F", "json"], input);
    // Should fail because regex:end requires match= to be specified
    assert_ne!(
        exit_code, 0,
        "Should fail when end pattern specified without match"
    );
    assert!(
        stderr.contains("Invalid") || stderr.contains("requires match"),
        "Should indicate that match is required, stderr: {}",
        stderr
    );
}

#[test]
fn test_multiline_regex_no_matches() {
    // If regex never matches, everything should be one event
    let input = r#"line 1
line 2
line 3"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", "regex:match=^NOMATCH", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "Should handle regex that never matches");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // All lines should be combined into one event since regex never matches
    assert_eq!(
        events.len(),
        1,
        "Non-matching regex should create single event"
    );
}

#[test]
fn test_multiline_regex_overlapping_patterns() {
    // Test when both start and end patterns could match the same line
    let input = r#"START-END
middle
START-END
other"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "raw",
            "-M",
            "regex:match=^START:end=END$",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "Should handle overlapping patterns");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // Should create events (exact behavior depends on implementation)
    assert!(!events.is_empty(), "Should create some events");
}

#[test]
fn test_multiline_regex_invalid_end_pattern() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "regex:end=[[[", "-F", "json"], "test");

    assert_ne!(exit_code, 0, "Invalid regex end pattern should fail");
    assert!(
        stderr.contains("Invalid") || stderr.contains("regex"),
        "Should indicate regex error"
    );
}

// Edge cases with parallel mode

#[test]
fn test_multiline_parallel_worker_boundaries() {
    // Create input with many multiline events to test worker boundaries
    let mut input = String::new();
    for i in 0..20 {
        input.push_str(&format!("2023-04-15T10:00:{:02} Event {}\n", i, i));
        input.push_str("  continuation line\n");
    }

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "timestamp",
            "--parallel",
            "--batch-size",
            "5",
            "-F",
            "json",
        ],
        &input,
    );
    assert_eq!(exit_code, 0, "Parallel mode should handle multiline events");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // Should create 20 events, each with their continuation line
    assert_eq!(
        events.len(),
        20,
        "Parallel workers should not split multiline events"
    );
}

#[test]
fn test_multiline_very_long_event() {
    // Test with a very long multiline event
    let mut input = String::from("START\n");
    for i in 0..1000 {
        input.push_str(&format!("  continuation line {}\n", i));
    }

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "-F", "json"], &input);
    assert_eq!(exit_code, 0, "Should handle very long multiline events");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    assert_eq!(
        events.len(),
        1,
        "Should create single event from long continuation"
    );
    let event_text = events[0]["line"].as_str().unwrap();
    assert!(event_text.contains("START"), "Should contain start line");
    assert!(
        event_text.contains("line 999"),
        "Should contain last continuation"
    );
}

// Test multiline with filters

#[test]
fn test_multiline_filter_on_partial_content() {
    // Filter should see the complete multiline event
    let input = r#"ERROR connection failed
    at database.rs:123
    timeout exceeded
INFO normal log"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "indent",
            "--filter",
            "e.line.contains(\"timeout\")",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "Should filter on complete multiline content");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();

    // Only the ERROR event should match (because it contains "timeout" in continuation)
    assert_eq!(
        events.len(),
        1,
        "Should filter based on full multiline event"
    );
    assert!(
        events[0]["line"].as_str().unwrap().contains("ERROR"),
        "Should be the ERROR event"
    );
}

#[test]
fn test_multiline_with_malformed_events() {
    // Test that malformed multiline patterns don't crash the pipeline
    let input = r#"    orphaned indented line at start
NORMAL line
    indented
NORMAL again"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "Should handle malformed multiline input gracefully"
    );
    assert!(!stdout.trim().is_empty(), "Should produce some output");
}

/// #368: `--multiline-join=newline` and the regex line formats didn't compose.
/// The trailing message capture couldn't cross a newline, so every multi-line
/// event was dropped as a parse error — while `space` and `empty` kept it. All
/// three joins must produce the same events; only the message text differs.
#[test]
fn test_multiline_join_newline_keeps_multiline_events() {
    let input = "2026-07-26 14:05:01 INFO  Started request /v1/pay\n\
                 2026-07-26 14:05:02 ERROR Payment failed\n\
                 java.lang.RuntimeException: upstream timeout\n\
                 \tat com.acme.pay.Client.charge(Client.java:88)\n\
                 2026-07-26 14:05:03 INFO  Retrying\n";

    for join in ["newline", "space", "empty"] {
        let (stdout, stderr, exit_code) = run_kelora_with_input(
            &["-M", "timestamp", "--multiline-join", join, "-F", "json"],
            input,
        );
        assert_eq!(exit_code, 0, "join={join} should exit successfully");
        assert!(
            !stderr.contains("Parse errors"),
            "join={join} should not drop events as parse errors: {stderr}"
        );

        let events: Vec<serde_json::Value> = stdout
            .lines()
            .filter(|line| line.trim_start().starts_with('{'))
            .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
            .collect();

        assert_eq!(events.len(), 3, "join={join} should keep all three events");
        assert_eq!(events[1]["level"].as_str().unwrap(), "ERROR");
        let msg = events[1]["msg"].as_str().unwrap();
        assert!(
            msg.contains("Payment failed") && msg.contains("Client.java:88"),
            "join={join} should keep the trace in the message: {msg}"
        );
    }

    // The point of `newline` is that the trace stays readable.
    let (stdout, _stderr, _exit_code) = run_kelora_with_input(
        &[
            "-M",
            "timestamp",
            "--multiline-join",
            "newline",
            "-F",
            "json",
        ],
        input,
    );
    let error_event: serde_json::Value = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .nth(1)
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .expect("second event");
    assert!(
        error_event["msg"].as_str().unwrap().contains('\n'),
        "newline join should preserve the line structure inside the message"
    );
}

/// Same defect in the syslog parser's trailing message capture (#368).
#[test]
fn test_multiline_join_newline_keeps_syslog_events() {
    let input = "Jul 26 14:05:01 host app: Started\n\
                 Jul 26 14:05:02 host app: Payment failed\n\
                 java.lang.RuntimeException: upstream timeout\n\
                 \tat com.acme.pay.Client.charge(Client.java:88)\n\
                 Jul 26 14:05:03 host app: Retrying\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "syslog",
            "-M",
            "timestamp",
            "--multiline-join",
            "newline",
            "-F",
            "json",
            "--input-year",
            "2026",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "should exit successfully");
    assert!(
        !stderr.contains("Parse errors"),
        "syslog events with joined newlines should parse: {stderr}"
    );

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("Should parse JSON line"))
        .collect();
    assert_eq!(events.len(), 3, "should keep all three syslog events");
    assert!(
        events[1]["msg"]
            .as_str()
            .unwrap()
            .contains("upstream timeout"),
        "continuation lines belong to the previous event's message"
    );
}

/// #368: a multi-line event is flushed by the line that *follows* it, and the
/// diagnostic used to name that line — pointing past the event it described.
/// Both the sequential and the parallel path must report the event's own first
/// line, and `meta.line_num` must agree between them.
#[test]
fn test_multiline_event_is_reported_at_its_first_line() {
    // The middle event spans lines 2-4 and cannot be parsed; the events on
    // lines 1 and 5 can.
    let input = "2026-07-26 14:05:01 INFO Started\n\
                 2026-07-26 14:05:02 no-level header\n\
                 continuation one\n\
                 continuation two\n\
                 2026-07-26 14:05:03 INFO Retrying\n";

    for extra_args in [vec![], vec!["--parallel"]] {
        let mut args = vec![
            "-M",
            "timestamp",
            "--multiline-join",
            "newline",
            "-F",
            "json",
        ];
        args.extend(extra_args.iter().copied());
        let (_stdout, stderr, exit_code) = run_kelora_with_input(&args, input);
        assert_eq!(exit_code, 0, "args={args:?} should exit successfully");
        // Stdin input has no filename, so errors read "line N:".
        assert!(
            stderr.contains("line 2:"),
            "args={args:?} should blame the event's first line: {stderr}"
        );
        assert!(
            !stderr.contains("line 5:"),
            "args={args:?} should not blame the line that flushed the event: {stderr}"
        );

        // `meta.line_num` follows the same rule: the event's first line.
        let mut ln_args = vec![
            "-M",
            "timestamp",
            "-e",
            "e.ln = meta.line_num",
            "-k",
            "ln",
            "-F",
            "json",
        ];
        ln_args.extend(extra_args.iter().copied());
        let (stdout, _stderr, exit_code) = run_kelora_with_input(&ln_args, input);
        assert_eq!(exit_code, 0, "args={ln_args:?} should exit successfully");
        let line_nums: Vec<i64> = stdout
            .lines()
            .filter(|line| line.trim_start().starts_with('{'))
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).expect("Should parse JSON line")
                    ["ln"]
                    .as_i64()
                    .expect("ln should be an integer")
            })
            .collect();
        assert_eq!(
            line_nums,
            vec![1, 5],
            "args={ln_args:?} should report each event's first line"
        );
    }
}

// ---------------------------------------------------------------------------
// Seam fixes (2026-07): blank-line policy, colon-safe options, file
// boundaries, provenance metadata, lock-in, caps, timeout flag, join defaults.
// See dev/multiline-exploration-2026-07.md.
// ---------------------------------------------------------------------------

fn json_events(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("valid JSON event line"))
        .collect()
}

#[test]
fn test_multiline_blank_line_inside_indented_block_is_continuation() {
    let input =
        "2024-01-01T10:00:00 ERROR boom\n  at foo\n\n  at bar\n2024-01-01T10:00:01 INFO ok\n";

    // The same input must produce the same events for -f line and -f raw:
    // the blank line stays inside the event (it used to split the event under
    // -f line and be silently deleted under -f raw).
    for fmt in ["line", "raw"] {
        let (stdout, _stderr, exit_code) = run_kelora_with_input(
            &[
                "-f",
                fmt,
                "-M",
                "indent",
                "--multiline-join=newline",
                "-F",
                "json",
            ],
            input,
        );
        assert_eq!(exit_code, 0);
        let events = json_events(&stdout);
        assert_eq!(events.len(), 2, "format {}: two events expected", fmt);
        let field = if fmt == "raw" { "raw" } else { "line" };
        let first = events[0][field].as_str().unwrap();
        assert!(
            first.contains("at foo") && first.contains("at bar"),
            "format {}: blank line must not split the trace: {:?}",
            fmt,
            first
        );
        assert!(
            first.contains("\n\n"),
            "format {}: interior blank line must be preserved",
            fmt
        );
    }
}

#[test]
fn test_multiline_regex_pattern_may_contain_colons() {
    let input = "10:00 first\ncont\n10:01 second\n";
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", r"regex:match=^\d{2}:\d{2}", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "colon in regex must parse: {}", stderr);
    assert_eq!(json_events(&stdout).len(), 2);
}

#[test]
fn test_multiline_regex_unknown_option_still_errors() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", "regex:match=^A:ends=^E", "-F", "json"],
        "A\n",
    );
    assert_eq!(exit_code, 2, "typo'd option must fail loudly");
    assert!(
        stderr.contains("Unknown regex option"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_multiline_event_never_spans_files_and_reports_provenance() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let path_a = dir.path().join("a.log");
    let path_b = dir.path().join("b.log");
    std::fs::File::create(&path_a)
        .unwrap()
        .write_all(b"2024-01-01T10:00:00 ERROR boom\n  at foo\n  at bar\n")
        .unwrap();
    std::fs::File::create(&path_b)
        .unwrap()
        .write_all(b"  orphan continuation\n2024-01-01T10:00:05 INFO next\n")
        .unwrap();

    for extra in [&[][..], &["--parallel"][..]] {
        let mut args = vec![
            "-f",
            "raw",
            "-M",
            "timestamp",
            "--exec",
            "e.file = meta.filename; e.n = meta.line_num",
            "-F",
            "json",
        ];
        args.extend_from_slice(extra);
        args.push(path_a.to_str().unwrap());
        args.push(path_b.to_str().unwrap());
        let (stdout, stderr, exit_code) = {
            let binary_path = env!("CARGO_BIN_EXE_kelora");
            let out = std::process::Command::new(binary_path)
                .args(&args)
                .output()
                .expect("run kelora");
            (
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
                out.status.code().unwrap_or(-1),
            )
        };
        assert_eq!(exit_code, 0, "{}", stderr);
        let events = json_events(&stdout);
        let mode = if extra.is_empty() {
            "sequential"
        } else {
            "parallel"
        };
        assert_eq!(events.len(), 3, "{}: file boundary must flush", mode);
        // Event 1 belongs entirely to file a and starts at line 1.
        assert!(events[0]["file"].as_str().unwrap().ends_with("a.log"));
        assert_eq!(events[0]["n"].as_i64().unwrap(), 1);
        assert!(!events[0]["raw"].as_str().unwrap().contains("orphan"));
        // The orphan continuation is file b's own (junk) event, not part of
        // file a's trace.
        assert!(events[1]["file"].as_str().unwrap().ends_with("b.log"));
        assert!(events[1]["raw"].as_str().unwrap().contains("orphan"));
        assert!(events[2]["file"].as_str().unwrap().ends_with("b.log"));
    }
}

#[test]
fn test_multiline_metadata_reports_events_first_line_in_parallel() {
    // Three 3-line events: line numbers must be 1, 4, 7 (they used to be the
    // event's index in its batch).
    let input = "2024-01-01T10:00:00 one\n  a\n  b\n2024-01-01T10:00:01 two\n  c\n  d\n2024-01-01T10:00:02 three\n  e\n  f\n";
    for extra in [&[][..], &["--parallel"][..]] {
        let mut args = vec![
            "-f",
            "raw",
            "-M",
            "timestamp",
            "--exec",
            "e.n = meta.line_num",
            "-F",
            "json",
        ];
        args.extend_from_slice(extra);
        let (stdout, _stderr, exit_code) = run_kelora_with_input(&args, input);
        assert_eq!(exit_code, 0);
        let ns: Vec<i64> = json_events(&stdout)
            .iter()
            .map(|e| e["n"].as_i64().unwrap())
            .collect();
        assert_eq!(ns, vec![1, 4, 7], "extra args: {:?}", extra);
    }
}

#[test]
fn test_multiline_blank_strategy_splits_paragraphs() {
    let input = "a\nb\n\nc\n\n\nd\ne\n";
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "raw",
            "-M",
            "blank",
            "--multiline-join=newline",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    let events = json_events(&stdout);
    let texts: Vec<&str> = events.iter().map(|e| e["raw"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["a\nb", "c", "d\ne"]);
}

#[test]
fn test_multiline_timestamp_locks_format_family() {
    let input = "2024-01-01T10:00:00 ERROR boom\nnow retrying with backoff\n1234567890 records processed\n17:03 was the incident window\n2024-01-01T10:00:01 INFO ok\n";

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "timestamp", "-F", "json"], input);
    assert_eq!(exit_code, 0);
    let events = json_events(&stdout);
    assert_eq!(
        events.len(),
        2,
        "prose/epoch/time-only prefixes must not split ISO-headed events: {:?}",
        events
    );

    // loose restores unlocked detection.
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "timestamp:loose", "-F", "json"], input);
    assert_eq!(exit_code, 0);
    assert!(json_events(&stdout).len() > 2, "loose must not lock");
}

#[test]
fn test_multiline_max_lines_cap_splits_and_warns() {
    let input = "H1\n a\n b\n c\n d\nH2\n";
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "raw",
            "-M",
            "indent",
            "--multiline-max-lines",
            "3",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    assert_eq!(json_events(&stdout).len(), 3, "capped event splits in two");
    assert!(
        stderr.contains("exceeded 3 lines"),
        "cap warning expected once: {}",
        stderr
    );
    assert_eq!(
        stderr.matches("exceeded 3 lines").count(),
        1,
        "warning fires once per run"
    );
}

#[test]
fn test_multiline_flag_validation() {
    // The auxiliary flags require --multiline.
    for args in [
        &["--multiline-join=newline"][..],
        &["--multiline-timeout", "1s"][..],
        &["--multiline-max-lines", "5"][..],
    ] {
        let mut full = vec!["-f", "raw"];
        full.extend_from_slice(args);
        let (_stdout, stderr, exit_code) = run_kelora_with_input(&full, "x\n");
        assert_eq!(exit_code, 2, "{:?} without -M must be rejected", args);
        assert!(stderr.contains("requires --multiline"), "{}", stderr);
    }

    // Bad duration is invalid usage.
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", "indent", "--multiline-timeout", "banana"],
        "x\n",
    );
    assert_eq!(exit_code, 2);
    assert!(stderr.contains("multiline-timeout"), "{}", stderr);
}

#[test]
fn test_multiline_all_join_defaults_to_newline_and_honors_override() {
    let input = "x\ny\n";
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "raw", "-M", "all", "-F", "json"], input);
    assert_eq!(exit_code, 0);
    assert_eq!(json_events(&stdout)[0]["raw"].as_str().unwrap(), "x\ny");

    // --multiline-join used to be silently ignored under `all`.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "raw",
            "-M",
            "all",
            "--multiline-join=space",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    assert_eq!(json_events(&stdout)[0]["raw"].as_str().unwrap(), "x y");
}

#[test]
fn test_multiline_keep_lines_hints_about_pre_assembly_filtering() {
    let input = "2024-01-01T10:00:00 ERROR boom\n  at foo\n";
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "raw", "-M", "timestamp", "--keep-lines", "ERROR"],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(
        stderr.contains("before multiline"),
        "expected pre-assembly hint: {}",
        stderr
    );
}

#[test]
fn test_multiline_sequential_and_parallel_agree() {
    // Deterministic corpus with events of varying shapes; timeout is off for
    // file/stdin-buffered input in tests? stdin counts as a stream, so pin the
    // timeout off explicitly to keep the comparison timing-independent.
    let mut input = String::new();
    for i in 0..500 {
        input.push_str(&format!(
            "2024-01-01T10:{:02}:{:02} event {}\n",
            i / 60,
            i % 60,
            i
        ));
        for j in 0..(i % 4) {
            input.push_str(&format!("  frame {}\n", j));
        }
        if i % 7 == 0 {
            input.push('\n');
        }
    }

    let args_base = [
        "-f",
        "raw",
        "-M",
        "timestamp",
        "--multiline-timeout",
        "0",
        "--multiline-join=newline",
        "-F",
        "json",
    ];
    let (seq, _e1, c1) = run_kelora_with_input(&args_base, &input);
    let mut par_args = args_base.to_vec();
    par_args.push("--parallel");
    let (par, _e2, c2) = run_kelora_with_input(&par_args, &input);
    assert_eq!(c1, 0);
    assert_eq!(c2, 0);
    assert_eq!(seq, par, "sequential and parallel must agree byte-for-byte");
}
