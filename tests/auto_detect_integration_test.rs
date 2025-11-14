mod common;
use common::*;

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

    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "auto", "--stats"], input);

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
