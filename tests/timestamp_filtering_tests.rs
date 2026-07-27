// tests/timestamp_filtering_tests.rs
use chrono::{Duration, Timelike, Utc};
use std::io::Write;
use std::process::{Command, Stdio};

mod common;
use common::{extract_stats_lines, stats_line};

/// Helper function to run kelora with given arguments and input via stdin
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    run_kelora_with_input_env(args, input, &[])
}

/// Like [`run_kelora_with_input`], but sets the given environment variables on
/// the child process only. This avoids mutating the test process's global
/// environment, which would race with other tests running in parallel.
fn run_kelora_with_input_env(
    args: &[&str],
    input: &str,
    envs: &[(&str, &str)],
) -> (String, String, i32) {
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    let binary_path = env!("CARGO_BIN_EXE_kelora");

    let mut cmd = Command::new(binary_path)
        .args(args)
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

/// Get current timestamp in ISO format for testing
fn get_test_timestamp_iso(offset_minutes: i64) -> String {
    let dt = Utc::now() + Duration::minutes(offset_minutes);
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Get current timestamp in space format for testing
fn get_test_timestamp_space(offset_minutes: i64) -> String {
    let dt = Utc::now() + Duration::minutes(offset_minutes);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[test]
fn test_since_basic_iso_format() {
    let old_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let new_ts = get_test_timestamp_iso(0); // now

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    eprintln!("STDOUT>>>{}<<<", stdout);
    eprintln!("STDERR>>>{}<<<", stderr);
    assert!(stdout.contains("new event"), "Should include recent event");
    assert!(!stdout.contains("old event"), "Should exclude old event");
}

#[test]
fn test_until_basic_iso_format() {
    let old_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let new_ts = get_test_timestamp_iso(0); // now

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let until_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--until", &until_ts], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(!stdout.contains("new event"), "Should exclude recent event");
    assert!(stdout.contains("old event"), "Should include old event");
}

#[test]
fn test_since_and_until_combined() {
    let very_old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let old_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let middle_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let new_ts = get_test_timestamp_iso(0); // now

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "very old event"}}
{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "middle event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        very_old_ts, old_ts, middle_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-90); // 90 minutes ago
    let until_ts = get_test_timestamp_iso(-15); // 15 minutes ago

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--until", &until_ts],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("very old event"),
        "Should exclude very old event"
    );
    assert!(stdout.contains("old event"), "Should include old event");
    assert!(
        stdout.contains("middle event"),
        "Should include middle event"
    );
    assert!(!stdout.contains("new event"), "Should exclude new event");
}

#[test]
fn test_since_relative_time() {
    let old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let new_ts = get_test_timestamp_iso(-30); // 30 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    // Test with -1h (1 hour ago)
    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--since=-1h"], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("old event"),
        "Should exclude event older than 1 hour"
    );
    assert!(
        stdout.contains("new event"),
        "Should include event newer than 1 hour"
    );
}

#[test]
fn test_until_relative_time() {
    let old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let new_ts = get_test_timestamp_iso(-30); // 30 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    // Test with -1h (1 hour ago)
    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--until=-1h"], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("old event"),
        "Should include event older than 1 hour"
    );
    assert!(
        !stdout.contains("new event"),
        "Should exclude event newer than 1 hour"
    );
}

#[test]
fn test_since_now_anchor_relative() {
    // Regression: `--since now-15m` is documented in --help-time but previously
    // failed with "Could not parse timestamp: now-15m" because resolve_time_range
    // bypassed parse_anchored_timestamp for self-relative `now+`/`now-` forms.
    let old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let new_ts = get_test_timestamp_iso(-5); // 5 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "now-15m"], &input);

    assert_eq!(
        exit_code, 0,
        "`--since now-15m` should be accepted. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("old event"),
        "Should exclude event older than 15 minutes"
    );
    assert!(
        stdout.contains("new event"),
        "Should include event within the last 15 minutes"
    );
}

#[test]
fn test_until_now_anchor_relative() {
    // Regression companion to test_since_now_anchor_relative for `now+`.
    let old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let new_ts = get_test_timestamp_iso(-5); // 5 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--until", "now-15m"], &input);

    assert_eq!(
        exit_code, 0,
        "`--until now-15m` should be accepted. stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("old event"),
        "Should include event older than 15 minutes"
    );
    assert!(
        !stdout.contains("new event"),
        "Should exclude event within the last 15 minutes"
    );
}

#[test]
fn test_since_space_separated_negative_relative() {
    // Regression: `--since -30m` (space-separated, documented in --help-time and
    // the --since help text) previously failed clap parsing with
    // "unexpected argument '-3' found". allow_hyphen_values fixes this.
    let old_ts = get_test_timestamp_iso(-120); // 2 hours ago
    let new_ts = get_test_timestamp_iso(-5); // 5 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "-30m"], &input);

    assert_eq!(
        exit_code, 0,
        "`--since -30m` (space form) should be accepted. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("old event"),
        "Should exclude event older than 30 minutes"
    );
    assert!(
        stdout.contains("new event"),
        "Should include event within the last 30 minutes"
    );
}

#[test]
fn test_since_special_values() {
    let today = chrono::Local::now().date_naive();
    let yesterday = today - Duration::days(1);

    let yesterday_ts = yesterday
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let today_ts = today
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "yesterday event"}}
{{"ts": "{}", "level": "info", "msg": "today event"}}"#,
        yesterday_ts, today_ts
    );

    // Test with "today"
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "today"], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("yesterday event"),
        "Should exclude yesterday event"
    );
    assert!(stdout.contains("today event"), "Should include today event");
}

#[test]
fn test_different_timestamp_formats() {
    let iso_ts = get_test_timestamp_iso(-60);
    let space_ts = get_test_timestamp_space(-30);
    let unix_ts = (Utc::now().timestamp() - 900).to_string(); // 15 minutes ago

    let input = format!(
        r#"{{"timestamp": "{}", "level": "info", "msg": "iso format"}}
{{"ts": "{}", "level": "info", "msg": "space format"}}
{{"time": "{}", "level": "info", "msg": "unix format"}}"#,
        iso_ts, space_ts, unix_ts
    );

    let since_ts = get_test_timestamp_iso(-45); // 45 minutes ago

    // Set TZ=UTC for consistent test behavior regardless of system timezone.
    // Pass it to the child only, so we don't mutate this process's global
    // environment and race with other tests running in parallel.
    let (stdout, stderr, exit_code) = run_kelora_with_input_env(
        &["-f", "json", "--since", &since_ts],
        &input,
        &[("TZ", "UTC")],
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("iso format"),
        "Should exclude ISO format event (too old)"
    );
    assert!(
        stdout.contains("space format"),
        "Should include space format event"
    );
    assert!(
        stdout.contains("unix format"),
        "Should include unix format event"
    );
}

#[test]
fn test_events_without_timestamps() {
    let with_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "with timestamp"}}
{{"level": "info", "msg": "without timestamp"}}
{{"random_field": "value", "msg": "also without timestamp"}}"#,
        with_ts
    );

    let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("with timestamp"),
        "Should include event with timestamp"
    );
    // In the new resiliency model, events without timestamps are filtered out
    // when using --since/--until filters
    assert!(
        !stdout.contains("without timestamp"),
        "Should filter out events without timestamps in resilient mode"
    );
    assert!(
        !stdout.contains("also without timestamp"),
        "Should filter out all events without valid timestamps"
    );
}

#[test]
fn test_timestamp_filtering_with_line_format() {
    let ts1 = get_test_timestamp_iso(-60);
    let ts2 = get_test_timestamp_iso(-30);

    let input = format!(
        "{} This is an old log line\n{} This is a new log line",
        ts1, ts2
    );

    let since_ts = get_test_timestamp_iso(-45); // 45 minutes ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--since", &since_ts], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    // For line format, timestamps aren't automatically parsed to event.parsed_ts,
    // so events without parsed timestamps are filtered out when using --since/--until
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "Line format without parsed timestamps should be filtered out when using --since"
    );
}

#[test]
fn test_events_without_timestamps_strict_mode() {
    let with_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "with timestamp"}}
{{"level": "info", "msg": "without timestamp"}}"#,
        with_ts
    );

    let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--strict"], &input);

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error in strict mode when encountering event without timestamp"
    );

    // Should process the first event with timestamp but fail on the second
    assert!(
        stdout.contains("with timestamp"),
        "Should process first event with timestamp before failing"
    );
}

#[test]
fn test_timestamp_filtering_with_custom_field() {
    // NOTE: --ts-field support for timestamp filtering is not yet fully implemented
    // This test uses standard 'ts' field name instead of custom field
    let old_ts = get_test_timestamp_iso(-60);
    let new_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-45); // 45 minutes ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(!stdout.contains("old event"), "Should exclude old event");
    assert!(stdout.contains("new event"), "Should include new event");
}

#[test]
fn test_timestamp_filtering_with_other_filters() {
    let old_ts = get_test_timestamp_iso(-60);
    let new_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "error", "msg": "old error"}}
{{"ts": "{}", "level": "info", "msg": "old info"}}
{{"ts": "{}", "level": "error", "msg": "new error"}}
{{"ts": "{}", "level": "info", "msg": "new info"}}"#,
        old_ts, old_ts, new_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-45); // 45 minutes ago
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--levels", "error"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(!stdout.contains("old error"), "Should exclude old error");
    assert!(!stdout.contains("old info"), "Should exclude old info");
    assert!(stdout.contains("new error"), "Should include new error");
    assert!(
        !stdout.contains("new info"),
        "Should exclude new info (wrong level)"
    );
}

#[test]
fn test_stats_timestamp_line_points_at_parsed_ts() {
    // The line announcing a successful parse is exactly where a user learns the
    // timestamp exists, so it is where they should learn how to reach it.
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","message":"event"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-s"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stdout);
    assert!(
        stdout.contains(
            "Timestamp: timestamp (auto-detected) - 1/1 parsed (100.0%) — access via meta.parsed_ts."
        ),
        "Stats should point at meta.parsed_ts once something parsed.\nSTDOUT:\n{}",
        stdout
    );
}

#[test]
fn test_stats_timestamp_line_omits_parsed_ts_when_nothing_parsed() {
    // With no parsed timestamp there is nothing to reach, so the pointer would
    // only be noise on top of the hint that already tells the user what to fix.
    let input = r#"{"message":"no timestamp here"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-s"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully: {}", stdout);
    assert!(
        !stdout.contains("meta.parsed_ts"),
        "Stats should stay quiet about meta.parsed_ts when nothing parsed.\nSTDOUT:\n{}",
        stdout
    );
}

#[test]
fn test_stats_report_custom_ts_field_failures() {
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","service":"api","message":"event"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-s", "--ts-field", "service"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stdout: {}",
        stdout
    );
    assert!(
        stdout.contains(
            "Timestamp: service (--ts-field) - 0/1 parsed (0.0%). Hint: Adjust --ts-format."
        ),
        "Stats should report the failure for the user-specified timestamp field.\nSTDOUT:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Warning: --ts-field service values could not be parsed"),
        "Should emit a summary warning for the failed --ts-field override.\nSTDOUT:\n{}",
        stdout
    );
}

#[test]
fn test_stats_report_custom_ts_format_failures() {
    let input = r#"{"timestamp":"not-a-timestamp","message":"event"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-s", "--ts-format", "%d"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stdout: {}",
        stdout
    );
    assert!(
        stdout.contains(
            "Timestamp: timestamp (auto-detected) - 0/1 parsed (0.0%). Hint: Try --ts-field or --ts-format."
        ),
        "Overall timestamp parsing should reflect the failure.\nSTDOUT:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Warning: --ts-format '%d' did not match any timestamp values"),
        "Should emit a summary warning for the failed --ts-format override.\nSTDOUT:\n{}",
        stdout
    );
}

#[test]
fn test_custom_ts_field_failure_strict_exits() {
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","service":"api","message":"event"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-s", "--ts-field", "service", "--strict"],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "strict mode should cause non-zero exit on override failure"
    );
    assert!(
        stdout.contains("Warning: --ts-field service values could not be parsed"),
        "Strict mode should still display the warning in stats output.\nSTDOUT:\n{}",
        stdout
    );
}

#[test]
fn test_custom_ts_field_failure_strict_without_stats() {
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","service":"api","message":"event"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--ts-field", "service", "--strict"], input);

    assert_eq!(
        exit_code, 1,
        "strict mode should cause non-zero exit on override failure"
    );
    assert!(
        stderr.contains("--ts-field service values could not be parsed"),
        "Strict mode should emit override failure message when stats are disabled.\nSTDERR:\n{}\nSTDOUT:\n{}",
        stderr,
        stdout
    );
}

#[test]
fn test_timestamp_filtering_with_exec_script() {
    let old_ts = get_test_timestamp_iso(-60);
    let new_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event", "count": 5}}
{{"ts": "{}", "level": "info", "msg": "new event", "count": 10}}"#,
        old_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-45); // 45 minutes ago
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--since",
            &since_ts,
            "--exec",
            "e.count = e.count * 2",
        ],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(!stdout.contains("old event"), "Should exclude old event");
    assert!(stdout.contains("new event"), "Should include new event");
    assert!(
        stdout.contains("\"count\":20")
            || stdout.contains("count: 20")
            || stdout.contains("count=20"),
        "Should have doubled the count. Got: {}",
        stdout
    );
}

#[test]
fn test_invalid_since_timestamp() {
    let input = r#"{"ts": "2023-07-04T12:34:56Z", "level": "info", "msg": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "invalid-timestamp"], input);

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error for invalid timestamp"
    );
    assert!(
        stderr.contains("Invalid --since timestamp"),
        "Should show error for invalid --since"
    );
}

#[test]
fn test_invalid_until_timestamp() {
    let input = r#"{"ts": "2023-07-04T12:34:56Z", "level": "info", "msg": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--until", "not-a-date"], input);

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error for invalid timestamp"
    );
    assert!(
        stderr.contains("Invalid --until timestamp"),
        "Should show error for invalid --until"
    );
}

#[test]
fn test_date_only_timestamp() {
    let today = chrono::Local::now().date_naive();
    let yesterday = today - Duration::days(1);

    let yesterday_ts = yesterday
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let today_ts = today
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "yesterday event"}}
{{"ts": "{}", "level": "info", "msg": "today event"}}"#,
        yesterday_ts, today_ts
    );

    let today_date = today.format("%Y-%m-%d").to_string();
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &today_date], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("yesterday event"),
        "Should exclude yesterday event"
    );
    assert!(stdout.contains("today event"), "Should include today event");
}

#[test]
fn test_time_only_timestamp() {
    let now = Utc::now();
    let earlier_today = now
        .with_hour(10)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap();
    let later_today = now
        .with_hour(14)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap();

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "morning event"}}
{{"ts": "{}", "level": "info", "msg": "afternoon event"}}"#,
        earlier_today.format("%Y-%m-%dT%H:%M:%SZ"),
        later_today.format("%Y-%m-%dT%H:%M:%SZ")
    );

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "12:00:00"], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    // Results depend on current time, but command should not error
}

#[test]
fn test_unix_timestamp_filtering() {
    let now = Utc::now().timestamp();
    let hour_ago = now - 3600;
    let half_hour_ago = now - 1800;

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        hour_ago, half_hour_ago
    );

    let since_unix = (now - 2700).to_string(); // 45 minutes ago
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_unix], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(!stdout.contains("old event"), "Should exclude old event");
    assert!(stdout.contains("new event"), "Should include new event");
}

#[test]
fn test_timestamp_filtering_parallel_mode() {
    let old_ts = get_test_timestamp_iso(-60);
    let new_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-45);
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--parallel"], &input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully in parallel mode. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("old event"),
        "Should exclude old event in parallel mode"
    );
    assert!(
        stdout.contains("new event"),
        "Should include new event in parallel mode"
    );
}

#[test]
fn test_timestamp_filtering_with_stats() {
    let old_ts = get_test_timestamp_iso(-60);
    let new_ts = get_test_timestamp_iso(-30);

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
{{"ts": "{}", "level": "info", "msg": "new event"}}"#,
        old_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-45);
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--with-stats"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with stats. stderr: {}",
        stderr
    );
    let stats = extract_stats_lines(&stderr);
    let events = stats_line(&stats, "Events created:");
    assert_eq!(
        events,
        "Events created: 2 total, 1 output, 1 filtered (50.0%)"
    );
}

#[test]
fn test_timestamp_filtering_stats_counts() {
    let very_old_ts = get_test_timestamp_iso(-180); // 3 hours ago
    let old_ts = get_test_timestamp_iso(-90); // 1.5 hours ago
    let recent_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let new_ts = get_test_timestamp_iso(-10); // 10 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "very old"}}
{{"ts": "{}", "level": "info", "msg": "old"}}
{{"ts": "{}", "level": "info", "msg": "recent"}}
{{"ts": "{}", "level": "info", "msg": "new"}}"#,
        very_old_ts, old_ts, recent_ts, new_ts
    );

    let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--with-stats"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let stats = extract_stats_lines(&stderr);
    let events = stats_line(&stats, "Events created:");
    assert_eq!(
        events,
        "Events created: 4 total, 2 output, 2 filtered (50.0%)"
    );
}

#[test]
fn test_timestamp_filtering_stats_with_mixed_timestamps() {
    let old_ts = get_test_timestamp_iso(-90); // 1.5 hours ago
    let recent_ts = get_test_timestamp_iso(-30); // 30 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old with timestamp"}}
{{"level": "info", "msg": "no timestamp event"}}
{{"ts": "{}", "level": "info", "msg": "recent with timestamp"}}
{{"random_field": "value", "msg": "another no timestamp"}}"#,
        old_ts, recent_ts
    );

    let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--with-stats"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let stats = extract_stats_lines(&stderr);
    let events = stats_line(&stats, "Events created:");
    assert_eq!(
        events,
        "Events created: 4 total, 1 output, 3 filtered (75.0%)"
    );
}

#[test]
fn test_timestamp_filtering_stats_all_filtered() {
    let old_ts1 = get_test_timestamp_iso(-180); // 3 hours ago
    let old_ts2 = get_test_timestamp_iso(-120); // 2 hours ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "old event 1"}}
{{"ts": "{}", "level": "info", "msg": "old event 2"}}"#,
        old_ts1, old_ts2
    );

    let since_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--with-stats"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let stats = extract_stats_lines(&stderr);
    let events = stats_line(&stats, "Events created:");
    assert_eq!(
        events,
        "Events created: 2 total, 0 output, 2 filtered (100.0%)"
    );
}

#[test]
fn test_timestamp_filtering_stats_none_filtered() {
    let recent_ts1 = get_test_timestamp_iso(-20); // 20 minutes ago
    let recent_ts2 = get_test_timestamp_iso(-10); // 10 minutes ago

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "recent event 1"}}
{{"ts": "{}", "level": "info", "msg": "recent event 2"}}"#,
        recent_ts1, recent_ts2
    );

    let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &since_ts, "--with-stats"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let stats = extract_stats_lines(&stderr);
    let events = stats_line(&stats, "Events created:");
    assert_eq!(
        events,
        "Events created: 2 total, 2 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_anchored_timestamp_since_plus() {
    let base_ts = get_test_timestamp_iso(-60); // 1 hour ago
    let event1_ts = get_test_timestamp_iso(-60); // At since time
    let event2_ts = get_test_timestamp_iso(-45); // 15 minutes after since
    let event3_ts = get_test_timestamp_iso(-30); // 30 minutes after since
    let event4_ts = get_test_timestamp_iso(-15); // 45 minutes after since

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "event 1"}}
{{"ts": "{}", "level": "info", "msg": "event 2"}}
{{"ts": "{}", "level": "info", "msg": "event 3"}}
{{"ts": "{}", "level": "info", "msg": "event 4"}}"#,
        event1_ts, event2_ts, event3_ts, event4_ts
    );

    // Show events from 1 hour ago for duration of 20 minutes
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &base_ts, "--until", "since+20m"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(stdout.contains("event 1"), "Should include event at since");
    assert!(
        stdout.contains("event 2"),
        "Should include event 15min after since"
    );
    assert!(
        !stdout.contains("event 3"),
        "Should exclude event 30min after since"
    );
    assert!(
        !stdout.contains("event 4"),
        "Should exclude event 45min after since"
    );
}

#[test]
fn test_anchored_timestamp_since_minus() {
    let base_ts = get_test_timestamp_iso(-30); // 30 minutes ago
    let event1_ts = get_test_timestamp_iso(-60); // 30 minutes before since
    let event2_ts = get_test_timestamp_iso(-45); // 15 minutes before since
    let event3_ts = get_test_timestamp_iso(-30); // At since time
    let event4_ts = get_test_timestamp_iso(-15); // 15 minutes after since

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "event 1"}}
{{"ts": "{}", "level": "info", "msg": "event 2"}}
{{"ts": "{}", "level": "info", "msg": "event 3"}}
{{"ts": "{}", "level": "info", "msg": "event 4"}}"#,
        event1_ts, event2_ts, event3_ts, event4_ts
    );

    // Show events ending 10 minutes before since
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", &base_ts, "--until", "since-10m"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    // No events should match (all are at or after since-10m boundary)
    assert!(
        !stdout.contains("event 1"),
        "Should exclude event 30min before since"
    );
    assert!(
        !stdout.contains("event 2"),
        "Should exclude event 15min before since"
    );
    assert!(!stdout.contains("event 3"), "Should exclude event at since");
    assert!(
        !stdout.contains("event 4"),
        "Should exclude event after since"
    );
}

#[test]
fn test_anchored_timestamp_until_minus() {
    let until_ts = get_test_timestamp_iso(-15); // 15 minutes ago
    let event1_ts = get_test_timestamp_iso(-60); // 45 minutes before until
    let event2_ts = get_test_timestamp_iso(-45); // 30 minutes before until
    let event3_ts = get_test_timestamp_iso(-30); // 15 minutes before until
    let event4_ts = get_test_timestamp_iso(-15); // At until time

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "event 1"}}
{{"ts": "{}", "level": "info", "msg": "event 2"}}
{{"ts": "{}", "level": "info", "msg": "event 3"}}
{{"ts": "{}", "level": "info", "msg": "event 4"}}"#,
        event1_ts, event2_ts, event3_ts, event4_ts
    );

    // Show events starting 30 minutes before until
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", "until-30m", "--until", &until_ts],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("event 1"),
        "Should exclude event 45min before until"
    );
    assert!(
        stdout.contains("event 2"),
        "Should include event 30min before until"
    );
    assert!(
        stdout.contains("event 3"),
        "Should include event 15min before until"
    );
    assert!(stdout.contains("event 4"), "Should include event at until");
}

#[test]
fn test_anchored_timestamp_circular_dependency_error() {
    let input = r#"{"ts": "2024-01-15T10:00:00Z", "level": "info", "msg": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", "until-1h", "--until", "since+1h"],
        input,
    );

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error for circular dependency"
    );
    assert!(
        stderr.contains("Cannot use both 'since' and 'until' anchors"),
        "Should show circular dependency error. stderr: {}",
        stderr
    );
}

#[test]
fn test_anchored_timestamp_missing_since_anchor_error() {
    let input = r#"{"ts": "2024-01-15T10:00:00Z", "level": "info", "msg": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--until", "since+30m"], input);

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error when since anchor is missing"
    );
    assert!(
        stderr.contains("'since' anchor requires --since"),
        "Should show missing anchor error. stderr: {}",
        stderr
    );
}

#[test]
fn test_anchored_timestamp_missing_until_anchor_error() {
    let input = r#"{"ts": "2024-01-15T10:00:00Z", "level": "info", "msg": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", "until-30m"], input);

    assert_ne!(
        exit_code, 0,
        "kelora should exit with error when until anchor is missing"
    );
    assert!(
        stderr.contains("'until' anchor requires --until"),
        "Should show missing anchor error. stderr: {}",
        stderr
    );
}

#[test]
fn test_anchored_timestamp_with_relative_time() {
    // Use absolute timestamps to avoid timing issues
    let base_ts = "2024-01-15T10:00:00Z";
    let event1_ts = "2024-01-15T10:00:00Z"; // At start time
    let event2_ts = "2024-01-15T10:30:00Z"; // 30 minutes after start
    let event3_ts = "2024-01-15T11:00:00Z"; // 1 hour after start
    let event4_ts = "2024-01-15T11:30:00Z"; // 1.5 hours after start

    let input = format!(
        r#"{{"ts": "{}", "level": "info", "msg": "event 1"}}
{{"ts": "{}", "level": "info", "msg": "event 2"}}
{{"ts": "{}", "level": "info", "msg": "event 3"}}
{{"ts": "{}", "level": "info", "msg": "event 4"}}"#,
        event1_ts, event2_ts, event3_ts, event4_ts
    );

    // Show events from 10:00 for 1 hour (until 11:00)
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--since", base_ts, "--until", "since+1h"],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    assert!(
        stdout.contains("event 1"),
        "Should include event 1 (at start)"
    );
    assert!(
        stdout.contains("event 2"),
        "Should include event 2 (30min after start)"
    );
    // Note: Both boundaries are inclusive, so event 3 at exactly 1h after start will be included
    assert!(
        stdout.contains("event 3"),
        "Should include event 3 (at the until boundary, which is inclusive)"
    );
    assert!(
        !stdout.contains("event 4"),
        "Should exclude event 4 (1.5h after start, beyond window)"
    );
}

// --- #287: naive-timestamp UTC-assumption diagnostic ----------------------
//
// Naive timestamps (no zone offset) are resolved with the default timezone
// (UTC unless --input-tz/TZ says otherwise). The hint surfaces that silent
// assumption, but only when the run depends on or materializes it: a time
// filter, a span, or --normalize-ts. TZ="" forces the "no zone chosen" path
// deterministically regardless of the parent environment.
const NAIVE_SYSLOG: &str = "Jan  2 15:04:05 host app: hello";

#[test]
fn test_naive_ts_hint_fires_with_since() {
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "syslog", "--since", "2020-01-01"],
        NAIVE_SYSLOG,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        stderr.contains("no zone offset") && stderr.contains("--input-tz"),
        "a naive timestamp with --since should warn about the UTC assumption: {stderr}"
    );
}

#[test]
fn test_naive_ts_hint_fires_with_span() {
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "syslog", "--span", "1m"],
        NAIVE_SYSLOG,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        stderr.contains("no zone offset"),
        "a naive timestamp with --span should warn: {stderr}"
    );
}

#[test]
fn test_naive_ts_hint_mentions_normalize() {
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "syslog", "--normalize-ts"],
        NAIVE_SYSLOG,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        stderr.contains("no zone offset") && stderr.contains("--normalize-ts"),
        "--normalize-ts bakes the offset into output, so the wording should call it out: {stderr}"
    );
}

#[test]
fn test_naive_ts_no_hint_without_time_op() {
    // Plain naive output never depends on the assumption -> stay quiet.
    let (_out, stderr, code) =
        run_kelora_with_input_env(&["-f", "syslog"], NAIVE_SYSLOG, &[("TZ", "")]);
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("no zone offset"),
        "without a time op the assumption changes nothing; no hint expected: {stderr}"
    );
}

#[test]
fn test_naive_ts_no_hint_with_explicit_input_tz() {
    // The user chose a zone explicitly; there is nothing silent to surface.
    let (_out, stderr, code) = run_kelora_with_input_env(
        &[
            "-f",
            "syslog",
            "--since",
            "2020-01-01",
            "--input-tz",
            "America/New_York",
        ],
        NAIVE_SYSLOG,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("no zone offset"),
        "explicit --input-tz means the zone was chosen; no hint expected: {stderr}"
    );
}

#[test]
fn test_naive_ts_no_hint_with_tz_env() {
    // A non-empty TZ is also a deliberate choice.
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "syslog", "--since", "2020-01-01"],
        NAIVE_SYSLOG,
        &[("TZ", "America/New_York")],
    );
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("no zone offset"),
        "a non-empty TZ is a chosen zone; no hint expected: {stderr}"
    );
}

#[test]
fn test_offset_ts_no_hint() {
    // Timestamps that carry an explicit offset are never assumed.
    let line = r#"127.0.0.1 - - [10/Oct/2024:13:55:36 -0700] "GET / HTTP/1.1" 200 1"#;
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "combined", "--since", "2020-01-01"],
        line,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("no zone offset"),
        "offset-bearing timestamps are not naive; no hint expected: {stderr}"
    );
}

#[test]
fn test_naive_ts_hint_suppressed_by_no_diagnostics() {
    let (_out, stderr, code) = run_kelora_with_input_env(
        &["-f", "syslog", "--since", "2020-01-01", "--no-diagnostics"],
        NAIVE_SYSLOG,
        &[("TZ", "")],
    );
    assert_eq!(code, 0);
    assert!(
        !stderr.contains("no zone offset"),
        "--no-diagnostics must suppress the naive-timestamp hint: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Prefilter/aggregate agreement matrix
//
// Every way of narrowing the event set must narrow the aggregates too. The
// invariant is a whole-pipeline one, so it is asserted as a matrix rather than
// as a single case: `--since`/`--until` used to be applied after the script
// stages, so metrics accumulated in a script stage counted events the time
// filter went on to discard, and a windowed run reported whole-file numbers.
// ---------------------------------------------------------------------------

/// Four events straddling two months, so a time window, a level filter and a
/// take limit each select a different, non-trivial subset.
const MATRIX_INPUT: &str = concat!(
    r#"{"t":"2024-01-01T00:00:00Z","id":"a","level":"error"}"#,
    "\n",
    r#"{"t":"2024-06-01T00:00:00Z","id":"b","level":"info"}"#,
    "\n",
    r#"{"t":"2024-06-02T00:00:00Z","id":"c","level":"error"}"#,
    "\n",
    r#"{"t":"2024-02-01T00:00:00Z","id":"d","level":"error"}"#,
    "\n",
);

/// Every prefilter kelora offers, as CLI fragments.
fn prefilters() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("--filter", vec!["--filter", "e.level == \"error\""]),
        ("--levels", vec!["-l", "error"]),
        ("--take", vec!["-n", "2"]),
        ("--since", vec!["--since", "2024-05-01"]),
        ("--until", vec!["--until", "2024-02-15"]),
        (
            "--since+--until",
            vec!["--since", "2024-01-15", "--until", "2024-06-01T12:00:00Z"],
        ),
    ]
}

/// Tally a field from the emitted event stream: the ground truth every
/// aggregate is measured against.
fn stream_counts(prefilter: &[&str], field: &str, extra: &[&str]) -> Vec<(String, usize)> {
    let mut args = vec!["-f", "json"];
    args.extend_from_slice(prefilter);
    args.extend_from_slice(extra);
    args.extend_from_slice(&["-k", field, "-F", "csvnh"]);

    let (stdout, stderr, code) = run_kelora_with_input(&args, MATRIX_INPUT);
    assert_eq!(code, 0, "event stream run failed: {stderr}");

    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        *counts.entry(line.to_string()).or_default() += 1;
    }
    counts.into_iter().collect()
}

/// Read a `track_freq` table back out of the tsv metrics stream
/// (`metric<TAB>key<TAB>count`).
fn freq_counts(prefilter: &[&str], field: &str, extra: &[&str]) -> Vec<(String, usize)> {
    let mut args = vec!["-f", "json"];
    args.extend_from_slice(prefilter);
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--freq", field, "--metrics=tsv"]);

    let (stdout, stderr, code) = run_kelora_with_input(&args, MATRIX_INPUT);
    assert_eq!(code, 0, "--freq run failed: {stderr}");

    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 3, "unexpected tsv metrics row: {line:?}");
        assert_eq!(cols[0], field, "unexpected metric name: {line:?}");
        let count: usize = cols[2].parse().expect("metric count should be an integer");
        *counts.entry(cols[1].to_string()).or_default() += count;
    }
    counts.into_iter().collect()
}

#[test]
fn test_every_prefilter_agrees_with_freq() {
    for (name, prefilter) in prefilters() {
        let stream = stream_counts(&prefilter, "level", &[]);
        let freq = freq_counts(&prefilter, "level", &[]);
        assert_eq!(
            freq, stream,
            "{name}: --freq must count exactly the events that reach the output \
             (`kelora --freq F` == `kelora -k F | sort | uniq -c`)"
        );
        assert!(
            !stream.is_empty(),
            "{name}: fixture should leave events for a meaningful comparison"
        );
    }
}

#[test]
fn test_every_prefilter_agrees_with_track_freq_in_exec() {
    // --freq is sugar for track_freq; an explicit script stage must agree too.
    for (name, prefilter) in prefilters() {
        let stream = stream_counts(&prefilter, "level", &[]);

        let mut args = vec!["-f", "json"];
        args.extend_from_slice(&prefilter);
        args.extend_from_slice(&[
            "--exec",
            "track_freq(\"level\", e.level)",
            "-m",
            "--metrics=tsv",
        ]);
        let (stdout, stderr, code) = run_kelora_with_input(&args, MATRIX_INPUT);
        assert_eq!(code, 0, "{name}: track_freq run failed: {stderr}");

        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 3, "{name}: unexpected tsv row: {line:?}");
            *counts.entry(cols[1].to_string()).or_default() +=
                cols[2].parse::<usize>().expect("integer count");
        }
        let tracked: Vec<(String, usize)> = counts.into_iter().collect();
        assert_eq!(
            tracked, stream,
            "{name}: track_freq in --exec must only see events that survive the prefilters"
        );
    }
}

#[test]
fn test_every_prefilter_agrees_with_drain_and_stats() {
    for (name, prefilter) in prefilters() {
        let expected: usize = stream_counts(&prefilter, "level", &[])
            .iter()
            .map(|(_, n)| n)
            .sum();

        // --drain: template counts must sum to the surviving event count.
        let mut drain_args = vec!["-f", "json"];
        drain_args.extend_from_slice(&prefilter);
        drain_args.extend_from_slice(&["--drain", "-k", "level"]);
        let (stdout, stderr, code) = run_kelora_with_input(&drain_args, MATRIX_INPUT);
        assert_eq!(code, 0, "{name}: --drain run failed: {stderr}");
        let drained: usize = stdout
            .lines()
            .filter_map(|l| l.trim().split(':').next()?.trim().parse::<usize>().ok())
            .sum();
        assert_eq!(
            drained, expected,
            "{name}: --drain template counts must sum to the surviving event count"
        );

        // --stats: the "output" figure must match too.
        let mut stats_args = vec!["-f", "json"];
        stats_args.extend_from_slice(&prefilter);
        stats_args.extend_from_slice(&["-k", "level", "-s"]);
        let (stdout, stderr, code) = run_kelora_with_input(&stats_args, MATRIX_INPUT);
        assert_eq!(code, 0, "{name}: --stats run failed: {stderr}");
        // `-s` is a data-only summary mode: the report goes to stdout.
        let events_line = stdout
            .lines()
            .map(str::trim)
            .find(|l| l.contains("Events created"))
            .unwrap_or_else(|| panic!("{name}: missing Events line in stats: {stdout}"));
        assert!(
            events_line.contains(&format!("{expected} output")),
            "{name}: --stats should report {expected} output, got: {events_line}"
        );
    }
}

#[test]
fn test_time_window_narrows_aggregates_in_parallel_mode() {
    // The parallel worker builds its own stage list; it must order the window
    // the same way, or -P would silently disagree with sequential mode.
    let stream = stream_counts(
        &["--since", "2024-05-01"],
        "level",
        &["-P", "--threads", "2"],
    );
    let freq = freq_counts(
        &["--since", "2024-05-01"],
        "level",
        &["-P", "--threads", "2"],
    );
    assert_eq!(
        freq, stream,
        "--freq under --parallel must respect the --since window"
    );
    assert_eq!(
        freq,
        vec![("error".to_string(), 1), ("info".to_string(), 1)],
        "only the two June events are in the window"
    );
}

#[test]
fn test_time_window_applies_before_script_stages() {
    // Direct statement of the ordering: an --exec side effect must not fire for
    // an event the time window excludes.
    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--since",
            "2024-05-01",
            "--exec",
            "print(\"saw \" + e.id)",
            "-k",
            "id",
        ],
        MATRIX_INPUT,
    );
    assert_eq!(code, 0, "run failed: {stderr}");
    assert!(
        !stdout.contains("saw a") && !stdout.contains("saw d"),
        "out-of-window events must not reach script stages: {stdout}"
    );
    assert!(
        stdout.contains("saw b") && stdout.contains("saw c"),
        "in-window events must still reach script stages: {stdout}"
    );
}

/// Events a script *creates* carry no parser timestamp, so the time window —
/// which has already run by the time they exist — does not apply to them at
/// all. This is a consequence of the window running ahead of the user stages:
/// while it ran on the way *out*, it saw `parsed_ts == None` on every emitted
/// event and skipped it in resilient mode, so `--since`/`--until` silently
/// destroyed every `emit_each` event, including in-window ones.
#[test]
fn test_emitted_events_are_not_subject_to_the_time_window() {
    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--since",
            "2024-05-01",
            "--exec",
            "emit_each([#{ts: \"2020-01-01T00:00:00Z\", id: \"synth\"}])",
            "-k",
            "id",
        ],
        MATRIX_INPUT,
    );
    assert_eq!(code, 0, "run failed: {stderr}");

    let synth = stdout.lines().filter(|l| l.contains("synth")).count();
    assert_eq!(
        synth, 2,
        "the two in-window events each emit one synthetic event, and an emitted \
         event survives the window regardless of its own timestamp: {stdout}"
    );
    assert!(
        !stdout.contains("id='a'") && !stdout.contains("id='d'"),
        "out-of-window input events must not reach the emitting stage: {stdout}"
    );
}

/// The recourse for narrowing emitted events by time: a `--filter` placed after
/// the emitting stage, which — unlike the window — is a user stage and so runs
/// where it is written.
#[test]
fn test_filter_after_emit_narrows_emitted_events_by_time() {
    let args = |threshold: &'static str| {
        vec![
            "-f",
            "json",
            "--since",
            "2024-05-01",
            "--exec",
            "emit_each([#{ts: \"2020-01-01T00:00:00Z\", id: \"synth\"}])",
            "--filter",
            threshold,
            "-k",
            "id",
        ]
    };

    let (dropped, stderr, code) = run_kelora_with_input(
        &args("to_datetime(e.ts) >= to_datetime(\"2024-05-01T00:00:00Z\")"),
        MATRIX_INPUT,
    );
    assert_eq!(code, 0, "run failed: {stderr}");
    assert!(
        !dropped.contains("synth"),
        "a filter after the emit stage must drop out-of-window emitted events: {dropped}"
    );

    let (kept, stderr, code) = run_kelora_with_input(
        &args("to_datetime(e.ts) >= to_datetime(\"2019-01-01T00:00:00Z\")"),
        MATRIX_INPUT,
    );
    assert_eq!(code, 0, "run failed: {stderr}");
    assert_eq!(
        kept.lines().filter(|l| l.contains("synth")).count(),
        2,
        "a threshold the emitted events satisfy must keep them: {kept}"
    );
}
