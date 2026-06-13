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
fn csv_multiline_parallel_matches_sequential() {
    // Parallel reassembles embedded-newline records too: the batcher keeps whole
    // records inside a batch and the worker's CsvChunker stitches the physical
    // lines back together, so output matches sequential mode exactly.
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "csv", "-F", "json", "-P"], RFC4180_MULTILINE);
    assert_eq!(exit_code, 0, "stdout: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 records, got: {stdout}");
    assert_eq!(lines[0], r#"{"name":"alice","note":"hello\nworld"}"#);
    assert_eq!(lines[1], r#"{"name":"bob","note":"ok"}"#);
}

#[test]
fn csv_multiline_parallel_strict_does_not_corrupt() {
    // The well-formed multi-line record must not trip the completeness guard
    // under parallel+strict now that records are reassembled before parsing.
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "csv", "-F", "json", "-P", "--strict"],
        RFC4180_MULTILINE,
    );
    assert_eq!(exit_code, 0, "parallel+strict stderr: {stderr}");
    assert_eq!(stdout.lines().count(), 2, "stdout: {stdout}");
    assert!(
        !stderr.contains("Unterminated quoted field"),
        "well-formed record must not be reported as unterminated: {stderr}"
    );
}

#[test]
fn csv_multiline_parallel_field_spanning_several_lines() {
    // A field broken across three physical lines is one record in parallel too.
    let input = "a,b\n\"x\",\"one\ntwo\nthree\"\np,q\n";
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "csv", "-F", "json", "-P"], input);
    assert_eq!(exit_code, 0, "stdout: {stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "stdout: {stdout}");
    assert_eq!(lines[0], r#"{"a":"x","b":"one\ntwo\nthree"}"#);
    assert_eq!(lines[1], r#"{"a":"p","b":"q"}"#);
}

#[test]
fn csv_multiline_parallel_holds_record_across_tiny_batches() {
    // --batch-size 1 would normally force one physical line per batch; the
    // record-aligned batcher must still keep the embedded-newline record whole.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "csv", "-F", "json", "-P", "--batch-size", "1"],
        RFC4180_MULTILINE,
    );
    assert_eq!(exit_code, 0, "stdout: {stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "stdout: {stdout}");
    assert_eq!(lines[0], r#"{"name":"alice","note":"hello\nworld"}"#);
    assert_eq!(lines[1], r#"{"name":"bob","note":"ok"}"#);
}

#[test]
fn csv_unterminated_quote_at_eof_errors_in_both_modes() {
    // A quote opened and never closed is genuinely malformed; both modes must
    // report it (and agree) rather than emit corrupt columns.
    let input = "name,note\n\"alice\",\"hello\nworld\n";
    for args in [
        &["-f", "csv", "-F", "json", "--strict"][..],
        &["-f", "csv", "-F", "json", "-P", "--strict"][..],
    ] {
        let (_stdout, stderr, exit_code) = run_kelora_with_input(args, input);
        assert_ne!(exit_code, 0, "{args:?} should fail, stderr: {stderr}");
        assert!(
            stderr.contains("Unterminated quoted field"),
            "{args:?} expected a clear cause, got: {stderr}"
        );
    }
}

#[test]
fn csv_blank_line_inside_quoted_field_is_preserved() {
    // An empty physical line inside a quoted value is part of the record, not a
    // standalone blank line to drop. Sequential and parallel must agree.
    let input = "a,b\n\"x\",\"line1\n\nline3\"\np,q\n";
    for args in [
        &["-f", "csv", "-F", "json"][..],
        &["-f", "csv", "-F", "json", "-P"][..],
    ] {
        let (stdout, _stderr, exit_code) = run_kelora_with_input(args, input);
        assert_eq!(exit_code, 0, "{args:?} stdout: {stdout}");
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{args:?} stdout: {stdout}");
        assert_eq!(lines[0], r#"{"a":"x","b":"line1\n\nline3"}"#, "{args:?}");
        assert_eq!(lines[1], r#"{"a":"p","b":"q"}"#, "{args:?}");
    }
}

#[test]
fn csv_ignore_lines_does_not_eat_record_continuation() {
    // --ignore-lines applies per physical line, but a line *inside* an open
    // quoted field is a continuation and must not be filtered out, or the record
    // is corrupted. Both modes must keep the whole value.
    let input = "a,b\n\"x\",\"keep\nDEBUG stuff\nmore\"\np,q\n";
    for args in [
        &["-f", "csv", "-F", "json", "--ignore-lines", "DEBUG"][..],
        &["-f", "csv", "-F", "json", "-P", "--ignore-lines", "DEBUG"][..],
    ] {
        let (stdout, _stderr, exit_code) = run_kelora_with_input(args, input);
        assert_eq!(exit_code, 0, "{args:?} stdout: {stdout}");
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{args:?} stdout: {stdout}");
        assert_eq!(
            lines[0], r#"{"a":"x","b":"keep\nDEBUG stuff\nmore"}"#,
            "{args:?}"
        );
        assert_eq!(lines[1], r#"{"a":"p","b":"q"}"#, "{args:?}");
    }
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
