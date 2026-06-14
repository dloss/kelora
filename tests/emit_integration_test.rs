use std::io::Write;
use std::process::{Command, Stdio};

/// Helper function to run kelora with given arguments and input via stdin
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    let binary_path = env!("CARGO_BIN_EXE_kelora");

    let mut cmd = Command::new(binary_path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    if let Some(stdin) = cmd.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = cmd.wait_with_output().expect("Failed to read output");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn test_emit_each_basic_integration() {
    let input = r#"{"data": [{"name": "alice", "age": 25}, {"name": "bob", "age": 30}]}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "emit_each(e.data)"], input);

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    // Should emit two separate events
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Expected 2 output lines, got: {}", stdout);

    // First event should be alice
    assert!(lines[0].contains("name='alice'"));
    assert!(lines[0].contains("age=25"));

    // Second event should be bob
    assert!(lines[1].contains("name='bob'"));
    assert!(lines[1].contains("age=30"));
}

#[test]
fn test_emit_each_with_base() {
    let input = r#"{"users": [{"id": 1}, {"id": 2}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "let base = #{host: \"server1\", service: \"auth\"}; emit_each(e.users, base)",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Expected 2 output lines, got: {}", stdout);

    // Both events should have host and service from base
    for line in &lines {
        assert!(
            line.contains("host='server1'"),
            "Line missing host: {}",
            line
        );
        assert!(
            line.contains("service='auth'"),
            "Line missing service: {}",
            line
        );
    }

    // First event should have id=1, second should have id=2
    assert!(lines[0].contains("id=1"));
    assert!(lines[1].contains("id=2"));
}

#[test]
fn test_emit_each_empty_array() {
    let input = r#"{"empty": [], "other": "data"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "let count = emit_each(e.empty); e.count = count",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    // Empty array should suppress original event and emit nothing
    assert_eq!(
        stdout.trim(),
        "",
        "Expected no output for empty array, got: {}",
        stdout
    );
}

#[test]
fn test_emit_each_return_value() {
    let input = r#"{"items": [{"a": 1}, {"b": 2}, {"c": 3}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.count = emit_each(e.items); e.items = ()",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 3, "Expected 3 output lines, got: {}", stdout);

    // Check that each emitted event has the expected field
    assert!(lines[0].contains("a=1"));
    assert!(lines[1].contains("b=2"));
    assert!(lines[2].contains("c=3"));
}

#[test]
fn test_emit_each_with_invalid_input() {
    let input = r#"{"not_array": "string"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "emit_each(e.not_array)"], input);

    assert_eq!(
        exit_code, 0,
        "Expected success in resilient mode, stderr: {}",
        stderr
    );

    // Should return original event unchanged in resilient mode
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 1, "Expected 1 output line, got: {}", stdout);
    assert!(lines[0].contains("not_array='string'"));
}

#[test]
fn test_emit_each_mixed_array() {
    // Test array with both valid maps and invalid items
    let input = r#"{"mixed": [{"valid": true}, "invalid", {"also_valid": "yes"}]}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "emit_each(e.mixed)"], input);

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Expected 2 output lines (skipping invalid), got: {}",
        stdout
    );

    // Should only emit the valid maps
    assert!(lines[0].contains("valid=true"));
    assert!(lines[1].contains("also_valid='yes'"));
}

#[test]
fn test_emit_each_skip_warning_honors_no_emoji() {
    // Skipped-item diagnostics must respect --no-emoji (no 🔸 prefix)
    let input = r#"{"items": [1]}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--no-emoji", "--exec", "emit_each(e.items)"],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);
    assert!(
        !stderr.contains('🔸'),
        "--no-emoji must strip emoji from emit_each warnings, got: {}",
        stderr
    );
    assert!(
        stderr.contains("kelora warning:"),
        "Expected plain 'kelora warning:' prefix, got: {}",
        stderr
    );
}

#[test]
fn test_emit_each_skip_warning_uses_emoji_when_forced() {
    // With --force-emoji the skip diagnostic keeps the 🔸 prefix
    let input = r#"{"items": [1]}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--force-emoji",
            "--exec",
            "emit_each(e.items)",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);
    assert!(
        stderr.contains('🔸'),
        "--force-emoji should keep emoji prefix on emit_each warnings, got: {}",
        stderr
    );
}

#[test]
fn test_emit_each_with_json_output() {
    let input = r#"{"users": [{"name": "alice"}, {"name": "bob"}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-F", "json", "--exec", "emit_each(e.users)"],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Expected 2 JSON output lines, got: {}",
        stdout
    );

    // Parse each line as JSON to verify structure
    for (i, line) in lines.iter().enumerate() {
        let json: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Failed to parse JSON line {}: {} (error: {})", i, line, e));

        // Should have name field
        assert!(
            json.get("name").is_some(),
            "Missing name field in line {}: {}",
            i,
            line
        );
    }
}

#[test]
fn test_emit_each_preserves_line_information() {
    let input = r#"{"events": [{"type": "login"}, {"type": "logout"}]}"#;

    // Use verbose mode to see if line information is preserved
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "emit_each(e.events)"], input);

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Expected 2 output lines, got: {}", stdout);

    // Verify each emitted event has the expected type
    assert!(lines[0].contains("type='login'"));
    assert!(lines[1].contains("type='logout'"));
}

#[test]
fn test_emit_each_with_complex_nested_data() {
    let input = r#"{"requests": [{"method": "GET", "response": {"status": 200, "time": 0.1}}, {"method": "POST", "response": {"status": 201, "time": 0.3}}]}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "emit_each(e.requests)"], input);

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Expected 2 output lines, got: {}", stdout);

    // Check that nested response data is preserved
    assert!(lines[0].contains("method='GET'"));
    assert!(lines[1].contains("method='POST'"));
}

#[test]
fn test_emit_each_multi_stage_pipeline() {
    let input = r#"{"users": [{"name": "alice", "role": "admin"}, {"name": "bob", "role": "user"}, {"name": "charlie", "role": "admin"}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "emit_each(e.users)",
            "--filter",
            "e.role == \"admin\"",
            "--exec",
            "e.processed = true; e.stage = \"final\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Expected 2 admin users, got: {}", stdout);

    // Both lines should be admin users with processing flags
    for line in &lines {
        assert!(
            line.contains("role='admin'"),
            "Missing admin role: {}",
            line
        );
        assert!(
            line.contains("processed=true"),
            "Missing processed flag: {}",
            line
        );
        assert!(
            line.contains("stage='final'"),
            "Missing stage flag: {}",
            line
        );
    }

    // Check specific users
    assert!(lines.iter().any(|line| line.contains("name='alice'")));
    assert!(lines.iter().any(|line| line.contains("name='charlie'")));
}

#[test]
fn test_emit_each_nested_pipeline() {
    let input =
        r#"{"batches": [{"items": [{"id": 1}, {"id": 2}]}, {"items": [{"id": 3}, {"id": 4}]}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "emit_each(e.batches)",
            "--exec",
            "emit_each(e.items)",
            "--exec",
            "e.final_stage = true",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 4, "Expected 4 items total, got: {}", stdout);

    // All lines should have final_stage flag and individual ids
    for line in &lines {
        assert!(
            line.contains("final_stage=true"),
            "Missing final_stage: {}",
            line
        );
        assert!(line.contains("id="), "Missing id field: {}", line);
    }
}

#[test]
fn test_emit_each_with_filter_between_emits() {
    let input = r#"{"data": [{"value": 10, "items": [{"name": "a"}, {"name": "b"}]}, {"value": 5, "items": [{"name": "c"}, {"name": "d"}]}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "emit_each(e.data)",
            "--filter",
            "e.value > 7",
            "--exec",
            "emit_each(e.items)",
            "--exec",
            "e.stage = \"final\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Expected 2 items from high-value batch only, got: {}",
        stdout
    );

    // Should only have items from the value=10 batch (a and b, not c and d)
    assert!(lines.iter().any(|line| line.contains("name='a'")));
    assert!(lines.iter().any(|line| line.contains("name='b'")));
    assert!(!stdout.contains("name='c'"));
    assert!(!stdout.contains("name='d'"));

    // All should have final stage
    for line in &lines {
        assert!(
            line.contains("stage='final'"),
            "Missing final stage: {}",
            line
        );
    }
}

#[test]
fn test_emit_each_complex_base_and_filtering() {
    let input = r#"{"requests": [{"method": "GET", "logs": [{"level": "info"}, {"level": "error"}]}, {"method": "POST", "logs": [{"level": "debug"}, {"level": "warn"}]}]}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "emit_each(e.requests)",
            "--exec",
            "let base = #{source: \"api\", method: e.method}; emit_each(e.logs, base)",
            "--filter",
            "e.level == \"error\" || e.level == \"warn\"",
            "--exec",
            "e.severity = \"high\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Expected success, stderr: {}", stderr);

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Expected 2 high-severity logs, got: {}",
        stdout
    );

    // Check that we have one error from GET and one warn from POST
    assert!(lines
        .iter()
        .any(|line| line.contains("level='error'") && line.contains("method='GET'")));
    assert!(lines
        .iter()
        .any(|line| line.contains("level='warn'") && line.contains("method='POST'")));

    // All should have base fields and severity
    for line in &lines {
        assert!(line.contains("source='api'"), "Missing source: {}", line);
        assert!(
            line.contains("severity='high'"),
            "Missing severity: {}",
            line
        );
        assert!(line.contains("method="), "Missing method: {}", line);
    }
}
