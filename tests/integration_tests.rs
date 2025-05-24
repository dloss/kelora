// tests/integration_tests.rs
use std::io::Write;
use std::process::{Command, Stdio};
use std::fs;
use tempfile::NamedTempFile;

/// Helper function to run kelora with given arguments and input
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    let mut cmd = Command::new("cargo")
        .arg("run")
        .arg("--")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    // Write input to stdin
    if let Some(stdin) = cmd.stdin.as_mut() {
        stdin.write_all(input.as_bytes()).expect("Failed to write to stdin");
    }

    let output = cmd.wait_with_output().expect("Failed to read output");
    
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1)
    )
}

/// Helper function to run kelora with a temporary file
fn run_kelora_with_file(args: &[&str], file_content: &str) -> (String, String, i32) {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file.write_all(file_content.as_bytes()).expect("Failed to write to temp file");
    
    let mut full_args = args.to_vec();
    full_args.push(temp_file.path().to_str().unwrap());
    
    let mut cmd = Command::new("cargo")
        .arg("run")
        .arg("--")
        .args(&full_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    let output = cmd.wait_with_output().expect("Failed to read output");
    
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1)
    )
}

#[test]
fn test_basic_jsonl_parsing() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Database connection failed","host":"db.example.com"}
{"timestamp":"2023-07-18T15:04:25.789Z","level":"INFO","message":"Retrying connection","retry_count":1}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("level=\"ERROR\""), "Should contain ERROR level");
    assert!(stdout.contains("message=\"Database connection failed\""), "Should contain error message");
    assert!(stdout.contains("level=\"INFO\""), "Should contain INFO level");
    assert!(stdout.contains("retry_count=1"), "Should contain retry count");
}

#[test]
fn test_basic_logfmt_parsing() {
    let input = r#"timestamp="2023-07-18T15:04:23.456Z" level=ERROR message="Database connection failed" host="db.example.com"
timestamp="2023-07-18T15:04:25.789Z" level=INFO message="Retrying connection" retry_count=1"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "logfmt"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("level=\"ERROR\""), "Should contain ERROR level");
    assert!(stdout.contains("message=\"Database connection failed\""), "Should contain error message");
    assert!(stdout.contains("level=\"INFO\""), "Should contain INFO level");
    assert!(stdout.contains("retry_count=1"), "Should contain retry count");
}

#[test]
fn test_key_filtering() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Database failed","host":"db.example.com","port":5432}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-k", "timestamp,level,message"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("timestamp=\"2023-07-18T15:04:23.456Z\""), "Should contain timestamp");
    assert!(stdout.contains("level=\"ERROR\""), "Should contain level");
    assert!(stdout.contains("message=\"Database failed\""), "Should contain message");
    // Should not contain filtered out fields
    assert!(!stdout.contains("host="), "Should not contain host field");
    assert!(!stdout.contains("port="), "Should not contain port field");
}

#[test]
fn test_common_fields_flag() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Database failed","host":"db.example.com","port":5432}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-c"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("timestamp=\"2023-07-18T15:04:23.456Z\""), "Should contain timestamp");
    assert!(stdout.contains("level=\"ERROR\""), "Should contain level");
    assert!(stdout.contains("message=\"Database failed\""), "Should contain message");
    // Should not contain other fields
    assert!(!stdout.contains("host="), "Should not contain host field");
    assert!(!stdout.contains("port="), "Should not contain port field");
}

#[test]
fn test_level_filtering() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Error occurred"}
{"timestamp":"2023-07-18T15:04:24.456Z","level":"INFO","message":"Info message"}
{"timestamp":"2023-07-18T15:04:25.456Z","level":"DEBUG","message":"Debug message"}
{"timestamp":"2023-07-18T15:04:26.456Z","level":"WARN","message":"Warning message"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-l", "ERROR,WARN"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("Error occurred"), "Should contain ERROR message");
    assert!(stdout.contains("Warning message"), "Should contain WARN message");
    assert!(!stdout.contains("Info message"), "Should not contain INFO message");
    assert!(!stdout.contains("Debug message"), "Should not contain DEBUG message");
}

#[test]
fn test_stats_only_mode() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Error 1"}
{"timestamp":"2023-07-18T15:04:24.456Z","level":"INFO","message":"Info 1"}
{"timestamp":"2023-07-18T15:04:25.456Z","level":"ERROR","message":"Error 2"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-S"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.is_empty(), "Stdout should be empty in stats-only mode");
    assert!(stderr.contains("Events shown: 3"), "Should show event count");
    assert!(stderr.contains("ERROR(2)"), "Should show ERROR count");
    assert!(stderr.contains("INFO(1)"), "Should show INFO count");
}

#[test]
fn test_stats_with_output() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Error 1"}
{"timestamp":"2023-07-18T15:04:24.456Z","level":"INFO","message":"Info 1"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-s"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(!stdout.is_empty(), "Stdout should contain log output");
    assert!(stdout.contains("Error 1"), "Should contain log messages");
    assert!(stderr.contains("Events shown: 2"), "Should show stats in stderr");
}

#[test]
fn test_jsonl_output_format() {
    let input = r#"timestamp="2023-07-18T15:04:23.456Z" level=ERROR message="Test message""#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "logfmt", "-F", "jsonl"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    
    // Parse the JSON output to verify it's valid
    let json_line = stdout.trim();
    let parsed: serde_json::Value = serde_json::from_str(json_line)
        .expect("Output should be valid JSON");
    
    assert_eq!(parsed["level"], "ERROR");
    assert_eq!(parsed["message"], "Test message");
    assert_eq!(parsed["timestamp"], "2023-07-18T15:04:23.456Z");
}

#[test]
fn test_syslog_parsing() {
    let input = r#"<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick on /dev/pts/8
<13>Oct 11 22:14:15 mymachine myapp[1234]: Application started successfully"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "syslog"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("hostname=\"mymachine\""), "Should extract hostname");
    assert!(stdout.contains("process=\"su\""), "Should extract process name");
    assert!(stdout.contains("process=\"myapp\""), "Should extract app name");
    assert!(stdout.contains("pid=1234"), "Should extract PID");
}

#[test]
fn test_file_input() {
    let content = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"File test"}
{"timestamp":"2023-07-18T15:04:24.456Z","level":"ERROR","message":"File error"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_file(&["-f", "jsonl"], content);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("File test"), "Should contain first message");
    assert!(stdout.contains("File error"), "Should contain second message");
}

#[test]
fn test_empty_input() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], "");
    
    assert_eq!(exit_code, 0, "kelora should handle empty input gracefully");
    assert!(stdout.is_empty(), "No output expected for empty input");
}

#[test]
fn test_malformed_json() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"Good line"}
{"malformed": json line}
{"timestamp":"2023-07-18T15:04:25.456Z","level":"INFO","message":"Another good line"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], input);
    
    assert_eq!(exit_code, 0, "kelora should handle parse errors gracefully");
    assert!(stdout.contains("Good line"), "Should contain valid lines");
    assert!(stdout.contains("Another good line"), "Should continue after errors");
    // Note: parse errors are ignored by default, but we could test with --debug flag
}

#[test]
fn test_debug_mode_with_errors() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"Good line"}
{"malformed": json line}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "--debug"], input);
    
    assert_eq!(exit_code, 0, "kelora should handle parse errors gracefully even in debug mode");
    assert!(stdout.contains("Good line"), "Should contain valid lines");
    assert!(stderr.contains("Parse error"), "Should show parse error in debug mode");
}

#[test]
fn test_mixed_formats_logfmt() {
    let input = r#"level=info msg="Simple message"
timestamp="2023-07-18T15:04:23.456Z" level=error message="Complex message" count=42 flag=true
empty_value= quoted_empty="" null_value=null"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "logfmt"], input);
    
    assert_eq!(exit_code, 0, "kelora should handle various logfmt patterns");
    assert!(stdout.contains("level=\"info\""), "Should handle simple logfmt");
    assert!(stdout.contains("count=42"), "Should handle numbers");
    assert!(stdout.contains("flag=true"), "Should handle booleans");
    assert!(stdout.contains("null_value=null"), "Should handle null values");
}

#[test]
fn test_case_insensitive_level_filtering() {
    let input = r#"{"level":"error","message":"Lowercase error"}
{"level":"ERROR","message":"Uppercase error"}
{"level":"Error","message":"Mixed case error"}
{"level":"info","message":"Info message"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-l", "error"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // All error variants should be included (case insensitive matching)
    assert!(stdout.contains("Lowercase error"), "Should match lowercase");
    assert!(stdout.contains("Uppercase error"), "Should match uppercase");
    assert!(stdout.contains("Mixed case error"), "Should match mixed case");
    assert!(!stdout.contains("Info message"), "Should not match info");
}

#[test]
fn test_timestamp_parsing_and_display() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"Test with timestamp"}
{"ts":"2023-07-18 15:04:24","level":"INFO","message":"Test with ts field"}
{"time":"Jul 18 15:04:25","level":"INFO","message":"Test with time field"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-k", "timestamp,message"], input);
    
    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(stdout.contains("timestamp=\"2023-07-18T15:04:23.456Z\""), "Should preserve ISO timestamp");
    // Note: timestamp parsing and normalization might be implementation dependent
}

#[test]
fn test_help_flag() {
    let mut cmd = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    let output = cmd.wait_with_output().expect("Failed to read output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    assert_eq!(output.status.code().unwrap_or(-1), 0, "Help should exit successfully");
    assert!(stdout.contains("kelora"), "Help should mention kelora");
    assert!(stdout.contains("log parser"), "Help should mention log parser");
}

#[test]
fn test_version_flag() {
    let mut cmd = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    let output = cmd.wait_with_output().expect("Failed to read output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    assert_eq!(output.status.code().unwrap_or(-1), 0, "Version should exit successfully");
    assert!(stdout.contains("0.1.0"), "Should show version number");
}

// Performance and edge case tests

#[test]
fn test_large_input_handling() {
    // Generate a reasonably large input to test memory handling
    let mut input = String::new();
    for i in 0..1000 {
        input.push_str(&format!(
            r#"{{"timestamp":"2023-07-18T15:04:{:02}.456Z","level":"INFO","message":"Message {}","id":{}}}"#,
            i % 60, i, i
        ));
        input.push('\n');
    }

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl", "-S"], &input);
    
    assert_eq!(exit_code, 0, "kelora should handle large input");
    assert!(stderr.contains("Events shown: 1000"), "Should process all 1000 events");
}

#[test]
fn test_very_long_lines() {
    let long_message = "a".repeat(10000);
    let input = format!(r#"{{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"{}"}}"#, long_message);

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], &input);
    
    assert_eq!(exit_code, 0, "kelora should handle very long lines");
    assert!(stdout.contains(&long_message[..100]), "Should contain part of long message");
}

#[test]
fn test_unicode_handling() {
    let input = r#"{"timestamp":"2023-07-18T15:04:23.456Z","level":"INFO","message":"Unicode test: 你好世界 🚀 café naïve résumé"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], input);
    
    assert_eq!(exit_code, 0, "kelora should handle unicode");
    assert!(stdout.contains("你好世界"), "Should preserve Chinese characters");
    assert!(stdout.contains("🚀"), "Should preserve emoji");
    assert!(stdout.contains("café"), "Should preserve accented characters");
}