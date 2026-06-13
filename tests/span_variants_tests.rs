mod common;
use common::*;

#[test]
fn test_field_span_basic() {
    let input = r#"{"request_id":"req-1","msg":"a"}
{"request_id":"req-1","msg":"b"}
{"request_id":"req-2","msg":"c"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "request_id",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("req-1:2"));
    assert!(stdout.contains("req-2:1"));
}

#[test]
fn test_field_span_interleaved_creates_multiple_spans() {
    let input = r#"{"request_id":"req-1","msg":"a"}
{"request_id":"req-2","msg":"b"}
{"request_id":"req-1","msg":"c"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "request_id",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);

    let mut seen = stdout
        .lines()
        .filter(|l| l.contains("req-1:") || l.contains("req-2:"))
        .collect::<Vec<_>>();
    seen.sort();
    assert_eq!(seen, vec!["req-1:1", "req-1:1", "req-2:1"]);
}

#[test]
fn test_field_span_missing_field_strict_errors() {
    let input = r#"{"msg":"missing id"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--span", "request_id", "--strict"], input);

    assert_eq!(exit_code, 1);
    assert!(stderr.contains("missing required field 'request_id'"));
}

#[test]
fn test_idle_span_forward_only_gaps() {
    let input = r#"{"ts":"2025-01-15T10:00:10Z","msg":"first"}
{"ts":"2025-01-15T10:00:05Z","msg":"out_of_order"}
{"ts":"2025-01-15T10:00:20Z","msg":"after_gap"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span-idle",
            "5s",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains(":2"), "first span should have 2 events");
    assert!(stdout.contains(":1"), "second span should have 1 event");
}

// Regression: a --span duration that fits in i64 milliseconds but exceeds
// chrono's representable datetime range used to abort the process (panic in
// ms_to_datetime, exit 134 under the release panic=abort profile). The window
// boundary must now clamp to the representable range and complete cleanly.
#[test]
fn test_time_span_huge_duration_does_not_panic() {
    let input = "ts=2024-01-01T00:00:00Z a=1\nts=2024-01-02T00:00:00Z a=2\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--span",
            "1000000000d",
            "--span-close",
            "print(\"closed:\" + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "should not abort; stderr: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "must not panic; stderr: {stderr}"
    );
    // Both events fall into the single (clamped) window.
    assert!(stdout.contains("closed:2"), "stdout: {stdout}");
}
