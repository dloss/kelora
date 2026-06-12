//! Regression tests for issue #239: a single non-UTF-8 byte must not abort the
//! stream and silently truncate everything after it. By default kelora now
//! decodes input losslessly (U+FFFD substitution, like grep) and reports a
//! diagnostic; `--strict-utf8` restores the historical abort behavior.

use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

/// Run kelora with raw byte input on stdin (input may be invalid UTF-8).
fn run_kelora_bytes(args: &[&str], input: &[u8]) -> (String, String, i32) {
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
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn lossy_decode_does_not_truncate_stream_default() {
    // The exact reproduction from the issue: a bad byte in the middle line.
    let input = b"{\"ok\":1}\n{\"a\":\"\xff bad\"}\n{\"ok\":2}\n";
    let (stdout, _stderr, exit_code) = run_kelora_bytes(&["-f", "json"], input);

    assert_eq!(exit_code, 0, "lossy decoding must not fail the run");
    assert!(stdout.contains("ok=1"), "line before bad byte: {stdout}");
    assert!(
        stdout.contains("ok=2"),
        "line AFTER bad byte must survive (no silent truncation): {stdout}"
    );
    assert!(
        stdout.contains('\u{fffd}'),
        "invalid byte should be replaced with U+FFFD: {stdout}"
    );
}

#[test]
fn lossy_decode_emits_diagnostic_by_default() {
    let input = b"a\n\xffb\nc\n";
    let (stdout, stderr, exit_code) = run_kelora_bytes(&["-f", "raw"], input);

    assert_eq!(exit_code, 0);
    // All three lines pass through in raw mode.
    assert!(stdout.contains("a") && stdout.contains("b") && stdout.contains("c"));
    assert!(
        stderr.contains("invalid UTF-8"),
        "a decode diagnostic should be surfaced: {stderr}"
    );
}

#[test]
fn strict_utf8_restores_abort_behavior() {
    let input = b"{\"ok\":1}\n{\"a\":\"\xff bad\"}\n{\"ok\":2}\n";
    let (stdout, stderr, exit_code) = run_kelora_bytes(&["-f", "json", "--strict-utf8"], input);

    assert_eq!(exit_code, 1, "--strict-utf8 should fail on invalid UTF-8");
    assert!(
        stderr.contains("valid UTF-8"),
        "should report the UTF-8 error: {stderr}"
    );
    // Pre-bad-byte output is still flushed, but the trailing valid line is not.
    assert!(stdout.contains("ok=1"));
    assert!(
        !stdout.contains("ok=2"),
        "strict mode aborts before the trailing line: {stdout}"
    );
}

#[test]
fn lossy_decode_works_for_file_input() {
    let mut file = NamedTempFile::new().expect("temp file");
    file.write_all(b"first\n\xffsecond\nthird\n")
        .expect("write");
    let path = file.path().to_str().unwrap();

    let binary_path = env!("CARGO_BIN_EXE_kelora");
    let output = Command::new(binary_path)
        .args(["-f", "raw", path])
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .output()
        .expect("run kelora");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("first"));
    assert!(
        stdout.contains("third"),
        "line after the bad byte must survive for file input: {stdout}"
    );
}
