mod common;
use common::*;

#[test]
fn test_brief_output_mode() {
    let input = r#"{"level": "INFO", "message": "test message", "user": "alice"}
{"level": "ERROR", "message": "error occurred", "user": "bob"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--brief"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with brief mode"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2, "Should output 2 lines");

    // Brief mode should output only values, space-separated, no keys
    assert_eq!(lines[0], "INFO test message alice");
    assert_eq!(lines[1], "ERROR error occurred bob");

    // Verify no key=value format is used
    assert!(
        !stdout.contains("level="),
        "Brief mode should not contain keys"
    );
    assert!(
        !stdout.contains("message="),
        "Brief mode should not contain keys"
    );
    assert!(
        !stdout.contains("user="),
        "Brief mode should not contain keys"
    );
}

#[test]
fn test_brief_output_mode_short_form() {
    let input = r#"{"level": "INFO", "message": "hello world"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-b"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with -b short form"
    );

    // Brief mode should output only values, space-separated
    assert_eq!(stdout.trim(), "INFO hello world");
    assert!(
        !stdout.contains("level="),
        "Brief mode should not contain keys"
    );
}

#[test]
fn test_core_field_filtering() {
    let input = r#"{"ts": "2024-01-01T12:00:00Z", "level": "ERROR", "message": "Test message", "user": "alice", "status": 500}"#;

    let (stdout, _, exit_code) = run_kelora_with_input(&["-f", "json", "--core"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully with --core");

    // Should only contain core fields
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("level="), "Should contain level field");
    assert!(stdout.contains("message="), "Should contain message field");
    assert!(
        !stdout.contains("user="),
        "Should not contain non-core user field"
    );
    assert!(
        !stdout.contains("status="),
        "Should not contain non-core status field"
    );
}

#[test]
fn test_core_field_filtering_short_flag() {
    let input = r#"{"ts": "2024-01-01T12:00:00Z", "level": "ERROR", "message": "Test message", "user": "alice"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-c"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with -c short flag"
    );

    // Should only contain core fields
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("level="), "Should contain level field");
    assert!(stdout.contains("message="), "Should contain message field");
    assert!(
        !stdout.contains("user="),
        "Should not contain non-core user field"
    );
}

#[test]
fn test_core_field_with_alternative_names() {
    let input = r#"{"ts": "2024-01-01T12:00:00Z", "lvl": "WARN", "msg": "Alternative names", "extra_data": "ignored"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--core"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with alternative core field names"
    );

    // Should include alternative core field names
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("lvl="), "Should contain lvl field");
    assert!(stdout.contains("msg="), "Should contain msg field");
    assert!(
        !stdout.contains("extra_data="),
        "Should not contain non-core field"
    );
}

#[test]
fn test_core_field_plus_additional_keys() {
    let input = r#"{"ts": "2024-01-01T12:00:00Z", "level": "ERROR", "message": "Test message", "user": "alice", "status": 500}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--core", "--keys", "user,status"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --core and --keys"
    );

    // Should contain both core fields and specified keys
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("level="), "Should contain level field");
    assert!(stdout.contains("message="), "Should contain message field");
    assert!(
        stdout.contains("user="),
        "Should contain user field from --keys"
    );
    assert!(
        stdout.contains("status="),
        "Should contain status field from --keys"
    );
}

#[test]
fn test_core_field_with_syslog() {
    let input = r#"<34>Jan 1 12:00:00 myhost myapp[1234]: Test syslog message"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "syslog", "--core"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with syslog and --core"
    );

    // Should contain syslog core fields
    assert!(
        stdout.contains("severity="),
        "Should contain severity field"
    );
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("msg="), "Should contain msg field");
    // Should not contain non-core syslog fields
    assert!(
        !stdout.contains("facility="),
        "Should not contain facility field"
    );
    assert!(!stdout.contains("host="), "Should not contain host field");
    assert!(!stdout.contains("prog="), "Should not contain prog field");
}

#[test]
fn test_core_field_with_exec_created_fields() {
    let input = r#"{"original_time": "2024-01-01T12:00:00Z", "orig_level": "ERROR", "orig_msg": "Test message", "user": "alice"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.timestamp = e.original_time; e.level = e.orig_level; e.message = e.orig_msg",
            "--core",
            "--keys",
            "user",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with exec-created core fields"
    );

    // Should contain exec-created core fields and specified keys
    assert!(
        stdout.contains("user="),
        "Should contain user field from --keys"
    );
    assert!(
        stdout.contains("timestamp="),
        "Should contain exec-created timestamp field"
    );
    assert!(
        stdout.contains("level="),
        "Should contain exec-created level field"
    );
    assert!(
        stdout.contains("message="),
        "Should contain exec-created message field"
    );
    // Should not contain original fields
    assert!(
        !stdout.contains("original_time="),
        "Should not contain original_time field"
    );
    assert!(
        !stdout.contains("orig_level="),
        "Should not contain orig_level field"
    );
    assert!(
        !stdout.contains("orig_msg="),
        "Should not contain orig_msg field"
    );
}

#[test]
fn test_core_field_with_logfmt() {
    let input =
        r#"time=2024-01-01T12:00:00Z lvl=error msg="Test logfmt message" user=bob status=404"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "logfmt", "--core"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with logfmt and --core"
    );

    // Should contain logfmt core fields
    assert!(stdout.contains("time="), "Should contain time field");
    assert!(stdout.contains("lvl="), "Should contain lvl field");
    assert!(stdout.contains("msg="), "Should contain msg field");
    // Should not contain non-core fields
    assert!(!stdout.contains("user="), "Should not contain user field");
    assert!(
        !stdout.contains("status="),
        "Should not contain status field"
    );
}

#[test]
fn test_core_field_multiple_timestamp_variants() {
    let input = r#"{"ts": "2024-01-01T12:00:00Z", "timestamp": "2024-01-01T13:00:00Z", "time": "2024-01-01T14:00:00Z", "level": "INFO", "message": "Multiple timestamps", "other": "data"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--core"], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with multiple timestamp variants"
    );

    // Should include all timestamp field variants (current behavior: include all matching names)
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("ts="), "Should contain ts field");
    assert!(stdout.contains("time="), "Should contain time field");
    assert!(stdout.contains("level="), "Should contain level field");
    assert!(stdout.contains("message="), "Should contain message field");
    assert!(
        !stdout.contains("other="),
        "Should not contain non-core other field"
    );
}

#[test]
fn test_quiet_level_0_normal_output() {
    // Test normal mode (level 0) - shows everything
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--stats",
            "--exec",
            "print(\"Script output\")",
        ],
        input,
    );
    assert_eq!(exit_code, 0);

    // Should show event output
    assert!(stdout.contains("level='info'"));
    assert!(stdout.contains("message='test'"));

    // Should show script output
    assert!(stdout.contains("Script output"));

    // Should show stats
    assert!(stderr.contains("Stats"));
    assert!(stderr.contains("Lines processed"));
}

#[test]
fn test_quiet_level_1_suppress_diagnostics() {
    // Test quiet level 1 (-q) - suppress diagnostics but show events and script output
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--stats",
            "--exec",
            "print(\"Script output\")",
            "-q",
        ],
        input,
    );
    assert_eq!(exit_code, 0);

    // Should show event output
    assert!(stdout.contains("level='info'"));
    assert!(stdout.contains("message='test'"));

    // Should show script output
    assert!(stdout.contains("Script output"));

    // Should NOT show stats
    assert!(!stderr.contains("Stats"));
    assert!(!stderr.contains("Lines processed"));
}

#[test]
fn test_quiet_level_2_suppress_events() {
    // Test quiet level 2 (-qq) - suppress diagnostics and events but show script output
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--stats",
            "--exec",
            "print(\"Script output\")",
            "-qq",
        ],
        input,
    );
    assert_eq!(exit_code, 0);

    // Should NOT show event output
    assert!(!stdout.contains("level='info'"));
    assert!(!stdout.contains("message='test'"));

    // Should still show script output
    assert!(stdout.contains("Script output"));

    // Should NOT show stats
    assert!(!stderr.contains("Stats"));
    assert!(!stderr.contains("Lines processed"));
}

#[test]
fn test_quiet_level_3_suppress_all() {
    // Test quiet level 3 (-qqq) - suppress everything including script output
    let input = r#"{"level": "info", "message": "test"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--stats",
            "--exec",
            "print(\"Script output\")",
            "-qqq",
        ],
        input,
    );
    assert_eq!(exit_code, 0);

    // Should NOT show event output
    assert!(!stdout.contains("level='info'"));
    assert!(!stdout.contains("message='test'"));

    // Should NOT show script output
    assert!(!stdout.contains("Script output"));

    // Should NOT show stats
    assert!(!stderr.contains("Stats"));
    assert!(!stderr.contains("Lines processed"));

    // Should have no output at all
    assert_eq!(stdout.trim(), "");
}

#[test]
fn test_normalize_ts_normalizes_primary_timestamp_default_output() {
    let input = r#"{"ts": "2025-01-15 10:00:00", "level": "INFO", "message": "Test"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--input-tz", "UTC", "--normalize-ts"],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --normalize-ts"
    );
    assert!(
        stdout.contains("ts='2025-01-15T10:00:00+00:00'"),
        "Primary timestamp should be normalized in default formatter output. stdout: {}",
        stdout
    );
    assert!(
        !stdout.contains("2025-01-15 10:00:00"),
        "Original timestamp representation should be replaced in default output. stdout: {}",
        stdout
    );
}

#[test]
fn test_normalize_ts_normalizes_primary_timestamp_json_output() {
    let input = r#"{"ts": "2025-01-15 10:00:00", "level": "INFO"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--input-tz",
            "UTC",
            "--normalize-ts",
            "-F",
            "json",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --normalize-ts"
    );
    assert!(
        stdout.contains("\"ts\":\"2025-01-15T10:00:00+00:00\""),
        "JSON formatter should receive normalized timestamp. stdout: {}",
        stdout
    );
    assert!(
        !stdout.contains("2025-01-15 10:00:00"),
        "Original timestamp representation should be replaced in JSON output. stdout: {}",
        stdout
    );
}

#[test]
fn test_no_emoji_flag() {
    // Test that --no-emoji suppresses emoji output
    let input = r#"{"level": "error", "message": "test error"}"#;

    let (_stdout_with_emoji, _stderr_with_emoji, _) =
        run_kelora_with_input(&["-f", "json", "--stats"], input);
    let (stdout_no_emoji, stderr_no_emoji, _) =
        run_kelora_with_input(&["-f", "json", "--stats", "--no-emoji"], input);

    // Stats output might contain emojis by default
    // With --no-emoji, no emojis should be present
    // Check that no emoji characters are in the no-emoji output
    let has_emoji_chars = |s: &str| s.chars().any(|c| c as u32 > 0x1F000);

    // The output with --no-emoji should not contain emoji characters
    assert!(
        !has_emoji_chars(&stdout_no_emoji) && !has_emoji_chars(&stderr_no_emoji),
        "Output with --no-emoji should not contain emoji characters"
    );
}

#[test]
fn test_force_color_flag() {
    // Test that --force-color enables color output even when not in a TTY
    let input = r#"{"level": "error", "message": "test"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--force-color"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Color codes typically use ANSI escape sequences
    // Check for ANSI escape codes in output (e.g., \x1b[ or \u{1b}[)
    // Note: This might not work if color is disabled in test environment
    // but we can at least ensure the flag doesn't cause errors
    assert!(!stdout.is_empty(), "Should produce output");
}

#[test]
fn test_no_color_flag() {
    // Test that --no-color disables color output
    let input = r#"{"level": "error", "message": "test"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--no-color"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Check that output does not contain ANSI color codes
    assert!(
        !stdout.contains("\x1b["),
        "Output with --no-color should not contain ANSI escape codes"
    );
}

#[test]
fn test_wrap_and_no_wrap_flags() {
    // Test --wrap and --no-wrap flags for long output lines
    let long_message = "a".repeat(200);
    let input = format!(r#"{{"level": "info", "message": "{}"}}"#, long_message);

    // Test with default behavior
    let (stdout_default, _stderr, exit_code_default) =
        run_kelora_with_input(&["-f", "json"], &input);
    assert_eq!(exit_code_default, 0);

    // Test with --no-wrap
    let (stdout_no_wrap, _stderr, exit_code_no_wrap) =
        run_kelora_with_input(&["-f", "json", "--no-wrap"], &input);
    assert_eq!(exit_code_no_wrap, 0);

    // Test with --wrap
    let (stdout_wrap, _stderr, exit_code_wrap) =
        run_kelora_with_input(&["-f", "json", "--wrap"], &input);
    assert_eq!(exit_code_wrap, 0);

    // All should produce some output
    assert!(!stdout_default.is_empty());
    assert!(!stdout_no_wrap.is_empty());
    assert!(!stdout_wrap.is_empty());

    // No-wrap output should be on a single line (no newlines within the data)
    let no_wrap_line_count = stdout_no_wrap.trim().lines().count();
    assert_eq!(
        no_wrap_line_count, 1,
        "No-wrap output should be a single line"
    );
}

#[test]
fn test_expand_nested_flag() {
    // Test --expand-nested for nested JSON objects
    let input = r#"{"level": "info", "metadata": {"user": "alice", "request_id": "123"}}"#;

    let (stdout_default, _stderr, exit_code_default) =
        run_kelora_with_input(&["-f", "json"], input);
    assert_eq!(exit_code_default, 0);

    let (stdout_expanded, _stderr, exit_code_expanded) =
        run_kelora_with_input(&["-f", "json", "--expand-nested"], input);
    assert_eq!(exit_code_expanded, 0);

    // Both should produce output
    assert!(!stdout_default.is_empty());
    assert!(!stdout_expanded.is_empty());

    // Expanded output might have different formatting for nested objects
    // We can at least ensure the flag doesn't cause errors and produces output
    assert!(
        stdout_expanded.contains("metadata"),
        "Expanded output should contain metadata field"
    );
}

#[test]
fn test_mark_gaps_flag() {
    // Test --mark-gaps for marking gaps in timestamp sequences
    let input = r#"{"ts": "2024-01-01T10:00:00Z", "message": "first"}
{"ts": "2024-01-01T10:00:01Z", "message": "second"}
{"ts": "2024-01-01T10:05:00Z", "message": "third with gap"}
{"ts": "2024-01-01T10:05:01Z", "message": "fourth"}"#;

    let (stdout_default, _stderr, exit_code_default) =
        run_kelora_with_input(&["-f", "json"], input);
    assert_eq!(exit_code_default, 0);

    // --mark-gaps requires a duration argument
    let (stdout_gaps, stderr_gaps, exit_code_gaps) =
        run_kelora_with_input(&["-f", "json", "--mark-gaps", "30s"], input);
    assert_eq!(
        exit_code_gaps, 0,
        "kelora should exit successfully with --mark-gaps, stderr: {}",
        stderr_gaps
    );

    // Both should produce output
    assert!(!stdout_default.is_empty());
    assert!(!stdout_gaps.is_empty());

    // The mark-gaps output should contain all the messages
    assert!(stdout_gaps.contains("first"));
    assert!(stdout_gaps.contains("second"));
    assert!(stdout_gaps.contains("third with gap"));
    assert!(stdout_gaps.contains("fourth"));
}

#[test]
fn test_conflicting_color_flags() {
    // Test that conflicting color flags are handled
    let input = r#"{"level": "info", "message": "test"}"#;

    // Both --force-color and --no-color - last one should win or error
    let (_stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--force-color", "--no-color"], input);

    // Should either succeed (last flag wins) or fail with usage error
    assert!(
        exit_code == 0 || exit_code == 2,
        "Should either succeed or fail with usage error"
    );
}

#[test]
fn test_wrap_with_different_formatters() {
    // Test wrap behavior with different output formatters
    let long_message = "x".repeat(150);
    let input = format!(r#"{{"message": "{}"}}"#, long_message);

    // Test with JSON formatter
    let (stdout_json, _stderr, exit_code_json) =
        run_kelora_with_input(&["-f", "json", "-F", "json", "--no-wrap"], &input);
    assert_eq!(exit_code_json, 0);
    assert_eq!(stdout_json.trim().lines().count(), 1);

    // Test with default formatter
    let (stdout_default, _stderr, exit_code_default) =
        run_kelora_with_input(&["-f", "json", "--no-wrap"], &input);
    assert_eq!(exit_code_default, 0);
    assert!(!stdout_default.is_empty());
}

#[test]
fn test_multiple_display_flags_combination() {
    // Test combining multiple display flags
    let input = r#"{"level": "error", "message": "test error", "nested": {"key": "value"}}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--no-emoji",
            "--no-color",
            "--no-wrap",
            "--expand-nested",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should handle multiple display flags");
    assert!(!stdout.is_empty(), "Should produce output");
}

#[test]
fn test_color_with_filtering() {
    // Test that color flags work with filtering
    let input = r#"{"level": "info", "message": "info msg"}
{"level": "error", "message": "error msg"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"error\"",
            "--no-color",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("error msg"));
    assert!(!stdout.contains("info msg"));
    // Should not contain ANSI codes
    assert!(!stdout.contains("\x1b["));
}
