mod common;
use common::*;

#[test]
fn test_help_flag() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["--help"], "");
    assert_eq!(exit_code, 0, "kelora --help should exit successfully");
    assert!(
        stdout.contains("command-line log analysis tool"),
        "Help should describe the tool"
    );
    assert!(
        stdout.contains("--filter"),
        "Help should mention filter option"
    );
    assert!(
        stdout.contains("--parallel"),
        "Help should mention parallel option"
    );
}

#[test]
fn test_quick_help_flag() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-h"], "");
    assert_eq!(exit_code, 0, "kelora -h should exit successfully");
    assert!(
        stdout.contains("Quick Examples:"),
        "Quick help should include example commands"
    );
    assert!(
        stdout.contains("--until now -l warn,error --stats"),
        "Quick help should include the fixed time-range example"
    );
    assert!(
        stdout.contains("levelmap/keymap/tailmap"),
        "Quick help should list tailmap in output formats"
    );
    assert!(
        stdout.contains("--freq"),
        "Quick help should mention --freq, the shortest path to a frequency table"
    );
}

#[test]
fn test_basic_json_parsing() {
    let input = r#"{"level": "INFO", "message": "Hello world", "status": 200}
{"level": "ERROR", "message": "Something failed", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-F", "json"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should output 2 lines");

    // Parse JSON output
    let first_line: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first_line["level"], "INFO");
    assert_eq!(first_line["status"], 200);
}

#[test]
fn test_filter_expression() {
    let input = r#"{"level": "INFO", "status": 200}
{"level": "ERROR", "status": 500}
{"level": "DEBUG", "status": 404}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-F", "json", "--filter", "e.status >= 400"],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should filter to 2 lines (status >= 400)");

    // Check that filtered results have status >= 400
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        let status = parsed["status"]
            .as_i64()
            .expect("Status should be a number");
        assert!(status >= 400, "Filtered results should have status >= 400");
    }
}

#[test]
fn test_empty_input() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json"], "");
    assert_eq!(exit_code, 0, "kelora should handle empty input gracefully");
    assert_eq!(stdout.trim(), "", "Empty input should produce no output");
}

#[test]
fn test_file_input() {
    let file_content = r#"{"level": "INFO", "message": "File input test"}
{"level": "ERROR", "message": "Another line"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_file(&["-f", "json"], file_content);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with file input"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should output 2 lines from file");
}

#[test]
fn test_text_output_format() {
    let input = r#"{"level": "INFO", "message": "Hello world", "status": 200}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-F", "default"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Text format should be key=value pairs
    assert!(
        stdout.contains("level='INFO'"),
        "Text output should contain level='INFO'"
    );
    assert!(
        stdout.contains("status=200"),
        "Text output should contain status=200"
    );
    assert!(
        stdout.contains("message='Hello world'"),
        "Text output should contain quoted message"
    );
}

#[test]
fn test_keys_filtering() {
    let input = r#"{"level": "INFO", "message": "Hello world", "status": 200, "timestamp": "2023-01-01T00:00:00Z"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-F", "json", "--keys", "level,status"],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Output should be valid JSON");

    // Should only contain specified keys
    assert!(parsed.get("level").is_some(), "Should contain level");
    assert!(parsed.get("status").is_some(), "Should contain status");
    assert!(
        parsed.get("message").is_none(),
        "Should not contain message"
    );
    assert!(
        parsed.get("timestamp").is_none(),
        "Should not contain timestamp"
    );
}

#[test]
fn test_exec_script() {
    let input = r#"{"level": "INFO", "status": 200}
{"level": "ERROR", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "e.alert_level = if e.status >= 400 { \"high\" } else { \"low\" };",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should output 2 lines");

    // Check that exec script added alert_level field
    let first_line: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first_line["alert_level"], "low");

    let second_line: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second_line["alert_level"], "high");
}

#[test]
fn test_begin_and_end_stages() {
    let input = r#"{"level": "INFO"}
{"level": "ERROR"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--begin",
            "print(\"Starting analysis...\")",
            "--end",
            "print(\"Analysis complete\")",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    assert!(
        stdout.contains("Starting analysis..."),
        "Begin stage should execute"
    );
    assert!(
        stdout.contains("Analysis complete"),
        "End stage should execute"
    );
}

#[test]
fn test_field_modification_and_addition() {
    let input = r#"{"user": "alice", "score": 85}
{"user": "bob", "score": 92}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "e.grade = if e.score >= 90 { \"A\" } else { \"B\" }; e.bonus_points = e.score * 0.1;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should output 2 lines");

    let first: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first["grade"], "B");
    assert_eq!(first["bonus_points"], 8.5);

    let second: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second["grade"], "A");
    assert_eq!(second["bonus_points"], 9.2);
}

#[test]
fn test_string_functions() {
    let input = r#"{"message": "Error: Something failed", "code": "123"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "e.has_error = e.message.contains(\"Error\"); e.code_num = e.code.to_int();",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Output should be valid JSON");
    assert_eq!(parsed["has_error"], true, "contains() function should work");
    assert_eq!(parsed["code_num"], 123, "to_int() function should work");
}

#[test]
fn test_complex_rhai_expressions() {
    let input = r#"{"user": "alice", "status": 404}
{"user": "bob", "status": 500}
{"user": "charlie", "status": 200}
{"user": "alice", "status": 403}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--filter",
            "e.status >= 400 && e.user.contains(\"a\")",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should filter to 2 lines (alice with status >= 400)"
    );

    // Verify both results are alice with status >= 400
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert_eq!(parsed["user"], "alice");
        let status = parsed["status"].as_i64().unwrap();
        assert!(status >= 400);
    }
}

#[test]
fn test_print_function_output() {
    let input = r#"{"user": "alice", "level": "INFO"}
{"user": "bob", "level": "ERROR"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "print(\"Processing user: \" + e.user);",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    assert!(
        stdout.contains("Processing user: alice"),
        "Should print alice debug message"
    );
    assert!(
        stdout.contains("Processing user: bob"),
        "Should print bob debug message"
    );
    assert!(
        stdout.contains("\"user\":\"alice\""),
        "Should also output JSON data"
    );
}
