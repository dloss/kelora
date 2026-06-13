// End-to-end regression tests for the usability/bug fixes:
//   1. CSV reader reassembles RFC 4180 quoted fields with embedded newlines
//      (sequential) instead of silently splitting them into corrupt rows.
//   2. --strict reports whole records, not a misleading ragged-row error.
//   3. -k/--keys does not falsely warn about fields created in --exec.
//   4. -l/--levels hints and --stats no longer advertise non-level field values
//      (e.g. a `severity` of "high") as matchable levels.

mod common;
use common::*;

const RFC4180_MULTILINE: &str = "name,note\n\"alice\",\"hello\nworld\"\n\"bob\",\"ok\"\n";

#[test]
fn csv_quoted_embedded_newline_is_one_record_sequential() {
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "csv", "-F", "json"], RFC4180_MULTILINE);
    assert_eq!(exit_code, 0, "stdout: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 records, got: {stdout}");
    assert_eq!(lines[0], r#"{"name":"alice","note":"hello\nworld"}"#);
    assert_eq!(lines[1], r#"{"name":"bob","note":"ok"}"#);
}

#[test]
fn csv_multiline_round_trips_through_csv_output() {
    // JSON with an embedded newline -> CSV -> back to JSON must be lossless.
    let input = "{\"a\":\"x\",\"b\":\"line1\\nline2\"}\n";
    let (csv, _stderr, code) = run_kelora_with_input(&["-j", "-F", "csv", "-k", "a,b"], input);
    assert_eq!(code, 0);

    let (json, _stderr, code) = run_kelora_with_input(&["-f", "csv", "-F", "json"], &csv);
    assert_eq!(code, 0);
    assert_eq!(
        json.lines().count(),
        1,
        "round-trip changed record count: {json}"
    );
    assert_eq!(json.trim(), r#"{"a":"x","b":"line1\nline2"}"#);
}

#[test]
fn csv_multiline_strict_does_not_misreport_ragged_rows() {
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "csv", "-F", "json", "--strict"], RFC4180_MULTILINE);
    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert_eq!(stdout.lines().count(), 2);
    assert!(
        !stderr.contains("expected 2"),
        "strict should not raise a ragged-row error here: {stderr}"
    );
}

#[test]
fn csv_multiline_parallel_errors_clearly_instead_of_corrupting() {
    // Parallel reads line-by-line; the parser's completeness guard turns what was
    // silent corruption into a clear, counted parse error pointing at sequential
    // mode. With --strict it is fatal.
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "csv", "-F", "json", "-P", "--strict"],
        RFC4180_MULTILINE,
    );
    assert_ne!(
        exit_code, 0,
        "parallel+strict should fail, stderr: {stderr}"
    );
    assert!(
        stderr.contains("Unterminated quoted field"),
        "expected a clear cause, got: {stderr}"
    );
}

#[test]
fn keys_hint_not_raised_for_exec_created_field() {
    let input = "{\"level\":\"INFO\"}\n{\"level\":\"WARN\"}\n";
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-j", "--exec", "e.tag = e.level + \"!\"", "-k", "tag"],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(stdout.contains("tag='INFO!'"), "stdout: {stdout}");
    assert!(
        !stderr.contains("never present"),
        "field created in --exec must not be flagged as missing: {stderr}"
    );
}

#[test]
fn keys_hint_still_raised_for_genuine_typo() {
    // The hint must still fire for a name that exists nowhere (input or exec).
    let input = "{\"level\":\"INFO\"}\n";
    let (_stdout, stderr, _exit_code) = run_kelora_with_input(&["-j", "-k", "nope"], input);
    assert!(
        stderr.contains("never present"),
        "real typo should still be flagged: {stderr}"
    );
}

#[test]
fn level_stats_do_not_list_non_level_field_values() {
    // `severity:"high"` must not be reported as a level: the level filter only
    // consults the first present level field (here `level`), so advertising
    // "high" would be a value -l can never match.
    let input = "{\"level\":\"WARN\",\"severity\":\"low\"}\n\
                 {\"level\":\"WARN\",\"severity\":\"high\"}\n";
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-j", "-s"], input);
    assert_eq!(exit_code, 0);
    let levels_line = stdout
        .lines()
        .find(|l| l.contains("Levels seen:"))
        .unwrap_or_default();
    assert!(
        levels_line.contains("WARN"),
        "expected WARN in: {levels_line}"
    );
    assert!(
        !levels_line.contains("high") && !levels_line.contains("low"),
        "severity values must not appear as levels: {levels_line}"
    );
}
