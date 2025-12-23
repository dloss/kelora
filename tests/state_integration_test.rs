// tests/state_integration_test.rs
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn kelora_binary_path() -> &'static str {
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    env!("CARGO_BIN_EXE_kelora")
}

/// Helper function to run kelora with given arguments and input via stdin
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    let mut cmd = Command::new(kelora_binary_path())
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
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
fn test_state_basic_set_and_get() {
    let input = r#"{"id": 1, "value": "first"}
{"id": 2, "value": "second"}
{"id": 3, "value": "third"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                state["count"] = (state["count"] ?? 0) + 1;
                e.event_number = state["count"];
            "#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Verify that the count increments correctly
    assert!(stdout.contains("\"event_number\":1"));
    assert!(stdout.contains("\"event_number\":2"));
    assert!(stdout.contains("\"event_number\":3"));
}

#[test]
fn test_state_deduplication() {
    let input = r#"{"request_id": "req-001", "status": "start"}
{"request_id": "req-002", "status": "start"}
{"request_id": "req-001", "status": "duplicate"}
{"request_id": "req-003", "status": "start"}
{"request_id": "req-002", "status": "duplicate"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                if state.contains(e.request_id) == false {
                    state[e.request_id] = true;
                    e.is_first = true;
                } else {
                    e.is_first = false;
                }
            "#,
            "--filter",
            "e.is_first == true",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Should only see the first occurrence of each request_id
    assert_eq!(stdout.matches("req-001").count(), 1);
    assert_eq!(stdout.matches("req-002").count(), 1);
    assert_eq!(stdout.matches("req-003").count(), 1);
    assert!(!stdout.contains("duplicate"));
}

#[test]
fn test_state_counting_by_category() {
    let input = r#"{"level": "ERROR", "message": "error 1"}
{"level": "INFO", "message": "info 1"}
{"level": "ERROR", "message": "error 2"}
{"level": "WARN", "message": "warn 1"}
{"level": "ERROR", "message": "error 3"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                let key = "count_" + e.level;
                state[key] = (state[key] ?? 0) + 1;
                e.count = state[key];
            "#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Verify that ERROR counts increment
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 5);

    // First ERROR should have count 1
    assert!(lines[0].contains("\"level\":\"ERROR\""));
    assert!(lines[0].contains("\"count\":1"));

    // INFO should have count 1
    assert!(lines[1].contains("\"level\":\"INFO\""));
    assert!(lines[1].contains("\"count\":1"));

    // Second ERROR should have count 2
    assert!(lines[2].contains("\"level\":\"ERROR\""));
    assert!(lines[2].contains("\"count\":2"));

    // WARN should have count 1
    assert!(lines[3].contains("\"level\":\"WARN\""));
    assert!(lines[3].contains("\"count\":1"));

    // Third ERROR should have count 3
    assert!(lines[4].contains("\"level\":\"ERROR\""));
    assert!(lines[4].contains("\"count\":3"));
}

#[test]
fn test_state_persistence_across_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create two input files
    let file1 = temp_dir.path().join("file1.jsonl");
    let file2 = temp_dir.path().join("file2.jsonl");

    fs::write(
        &file1,
        r#"{"id": 1, "file": "first"}
{"id": 2, "file": "first"}"#,
    )
    .expect("Failed to write file1");

    fs::write(
        &file2,
        r#"{"id": 3, "file": "second"}
{"id": 4, "file": "second"}"#,
    )
    .expect("Failed to write file2");

    let output = Command::new(kelora_binary_path())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args([
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                state["count"] = (state["count"] ?? 0) + 1;
                e.global_count = state["count"];
            "#,
            file1.to_str().unwrap(),
            file2.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "Command should succeed. stderr: {}",
        stderr
    );

    // Verify that count continues across files
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4);

    assert!(lines[0].contains("\"global_count\":1"));
    assert!(lines[1].contains("\"global_count\":2"));
    assert!(lines[2].contains("\"global_count\":3"));
    assert!(lines[3].contains("\"global_count\":4"));
}

#[test]
fn test_state_in_all_stages() {
    let input = r#"{"id": 1}
{"id": 2}
{"id": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--begin",
            r#"state["initialized"] = true; state["event_count"] = 0"#,
            "--exec",
            r#"
                state["event_count"] = state["event_count"] + 1;
                e.was_initialized = state["initialized"];
                e.event_number = state["event_count"];
            "#,
            "--end",
            r#"print("Total events: " + state["event_count"])"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Check that state was available in all stages
    assert_eq!(stdout.matches("\"was_initialized\":true").count(), 3);
    assert!(stdout.contains("Total events: 3"));
}

#[test]
fn test_state_contains_and_len() {
    let input = r#"{"key": "a", "value": 1}
{"key": "b", "value": 2}
{"key": "c", "value": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                state[e.key] = e.value;
                e.state_size = state.len();
                e.has_a = state.contains("a");
                e.has_d = state.contains("d");
            "#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);

    // First event: state has 1 key (a)
    assert!(lines[0].contains("\"state_size\":1"));
    assert!(lines[0].contains("\"has_a\":true"));
    assert!(lines[0].contains("\"has_d\":false"));

    // Second event: state has 2 keys (a, b)
    assert!(lines[1].contains("\"state_size\":2"));
    assert!(lines[1].contains("\"has_a\":true"));

    // Third event: state has 3 keys (a, b, c)
    assert!(lines[2].contains("\"state_size\":3"));
    assert!(lines[2].contains("\"has_a\":true"));
}

#[test]
fn test_state_not_available_in_parallel_mode() {
    let input = r#"{"id": 1}
{"id": 2}
{"id": 3}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--strict",
            "--exec",
            r#"state["count"] = (state["count"] ?? 0) + 1"#,
        ],
        input,
    );

    // Should fail with an error about state not being available in parallel mode
    assert_ne!(
        exit_code, 0,
        "Command should fail when accessing state in parallel mode"
    );
    assert!(
        stderr.contains("state") && stderr.contains("parallel"),
        "Error message should mention state and parallel mode. stderr: {}",
        stderr
    );
}

#[test]
fn test_state_type_mixing() {
    let input = r#"{"step": 1}
{"step": 2}
{"step": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                if e.step == 1 {
                    state["data"] = 0;
                } else if e.step == 2 {
                    state["data"] = state["data"] + 1;
                } else if e.step == 3 {
                    state["data"] = "string";
                }
                e.data_type = type_of(state["data"]);
            "#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Verify that types can change
    assert!(stdout.contains("\"data_type\":\"i64\"") || stdout.contains("\"data_type\":\"i32\""));
    assert!(stdout.contains("\"data_type\":\"string\""));
}

#[test]
fn test_state_with_complex_values() {
    let input = r#"{"user": "alice", "action": "login"}
{"user": "bob", "action": "login"}
{"user": "alice", "action": "logout"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                if !state.contains(e.user) {
                    state[e.user] = #{};
                }
                let user_state = state[e.user];
                user_state[e.action] = (user_state[e.action] ?? 0) + 1;
                state[e.user] = user_state;
                e.user_actions = state[e.user];
            "#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Verify that nested maps work
    assert!(stdout.contains("alice"));
    assert!(stdout.contains("bob"));
    assert!(stdout.contains("login"));
    assert!(stdout.contains("logout"));
}
