mod common;
use common::*;
use std::fs;
use tempfile::TempDir;

/// Default input format uses auto-detect
#[test]
fn test_default_format_is_auto() {
    let input = r#"{"level": "info", "message": "auto"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("level='info'") || stdout.contains("\"level\""),
        "Should parse JSON by default"
    );
}

/// Test auto-detection of JSON format
#[test]
fn test_auto_detect_json() {
    let input = r#"{"level": "info", "message": "test message"}
{"level": "error", "message": "error occurred"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto", "-F", "json"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("\"level\""),
        "Should output JSON with level field"
    );
    assert!(
        stdout.contains("\"message\""),
        "Should output JSON with message field"
    );
}

/// Test auto-detection of syslog RFC5424 format
#[test]
fn test_auto_detect_syslog_rfc5424() {
    let input = "<34>1 2023-04-15T10:00:00.000Z hostname myapp 1234 ID47 - Test message from app";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("myapp") || stdout.contains("hostname"),
        "Should parse and output syslog content"
    );
}

/// Test auto-detection of syslog RFC3164 format
#[test]
fn test_auto_detect_syslog_rfc3164() {
    let input =
        "<13>Apr 15 10:00:00 server1 sshd[1234]: Accepted publickey for user from 192.168.1.1";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("sshd") || stdout.contains("server1"),
        "Should parse and output syslog content"
    );
}

/// Test auto-detection of syslog without priority
#[test]
fn test_auto_detect_syslog_no_priority() {
    let input = "Jan 15 10:30:45 server1 sshd[1234]: Accepted publickey for user";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Should detect as syslog and parse it
    assert!(!stdout.is_empty(), "Should produce output");
}

/// Test auto-detection of CEF format
#[test]
fn test_auto_detect_cef() {
    let input =
        "CEF:0|Vendor|Product|1.0|100|EventName|5|src=192.168.1.1 dst=10.0.0.1 msg=Test event";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("Vendor") || stdout.contains("Product") || stdout.contains("EventName"),
        "Should parse and output CEF content"
    );
}

/// Test auto-detection of Apache/Nginx combined log format
#[test]
fn test_auto_detect_combined_logs() {
    let input = r#"192.168.1.100 - - [15/Apr/2023:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1234 "-" "Mozilla/5.0""#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("GET") || stdout.contains("192.168") || stdout.contains("200"),
        "Should parse and output combined log content"
    );
}

/// Test auto-detection of logfmt format
#[test]
fn test_auto_detect_logfmt() {
    let input = "time=2023-04-15T10:00:00Z level=info msg=test_message user_id=123 request_id=abc";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("level") || stdout.contains("msg") || stdout.contains("info"),
        "Should parse and output logfmt content"
    );
}

/// Test auto-detection of CSV format with headers
#[test]
fn test_auto_detect_csv() {
    let input = "name,age,city,status\nJohn,30,NYC,active\nJane,25,LA,inactive";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("John") || stdout.contains("Jane") || stdout.contains("NYC"),
        "Should parse and output CSV content"
    );
}

/// Test auto-detection of CSV without headers
#[test]
fn test_auto_detect_csv_no_headers() {
    let input = "1,2,3\n4,5,6\n7,8,9";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Should detect as CSV and parse numeric values
    assert!(!stdout.is_empty(), "Should produce output");
}

/// Test auto-detection of TSV format
#[test]
fn test_auto_detect_tsv() {
    let input = "name\tage\tcity\nAlice\t28\tBoston\nBob\t35\tSeattle";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("Alice") || stdout.contains("Bob") || stdout.contains("Boston"),
        "Should parse and output TSV content"
    );
}

/// Test fallback to line format for plain text
#[test]
fn test_auto_detect_fallback_to_line() {
    let input =
        "This is just some random plain text without any structure\nAnother line of plain text";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("random plain text") || stdout.contains("Another line"),
        "Should output plain text lines"
    );
}

/// Test malformed JSON falls back to line format
#[test]
fn test_auto_detect_malformed_json_fallback() {
    let input = r#"{"incomplete": "json object"
This line is not JSON at all"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    // Should not crash, should process lines (may detect first as line after failing JSON parse)
    assert_eq!(
        exit_code, 0,
        "kelora should handle malformed input gracefully"
    );
    assert!(!stdout.is_empty(), "Should produce some output");
}

/// Test auto-detection with filtering
#[test]
fn test_auto_detect_with_filter() {
    let input = r#"{"level": "info", "message": "info message"}
{"level": "error", "message": "error message"}
{"level": "debug", "message": "debug message"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "auto",
            "-F",
            "json",
            "--filter",
            "e.level == \"error\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("error message"),
        "Should contain filtered error message"
    );
    assert!(
        !stdout.contains("info message"),
        "Should not contain info message"
    );
    assert!(
        !stdout.contains("debug message"),
        "Should not contain debug message"
    );
}

/// Test auto-detection with stats
#[test]
fn test_auto_detect_with_stats() {
    let input = r#"{"level": "info", "message": "msg1"}
{"level": "error", "message": "msg2"}
{"level": "info", "message": "msg3"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "auto", "--with-stats"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("Events") || stderr.contains("processed"),
        "Should show stats, got: {}",
        stderr
    );
}

/// Test auto-detection with mixed formats (should use first line to detect)
#[test]
fn test_auto_detect_uses_first_line() {
    // First line is JSON, so everything should be parsed as JSON
    let input = r#"{"level": "info", "message": "json line"}
This is plain text
Another plain line"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "auto", "-F", "json"], input);

    // May fail if strict mode catches parse errors on non-JSON lines, which is acceptable
    if exit_code != 0 {
        // If it fails, check that it at least tried to parse the JSON line
        // This is acceptable behavior - detecting JSON and then failing on invalid lines
        assert!(
            stderr.contains("Parse") || stderr.contains("parse"),
            "Should indicate parse error for mixed format input"
        );
    } else {
        // If it succeeds, should have parsed the first JSON line
        assert!(stdout.contains("json line"), "Should parse first JSON line");
    }
}

/// Test auto-detection with empty input
#[test]
fn test_auto_detect_empty_input() {
    let input = "";

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should handle empty input gracefully");
}

/// Test auto-detection with only whitespace
#[test]
fn test_auto_detect_whitespace_only() {
    let input = "   \n\t\n   ";

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should handle whitespace-only input gracefully"
    );
}

/// Test auto-detection priority: JSON over other formats
#[test]
fn test_auto_detect_priority_json() {
    // A line that could be ambiguous - starts with { so should be detected as JSON
    let input = r#"{"timestamp": "2023-01-01", "data": "value"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto", "-F", "json"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("timestamp") && stdout.contains("data"),
        "Should parse as JSON"
    );
}

/// Test auto-detection of multiple syslog formats in sequence
#[test]
fn test_auto_detect_multiple_syslog_lines() {
    let input = r#"<34>1 2023-04-15T10:00:00.000Z host1 app1 - - - message1
<35>1 2023-04-15T10:01:00.000Z host2 app2 - - - message2
<36>1 2023-04-15T10:02:00.000Z host3 app3 - - - message3"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stdout.contains("app1") || stdout.contains("app2") || stdout.contains("app3"),
        "Should parse all syslog lines"
    );
}

/// Test auto-detection with exec script
#[test]
fn test_auto_detect_with_exec() {
    let input = r#"{"count": 1}
{"count": 2}
{"count": 3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "auto", "--exec", "e.doubled = e.count.to_int() * 2"],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully, stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("doubled"),
        "Should execute script on auto-detected JSON, stdout: {}",
        stdout
    );
}

/// Test that invalid format strings work correctly with auto
#[test]
fn test_auto_detect_format_string() {
    let input = r#"{"level": "info", "msg": "test"}"#;

    // Using -f auto should work and detect JSON
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "auto"], input);

    assert_eq!(exit_code, 0, "kelora -f auto should work");
    assert!(!stdout.is_empty(), "Should produce output");
}

#[test]
fn test_auto_detect_uses_first_non_empty_line() {
    let input = "\n\n{\"msg\":\"json-after-blanks\"}\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "auto", "-F", "json"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert_eq!(
        stdout.trim(),
        r#"{"msg":"json-after-blanks"}"#,
        "auto detection should skip leading blanks for detection and still parse JSON"
    );
}

/// Write `contents` into `dir` as `name` and return the path as a String.
fn write_input(dir: &TempDir, name: &str, contents: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, contents).expect("failed to write test input");
    path.to_string_lossy().into_owned()
}

const JSON_INPUT: &str =
    "{\"level\":\"ERROR\",\"msg\":\"boom\"}\n{\"level\":\"INFO\",\"msg\":\"ok\"}\n";

/// A completely empty leading file must not pin detection to `line`.
///
/// Regression: detection stopped at the first file it could *open*, so an empty
/// first file made every later file parse as whole lines — with no warning,
/// since an empty file also cleared the "fell back to line" hint.
#[test]
fn test_auto_detect_skips_empty_leading_file() {
    let dir = TempDir::new().expect("tempdir");
    let empty = write_input(&dir, "empty.log", "");
    let json = write_input(&dir, "data.json", JSON_INPUT);

    let (stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto"], &[&empty, &json]);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        stdout.contains("level='ERROR'") && stdout.contains("msg='boom'"),
        "should detect JSON from the first file with content, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("line='"),
        "must not fall back to whole-line parsing, got: {}",
        stdout
    );
}

/// Same regression in parallel mode, which has its own detection entry point.
#[test]
fn test_auto_detect_skips_empty_leading_file_parallel() {
    let dir = TempDir::new().expect("tempdir");
    let empty = write_input(&dir, "empty.log", "");
    let json = write_input(&dir, "data.json", JSON_INPUT);

    let (stdout, stderr, exit_code) =
        run_kelora_with_files(&["-f", "auto", "--threads", "2"], &[&empty, &json]);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        stdout.contains("level='ERROR'"),
        "parallel detection should skip the empty file too, got: {}",
        stdout
    );
}

/// A leading file holding only blank lines is equally undetectable, so the scan
/// must keep going rather than settling for `line`.
#[test]
fn test_auto_detect_skips_blank_only_leading_file() {
    let dir = TempDir::new().expect("tempdir");
    let blank = write_input(&dir, "blank.log", "\n\n\n");
    let json = write_input(&dir, "data.json", JSON_INPUT);

    let (stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto"], &[&blank, &json]);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        stdout.contains("level='ERROR'"),
        "should detect JSON past the blank-only file, got: {}",
        stdout
    );
}

/// Detection stops at the first file that *has* content, so a later file in a
/// different format is still parsed with the earlier file's format. This pins
/// the "first non-empty line" contract the skip-empty behavior extends.
#[test]
fn test_auto_detect_stops_at_first_file_with_content() {
    let dir = TempDir::new().expect("tempdir");
    let json = write_input(&dir, "a.json", "{\"msg\":\"first\"}\n");
    let plain = write_input(&dir, "b.log", "not json at all\n");

    let (stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto"], &[&json, &plain]);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        stdout.contains("msg='first'"),
        "should detect JSON from the first file with content, got: {}",
        stdout
    );
}

/// All inputs empty: nothing to detect, so `line` is the right answer and the
/// run must stay silent and successful rather than erroring.
#[test]
fn test_auto_detect_all_empty_files_is_not_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let a = write_input(&dir, "a.log", "");
    let b = write_input(&dir, "b.log", "");

    let (stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto"], &[&a, &b]);

    assert_eq!(exit_code, 0, "empty inputs are not an error: {}", stderr);
    assert!(stdout.is_empty(), "no events expected, got: {}", stdout);
    assert!(
        !stderr.contains("hint"),
        "no input at all should not trigger the fell-back-to-line hint: {}",
        stderr
    );
}

/// Blank-lines-only input still counts as "we read something", so the
/// fell-back-to-line hint must survive the skip-empty change.
#[test]
fn test_auto_detect_blank_only_input_still_hints() {
    let dir = TempDir::new().expect("tempdir");
    let blank = write_input(&dir, "blank.log", "\n\n");

    let (_stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto", "--hints"], &[&blank]);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stderr);
    assert!(
        stderr.contains("No input format detected"),
        "blank-only input should still hint about the line fallback: {}",
        stderr
    );
}

/// An unopenable file ahead of an empty one must not be swallowed: detection
/// falls through to `line`, but the open failure is still reported non-zero.
#[test]
fn test_auto_detect_reports_open_failure_alongside_empty_file() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("nope.log").to_string_lossy().into_owned();
    let empty = write_input(&dir, "empty.log", "");

    let (_stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "auto"], &[&missing, &empty]);

    assert_ne!(exit_code, 0, "missing input must still fail the run");
    assert!(
        stderr.contains("nope.log"),
        "the unopenable file must be named on stderr: {}",
        stderr
    );
}
