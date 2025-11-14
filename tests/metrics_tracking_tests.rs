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
            "track_count(\"events_total\");",
            "--metrics",
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
            "track_count(\"errors\")",
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
fn test_track_bucket_function() {
    let input = r#"{"status": "200", "method": "GET"}
{"status": "404", "method": "POST"}
{"status": "200", "method": "GET"}
{"status": "500", "method": "PUT"}
{"status": "404", "method": "GET"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "--exec", "track_bucket(\"status_counts\", e.status); track_bucket(\"method_counts\", e.method);",
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
        "--exec", "track_count(\"total\"); track_unique(\"users\", e.user); track_bucket(\"status_dist\", e.status); track_min(\"min_time\", e.response_time); track_max(\"max_time\", e.response_time);",
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
            "--metrics",
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
fn test_track_bucket_with_unit() {
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
            "track_bucket(\"status_dist\", e.status); track_bucket(\"user_dist\", e.user.or_empty());",
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
            "track_count(\"total\"); track_count(\"level_\" + e.level); track_sum(\"message_length\", e.message.len())",
            "--metrics",
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
            "track_count(\"total\"); track_count(\"level_\" + e.level); track_sum(\"message_length\", e.message.len())",
            "--metrics",
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
            "track_count(\"total\"); track_count(\"level_\" + e.level)",
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
fn test_metrics_parallel_consistency() {
    // Test that parallel mode produces correct metrics with different batch sizes
    let input = r#"{"level":"info","message":"test1"}
{"level":"error","message":"test2"}
{"level":"info","message":"test3"}
{"level":"warn","message":"test4"}
{"level":"error","message":"test5"}"#;

    let exec_script = "track_count(\"total\"); track_count(\"level_\" + e.level)";

    // Run in parallel mode with batch-size 1
    let (_stdout1, stderr1, exit_code1) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            exec_script,
            "--metrics",
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
            "--metrics",
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
            "-F",
            "none",
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
            "-F",
            "none",
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
            "-F",
            "none",
            "--span",
            "2",
            "--exec",
            "track_count(\"events\");",
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
fn test_span_close_requires_span() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["--span-close", "print(1);"], "");
    assert_eq!(exit_code, 2, "missing --span should return invalid usage");
    assert!(
        stderr.contains("--span-close requires --span"),
        "error message should explain dependency"
    );
}

#[test]
fn test_metrics_json_flag() {
    // Test --metrics-json outputs JSON format
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"total\"); track_count(\"level_\" + e.level);",
            "--metrics-json",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Check that metrics output is in JSON format
    assert!(
        stderr.contains("{") && stderr.contains("}"),
        "Metrics output should be JSON format"
    );
    // Should be parseable as JSON
    let json_start = stderr.find('{').expect("Should find JSON start");
    let json_str = &stderr[json_start..];
    let _: serde_json::Value =
        serde_json::from_str(json_str).expect("Metrics output should be valid JSON");
}

#[test]
fn test_stats_only_flag() {
    // Test --stats-only suppresses event output, only shows stats
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--stats-only"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stdout should be empty (no events output)
    assert!(
        stdout.trim().is_empty(),
        "Stdout should be empty with --stats-only, got: {}",
        stdout
    );

    // Stderr should contain stats
    assert!(
        stderr.contains("Lines processed") || stderr.contains("Events"),
        "Should show stats in stderr, got: {}",
        stderr
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
            "track_count(\"total\");",
            "--metrics",
            "--stats",
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
            "track_count(\"total\");",
            "--metrics",
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
    // Test --metrics-json with --metrics-file
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let metrics_file_path = temp_file.path().to_str().unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"total\");",
            "--metrics-json",
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
    // Test --stats-only with filtering to verify stats show filtered counts
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}
{"level": "error", "message": "test4"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"error\"",
            "--stats-only",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stdout should be empty
    assert!(stdout.trim().is_empty(), "Stdout should be empty");

    // Stats should show 2 output events and 2 filtered
    assert!(
        stderr.contains("Events") && (stderr.contains("2 output") || stderr.contains("output, 2")),
        "Stats should show filtered counts, got: {}",
        stderr
    );
}

#[test]
fn test_stats_only_with_exec() {
    // Test that --stats-only suppresses event output but exec script still runs
    let input = r#"{"count": 1}
{"count": 2}
{"count": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"total\", e.count);",
            "--stats-only",
            "--metrics",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Stdout should be empty (no event output)
    assert!(stdout.trim().is_empty(), "Stdout should be empty");

    // Stats and metrics should still appear
    assert!(stderr.contains("Lines processed"), "Should show stats");
    assert!(
        stderr.contains("total") && stderr.contains("6"),
        "Should show metrics with correct sum, got: {}",
        stderr
    );
}

#[test]
fn test_conflicting_quiet_and_stats_flags() {
    // Test that -q (quiet) and --stats work together (quiet suppresses other diagnostics)
    let input = r#"{"level": "info", "message": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--stats", "-q"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // With -q, stats should be suppressed
    assert!(
        !stderr.contains("Lines processed"),
        "Quiet mode should suppress stats"
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
            "--stats",
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
            "track_count(\"total\");",
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

    let (_stdout1, stderr1, exit_code1) = run_kelora_with_input(&["-f", "json", "--stats"], input);
    let (_stdout2, stderr2, exit_code2) = run_kelora_with_input(&["-f", "json", "--stats"], input);

    assert_eq!(exit_code1, 0);
    assert_eq!(exit_code2, 0);

    // Stats format should be consistent
    assert!(stderr1.contains("Lines processed"));
    assert!(stderr2.contains("Lines processed"));
    assert!(stderr1.contains("Events created") || stderr1.contains("Events"));
    assert!(stderr2.contains("Events created") || stderr2.contains("Events"));
}
