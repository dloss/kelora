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
fn docs_recipe_detects_latency_jump_with_delta() {
    let input = r#"{"svc":"api","duration_ms":100}
{"svc":"api","duration_ms":120}
{"svc":"api","duration_ms":900}
{"svc":"api","duration_ms":910}"#;

    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.delta_ms = delta("duration_ms")"#,
            "--filter",
            "e.delta_ms != () && e.delta_ms > 500",
        ],
        input,
    );

    assert_eq!(code, 0, "stderr: {}", stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"duration_ms\":900"));
    assert!(lines[0].contains("\"delta_ms\":780.0") || lines[0].contains("\"delta_ms\":780"));
}

#[test]
fn docs_recipe_compares_against_three_back_baseline() {
    let input = r#"{"value":10}
{"value":20}
{"value":30}
{"value":50}"#;

    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.baseline_3 = lag("value", 3); e.delta_3 = delta("value", 3);"#,
            "--filter",
            "e.delta_3 != () && e.delta_3 >= 40",
        ],
        input,
    );

    assert_eq!(code, 0, "stderr: {}", stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"value\":50"));
    assert!(lines[0].contains("\"baseline_3\":10"));
    assert!(lines[0].contains("\"delta_3\":40.0") || lines[0].contains("\"delta_3\":40"));
}

#[test]
fn docs_recipe_smooths_latency_with_ewma() {
    let input = r#"{"latency_ms":100}
{"latency_ms":200}
{"latency_ms":50}"#;

    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.latency_smooth = ewma("latency_ms", e.latency_ms.to_float(), 0.5)"#,
        ],
        input,
    );

    assert_eq!(code, 0, "stderr: {}", stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].contains("\"latency_smooth\":100.0")
            || lines[0].contains("\"latency_smooth\":100")
    );
    assert!(
        lines[1].contains("\"latency_smooth\":150.0")
            || lines[1].contains("\"latency_smooth\":150")
    );
    assert!(
        lines[2].contains("\"latency_smooth\":100.0")
            || lines[2].contains("\"latency_smooth\":100")
    );
}
