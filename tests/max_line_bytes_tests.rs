//! Tests for the `--max-line-bytes` per-line memory circuit breaker.
//!
//! A newline-free stream (e.g. a tiny compressed payload that decompresses into
//! one enormous line) must not grow the read buffer without bound. By default an
//! over-limit line is truncated to the cap with a warning (exit 0); `--strict`
//! turns it into a hard error (exit 1); `0` disables the cap entirely.

use std::io::Write;
use std::process::{Command, Stdio};

fn run_kelora_bytes(args: &[&str], input: &[u8]) -> (Vec<u8>, String, i32) {
    let binary_path = env!("CARGO_BIN_EXE_kelora");
    let mut cmd = Command::new(binary_path)
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    cmd.stdin
        .as_mut()
        .expect("stdin")
        .write_all(input)
        .expect("Failed to write stdin");

    let output = cmd.wait_with_output().expect("Failed to read output");
    (
        output.stdout,
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn oversized_line_is_truncated_and_warned_by_default() {
    // One 100 KB newline-free line, then a normal line, capped at 1 KiB.
    let mut input = vec![b'x'; 100_000];
    input.push(b'\n');
    input.extend_from_slice(b"after\n");

    let (stdout, stderr, exit_code) = run_kelora_bytes(&["-f", "line", "--max-line-bytes", "1KiB"], &input);

    assert_eq!(exit_code, 0, "truncation is a recovery, exit stays 0: {stderr}");
    // The first emitted line carries at most the cap (1024) worth of payload.
    let first = String::from_utf8_lossy(&stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let x_count = first.bytes().filter(|&b| b == b'x').count();
    assert_eq!(x_count, 1024, "payload must be clipped to the cap: got {x_count}");
    // The following line must survive — the stream resumes after the discarded tail.
    assert!(
        String::from_utf8_lossy(&stdout).contains("after"),
        "line after the oversized one must survive"
    );
    assert!(
        stderr.contains("max-line-bytes") && stderr.contains("truncated"),
        "a truncation warning should be emitted: {stderr}"
    );
}

#[test]
fn oversized_line_is_fatal_under_strict() {
    let mut input = vec![b'x'; 100_000];
    input.push(b'\n');

    let (_stdout, stderr, exit_code) =
        run_kelora_bytes(&["-f", "line", "--max-line-bytes", "1KiB", "--strict"], &input);

    assert_eq!(exit_code, 1, "over-limit line aborts under --strict: {stderr}");
    assert!(
        stderr.contains("max-line-bytes"),
        "error should name the limit: {stderr}"
    );
}

#[test]
fn cap_of_zero_disables_the_limit() {
    let mut input = vec![b'x'; 100_000];
    input.push(b'\n');

    let (stdout, stderr, exit_code) =
        run_kelora_bytes(&["-f", "line", "--max-line-bytes", "0"], &input);

    assert_eq!(exit_code, 0);
    let x_count = stdout.iter().filter(|&&b| b == b'x').count();
    assert_eq!(x_count, 100_000, "no truncation when disabled");
    assert!(
        !stderr.contains("truncated"),
        "no warning when the cap is disabled: {stderr}"
    );
}

#[test]
fn normal_lines_under_the_default_are_untouched() {
    let input = b"hello\nworld\n";
    let (stdout, stderr, exit_code) = run_kelora_bytes(&["-f", "line"], input);

    assert_eq!(exit_code, 0);
    let out = String::from_utf8_lossy(&stdout);
    assert!(out.contains("hello") && out.contains("world"));
    assert!(
        !stderr.contains("truncated"),
        "default cap must not trip on normal input: {stderr}"
    );
}

#[test]
fn invalid_size_value_is_rejected() {
    let (_stdout, stderr, exit_code) =
        run_kelora_bytes(&["-f", "line", "--max-line-bytes", "banana"], b"x\n");

    assert_eq!(exit_code, 2, "bad size value is a usage error: {stderr}");
    assert!(
        stderr.contains("max-line-bytes"),
        "error should name the option: {stderr}"
    );
}
