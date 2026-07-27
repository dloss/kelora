// Integration tests for the three-tier diagnostic model: errors (⚠️),
// warnings (🔸), and hints (💡), and the flags / env vars that gate them.
//
// Gating contract:
//   - hints  → hidden by --no-hints / --no-diagnostics / KELORA_NO_HINTS / --silent
//   - warns  → hidden by --no-warnings / --no-diagnostics / KELORA_NO_WARNINGS / --silent
//   - errors → hidden only by --silent
//   - explicit --hints / --warnings / --diagnostics override an env/config default
//   - data-only modes (--metrics) hush hints but still surface warnings to stderr
//   - exception: --discover does not hush the format-fallback hint (#343)

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

// --- Detection-notice gating: warnings/hints follow the tier flags with NO
// terminal gate (so they reach redirected/CI stderr), while the auto-detect
// STATUS notice is silent on success and surfaces only under -v. The test
// harness pipes stderr, i.e. stderr is NOT a TTY — exactly the redirected case. ---

// First line is valid JSON (locks auto-detect to json), then enough garbage to
// trip the parse-failure WARNING threshold (>=10 errors).
fn parse_failure_input() -> String {
    let mut s = String::from("{\"a\":1}\n");
    for i in 0..12 {
        s.push_str(&format!("garbage line {i}\n"));
    }
    s
}
const PF_ARGS: &[&str] = &["-f", "auto", "--no-emoji"];
// Ambiguous plain text auto-detects to nothing and falls back to `line`,
// emitting the format-fallback HINT.
const FALLBACK_ARGS: &[&str] = &["--no-emoji"];
const FALLBACK_INPUT: &str = "just some plain text\nmore plain text\n";

#[test]
fn parse_failure_warning_surfaces_with_redirected_stderr() {
    // The whole point: a "parsing mostly failed" warning must reach a stuck user
    // even when stderr is captured (CI / 2>file) — no terminal gate.
    let (_o, stderr, _c) = run(PF_ARGS, &parse_failure_input());
    assert!(
        has_warning(&stderr),
        "parse-failure warning must surface on redirected stderr: {stderr}"
    );
}

#[test]
fn parse_failure_warning_obeys_only_the_warning_tier() {
    let input = parse_failure_input();

    let mut nw = PF_ARGS.to_vec();
    nw.push("--no-warnings");
    let (_o, s, _c) = run(&nw, &input);
    assert!(!has_warning(&s), "--no-warnings hides parse-failure: {s}");

    // --no-hints must NOT touch a warning.
    let mut nh = PF_ARGS.to_vec();
    nh.push("--no-hints");
    let (_o, s2, _c) = run(&nh, &input);
    assert!(
        has_warning(&s2),
        "--no-hints must not hide the parse-failure warning: {s2}"
    );
}

#[test]
fn fallback_hint_surfaces_piped_and_obeys_only_the_hint_tier() {
    let (_o, s, _c) = run(FALLBACK_ARGS, FALLBACK_INPUT);
    assert!(
        has_hint(&s),
        "format-fallback hint must surface on redirected stderr: {s}"
    );

    let mut nh = FALLBACK_ARGS.to_vec();
    nh.push("--no-hints");
    let (_o, s2, _c) = run(&nh, FALLBACK_INPUT);
    assert!(!has_hint(&s2), "--no-hints hides the fallback hint: {s2}");

    let mut nw = FALLBACK_ARGS.to_vec();
    nw.push("--no-warnings");
    let (_o, s3, _c) = run(&nw, FALLBACK_INPUT);
    assert!(
        has_hint(&s3),
        "--no-warnings must not hide the fallback hint: {s3}"
    );
}

// --- #343: --discover is the one data-only mode that must NOT hush the
// format-fallback hint. `-h` advertises `-d` as the first command to run on an
// unknown file, and the hint is the actionable half of the footer's
// `format: line (auto-detected)` — withholding it there sends the user to
// --help-formats to rediscover the `cols:` syntax the hint already names. ---

#[test]
fn fallback_hint_survives_the_discover_hush() {
    let mut d = FALLBACK_ARGS.to_vec();
    d.push("-d");
    let (_o, s, _c) = run(&d, FALLBACK_INPUT);
    assert!(
        has_hint(&s),
        "--discover must not hush the format-fallback hint: {s}"
    );

    // An *explicit* suppression still wins over the exemption.
    for flag in ["--no-hints", "--no-diagnostics", "--silent"] {
        let mut args = d.clone();
        args.push(flag);
        let (_o, s2, _c) = run(&args, FALLBACK_INPUT);
        assert!(!has_hint(&s2), "{flag} must still hide the hint: {s2}");
    }

    let (_o, s3, _c) = run_env(&d, FALLBACK_INPUT, &[("KELORA_NO_HINTS", "1")]);
    assert!(!has_hint(&s3), "KELORA_NO_HINTS must still apply: {s3}");
}

#[test]
fn fallback_hint_stays_hushed_in_the_other_data_only_modes() {
    // The exemption is scoped to --discover on purpose: under -m you already
    // know parsing worked, so the advice is noise. --hints still re-enables it.
    let mut m = FALLBACK_ARGS.to_vec();
    m.push("-m");
    let (_o, s, _c) = run(&m, FALLBACK_INPUT);
    assert!(!has_hint(&s), "-m must still hush the fallback hint: {s}");

    let mut mh = m.clone();
    mh.push("--hints");
    let (_o, s2, _c) = run(&mh, FALLBACK_INPUT);
    assert!(has_hint(&s2), "--hints re-enables it under -m: {s2}");
}

#[test]
fn discover_keeps_the_hint_off_the_stdout_data_channel() {
    // The hint is stderr-only, so `-d=json` stays machine-readable.
    let (stdout, stderr, _c) = run(&["--no-emoji", "-d=json"], FALLBACK_INPUT);
    assert!(has_hint(&stderr), "hint belongs on stderr: {stderr}");
    assert!(
        !stdout.contains("kelora hint:"),
        "hint must not pollute discover stdout: {stdout}"
    );
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .expect("discover json output must stay parseable");
}

#[test]
fn fallback_hint_is_emitted_at_most_once_per_run() {
    // auto-per-file detects per file, so N unstructured files used to print N
    // copies of the same paragraph. The advice names no file — once is enough.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let paths: Vec<String> = (0..3)
        .map(|i| {
            let p = dir.path().join(format!("plain{i}.log"));
            std::fs::write(&p, FALLBACK_INPUT).expect("write input");
            p.to_string_lossy().into_owned()
        })
        .collect();

    let mut args = vec!["--no-emoji", "-f", "auto-per-file", "-d"];
    args.extend(paths.iter().map(|p| p.as_str()));
    let (_o, stderr, _c) = run(&args, "");

    let count = stderr.matches("kelora hint:").count();
    assert_eq!(
        count, 1,
        "expected exactly one fallback hint, got: {stderr}"
    );
}

#[test]
fn autodetect_info_is_verbose_only() {
    // The 🔹 "Auto-detected format" status line is "what kelora did": a confident
    // detection is unsurprising, so a normal run stays silent (Rule of Silence)...
    let (_o, s, _c) = run(&["-f", "auto", "--no-emoji"], "{\"a\":1}\n");
    assert!(
        !s.contains("Auto-detected format"),
        "auto-detect status must be silent without -v: {s}"
    );
    // ...and --diagnostics, which forces the advisory tiers on, does NOT pull in
    // status — it's not a diagnostic.
    let (_o, s2, _c) = run(
        &["-f", "auto", "--no-emoji", "--diagnostics"],
        "{\"a\":1}\n",
    );
    assert!(
        !s2.contains("Auto-detected format"),
        "status is not a diagnostic; --diagnostics must not show it: {s2}"
    );
    // It surfaces only under -v/--verbose — and then even on redirected stderr,
    // because -v is an explicit "show me what you did".
    let (_o, s3, _c) = run(&["-f", "auto", "--no-emoji", "-v"], "{\"a\":1}\n");
    assert!(
        s3.contains("Auto-detected format: json"),
        "-v must surface the auto-detect status: {s3}"
    );
}
