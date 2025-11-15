// tests/timestamp_filtering_tests.rs
use chrono::{Duration, Timelike, Utc};
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

/// Helper function to run kelora with given arguments and input via stdin
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    let binary_path = if cfg!(debug_assertions) {
        "./target/debug/kelora"
    } else {
        "./target/release/kelora"
    };

    let mut cmd = Command::new(binary_path)
        .args(args)
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

/// Helper function to run kelora with a temporary file
#[allow(dead_code)]
fn run_kelora_with_file(args: &[&str], file_content: &str) -> (String, String, i32) {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(file_content.as_bytes())
        .expect("Failed to write to temp file");

    let mut full_args = args.to_vec();
    full_args.push(temp_file.path().to_str().unwrap());

    let binary_path = if cfg!(debug_assertions) {
        "./target/debug/kelora"
    } else {
        "./target/release/kelora"
    };

    let output = Command::new(binary_path)
        .args(&full_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute kelora");

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

    // Set TZ=UTC for consistent test behavior regardless of system timezone
    std::env::set_var("TZ", "UTC");
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--since", &since_ts], &input);
    std::env::remove_var("TZ");

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
fn test_stats_report_custom_ts_field_failures() {
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","service":"api","message":"event"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-S", "--ts-field", "service"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains(
            "Timestamp: service (--ts-field) - 0/1 parsed (0.0%). Hint: Adjust --ts-format."
        ),
        "Stats should report the failure for the user-specified timestamp field.\nSTDERR:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Warning: --ts-field service values could not be parsed"),
        "Should emit a summary warning for the failed --ts-field override.\nSTDERR:\n{}",
        stderr
    );
}

#[test]
fn test_stats_report_custom_ts_format_failures() {
    let input = r#"{"timestamp":"not-a-timestamp","message":"event"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-S", "--ts-format", "%d"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains(
            "Timestamp: timestamp (auto-detected) - 0/1 parsed (0.0%). Hint: Try --ts-field or --ts-format."
        ),
        "Overall timestamp parsing should reflect the failure.\nSTDERR:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Warning: --ts-format '%d' did not match any timestamp values"),
        "Should emit a summary warning for the failed --ts-format override.\nSTDERR:\n{}",
        stderr
    );
}

#[test]
fn test_custom_ts_field_failure_strict_exits() {
    let input = r#"{"timestamp":"2024-01-15T10:00:00Z","service":"api","message":"event"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-S", "--ts-field", "service", "--strict"],
        input,
    );

    assert_eq!(
        exit_code, 1,
        "strict mode should cause non-zero exit on override failure"
    );
    assert!(
        stderr.contains("Warning: --ts-field service values could not be parsed"),
        "Strict mode should still display the warning in stats output.\nSTDERR:\n{}",
        stderr
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

// TODO: Update test for new stats format
// #[test]
// fn test_timestamp_filtering_with_stats() {
//     let old_ts = get_test_timestamp_iso(-60);
//     let new_ts = get_test_timestamp_iso(-30);
//
//     let input = format!(
//         r#"{{"ts": "{}", "level": "info", "msg": "old event"}}
// {{"ts": "{}", "level": "info", "msg": "new event"}}"#,
//         old_ts, new_ts
//     );
//
//     let since_ts = get_test_timestamp_iso(-45);
//     let (_stdout, stderr, exit_code) =
//         run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--stats"], &input);
//
//     assert_eq!(
//         exit_code, 0,
//         "kelora should exit successfully with stats. stderr: {}",
//         stderr
//     );
//     // Note: with --stats, output goes to stderr, not stdout
//     assert!(
//         stderr.contains("Events created: 2 total"),
//         "Should show 2 events created"
//     );
//     assert!(
//         stderr.contains("1 output, 1 filtered"),
//         "Should show 1 output and 1 filtered"
//     );
// }

// TODO: Update test for new stats format
// #[test]
// fn test_timestamp_filtering_stats_counts() {
//     let very_old_ts = get_test_timestamp_iso(-180); // 3 hours ago
//     let old_ts = get_test_timestamp_iso(-90); // 1.5 hours ago
//     let recent_ts = get_test_timestamp_iso(-30); // 30 minutes ago
//     let new_ts = get_test_timestamp_iso(-10); // 10 minutes ago
//
//     let input = format!(
//         r#"{{"ts": "{}", "level": "info", "msg": "very old"}}
// {{"ts": "{}", "level": "info", "msg": "old"}}
// {{"ts": "{}", "level": "info", "msg": "recent"}}
// {{"ts": "{}", "level": "info", "msg": "new"}}"#,
//         very_old_ts, old_ts, recent_ts, new_ts
//     );
//
//     // Filter to only include events from the last hour
//     let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
//     let (_stdout, stderr, exit_code) =
//         run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--stats"], &input);
//
//     assert_eq!(
//         exit_code, 0,
//         "kelora should exit successfully. stderr: {}",
//         stderr
//     );
//
//     // Should have 4 events created total
//     assert!(
//         stderr.contains("Events created: 4 total"),
//         "Should show 4 events created total"
//     );
//
//     // Should have 2 events output (recent and new), 2 filtered (very old and old)
//     assert!(
//         stderr.contains("2 output, 2 filtered"),
//         "Should show 2 output and 2 filtered"
//     );
//
//     // Lines should all be processed (none filtered at line level)
//     assert!(
//         stderr.contains("Lines processed: 4 total, 0 filtered"),
//         "Should show 4 lines processed, 0 filtered"
//     );
// }

// TODO: Update test for new stats format
// #[test]
// fn test_timestamp_filtering_stats_with_mixed_timestamps() {
//     let old_ts = get_test_timestamp_iso(-90); // 1.5 hours ago
//     let recent_ts = get_test_timestamp_iso(-30); // 30 minutes ago
//
//     let input = format!(
//         r#"{{"ts": "{}", "level": "info", "msg": "old with timestamp"}}
// {{"level": "info", "msg": "no timestamp event"}}
// {{"ts": "{}", "level": "info", "msg": "recent with timestamp"}}
// {{"random_field": "value", "msg": "another no timestamp"}}"#,
//         old_ts, recent_ts
//     );
//
//     // Filter to only include events from the last hour
//     let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
//     let (_stdout, stderr, exit_code) =
//         run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--stats"], &input);
//
//     assert_eq!(
//         exit_code, 0,
//         "kelora should exit successfully. stderr: {}",
//         stderr
//     );
//
//     // Should have 4 events created total
//     assert!(
//         stderr.contains("Events created: 4 total"),
//         "Should show 4 events created total"
//     );
//
//     // Should have 3 events output (recent + 2 without timestamps), 1 filtered (old)
//     assert!(
//         stderr.contains("3 output, 1 filtered"),
//         "Should show 3 output and 1 filtered"
//     );
// }

// TODO: Update test for new stats format
// #[test]
// fn test_timestamp_filtering_stats_all_filtered() {
//     let old_ts1 = get_test_timestamp_iso(-180); // 3 hours ago
//     let old_ts2 = get_test_timestamp_iso(-120); // 2 hours ago
//
//     let input = format!(
//         r#"{{"ts": "{}", "level": "info", "msg": "old event 1"}}
// {{"ts": "{}", "level": "info", "msg": "old event 2"}}"#,
//         old_ts1, old_ts2
//     );
//
//     // Filter to only include events from the last 30 minutes
//     let since_ts = get_test_timestamp_iso(-30); // 30 minutes ago
//     let (_stdout, stderr, exit_code) =
//         run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--stats"], &input);
//
//     assert_eq!(
//         exit_code, 0,
//         "kelora should exit successfully. stderr: {}",
//         stderr
//     );
//
//     // Should have 2 events created total
//     assert!(
//         stderr.contains("Events created: 2 total"),
//         "Should show 2 events created total"
//     );
//
//     // Should have 0 events output, 2 filtered
//     assert!(
//         stderr.contains("0 output, 2 filtered"),
//         "Should show 0 output and 2 filtered"
//     );
// }

// TODO: Update test for new stats format
// #[test]
// fn test_timestamp_filtering_stats_none_filtered() {
//     let recent_ts1 = get_test_timestamp_iso(-20); // 20 minutes ago
//     let recent_ts2 = get_test_timestamp_iso(-10); // 10 minutes ago
//
//     let input = format!(
//         r#"{{"ts": "{}", "level": "info", "msg": "recent event 1"}}
// {{"ts": "{}", "level": "info", "msg": "recent event 2"}}"#,
//         recent_ts1, recent_ts2
//     );
//
//     // Filter to include events from the last hour (should include all)
//     let since_ts = get_test_timestamp_iso(-60); // 1 hour ago
//     let (_stdout, stderr, exit_code) =
//         run_kelora_with_input(&["-f", "json", "--since", &since_ts, "--stats"], &input);
//
//     assert_eq!(
//         exit_code, 0,
//         "kelora should exit successfully. stderr: {}",
//         stderr
//     );
//
//     // Should have 2 events created total
//     assert!(
//         stderr.contains("Events created: 2 total"),
//         "Should show 2 events created total"
//     );
//
//     // Should have 2 events output, 0 filtered
//     assert!(
//         stderr.contains("2 output, 0 filtered"),
//         "Should show 2 output and 0 filtered"
//     );
// }

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
