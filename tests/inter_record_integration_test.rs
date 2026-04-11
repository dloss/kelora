use std::io::Write;
use std::process::{Command, Stdio};

fn kelora_binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_kelora")
}

fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    let mut cmd = Command::new(kelora_binary_path())
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null")
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
fn test_delta_and_prev_across_events() {
    let input = r#"{"duration_ms": 100}
{"duration_ms": 130}
{"duration_ms": 210}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.prev = prev("duration_ms"); e.delta = delta("duration_ms");"#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("\"prev\":null") || !lines[0].contains("\"prev\":"));
    assert!(lines[1].contains("\"prev\":100"));
    assert!(lines[1].contains("\"delta\":30.0") || lines[1].contains("\"delta\":30"));
    assert!(lines[2].contains("\"prev\":130"));
    assert!(lines[2].contains("\"delta\":80.0") || lines[2].contains("\"delta\":80"));
}

#[test]
fn test_filtered_events_still_advance_inter_record_history() {
    let input = r#"{"duration_ms": 100}
{"duration_ms": 120}
{"duration_ms": 200}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.delta = delta("duration_ms");"#,
            "--filter",
            "e.duration_ms >= 120",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"duration_ms\":120"));
    assert!(lines[0].contains("\"delta\":20.0") || lines[0].contains("\"delta\":20"));
    assert!(lines[1].contains("\"duration_ms\":200"));
    assert!(lines[1].contains("\"delta\":80.0") || lines[1].contains("\"delta\":80"));
}

#[test]
fn test_inter_record_helpers_error_in_parallel_mode() {
    let input = r#"{"duration_ms": 100}
{"duration_ms": 110}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--strict",
            "--exec",
            r#"e.prev = prev("duration_ms");"#,
        ],
        input,
    );

    assert_ne!(exit_code, 0);
    assert!(stderr.contains("'prev' is not available in --parallel mode"));
}
