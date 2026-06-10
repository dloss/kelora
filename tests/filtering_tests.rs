mod common;
use common::*;

#[test]
fn test_skip_lines_functionality() {
    // Test with headers in CSV-style data
    let input = r#"header1,header2,header3
description,more info,extra
alice,user,200
bob,admin,404
charlie,guest,500"#;

    // Test skipping first 2 lines (headers)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "--skip-lines",
            "2",
            "--filter",
            "line.contains(\"user\") || line.contains(\"admin\")",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "Should have 2 lines after skipping headers and filtering"
    );
    assert!(
        stdout.contains("alice,user,200"),
        "Should contain alice line"
    );
    assert!(stdout.contains("bob,admin,404"), "Should contain bob line");
    assert!(!stdout.contains("header1"), "Should not contain header1");
    assert!(
        !stdout.contains("description"),
        "Should not contain description line"
    );

    // Test with parallel processing
    let (stdout_parallel, _stderr_parallel, exit_code_parallel) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "--skip-lines",
            "2",
            "--parallel",
            "--filter",
            "line.contains(\"user\") || line.contains(\"admin\")",
        ],
        input,
    );
    assert_eq!(
        exit_code_parallel, 0,
        "kelora should exit successfully in parallel mode"
    );

    let lines_parallel: Vec<&str> = stdout_parallel.trim().lines().collect();
    assert_eq!(
        lines_parallel.len(),
        2,
        "Parallel processing should give same result"
    );
}

#[test]
fn test_skip_lines_with_zero() {
    let input = r#"line1
line2
line3"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--skip-lines", "0"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should process all lines when skip-lines is 0"
    );
}

#[test]
fn test_skip_lines_greater_than_input() {
    let input = r#"line1
line2"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--skip-lines", "5"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        0,
        "Should produce no output when skipping more lines than available"
    );
}

#[test]
fn test_ignore_lines_functionality() {
    let input = r#"{"level": "INFO", "message": "This is an info message"}
# This is a comment line
{"level": "ERROR", "message": "This is an error message"}

{"level": "DEBUG", "message": "This is a debug message"}
# Another comment
{"level": "WARN", "message": "This is a warning"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "^#.*|^$", // Ignore comments and empty lines
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "Should output 4 lines (comments and empty lines ignored)"
    );

    // Verify all lines are valid JSON (no comments or empty lines)
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
    }
}

#[test]
fn test_ignore_lines_with_specific_pattern() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}
{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "systemd", // Ignore lines containing "systemd"
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines pattern"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (systemd lines ignored)"
    );

    // Verify systemd lines are not present
    for line in lines {
        assert!(
            !line.contains("systemd"),
            "Output should not contain systemd lines"
        );
    }
}

#[test]
fn test_keep_lines_functionality() {
    let input = r#"{"level": "INFO", "message": "This is an info message"}
# This is a comment line
{"level": "ERROR", "message": "This is an error message"}

{"level": "DEBUG", "message": "This is a debug message"}
# Another comment
{"level": "WARN", "message": "This is a warning"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            r#"^\{"#, // Keep only lines starting with JSON (curly brace)
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with keep-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "Should output 4 lines (only JSON lines kept)"
    );

    // Verify all lines are valid JSON (no comments or empty lines)
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
    }
}

#[test]
fn test_zero_results_with_unseen_filter_field_emits_hint() {
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.levle == "INFO""#], input);

    assert_eq!(
        exit_code, 0,
        "missing filter fields should remain non-fatal in resilient mode"
    );
    assert!(
        stderr.contains("0 events matched"),
        "stderr should explain empty results: {}",
        stderr
    );
    assert!(
        stderr.contains("levle"),
        "stderr should include the unseen field name: {}",
        stderr
    );
}

#[test]
fn test_bare_field_reference_suggests_e_prefix() {
    // The most common newcomer mistake: referencing a field without the `e.`
    // prefix. The hint should point straight at `e.<field>` rather than a
    // string-similar scope variable.
    let input = r#"{"level": "INFO", "status": 200}"#;

    let (_stdout, stderr, _exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", "status >= 500"], input);

    assert!(
        stderr.contains("e.status"),
        "stderr should suggest the e.-prefixed field for a bare reference: {}",
        stderr
    );
}

#[test]
fn test_near_miss_bare_field_suggests_closest_e_prefixed_field() {
    let input = r#"{"level": "INFO", "status": 200}"#;

    let (_stdout, stderr, _exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", "statuss >= 500"], input);

    assert!(
        stderr.contains("e.status"),
        "stderr should suggest the closest e.-prefixed field for a near-miss: {}",
        stderr
    );
}

#[test]
fn test_zero_results_with_existing_filter_field_does_not_emit_typo_hint() {
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.level == "ERROR""#], input);

    assert_eq!(
        exit_code, 0,
        "non-matching filters should remain successful"
    );
    assert!(
        !stderr.contains("0 events matched"),
        "stderr should not suggest a typo for a legitimate filter miss: {}",
        stderr
    );
}

#[test]
fn test_keep_lines_with_specific_pattern() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}
{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            "ERROR|WARN", // Keep only ERROR and WARN level lines
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with keep-lines pattern"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output 2 lines (only ERROR and WARN lines kept)"
    );

    // Verify only ERROR and WARN lines are present
    for line in lines {
        assert!(
            line.contains("ERROR") || line.contains("WARN"),
            "Output should only contain ERROR or WARN lines"
        );
    }
}

#[test]
fn test_combined_keep_lines_and_ignore_lines() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
# This is a comment line
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}

{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}
# Another comment"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            r#"^\{"#, // Keep only lines starting with JSON (curly brace)
            "--ignore-lines",
            "systemd", // Then ignore lines containing "systemd"
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with both keep-lines and ignore-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (JSON lines kept, then systemd lines ignored)"
    );

    // Verify lines are valid JSON and don't contain systemd
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
        assert!(
            !line.contains("systemd"),
            "Output should not contain systemd lines"
        );
    }

    // Verify specific levels are present
    let content = stdout.trim();
    assert!(content.contains("INFO"));
    assert!(content.contains("ERROR"));
    assert!(content.contains("WARN"));
    assert!(!content.contains("DEBUG")); // DEBUG lines contain systemd
}

#[test]
fn test_ignore_lines_with_stats() {
    let input = r#"{"level": "INFO", "message": "Valid message 1"}
# Comment to ignore
{"level": "ERROR", "message": "Valid message 2"}
# Another comment
{"level": "WARN", "message": "Valid message 3"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "^#",
            "--with-stats",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines and stats enabled"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output 3 non-comment lines");

    let stats = extract_stats_lines(&stderr);
    let lines_processed = stats
        .iter()
        .find(|line| line.starts_with("Lines processed:"))
        .expect("Stats should report line counts");
    assert_eq!(
        lines_processed,
        "Lines processed: 5 total, 2 filtered (40.0%), 0 errors (0.0%)"
    );

    let events_created = stats
        .iter()
        .find(|line| line.starts_with("Events created:"))
        .expect("Stats should report event counts");
    assert_eq!(
        events_created,
        "Events created: 3 total, 3 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_multiple_filters() {
    let input = r#"{"level": "INFO", "status": 200, "response_time": 50}
{"level": "ERROR", "status": 500, "response_time": 100}
{"level": "WARN", "status": 404, "response_time": 200}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--filter",
            "e.status >= 400",
            "--filter",
            "e.response_time > 150",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        1,
        "Should filter to 1 line matching both conditions"
    );

    let parsed: serde_json::Value =
        serde_json::from_str(lines[0]).expect("Line should be valid JSON");
    assert_eq!(parsed["level"], "WARN");
    assert_eq!(parsed["status"], 404);
    assert_eq!(parsed["response_time"], 200);
}

#[test]
fn test_ordered_filter_exec_stages() {
    // Test that filter and exec stages execute in the exact CLI order
    let input = r#"{"status": "200", "message": "OK"}"#;

    // Test correct order: exec (convert) -> filter -> filter -> exec (add field)
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--ignore-config",
            "--exec",
            "e.status=e.status.to_int()",
            "--filter",
            "e.status > 100",
            "--filter",
            "e.status < 400",
            "--exec",
            "e.level=\"info\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("status=200"));
    assert!(stdout.contains("level='info'"));

    // Test wrong order: filter before conversion should fail
    let (stdout2, _stderr2, _exit_code2) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.status > 100", // This will fail on string "200"
            "--exec",
            "e.status=e.status.to_int()",
            "--filter",
            "e.status < 400",
            "--exec",
            "e.level=\"info\"",
        ],
        input,
    );

    // Should produce no output because string "200" > 100 comparison doesn't work as expected
    assert!(stdout2.trim().is_empty());
}

#[test]
fn test_complex_ordered_pipeline() {
    // Test a more complex pipeline with transformations and filtering
    let input = r#"{"value": 5}
{"value": 15}
{"value": 25}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--ignore-config",
            "--exec",
            "e.doubled = e.value * 2",
            "--filter",
            "e.doubled > 20",
            "--exec",
            "e.status = if e.doubled > 30 { \"high\" } else { \"medium\" }",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stderr, "");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2); // Should filter out value=5 (doubled=10)

    // Check first line (value=15, doubled=30, status="medium")
    assert!(lines[0].contains("value=15"));
    assert!(lines[0].contains("doubled=30"));
    assert!(lines[0].contains("status='medium'"));

    // Check second line (value=25, doubled=50, status="high")
    assert!(lines[1].contains("value=25"));
    assert!(lines[1].contains("doubled=50"));
    assert!(lines[1].contains("status='high'"));
}

#[test]
fn test_levels_before_exec_limits_exec_work() {
    let input = r#"{"level":"ERROR","message":"fail"}
{"level":"INFO","message":"ok"}
{"level":"WARN","message":"heads-up"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--levels",
            "error",
            "--exec",
            "track_count(\"exec_runs\")",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Filtering by --levels before --exec should succeed"
    );

    let exec_metric_line = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("exec_runs"))
        .expect("Metrics output should list exec_runs");
    assert!(
        exec_metric_line.contains("= 1"),
        "Exec stage should run once when --levels precedes it (saw `{}`)",
        exec_metric_line.trim()
    );
}

#[test]
fn test_exec_before_levels_observes_all_events() {
    let input = r#"{"level":"ERROR","message":"fail"}
{"level":"INFO","message":"ok"}
{"level":"WARN","message":"heads-up"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_count(\"exec_runs\")",
            "--levels",
            "error",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Filtering by --levels after --exec should succeed"
    );

    let exec_metric_line = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("exec_runs"))
        .expect("Metrics output should list exec_runs");
    assert!(
        exec_metric_line.contains("= 3"),
        "Exec stage should run on all three events when it precedes --levels (saw `{}`)",
        exec_metric_line.trim()
    );
}
