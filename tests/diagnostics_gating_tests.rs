// Integration tests for the three-tier diagnostic model: errors (⚠️),
// warnings (🔸), and hints (💡), and the flags / env vars that gate them.
//
// Gating contract:
//   - hints  → hidden by --no-hints / --no-diagnostics / KELORA_NO_HINTS / --silent
//   - warns  → hidden by --no-warnings / --no-diagnostics / KELORA_NO_WARNINGS / --silent
//   - errors → hidden only by --silent
//   - explicit --hints / --warnings / --diagnostics override an env/config default
//   - data-only modes (--metrics) hush hints but still surface warnings to stderr

use std::io::Write;
use std::process::{Command, Stdio};

fn run_env(args: &[&str], input: &str, envs: &[(&str, &str)]) -> (String, String, i32) {
    let binary_path = env!("CARGO_BIN_EXE_kelora");
    let mut cmd = Command::new(binary_path)
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null")
        // Clear any inherited toggles so the test environment is deterministic.
        .env_remove("KELORA_NO_HINTS")
        .env_remove("KELORA_NO_WARNINGS")
        .env_remove("KELORA_NO_TIPS")
        .envs(envs.iter().copied())
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

fn run(args: &[&str], input: &str) -> (String, String, i32) {
    run_env(args, input, &[])
}

// A filter that references a field that never appears triggers a zero-result
// HINT. The "no input format" fallback also emits a hint, so pin the format.
const HINT_ARGS: &[&str] = &["-f", "logfmt", "--no-emoji", "--filter", "e.nope == \"x\""];
// --span + --parallel is a configuration conflict that emits a WARNING.
const WARN_ARGS: &[&str] = &["-f", "logfmt", "--no-emoji", "--span", "5", "--parallel"];
const INPUT: &str = "a=1\nb=2\n";

fn has_hint(stderr: &str) -> bool {
    stderr.contains("kelora hint:")
}
fn has_warning(stderr: &str) -> bool {
    stderr.contains("kelora warning:")
}

#[test]
fn hint_shown_by_default() {
    let (_o, stderr, code) = run(HINT_ARGS, INPUT);
    assert_eq!(code, 0);
    assert!(has_hint(&stderr), "hint should show by default: {stderr}");
}

#[test]
fn warning_shown_by_default() {
    let (_o, stderr, code) = run(WARN_ARGS, INPUT);
    assert_eq!(code, 0);
    assert!(
        has_warning(&stderr),
        "warning should show by default: {stderr}"
    );
}

#[test]
fn no_hints_suppresses_hint_but_keeps_warning() {
    let mut args = HINT_ARGS.to_vec();
    args.push("--no-hints");
    let (_o, stderr, _c) = run(&args, INPUT);
    assert!(
        !has_hint(&stderr),
        "--no-hints should hide the hint: {stderr}"
    );

    let mut wargs = WARN_ARGS.to_vec();
    wargs.push("--no-hints");
    let (_o, wstderr, _c) = run(&wargs, INPUT);
    assert!(
        has_warning(&wstderr),
        "--no-hints must NOT hide warnings: {wstderr}"
    );
}

#[test]
fn no_warnings_suppresses_warning_but_keeps_hint() {
    let mut args = WARN_ARGS.to_vec();
    args.push("--no-warnings");
    let (_o, stderr, _c) = run(&args, INPUT);
    assert!(
        !has_warning(&stderr),
        "--no-warnings should hide the warning: {stderr}"
    );

    let mut hargs = HINT_ARGS.to_vec();
    hargs.push("--no-warnings");
    let (_o, hstderr, _c) = run(&hargs, INPUT);
    assert!(
        has_hint(&hstderr),
        "--no-warnings must NOT hide hints: {hstderr}"
    );
}

#[test]
fn no_diagnostics_suppresses_both() {
    let mut hargs = HINT_ARGS.to_vec();
    hargs.push("--no-diagnostics");
    let (_o, hstderr, _c) = run(&hargs, INPUT);
    assert!(
        !has_hint(&hstderr),
        "--no-diagnostics hides hints: {hstderr}"
    );

    let mut wargs = WARN_ARGS.to_vec();
    wargs.push("--no-diagnostics");
    let (_o, wstderr, _c) = run(&wargs, INPUT);
    assert!(
        !has_warning(&wstderr),
        "--no-diagnostics hides warnings: {wstderr}"
    );
}

#[test]
fn env_no_hints_suppresses_hint() {
    let (_o, stderr, _c) = run_env(HINT_ARGS, INPUT, &[("KELORA_NO_HINTS", "1")]);
    assert!(
        !has_hint(&stderr),
        "KELORA_NO_HINTS should hide the hint: {stderr}"
    );
}

#[test]
fn env_no_warnings_suppresses_warning() {
    let (_o, stderr, _c) = run_env(WARN_ARGS, INPUT, &[("KELORA_NO_WARNINGS", "1")]);
    assert!(
        !has_warning(&stderr),
        "KELORA_NO_WARNINGS should hide the warning: {stderr}"
    );
}

#[test]
fn explicit_hints_flag_overrides_env() {
    let mut args = HINT_ARGS.to_vec();
    args.push("--hints");
    let (_o, stderr, _c) = run_env(&args, INPUT, &[("KELORA_NO_HINTS", "1")]);
    assert!(
        has_hint(&stderr),
        "explicit --hints must override KELORA_NO_HINTS: {stderr}"
    );
}

#[test]
fn explicit_warnings_flag_overrides_env() {
    let mut args = WARN_ARGS.to_vec();
    args.push("--warnings");
    let (_o, stderr, _c) = run_env(&args, INPUT, &[("KELORA_NO_WARNINGS", "1")]);
    assert!(
        has_warning(&stderr),
        "explicit --warnings must override KELORA_NO_WARNINGS: {stderr}"
    );
}

#[test]
fn removed_kelora_no_tips_no_longer_suppresses() {
    // KELORA_NO_TIPS was removed in v2.0; it must be inert now.
    let (_o, stderr, _c) = run_env(HINT_ARGS, INPUT, &[("KELORA_NO_TIPS", "1")]);
    assert!(
        has_hint(&stderr),
        "KELORA_NO_TIPS is removed and must not gate hints: {stderr}"
    );
}

#[test]
fn silent_suppresses_everything() {
    let mut hargs = HINT_ARGS.to_vec();
    hargs.push("--silent");
    let (_o, hstderr, _c) = run(&hargs, INPUT);
    assert!(!has_hint(&hstderr), "--silent hides hints: {hstderr}");

    let mut wargs = WARN_ARGS.to_vec();
    wargs.push("--silent");
    let (_o, wstderr, _c) = run(&wargs, INPUT);
    assert!(!has_warning(&wstderr), "--silent hides warnings: {wstderr}");
}
