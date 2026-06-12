mod common;
use common::*;
use tempfile::NamedTempFile;

#[test]
fn test_metrics_output_exposes_only_user_keys() {
    let input = r#"{"msg": "first"}
{"msg": "second"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"events_total\", 1);",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --metrics"
    );
    assert!(
        stderr.contains("Tracked metrics") || stderr.contains("Tracked metrics:\""),
        "Metrics banner should be present in stderr"
    );
    assert!(
        stderr.contains("events_total"),
        "User metric key should appear in metrics output"
    );
    assert!(
        !stderr.contains("__kelora_stats"),
        "Internal stats keys must not leak into metrics output"
    );
}

#[test]
fn test_metrics_output_has_no_leading_newline_when_events_suppressed() {
    let input = r#"{"level": "INFO"}
{"level": "ERROR"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"level\", e.level)",
            "--with-metrics",
            "-q",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully when metrics are requested"
    );
    assert!(
        !stderr.starts_with('\n'),
        "Metrics output should not start with a leading newline when no events are shown: {:?}",
        stderr
    );
    assert!(
        stderr.contains("Tracked metrics"),
        "Metrics header should be present"
    );
}

#[test]
fn test_metrics_mode_surfaces_exec_errors_on_stderr() {
    // Regression: --metrics implies suppress_diagnostics, which used to hide the
    // per-event script-error summary entirely. Script errors go to stderr and
    // cannot pollute the (stdout) metrics, so they must still be surfaced.
    // The removed 1.x single-argument track_count form errors with a migration hint.
    let input = r#"{"status": 500}
{"status": 503}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--exec", "track_count(e.status)", "--metrics"],
        input,
    );

    // The 1.x form errors on every event, but exec is best-effort: it rolls back
    // and emits, so the run is recovered (exit 0). The summary must still surface.
    assert_eq!(
        exit_code, 0,
        "exec errors are recovered (best-effort), even in --metrics"
    );
    assert!(
        stderr.contains("Exec errors"),
        "metrics mode should still surface the exec-error summary: {}",
        stderr
    );
    assert!(
        stderr.contains("track_count(\"status\", e.status)"),
        "the surfaced error should carry the migration hint: {}",
        stderr
    );
}

#[test]
fn test_silent_suppresses_metrics_exec_errors() {
    // --silent is the one switch that does hide error summaries.
    let input = r#"{"status": 500}
{"status": 503}"#;

    let (stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(e.status)",
            "--metrics",
            "--silent",
        ],
        input,
    );

    assert_eq!(stdout.trim(), "", "--silent should suppress stdout");
    assert!(
        !stderr.contains("Exec errors"),
        "--silent should suppress the exec-error summary: {}",
        stderr
    );
}

#[test]
fn test_metrics_mode_reports_every_event_failure_scope() {
    // Regression: data-only modes disabled stats collection, which zeroed
    // events_created and silently dropped the "affecting every event" scope
    // signal -- the exact silent failure a stuck user hits. The scope fact is
    // part of the error summary, so it must surface in plain --metrics.
    let input = r#"{"status": 500}
{"status": 503}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--exec", "track_count(e.status)", "--metrics"],
        input,
    );

    // exec is best-effort, so erroring on every event is recovered (exit 0); the
    // "affecting every event" scope fact is still reported.
    assert_eq!(exit_code, 0, "exec errors are recovered (best-effort)");
    assert!(
        stdout.contains("No metrics tracked"),
        "stdout still reports the empty data channel: {}",
        stdout
    );
    assert!(
        stderr.contains("affecting every event"),
        "the error summary should report total-failure scope: {}",
        stderr
    );
    assert!(
        !stderr.contains("Use --strict"),
        "the advisory coaching is suppressed by default in data-only modes: {}",
        stderr
    );
}

#[test]
fn test_metrics_diagnostics_shows_every_event_coaching() {
    // An explicit --diagnostics opts back into the advisory coaching even in
    // data-only modes, pointing the user at --strict / --verbose.
    let input = r#"{"status": 500}
{"status": 503}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--diagnostics",
            "--exec",
            "track_count(e.status)",
            "--metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "exec errors are recovered; --diagnostics only controls coaching"
    );
    assert!(
        stderr.contains("affecting every event"),
        "scope fact should still be present: {}",
        stderr
    );
    assert!(
        stderr.contains("Use --strict"),
        "--diagnostics should re-enable the coaching sentence: {}",
        stderr
    );
}

#[test]
fn test_filter_errors_count_every_failure_not_just_one() {
    // Regression: a filter that errored on every line reported "Filter errors:
    // 1 total" because the filter error path never synced track_error's
    // thread-local writes back into ctx.internal_tracker -- the next event's
    // set_thread_tracking_state reinstalled the stale map and clobbered the
    // increment, so only the final event's contribution survived. The exec path
    // already synced; the filter path did not.
    let input = "x=1\nx=2\nx=3\nx=4\nx=5\n";

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "logfmt", "--filter", "nonexistent_fn(e.x)"], input);

    // The filter errors on every line, so it never matched anything -> exit 1.
    assert_eq!(
        exit_code, 1,
        "a filter that errors on every event fails the run: {}",
        stderr
    );
    assert!(
        stderr.contains("Filter errors: 5 total"),
        "every failing line should be counted, not deduped to 1: {}",
        stderr
    );
    assert!(
        stderr.contains("affecting every event"),
        "total-failure scope should be reported: {}",
        stderr
    );
}

#[test]
fn test_metrics_mode_surfaces_parse_errors() {
    // Regression: data-only modes used to swallow parse errors entirely (no
    // summary at all). They must still be *reported* on stderr in --metrics.
    // The exit code is recovered here (1 of 3 lines parsed -> partial failure),
    // matching normal mode; see test_metrics_mode_all_lines_fail_to_parse for the
    // wrong-format case that does fail.
    let input = "not json\nalso not json\n{\"action\": \"x\"}";

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"action\", e.action)",
            "--metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered (1 of 3 parsed), even in --metrics"
    );
    assert!(
        stderr.contains("parse error"),
        "parse errors should still be reported on stderr in --metrics: {}",
        stderr
    );
}

#[test]
fn test_metrics_mode_all_lines_fail_to_parse() {
    // The wrong-format case: no line parses, so the parse stage never succeeded.
    // That is an unusable-input failure and must exit 1 even in --metrics.
    let input = "not json\nalso not json\n";

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"action\", e.action)",
            "--metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "input where no line parses is a failure and must exit non-zero even in --metrics"
    );
    assert!(
        stderr.contains("parse error"),
        "parse errors should be reported on stderr in --metrics: {}",
        stderr
    );
}

#[test]
fn test_metrics_command_reports_when_nothing_was_tracked() {
    let input = r#"{"level": "INFO"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--metrics"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --metrics"
    );
    assert!(
        stdout.contains("No metrics tracked"),
        "explicit metrics output should say when nothing was tracked: {}",
        stdout
    );
}

#[test]
fn test_global_tracking() {
    let input = r#"{"level": "INFO", "status": 200}
{"level": "ERROR", "status": 500}
{"level": "ERROR", "status": 404}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.status >= 400",
            "--exec",
            "track_sum(\"errors\", 1)",
            "--end",
            "print(`Errors: ${metrics[\"errors\"]}`)",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // The end stage should print to stdout (Rhai print goes to stdout in this implementation)
    assert!(
        stdout.contains("Errors: 2"),
        "Should track filtered error lines"
    );
}

#[test]
fn test_tracking_with_min_max() {
    let input = r#"{"response_time": 150, "status": 200}
{"response_time": 500, "status": 404}
{"response_time": 75, "status": 200}
{"response_time": 800, "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_min(\"min_time\", e.response_time); track_max(\"max_time\", e.response_time);",
            "--end",
            "print(`Min: ${metrics[\"min_time\"]}, Max: ${metrics[\"max_time\"]}`);",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    assert!(
        stdout.contains("Min: 75"),
        "Should track minimum response time"
    );
    assert!(
        stdout.contains("Max: 800"),
        "Should track maximum response time"
    );
}

#[test]
fn test_track_unique_function() {
    let input = r#"{"ip": "1.1.1.1", "user": "alice"}
{"ip": "2.2.2.2", "user": "bob"}
{"ip": "1.1.1.1", "user": "charlie"}
{"ip": "3.3.3.3", "user": "alice"}
{"ip": "2.2.2.2", "user": "dave"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "--exec", "track_unique(\"unique_ips\", e.ip); track_unique(\"unique_users\", e.user);",
        "--end", "print(`IPs: ${metrics[\"unique_ips\"].len()}, Users: ${metrics[\"unique_users\"].len()}`);"
    ], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should collect 3 unique IPs and 4 unique users
    assert!(
        stdout.contains("IPs: 3"),
        "Should track 3 unique IP addresses"
    );
    assert!(stdout.contains("Users: 4"), "Should track 4 unique users");
}

#[test]
fn test_track_count_function() {
    let input = r#"{"status": "200", "method": "GET"}
{"status": "404", "method": "POST"}
{"status": "200", "method": "GET"}
{"status": "500", "method": "PUT"}
{"status": "404", "method": "GET"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "--exec", "track_count(\"status_counts\", e.status); track_count(\"method_counts\", e.method);",
        "--end", "print(`Status 200: ${metrics[\"status_counts\"].get(\"200\") ?? 0}, GET requests: ${metrics[\"method_counts\"].get(\"GET\") ?? 0}`);"
    ], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should count 2 occurrences of status 200 and 3 GET requests
    assert!(
        stdout.contains("Status 200: 2"),
        "Should count 2 occurrences of status 200"
    );
    assert!(
        stdout.contains("GET requests: 3"),
        "Should count 3 GET requests"
    );
}

#[test]
fn test_track_unique_metrics_file_outputs_array() {
    let input = r#"{"pattern": "User logged in"}
{"pattern": "User logged out"}
{"pattern": "User logged in"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_unique(\"patterns\", e.pattern);",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let patterns = metrics_json["patterns"]
        .as_array()
        .expect("patterns should be an array");
    assert_eq!(
        patterns.len(),
        2,
        "Should store exactly two unique patterns"
    );
    assert_eq!(
        patterns[0],
        serde_json::Value::String("User logged in".to_string()),
        "Should preserve insertion order for first pattern"
    );
    assert_eq!(
        patterns[1],
        serde_json::Value::String("User logged out".to_string()),
        "Should preserve insertion order for second pattern"
    );
}

#[test]
fn test_mixed_tracking_functions() {
    let input = r#"{"user": "alice", "response_time": 100, "status": "200"}
{"user": "bob", "response_time": 250, "status": "404"}
{"user": "alice", "response_time": 180, "status": "200"}
{"user": "charlie", "response_time": 50, "status": "500"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "--exec", "track_sum(\"total\", 1); track_unique(\"users\", e.user); track_count(\"status_dist\", e.status); track_min(\"min_time\", e.response_time); track_max(\"max_time\", e.response_time);",
        "--end", "print(`Total: ${metrics[\"total\"]}, Users: ${metrics[\"users\"].len()}, Min: ${metrics[\"min_time\"]}, Max: ${metrics[\"max_time\"]}`);"
    ], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    assert!(stdout.contains("Total: 4"), "Should count 4 total records");
    assert!(stdout.contains("Users: 3"), "Should track 3 unique users");
    assert!(
        stdout.contains("Min: 50"),
        "Should track minimum response time"
    );
    assert!(
        stdout.contains("Max: 250"),
        "Should track maximum response time"
    );
}

#[test]
fn test_track_unique_with_unit_values() {
    let input = r#"{"user": "alice", "optional": "value1"}
{"user": "bob"}
{"user": "charlie", "optional": "value2"}
{"user": "dave", "optional": "value1"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_unique(\"users\", e.user); track_unique(\"optionals\", e.optional.or_empty());",
            "--end",
            "print(`Users: ${metrics[\"users\"].len()}, Optionals: ${metrics[\"optionals\"].len()}`);"
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should track 4 users and 2 optional values (bob's missing optional is skipped)
    assert!(stdout.contains("Users: 4"), "Should track 4 unique users");
    assert!(
        stdout.contains("Optionals: 2"),
        "Should track 2 unique optional values, skipping Unit"
    );
}

#[test]
fn test_track_unique_with_empty_arrays() {
    let input = r#"{"id": 1, "tags": ["a", "b"]}
{"id": 2, "tags": []}
{"id": 3, "tags": ["c"]}
{"id": 4, "tags": []}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "let tag_str = e.tags.join(\",\"); track_unique(\"tag_sets\", tag_str.or_empty());",
            "--end",
            "print(`Unique: ${metrics[\"tag_sets\"].len()}`);",
            "--with-metrics",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should track only 2 unique tag sets (empty array joins become empty string, then Unit, skipped)
    assert!(
        stdout.contains("Unique: 2"),
        "Should track 2 unique tag sets, skipping empty arrays"
    );
}

#[test]
fn test_track_sum_min_max_with_unit() {
    let input = r#"{"score": "100"}
{"score": ""}
{"score": "50"}
{"score": "200"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "let s = e.score.to_int(); track_sum(\"total\", s); track_min(\"min\", s); track_max(\"max\", s);",
            "--end",
            "print(`Sum: ${metrics[\"total\"]}, Min: ${metrics[\"min\"]}, Max: ${metrics[\"max\"]}`);"
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Empty string to_int() returns Unit, which should be skipped
    // So sum of 100 + 50 + 200 = 350, min = 50, max = 200
    assert!(
        stdout.contains("Sum: 350"),
        "Should sum only valid integers, skipping Unit"
    );
    assert!(
        stdout.contains("Min: 50"),
        "Should track minimum, skipping Unit"
    );
    assert!(
        stdout.contains("Max: 200"),
        "Should track maximum, skipping Unit"
    );
}

#[test]
fn test_track_count_with_unit() {
    let input = r#"{"status": "200", "user": "alice"}
{"status": "404"}
{"status": "200", "user": "bob"}
{"status": "500", "user": ""}
{"status": "200"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"status_dist\", e.status); track_count(\"user_dist\", e.user.or_empty());",
            "--end",
            "print(`Status_200: ${metrics[\"status_dist\"].get(\"200\") ?? 0}, Users: ${metrics[\"user_dist\"].len()}`);"
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should bucket status normally, but skip empty users
    assert!(
        stdout.contains("Status_200: 3"),
        "Should count 3 occurrences of status 200"
    );
    assert!(
        stdout.contains("Users: 2"),
        "Should bucket only 2 users (alice, bob), skipping empty and missing"
    );
}

#[test]
fn test_metrics_sequential_mode_basic() {
    let input = r#"{"level":"info","message":"test1"}
{"level":"error","message":"test2"}
{"level":"info","message":"test3"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1); track_sum(\"level_\" + e.level, 1); track_sum(\"message_length\", e.message.len())",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Check that metrics output appears in stderr
    assert!(
        stderr.contains("Tracked metrics"),
        "Should contain metrics header"
    );
    assert!(
        stderr.contains("total        = 3"),
        "Should count total events"
    );
    assert!(
        stderr.contains("level_info   = 2"),
        "Should count info events"
    );
    assert!(
        stderr.contains("level_error  = 1"),
        "Should count error events"
    );
    assert!(
        stderr.contains("message_length = 15"),
        "Should sum message lengths"
    );

    // Check that main output still appears in stdout
    assert!(
        stdout.contains("level='info'"),
        "Should output processed events"
    );
    assert!(
        stdout.contains("level='error'"),
        "Should output processed events"
    );
}

#[test]
fn test_metrics_parallel_mode_basic() {
    let input = r#"{"level":"info","message":"test1"}
{"level":"error","message":"test2"}
{"level":"info","message":"test3"}
{"level":"warn","message":"test4"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1); track_sum(\"level_\" + e.level, 1); track_sum(\"message_length\", e.message.len())",
            "--with-metrics",
            "--parallel",
            "--batch-size",
            "2",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Check that metrics output appears in stderr (same as sequential)
    assert!(
        stderr.contains("Tracked metrics"),
        "Should contain metrics header"
    );
    assert!(
        stderr.contains("total        = 4"),
        "Should count total events across workers"
    );
    assert!(
        stderr.contains("level_info   = 2"),
        "Should count info events across workers"
    );
    assert!(
        stderr.contains("level_error  = 1"),
        "Should count error events across workers"
    );
    assert!(
        stderr.contains("level_warn   = 1"),
        "Should count warn events across workers"
    );
    assert!(
        stderr.contains("message_length = 20"),
        "Should sum message lengths in parallel"
    );

    // Check that main output still appears in stdout
    assert!(
        stdout.contains("level='info'"),
        "Should output processed events"
    );
    assert!(
        stdout.contains("level='error'"),
        "Should output processed events"
    );
    assert!(
        stdout.contains("level='warn'"),
        "Should output processed events"
    );
}

#[test]
fn test_metrics_file_output() {
    let input = r#"{"level":"info","message":"test1"}
{"level":"error","message":"test2"}"#;

    // Create a temporary file for metrics output
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1); track_sum(\"level_\" + e.level, 1)",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Read the metrics file content
    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");

    // Parse as JSON to verify structure
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    // Check metrics content
    assert_eq!(metrics_json["total"], 2, "Should have total count");
    assert_eq!(metrics_json["level_info"], 1, "Should have info count");
    assert_eq!(metrics_json["level_error"], 1, "Should have error count");

    // No metrics should appear in stderr when using file output only
    assert!(
        !stderr.contains("Tracked metrics"),
        "Should not display metrics to stderr"
    );
}

#[test]
fn test_track_sum_handles_float_values() {
    let input = r#"{"value":1.5}
{"value":2}
{"value":2.5}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total_value\", e.value)",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let total_value = metrics_json["total_value"]
        .as_f64()
        .expect("Should have float sum");
    assert!(
        (total_value - 6.0).abs() < f64::EPSILON,
        "Sum should match input values"
    );
}

#[test]
fn test_track_avg_basic() {
    let input = r#"{"value":10}
{"value":20}
{"value":30}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_avg(\"average_value\", e.value)",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let avg_value = metrics_json["average_value"]
        .as_f64()
        .expect("Should have average value");
    assert!(
        (avg_value - 20.0).abs() < f64::EPSILON,
        "Average should be 20.0, got {}",
        avg_value
    );
}

#[test]
fn test_track_avg_with_float_values() {
    let input = r#"{"value":1.5}
{"value":2.0}
{"value":2.5}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_avg(\"avg_value\", e.value)",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let avg_value = metrics_json["avg_value"]
        .as_f64()
        .expect("Should have average value");
    assert!(
        (avg_value - 2.0).abs() < f64::EPSILON,
        "Average should be 2.0, got {}",
        avg_value
    );
}

#[test]
fn test_track_avg_with_unit() {
    let input = r#"{"score": "100"}
{"score": ""}
{"score": "50"}
{"score": "200"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "let s = e.score.to_int(); track_avg(\"avg_score\", s);",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let avg_score = metrics_json["avg_score"]
        .as_f64()
        .expect("Should have average score");

    // Empty string to_int() returns Unit, which should be skipped
    // So average of 100 + 50 + 200 = 350 / 3 = 116.666...
    let expected_avg = 350.0 / 3.0;
    assert!(
        (avg_score - expected_avg).abs() < 0.001,
        "Average should be approximately 116.67, got {}",
        avg_score
    );
}

#[test]
fn test_track_avg_parallel_mode() {
    let input = r#"{"value":10}
{"value":20}
{"value":30}
{"value":40}
{"value":50}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_avg(\"avg_value\", e.value)",
            "--metrics-file",
            metrics_file_path,
            "--parallel",
            "--batch-size",
            "2",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully in parallel mode"
    );

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    let avg_value = metrics_json["avg_value"]
        .as_f64()
        .expect("Should have average value");

    // Average of 10 + 20 + 30 + 40 + 50 = 150 / 5 = 30.0
    assert!(
        (avg_value - 30.0).abs() < f64::EPSILON,
        "Average should be 30.0 in parallel mode, got {}",
        avg_value
    );
}

#[test]
fn test_metrics_parallel_consistency() {
    // Test that parallel mode produces correct metrics with different batch sizes
    let input = r#"{"level":"info","message":"test1"}
{"level":"error","message":"test2"}
{"level":"info","message":"test3"}
{"level":"warn","message":"test4"}
{"level":"error","message":"test5"}"#;

    let exec_script = "track_sum(\"total\", 1); track_sum(\"level_\" + e.level, 1)";

    // Run in parallel mode with batch-size 1
    let (_stdout1, stderr1, exit_code1) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            exec_script,
            "--with-metrics",
            "--parallel",
            "--batch-size",
            "1",
        ],
        input,
    );
    assert_eq!(
        exit_code1, 0,
        "Parallel mode batch-size 1 should exit successfully"
    );

    // Run in parallel mode with batch-size 2
    let (_stdout2, stderr2, exit_code2) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            exec_script,
            "--with-metrics",
            "--parallel",
            "--batch-size",
            "2",
        ],
        input,
    );
    assert_eq!(
        exit_code2, 0,
        "Parallel mode batch-size 2 should exit successfully"
    );

    // Both should have identical metrics
    assert!(
        stderr1.contains("total        = 5"),
        "Batch-size 1 should count all events"
    );
    assert!(
        stderr2.contains("total        = 5"),
        "Batch-size 2 should count all events"
    );

    assert!(
        stderr1.contains("level_info   = 2"),
        "Batch-size 1 should count info events"
    );
    assert!(
        stderr2.contains("level_info   = 2"),
        "Batch-size 2 should count info events"
    );

    assert!(
        stderr1.contains("level_error  = 2"),
        "Batch-size 1 should count error events"
    );
    assert!(
        stderr2.contains("level_error  = 2"),
        "Batch-size 2 should count error events"
    );

    assert!(
        stderr1.contains("level_warn   = 1"),
        "Batch-size 1 should count warn events"
    );
    assert!(
        stderr2.contains("level_warn   = 1"),
        "Batch-size 2 should count warn events"
    );
}

#[test]
fn test_span_count_closes_on_n() {
    let input = r#"{"msg": "a"}
{"msg": "b"}
{"msg": "c"}
{"msg": "d"}
{"msg": "e"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--span-close",
            r#"print(span.id + ":" + span.size.to_string());"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "span count mode should exit successfully");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines, vec!["#0:2", "#1:2", "#2:1"]);
}

#[test]
fn test_span_metadata_statuses() {
    let input = r#"{"ts": "2023-01-01T00:00:05Z", "msg": "first"}
{"ts": "2023-01-01T00:01:10Z", "msg": "second"}
{"ts": "2023-01-01T00:00:20Z", "msg": "late"}
{"msg": "missing"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--exec",
            r#"let id = if meta.span_id == () { "null" } else { meta.span_id }; print(meta.span_status + ":" + id);"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "span metadata check should exit successfully");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines,
        vec![
            "included:2023-01-01T00:00:00Z/1m",
            "included:2023-01-01T00:01:00Z/1m",
            "late:2023-01-01T00:00:00Z/1m",
            "unassigned:null",
        ]
    );
}

#[test]
fn test_span_metrics_track_counts() {
    let input = r#"{"msg": "one"}
{"msg": "two"}
{"msg": "three"}
{"msg": "four"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--exec",
            "track_sum(\"events\", 1);",
            "--span-close",
            r#"let metrics = span.metrics; let count = metrics["events"]; print(span.id + ":" + count.to_string());"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "span metrics should exit successfully");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines, vec!["#0:2", "#1:2"]);
}

#[test]
fn test_span_metrics_track_avg_per_window() {
    // track_avg stores cumulative {sum, count}, so span.metrics can report the
    // true per-window average as (Δsum / Δcount).
    let input = r#"{"t":"2025-01-01T00:00:00Z","rt":1.0}
{"t":"2025-01-01T00:00:30Z","rt":9.0}
{"t":"2025-01-01T00:01:10Z","rt":2.0}
{"t":"2025-01-01T00:01:40Z","rt":3.0}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-j",
            "-q",
            "--span",
            "1m",
            "--exec",
            "track_avg(\"rt_avg\", e.rt);",
            "--span-close",
            r#"print(span.id + ":" + span.metrics["rt_avg"].to_string());"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "span avg should exit successfully");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines,
        vec![
            "2025-01-01T00:00:00Z/1m:5.0", // (1 + 9) / 2
            "2025-01-01T00:01:00Z/1m:2.5", // (2 + 3) / 2
        ]
    );
}

#[test]
fn test_span_metrics_non_additive_warns_and_omits() {
    // Non-additive aggregators (max, percentiles) cannot be reduced to a single
    // window. They must be omitted from span.metrics *and* trigger a warning,
    // rather than being silently dropped or reporting a global extreme.
    let input = r#"{"t":"2025-01-01T00:00:00Z","rt":1.0}
{"t":"2025-01-01T00:00:30Z","rt":9.0}
{"t":"2025-01-01T00:01:10Z","rt":2.0}
{"t":"2025-01-01T00:01:40Z","rt":3.0}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-j",
            "-q",
            "--span",
            "1m",
            "--exec",
            "track_max(\"rt_max\", e.rt); track_percentiles(\"rt\", e.rt);",
            "--span-close",
            r#"print(span.id + " -> " + span.metrics.to_string());"#,
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "non-additive span metrics should not be fatal"
    );

    // The misleading global max must not leak into span.metrics for any window.
    assert!(
        !stdout.contains("rt_max") && !stdout.contains("rt_p"),
        "non-additive metrics must be omitted from span.metrics, got: {}",
        stdout
    );

    // A loud, once-per-key warning must explain the omission and point to the
    // span.events workaround.
    assert!(
        stderr.contains("span.metrics omits 'rt_max'") && stderr.contains("track_max"),
        "expected a warning naming the omitted max metric, got: {}",
        stderr
    );
    assert!(
        stderr.contains("track_percentiles"),
        "expected a warning for omitted percentiles, got: {}",
        stderr
    );
    assert!(
        stderr.contains("span.events"),
        "warning should point to the span.events workaround, got: {}",
        stderr
    );

    // The warning fires once per key, not once per span window.
    assert_eq!(
        stderr.matches("span.metrics omits 'rt_max'").count(),
        1,
        "max warning should be emitted only once across spans, got: {}",
        stderr
    );
}

#[test]
fn test_span_metrics_non_additive_suppressed_by_no_diagnostics() {
    // --no-diagnostics is the standard way to silence diagnostics; the
    // non-additive warning must honor it.
    let input = r#"{"t":"2025-01-01T00:00:00Z","rt":1.0}
{"t":"2025-01-01T00:01:10Z","rt":2.0}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-j",
            "-q",
            "--no-diagnostics",
            "--span",
            "1m",
            "--exec",
            "track_max(\"rt_max\", e.rt);",
            "--span-close",
            "print(span.id);",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(
        !stderr.contains("span.metrics omits"),
        "--no-diagnostics should suppress the non-additive warning, got: {}",
        stderr
    );
}

#[test]
fn test_span_close_requires_span() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["--span-close", "print(1);"], "");
    assert_eq!(exit_code, 2, "missing --span should return invalid usage");
    assert!(
        stderr.contains("--span-close requires --span"),
        "error message should explain dependency"
    );
    assert!(
        stderr.contains("--span N") && stderr.contains("--span-idle 30s"),
        "error message should suggest fixed-size and idle span forms: {}",
        stderr
    );
}

#[test]
fn test_metrics_json_flag() {
    // Test --metrics=json outputs JSON format
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1); track_sum(\"level_\" + e.level, 1);",
            "--metrics=json",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Check that metrics output is in JSON format (data-only mode outputs to stdout)
    assert!(
        stdout.contains("{") && stdout.contains("}"),
        "Metrics output should be JSON format"
    );
    // Should be parseable as JSON
    let json_start = stdout.find('{').expect("Should find JSON start");
    let json_str = &stdout[json_start..];
    let _: serde_json::Value =
        serde_json::from_str(json_str).expect("Metrics output should be valid JSON");
}

#[test]
fn test_stats_only_flag() {
    // Test -s suppresses event output, only shows stats
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-s"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stdout should contain stats (data-only mode outputs to stdout)
    assert!(
        stdout.contains("Lines processed") || stdout.contains("Events"),
        "Should show stats in stdout, got: {}",
        stdout
    );
}

#[test]
fn test_metrics_and_stats_together() {
    // Test that --metrics and --stats can be used together
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "--with-metrics",
            "--with-stats",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should show both metrics and stats
    assert!(
        stderr.contains("Tracked metrics"),
        "Should show metrics header"
    );
    assert!(
        stderr.contains("Lines processed") || stderr.contains("Events"),
        "Should show stats"
    );
}

#[test]
fn test_metrics_file_and_metrics_flag_together() {
    // Test that --metrics-file and --metrics (-m) can be used together
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "--with-metrics",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should show metrics in both stderr and file
    assert!(
        stderr.contains("Tracked metrics"),
        "Should show metrics in stderr"
    );

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");
    assert_eq!(metrics_json["total"], 2, "Metrics file should contain data");
}

#[test]
fn test_metrics_json_with_file_output() {
    // Test --metrics=json with --metrics-file
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "--metrics=json",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");
    let metrics_json: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");
    assert_eq!(metrics_json["total"], 2, "Metrics file should contain data");
}

#[test]
fn test_stats_only_with_filter() {
    // Test -s with filtering to verify stats show filtered counts
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}
{"level": "error", "message": "test4"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--filter", "e.level == \"error\"", "-s"],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stats should show 2 output events and 2 filtered (in stdout for data-only mode)
    assert!(
        stdout.contains("Events") && (stdout.contains("2 output") || stdout.contains("output, 2")),
        "Stats should show filtered counts, got: {}",
        stdout
    );
}

#[test]
fn test_stats_only_with_exec() {
    // Test that -s suppresses event output but exec script still runs
    let input = r#"{"count": 1}
{"count": 2}
{"count": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", e.count);",
            "-s",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // With -s and --with-metrics: stats go to stdout, metrics to stderr, events suppressed
    assert!(
        stdout.contains("Lines processed"),
        "Should show stats in stdout"
    );
    assert!(
        stderr.contains("total") && stderr.contains("6"),
        "Should show metrics in stderr with correct sum, got: {}",
        stderr
    );
}

#[test]
fn test_conflicting_quiet_and_stats_flags() {
    // Test that -q (no events) and --stats work together (stats still emit)
    let input = r#"{"level": "info", "message": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--with-stats", "-q"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // With -q, stats should still emit even though events are suppressed
    assert!(
        stderr.contains("Lines processed"),
        "Stats should emit with -q"
    );
}

#[test]
fn test_metrics_without_tracking_calls() {
    // Test --metrics flag when no tracking functions are called
    let input = r#"{"level": "info", "message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--metrics"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should indicate no metrics were tracked
    assert!(
        stderr.contains("No user metrics")
            || stderr.is_empty()
            || !stderr.contains("Tracked metrics:"),
        "Should indicate no metrics or not show metrics header, got: {}",
        stderr
    );
}

#[test]
fn test_stats_with_parallel_mode() {
    // Test that stats work correctly with parallel mode
    let input: String = (1..=100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--filter",
            "line.to_int() % 10 == 0",
            "--with-stats",
            "--parallel",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stats should show correct counts
    assert!(
        stderr.contains("Lines processed: 100 total"),
        "Should show 100 lines processed"
    );
    assert!(stderr.contains("10 output"), "Should show 10 output events");
}

#[test]
fn test_metrics_file_invalid_path() {
    // Test that invalid metrics file path produces error message
    let input = r#"{"level": "info", "message": "test"}"#;

    let (_stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "--metrics-file",
            "/invalid/path/that/does/not/exist/metrics.json",
        ],
        input,
    );

    // Should show error message about metrics file failure
    assert!(
        stderr.to_lowercase().contains("failed to write metrics")
            || stderr.to_lowercase().contains("no such file"),
        "Should show error message about metrics file, got: {}",
        stderr
    );
}

#[test]
fn test_stats_format_consistency() {
    // Test that stats format is consistent across runs
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (_stdout1, stderr1, exit_code1) =
        run_kelora_with_input(&["-f", "json", "--with-stats"], input);
    let (_stdout2, stderr2, exit_code2) =
        run_kelora_with_input(&["-f", "json", "--with-stats"], input);

    assert_eq!(exit_code1, 0);
    assert_eq!(exit_code2, 0);

    // Stats format should be consistent
    assert!(stderr1.contains("Lines processed"));
    assert!(stderr2.contains("Lines processed"));
    assert!(stderr1.contains("Events created") || stderr1.contains("Events"));
    assert!(stderr2.contains("Events created") || stderr2.contains("Events"));
}

#[test]
fn test_metrics_json_with_metrics_file_writes_json_to_file() {
    // Test that --metrics=json with --metrics-file writes JSON to the file
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "--metrics=json",
            "--metrics-file",
            metrics_file_path,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let metrics_content =
        std::fs::read_to_string(metrics_file_path).expect("Failed to read metrics file");

    // File content should be valid JSON
    let _: serde_json::Value =
        serde_json::from_str(&metrics_content).expect("Metrics file should contain valid JSON");

    // Should contain our tracked metric
    assert!(
        metrics_content.contains("total"),
        "Metrics file should contain tracked metric"
    );
}

#[test]
fn test_stats_only_with_metrics_json() {
    // Test that -s works with --metrics=json (both in data-only mode)
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", 1);",
            "-s",
            "--metrics=json",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should NOT output events
    assert!(
        !stdout.contains("test1") && !stdout.contains("test2"),
        "Should not output events in data-only mode"
    );

    // Should output both stats and JSON metrics to stdout (data-only mode)
    assert!(
        stdout.contains("Lines processed") || stdout.contains("Events"),
        "Should output stats in data-only mode"
    );
    assert!(
        stdout.contains("{") && stdout.contains("}"),
        "Should output JSON metrics in data-only mode"
    );
}

#[test]
fn test_conflicting_stats_flags() {
    // Test that --stats and --no-stats together is handled
    // (--no-stats should take precedence as it's more specific)
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--with-stats", "--no-stats"], input);

    assert_eq!(exit_code, 0, "kelora should handle conflicting flags");

    // With --no-stats, stats should be suppressed in both stdout and stderr
    assert!(
        !stderr.contains("Stats:")
            && !stderr.contains("📈 Stats:")
            && !stdout.contains("Lines processed")
            && !stdout.contains("Events"),
        "--no-stats should suppress stats output"
    );
}

#[test]
fn test_quiet_level_1_suppresses_diagnostics() {
    // Test that -q suppresses events (diagnostics remain)
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--with-stats", "-q"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Events should be suppressed
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "Events should be suppressed with -q"
    );

    // stderr should still show stats
    assert!(
        stderr.contains("Lines processed"),
        "Stats should emit with -q"
    );
}

#[test]
fn test_quiet_and_no_diagnostics_suppress_terminal_output() {
    // Test that combining --quiet with --no-diagnostics suppresses both streams
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-q", "--no-diagnostics"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // stdout should be empty (events suppressed)
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "stdout should be empty when events are suppressed"
    );

    // stderr should also be empty (diagnostics suppressed)
    assert!(
        stderr.is_empty() || stderr.trim().is_empty(),
        "stderr should be empty when diagnostics are suppressed"
    );
}

#[test]
fn test_quiet_level_3_suppresses_script_output() {
    // Test that --no-script-output suppresses script print/eprint
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--ignore-config",
            "--exec",
            "print(\"this should not appear\");",
            "--no-script-output",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Script output should be suppressed while events still emit
    assert!(
        !stdout.contains("this should not appear"),
        "Script print() output should be suppressed with --no-script-output"
    );

    // Events should still appear
    assert!(
        stdout.contains("test"),
        "Event output should remain with --no-script-output"
    );
    assert!(stderr.is_empty() || stderr.trim().is_empty());
}

#[test]
fn test_metrics_with_large_number_of_unique_values() {
    // Stress test: track many unique values
    let input: String = (0..1000)
        .map(|i| format!(r#"{{"id": {}, "value": "item_{}"}}"#, i, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_unique(\"unique_ids\", e.id);",
            "--end",
            "print(`Unique count: ${metrics[\"unique_ids\"].len()}`);",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "kelora should handle large unique sets");

    // Should track all 1000 unique IDs
    assert!(
        stdout.contains("Unique count: 1000"),
        "Should track 1000 unique values"
    );
}

#[test]
fn test_stats_only_with_no_input() {
    // Test -s with empty input
    let input = "";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-s"], input);

    assert_eq!(exit_code, 0, "kelora should handle empty input");

    // Stats should show 0 events (in stdout for data-only mode)
    assert!(
        stdout.contains("0 total") || stdout.contains("Lines processed: 0"),
        "Stats should show 0 events in stdout, got: {}",
        stdout
    );
}

#[test]
fn test_track_avg_finalized_in_parallel_end() {
    // Regression: in --parallel, the workers' __op_ metadata must reach the
    // end stage so `metrics["avg"]` is a number, not a raw {sum, count} map.
    let input = r#"{"ms":10}
{"ms":20}
{"ms":30}
{"ms":40}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "2",
            "--exec",
            "track_avg(\"lat\", e.ms)",
            "--end",
            "print(`avg=${metrics[\"lat\"]}`)",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("avg=25"),
        "parallel --end should see the finalized average, got: {}",
        stdout
    );
}

#[test]
fn test_skip_diagnostics_surface_in_parallel_mode() {
    // Regression: skipped-unit counters must survive the worker -> global
    // tracker channel so field-name typos stay detectable under --parallel.
    let input = r#"{"ms":10}
{"ms":20}
{"ms":30}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "1",
            "--exec",
            "track_sum(\"bytes\", e.nosuch)",
            "--metrics",
            "--diagnostics",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("skipped events with missing values") && stderr.contains("bytes (3)"),
        "parallel runs should report skipped-unit counts: {}",
        stderr
    );
}

#[test]
fn test_track_count_float_category_labels_match_1x() {
    // Regression: float categories must stringify like 1.x track_bucket did
    // (Rust f64 Display: 200.0 -> "200"), not Rhai's Display ("200.0").
    let input = r#"{"v": 200.0}
{"v": 0.5}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"b\", e.v)",
            "--metrics=json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("\"200\"") && !stdout.contains("\"200.0\""),
        "float category 200.0 should keep the 1.x label \"200\": {}",
        stdout
    );
    assert!(
        stdout.contains("\"0.5\""),
        "fractional categories keep their value: {}",
        stdout
    );
}

#[test]
fn test_cross_stage_op_conflict_warns_in_parallel() {
    // The per-call conflict check cannot see across threads; the merge
    // boundary must surface a begin-vs-exec metric-name conflict instead of
    // silently blending the values.
    let input = r#"{"ms":10}
{"ms":20}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "1",
            "--begin",
            "track_sum(\"x\", 100)",
            "--exec",
            "track_min(\"x\", e.ms)",
            "--metrics",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("different track functions across stages")
            && stderr.contains("track_sum")
            && stderr.contains("track_min"),
        "cross-stage conflicts should be warned about: {}",
        stderr
    );
}

#[test]
fn test_multiline_parallel_avg_finalized() {
    // Regression: the multiline (event-batch) worker path must ship __op_
    // metadata too, or avg metrics render as raw maps and merge as "replace".
    let input = "start one\n  detail a\nstart two\n  detail b\nstart three\n  detail c\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "indent",
            "--parallel",
            "--batch-size",
            "1",
            "-q",
            "--exec",
            "track_avg(\"len\", e.line.len()); track_count(\"kind\", \"joined\")",
            "--metrics",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        !stdout.contains("sum") && !stdout.contains("count\":"),
        "avg must be finalized to a number, not a raw map: {}",
        stdout
    );
    assert!(
        stdout.contains("\"joined\": 3") || stdout.contains("joined"),
        "count categories should merge across event batches: {}",
        stdout
    );
}
