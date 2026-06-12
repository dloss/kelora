mod common;
use common::*;

#[test]
fn test_error_handling_resilient_mode() {
    let input = r#"{"level": "INFO", "status": 200}
invalid json line
{"level": "ERROR", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json"], input);
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered: valid events were emitted, so the run succeeds (use --strict to fail on any parse error)"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should skip invalid line and output 2 valid lines"
    );
}

#[test]
fn test_error_handling_resilient_with_summary() {
    let input = r#"{"level": "INFO", "status": 200}
invalid json line
{"level": "ERROR", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered: valid events were emitted, so the run succeeds (use --strict to fail on any parse error)"
    );

    let lines: Vec<&str> = stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output 2 valid lines, skipping invalid line"
    );

    // In resilient mode, invalid lines are skipped, not emitted as events
    // Check that both valid lines are properly formatted JSON
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|_| {
            panic!("All output lines should be valid JSON, but got: '{}'", line)
        });
    }

    // In resilient mode, parsing errors are handled silently by skipping invalid lines
    // This behavior may or may not produce stderr output depending on implementation details
}

#[test]
fn test_error_handling_resilient_mixed_input() {
    let input = r#"{"valid": "json", "status": 200}
{malformed json line}
{"another": "valid", "status": 404}
not json at all
{"final": "entry", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered: valid events were emitted, so the run succeeds (use --strict to fail on any parse error)"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 valid JSON lines, skipping malformed ones"
    );

    // Verify all output lines are valid JSON
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line)
            .expect("All output lines should be valid JSON");
    }
}

#[test]
fn test_error_handling_strict_mode() {
    let input = r#"{"level": "INFO", "status": 200}
invalid json line
{"level": "ERROR", "status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--strict"], input);
    assert_ne!(
        exit_code, 0,
        "kelora should exit with error code in strict mode when encountering invalid input"
    );

    // Should only output the first valid line before failing
    let lines: Vec<&str> = stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty())
        .collect();
    assert!(
        lines.len() <= 1,
        "Should output at most one line before failing in strict mode"
    );
}

#[test]
fn test_quiet_levels_with_errors() {
    // Test that silent/quiet still preserve exit codes for errors
    let input = r#"{"level": "info", "message": "test"}"#;

    // Test with a filter that would cause an error
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.nonexistent.field == true",
            "--strict",
            "--silent",
        ],
        input,
    );

    // Should have non-zero exit code due to error
    assert_ne!(exit_code, 0);

    // Should have no output in silent mode
    assert_eq!(stdout.trim(), "");

    // In strict mode with --silent, even error messages should be suppressed
    // but exit code should still indicate failure
}

#[test]
fn test_error_stats_sequential_mode() {
    let input = r#"{"valid": "json", "status": 200}
{malformed json line}
{"another": "valid", "status": 404}
not json at all
{"final": "entry", "status": 500}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--with-stats"], input);
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered (3 of 5 lines parsed); the run succeeds with valid output"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should emit only the valid JSON lines");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 5 total, 0 filtered (0.0%), 2 errors (40.0%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 3 total, 3 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_error_stats_parallel_mode() {
    let input = r#"{"valid": "json", "status": 200}
{malformed json line}
{"another": "valid", "status": 404}
not json at all
{"final": "entry", "status": 500}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--with-stats",
            "--parallel",
            "--batch-size",
            "2",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "parallel mode recovers partial parse failures (3 of 5 lines parsed) and succeeds, like sequential mode"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should emit only the valid JSON lines");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 5 total, 0 filtered (0.0%), 2 errors (40.0%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 3 total, 3 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_error_stats_with_filter_expression() {
    let input = r#"{"valid": "json", "status": 200}
{malformed json line}
{"another": "valid", "status": 404}
not json at all
{"final": "entry", "status": 500}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--filter", "e.status >= 400", "--with-stats"],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered; parse errors are reported but do not fail the run"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should emit only events with status >= 400");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 5 total, 0 filtered (0.0%), 2 errors (40.0%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 3 total, 2 output, 1 filtered (33.3%)"
    );
}

#[test]
fn test_partial_exec_errors_are_reported_but_recovered() {
    // One event errors (string / int), one succeeds (10 / 5). A runtime error on
    // *some* events is recovered in default mode: it is reported but does not fail
    // the run, because the exec stage still succeeded at least once.
    let input = "{\"level\": \"INFO\"}\n{\"level\": 10}";

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "e.level / 5"], input);

    assert_eq!(
        exit_code, 0,
        "partial resilient runtime exec errors are recovered and must not affect the exit code"
    );
    assert!(
        stderr.contains("Exec errors:") || stderr.contains("Mixed errors:"),
        "stderr should include a runtime error summary: {}",
        stderr
    );
}

#[test]
fn test_exec_errors_on_every_event_fail_the_run() {
    // The exec errors on the only event, so the stage never once succeeded. That
    // is a deterministic operator error (a broken transform), not data noise, so
    // it fails the run with exit 1 even in default resilient mode (issue #241).
    let input = r#"{"level": "INFO"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exec", "e.level / 5"], input);

    assert_eq!(
        exit_code, 1,
        "an exec that errors on every event never succeeded, so the run fails"
    );
    assert!(
        stderr.contains("Exec errors:") || stderr.contains("Mixed errors:"),
        "stderr should still include a runtime error summary: {}",
        stderr
    );
}

#[test]
fn test_filter_errors_on_every_event_fail_the_run() {
    // A filter typo (`status` instead of `e.status`) errors on every event, so the
    // filter never once succeeded. This is the core #241 case: a totally broken
    // filter must not masquerade as success.
    let input = "{\"status\": 200}\n{\"status\": 500}";

    let (_stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", "status >= 500", "-q"], input);

    assert_eq!(
        exit_code, 1,
        "a filter that errors on every event never matched anything and must fail the run"
    );
}

#[test]
fn test_broken_exec_behind_selective_filter_fails() {
    // The filter legitimately drops half the events; the exec then errors on every
    // event it actually sees. A global error/event ratio would miss this (2 errors
    // < 4 events), but per-kind success tracking catches that the exec never once
    // succeeded.
    let input = "{\"level\": \"ERROR\"}\n{\"level\": \"INFO\"}\n{\"level\": \"ERROR\"}\n{\"level\": \"INFO\"}";

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"ERROR\"",
            "--exec",
            "e.x = e.missing + 1",
            "-q",
        ],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "a broken exec behind a selective filter must fail even though most events were filtered out"
    );
}

#[test]
fn test_broken_filter_fails_under_no_diagnostics() {
    // The exit-code signal lives in the always-on tracker, so a fully broken
    // filter fails the run even when stats collection is off (--no-diagnostics).
    let input = "{\"status\": 200}\n{\"status\": 500}";

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "status >= 500",
            "-q",
            "--no-diagnostics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "exit code must not depend on diagnostics: a broken filter fails under --no-diagnostics too"
    );
}

#[test]
fn test_exec_type_errors_fail_in_strict_mode() {
    let input = r#"{"level": "INFO"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--strict", "--exec", "e.level / 5"], input);

    assert_ne!(
        exit_code, 0,
        "strict runtime exec errors should affect the exit code"
    );
    assert!(
        stderr.contains("Pipeline error") || stderr.contains("exec error"),
        "stderr should include a strict exec failure: {}",
        stderr
    );
}

#[test]
fn test_error_stats_with_ignore_lines() {
    let input = r#"# This is a comment
{"valid": "json", "status": 200}
{malformed json line}
# Another comment
{"another": "valid", "status": 404}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--ignore-lines", "^#", "--with-stats"],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "a single recovered parse error among valid lines does not fail the run"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should emit the two valid JSON lines");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 5 total, 2 filtered (40.0%), 1 errors (20.0%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 2 total, 2 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_error_stats_no_errors() {
    let input = r#"{"valid": "json", "status": 200}
{"another": "valid", "status": 404}
{"final": "entry", "status": 500}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--filter", "e.status >= 400", "--with-stats"],
        input,
    );
    assert_eq!(exit_code, 0, "All-valid input should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should emit only the two matching events");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 3 total, 0 filtered (0.0%), 0 errors (0.0%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 3 total, 2 output, 1 filtered (33.3%)"
    );
}

#[test]
fn test_error_stats_parallel_vs_sequential_consistency() {
    let input = r#"{"valid": "json", "status": 200}
{malformed json line}
{"another": "valid", "status": 404}
not json at all
{"final": "entry", "status": 500}
invalid json again"#;

    let (stdout_seq, stderr_seq, exit_code_seq) = run_kelora_with_input(
        &["-f", "json", "--filter", "e.status >= 400", "--with-stats"],
        input,
    );
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.status >= 400",
            "--with-stats",
            "--parallel",
            "--batch-size",
            "2",
        ],
        input,
    );

    assert_eq!(
        exit_code_seq, 1,
        "Sequential mode should propagate parse errors via exit status"
    );
    assert_eq!(
        exit_code_par, 1,
        "Parallel mode should report errors in resilient mode"
    );

    let seq_lines: Vec<&str> = stdout_seq.trim().lines().collect();
    let par_lines: Vec<&str> = stdout_par.trim().lines().collect();
    assert_eq!(
        seq_lines.len(),
        par_lines.len(),
        "Sequential and parallel runs should emit the same number of events"
    );

    let stats_seq = extract_stats_lines(&stderr_seq);
    let stats_par = extract_stats_lines(&stderr_par);
    assert_eq!(
        stats_line(&stats_seq, "Lines processed:"),
        stats_line(&stats_par, "Lines processed:")
    );
    assert_eq!(
        stats_line(&stats_seq, "Events created:"),
        stats_line(&stats_par, "Events created:")
    );
    assert_eq!(
        stats_line(&stats_seq, "Lines processed:"),
        "Lines processed: 6 total, 0 filtered (0.0%), 3 errors (50.0%)"
    );
    assert_eq!(
        stats_line(&stats_seq, "Events created:"),
        "Events created: 3 total, 2 output, 1 filtered (33.3%)"
    );
}

#[test]
fn test_error_stats_multiline_mode() {
    let input = r#"{"valid": "json", "message": "line1\nline2"}
{malformed json line}
{"another": "valid", "message": "single line"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--with-stats"], input);
    assert_eq!(
        exit_code, 0,
        "partial parse failures are recovered (2 of 3 events parsed); the run succeeds"
    );
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should emit the two valid JSON events");

    let stats = extract_stats_lines(&stderr);
    assert_eq!(
        stats_line(&stats, "Lines processed:"),
        "Lines processed: 3 total, 0 filtered (0.0%), 1 errors (33.3%)"
    );
    assert_eq!(
        stats_line(&stats, "Events created:"),
        "Events created: 2 total, 2 output, 0 filtered (0.0%)"
    );

    let (_stdout_multi, stderr_multi, exit_code_multi) = run_kelora_with_input(
        &["-f", "json", "--multiline", "indent", "--with-stats"],
        input,
    );
    assert_eq!(
        exit_code_multi, 0,
        "multiline mode recovers partial parse failures and succeeds, like the line path"
    );
    let stats_multi = extract_stats_lines(&stderr_multi);
    assert_eq!(
        stats_line(&stats_multi, "Lines processed:"),
        "Lines processed: 3 total, 0 filtered (0.0%), 1 errors (33.3%)"
    );
    assert_eq!(
        stats_line(&stats_multi, "Events created:"),
        "Events created: 2 total, 2 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_assert_failure_fails_under_no_diagnostics() {
    // An --assert violation is a structural/explicit gate: it must fail the run
    // in every mode. --no-diagnostics turns off stats collection, so the failure
    // count must be tracked independently of it.
    let input = r#"{"status": 200}"#;

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--assert",
            "e.status >= 500",
            "-q",
            "--no-diagnostics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "an --assert violation must fail the run even under --no-diagnostics"
    );
}

#[test]
fn test_missing_input_file_fails_under_no_diagnostics() {
    // A named input that can't be opened is a structural failure, not data noise.
    let (_stdout, _stderr, exit_code) = run_kelora(&[
        "-f",
        "json",
        "/nonexistent/kelora_missing_input.json",
        "-q",
        "--no-diagnostics",
    ]);

    assert_eq!(
        exit_code, 1,
        "a file that cannot be opened must fail the run even under --no-diagnostics"
    );
}

#[test]
fn test_missing_input_file_fails_in_parallel() {
    // Regression: parallel get_final_stats did not read the process-wide
    // file-failure atomic, so a missing input silently exited 0 in --parallel.
    let (_stdout, _stderr, exit_code) = run_kelora(&[
        "-f",
        "json",
        "--parallel",
        "/nonexistent/kelora_missing_input.json",
        "-q",
    ]);

    assert_eq!(
        exit_code, 1,
        "a file that cannot be opened must fail the run in parallel mode too"
    );
}
