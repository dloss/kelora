mod common;
use common::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

#[test]
fn test_explicit_stdin_with_dash() {
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-"], input);

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("test1"));
    assert!(stdout.contains("test2"));
    assert!(stdout.contains("test3"));
}

#[test]
fn test_missing_file_reports_name() {
    let missing = "tests/data/file_should_not_exist_12345.log";
    assert!(
        !Path::new(missing).exists(),
        "Test assumes missing file does not exist"
    );

    let (_stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "json"], &[missing]);

    assert_ne!(exit_code, 0, "Should fail when file is missing");
    assert!(
        stderr.contains(missing),
        "stderr should mention missing filename: {}",
        stderr
    );
}

#[test]
fn test_quoted_glob_reports_shell_expansion_hint() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_files(&["-f", "json"], &["examples/*.jsonl"]);

    assert_ne!(
        exit_code, 0,
        "quoted glob should fail as a literal missing file"
    );
    assert!(
        stderr.contains("Shell glob patterns must be expanded by the shell"),
        "stderr should explain quoted glob behavior: {}",
        stderr
    );
}

#[test]
fn test_missing_file_is_in_error_summary() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_file, "ok").expect("Failed to write temp file");

    let missing = "tests/data/file_should_not_exist_98765.log";
    assert!(
        !Path::new(missing).exists(),
        "Test assumes missing file does not exist"
    );

    let (_stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "line"],
        &[temp_file.path().to_str().unwrap(), missing],
    );

    assert_ne!(exit_code, 0, "Should fail when file is missing");
    assert!(
        stderr.contains("file failed to open"),
        "Error summary should mention failed file open: {}",
        stderr
    );
    assert!(
        stderr.contains(missing),
        "Error summary should include missing filename: {}",
        stderr
    );
}

#[test]
fn test_stdin_mixed_with_files() {
    // Create a temporary file
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(b"{\"level\": \"debug\", \"message\": \"from file\"}\n")
        .expect("Failed to write to temp file");

    let stdin_input = r#"{"level": "info", "message": "from stdin"}"#;

    // Test file first, then stdin
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kelora"))
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json", temp_file.path().to_str().unwrap(), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    if let Some(stdin) = cmd.stdin.as_mut() {
        stdin
            .write_all(stdin_input.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = cmd.wait_with_output().expect("Failed to read output");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("from file"));
    assert!(stdout.contains("from stdin"));
}

#[test]
fn test_multiple_stdin_rejected() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-", "-"], "test");

    assert_ne!(exit_code, 0);
    assert!(stderr.contains("stdin (\"-\") can only be specified once"));
    assert!(stdout.is_empty());
}

#[test]
fn test_stdin_large_input_performance() {
    // Generate 1000 log entries to test performance
    let mut large_input = String::new();
    for i in 1..=1000 {
        large_input.push_str(&format!(
            "{{\"user\":\"user{}\",\"status\":{},\"message\":\"Message {}\",\"id\":{}}}\n",
            i,
            200 + (i % 300),
            i,
            i
        ));
    }

    let start_time = std::time::Instant::now();
    let (stdout, _, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.status >= 400",
            "--exec",
            "track_sum(\"errors\", 1);",
            "--end",
            "print(`Errors: ${metrics[\"errors\"]}`);",
        ],
        &large_input,
    );
    let duration = start_time.elapsed();

    assert_eq!(
        exit_code, 0,
        "kelora should handle large input successfully"
    );
    assert!(
        stdout.contains("Errors:"),
        "Should count errors in large dataset"
    );

    // Performance check: should process 1000 lines in reasonable time
    assert!(
        duration.as_millis() < 5000,
        "Should process 1000 lines in less than 5 seconds, took {}ms",
        duration.as_millis()
    );
}

#[test]
fn test_filename_tracking_json_sequential() {
    // Test filename tracking with JSON format in sequential mode
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"{\"message\": \"test1\"}\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"{\"message\": \"test2\"}\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f",
            "json",
            "--exec",
            "print(\"File: \" + meta.filename + \", Message: \" + e.message)",
        ],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("File: ") && stdout.contains("Message: test1"),
        "Should show filename and message for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("File: ") && stdout.contains("Message: test2"),
        "Should show filename and message for file2: {}",
        stdout
    );
}

#[test]
fn test_filename_tracking_json_parallel() {
    // Test filename tracking with JSON format in parallel mode
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"{\"message\": \"test1\"}\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"{\"message\": \"test2\"}\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f",
            "json",
            "--parallel",
            "--exec",
            "print(\"File: \" + meta.filename + \", Message: \" + e.message)",
        ],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("File: ") && stdout.contains("Message: test1"),
        "Should show filename and message for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("File: ") && stdout.contains("Message: test2"),
        "Should show filename and message for file2: {}",
        stdout
    );
}

#[test]
fn test_filename_tracking_line_format() {
    // Test filename tracking with line format
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"line from file1\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"line from file2\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f",
            "line",
            "--exec",
            "print(\"File: \" + meta.filename + \", Line: \" + line)",
        ],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("File: ") && stdout.contains("Line: line from file1"),
        "Should show filename and content for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("File: ") && stdout.contains("Line: line from file2"),
        "Should show filename and content for file2: {}",
        stdout
    );
}

#[test]
fn test_filename_tracking_with_file_order() {
    // Test filename tracking with file ordering
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"first\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"second\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f",
            "line",
            "--file-order",
            "name",
            "--exec",
            "print(\"Processing: \" + meta.filename + \" -> \" + line)",
        ],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Processing: ") && stdout.contains("first"),
        "Should process first file: {}",
        stdout
    );
    assert!(
        stdout.contains("Processing: ") && stdout.contains("second"),
        "Should process second file: {}",
        stdout
    );
}

#[test]
fn test_no_input_with_begin_only() {
    // Test --no-input with only --begin stage
    let (stdout, _stderr, exit_code) =
        run_kelora(&["--no-input", "--begin", "print(\"Hello, World!\")"]);

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Hello, World!"),
        "Should execute begin stage: {}",
        stdout
    );
}

#[test]
fn test_no_input_with_begin_and_end() {
    // Test --no-input with both --begin and --end stages
    let (stdout, _stderr, exit_code) = run_kelora(&[
        "--no-input",
        "--begin",
        "conf.counter = 0; for i in 0..5 { conf.counter += i; }",
        "--end",
        "print(`Sum: ${conf.counter}`)",
    ]);

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Sum: 10"),
        "Should execute begin and end stages: {}",
        stdout
    );
}

#[test]
fn test_no_input_conflicts_with_files() {
    // Test that --no-input conflicts with file arguments
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(b"test\n")
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) =
        run_kelora_with_files(&["--no-input"], &[temp_file.path().to_str().unwrap()]);

    assert_ne!(exit_code, 0, "Should fail with error");
    assert!(
        stderr.contains("--no-input cannot be used with input files"),
        "Should show conflict error: {}",
        stderr
    );
    assert!(
        stderr.contains("Remove --no-input to read files"),
        "Should explain how to resolve the conflict: {}",
        stderr
    );
    assert!(stdout.is_empty());
}

#[test]
fn test_no_input_with_metrics() {
    // Test --no-input with metrics tracking in begin/end stages
    let (stdout, _stderr, exit_code) = run_kelora(&[
        "--no-input",
        "--begin",
        "for i in 0..10 { track_sum(\"iterations\", 1); }",
        "--end",
        "print(`Total iterations: ${metrics[\"iterations\"]}`)",
    ]);

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Total iterations: 10"),
        "Should track metrics across stages: {}",
        stdout
    );
}

#[test]
fn test_no_input_sequential_mode() {
    // Test --no-input in sequential mode (default)
    let (stdout, _stderr, exit_code) =
        run_kelora(&["--no-input", "--begin", "print(\"Sequential mode\")"]);

    assert_eq!(exit_code, 0, "Should work in sequential mode");
    assert!(stdout.contains("Sequential mode"));
}

#[test]
fn test_no_input_parallel_mode() {
    // Test --no-input with --parallel
    let (stdout, _stderr, exit_code) = run_kelora(&[
        "--no-input",
        "--parallel",
        "--begin",
        "print(\"Parallel mode\")",
    ]);

    assert_eq!(exit_code, 0, "Should work in parallel mode");
    assert!(stdout.contains("Parallel mode"));
}

#[test]
fn test_merge_sorted_merges_files_chronologically() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n{\"ts\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should merge files successfully");
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["a", "b", "c", "d"]);
}

#[test]
fn test_merge_sorted_auto_detects_standard_timestamp_field_names() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"timestamp\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"timestamp\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"timestamp\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n{\"timestamp\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should merge files successfully");
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["a", "b", "c", "d"]);
}

#[test]
fn test_merge_sorted_rejects_parallel_mode() {
    let (stdout, stderr, exit_code) = run_kelora(&["-f", "json", "--parallel", "--merge-sorted"]);
    assert_ne!(exit_code, 0, "Expected validation error");
    assert!(
        stderr.contains("--merge-sorted is not supported with --parallel"),
        "Unexpected error message: {}",
        stderr
    );
    assert!(stdout.is_empty());
}

#[test]
fn test_merge_sorted_supports_logfmt() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"ts=2025-01-01T00:00:02Z msg=b\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"ts=2025-01-01T00:00:01Z msg=a\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "logfmt", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should merge logfmt inputs successfully");
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["a", "b"]);
}

#[test]
fn test_merge_sorted_rejects_csv_for_now() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"ts,msg\n2025-01-01T00:00:02Z,b\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"ts,msg\n2025-01-01T00:00:01Z,a\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "csv", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(exit_code, 0, "CSV merge should be rejected for now");
    assert!(
        stdout.is_empty(),
        "No events should be emitted for unsupported format"
    );
}

#[test]
fn test_merge_sorted_assumes_each_file_is_already_sorted() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"ts\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(
        exit_code, 0,
        "merge-sorted should fail when an input file is not timestamp-sorted"
    );
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["b", "c", "d"]);
}

#[test]
fn test_merge_sorted_missing_timestamps_fail_immediately_in_streaming_mode() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"msg\":\"missing\"}\n{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n{\"ts\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(
        exit_code, 0,
        "merge-sorted should fail when a later event is missing a timestamp"
    );
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["a", "b"]);
    assert!(
        stderr.contains("--merge-sorted requires a timestamp for every event"),
        "Expected immediate merge failure, got: {}",
        stderr
    );
}

#[test]
fn test_merge_sorted_missing_timestamps_fail_in_strict_mode() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"msg\":\"missing\"}\n{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n")
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted", "--strict"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(
        exit_code, 0,
        "Strict mode should fail on missing timestamps"
    );
    assert!(stdout.contains("\"msg\":\"a\""));
    assert!(stdout.contains("\"msg\":\"b\""));
    assert!(
        stderr.contains("--merge-sorted requires a timestamp for every event"),
        "Expected strict-mode failure to mention missing timestamp, got: {}",
        stderr
    );
    assert!(
        stderr.contains("--ts-field"),
        "Expected strict-mode failure to suggest --ts-field override, got: {}",
        stderr
    );
}

#[test]
fn test_merge_sorted_fails_fast_on_disordered_events() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"ts\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(
        exit_code, 0,
        "merge-sorted should fail when an input file is not timestamp-sorted"
    );
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("Output line should be JSON")
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(messages, vec!["b", "c", "d"]);
    assert!(
        stderr.contains("input is not sorted by timestamp"),
        "Expected disordered-input failure, got: {}",
        stderr
    );
    assert!(
        stderr.contains("earlier than the previous event"),
        "Expected disordered-input details, got: {}",
        stderr
    );
}

#[test]
fn test_merge_sorted_disordered_events_fail_in_strict_mode() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:04Z\",\"msg\":\"d\"}\n{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n",
        )
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(
            b"{\"ts\":\"2025-01-01T00:00:02Z\",\"msg\":\"b\"}\n{\"ts\":\"2025-01-01T00:00:03Z\",\"msg\":\"c\"}\n",
        )
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted", "--strict"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(exit_code, 0, "Strict mode should fail on disordered input");
    assert!(stdout.contains("\"msg\":\"b\""));
    assert!(stdout.contains("\"msg\":\"c\""));
    assert!(stdout.contains("\"msg\":\"d\""));
    assert!(!stdout.contains("\"msg\":\"a\""));
    assert!(
        stderr.contains("input is not sorted by timestamp"),
        "Expected strict-mode failure to mention disordered input, got: {}",
        stderr
    );
    assert!(
        stderr.contains("earlier than the previous event"),
        "Expected strict-mode failure to mention disordered input, got: {}",
        stderr
    );
}

#[test]
fn test_merge_sorted_fails_before_output_when_file_has_no_initial_mergeable_event() {
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"{\"msg\":\"missing\"}\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"{\"ts\":\"2025-01-01T00:00:01Z\",\"msg\":\"a\"}\n")
        .expect("Failed to write to temp file");

    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "-F", "json", "--merge-sorted"],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_ne!(exit_code, 0, "merge-sorted should fail during priming");
    assert!(
        stdout.is_empty(),
        "priming failures should happen before output"
    );
    assert!(
        stderr.contains("--merge-sorted requires a timestamp for every event"),
        "Expected priming failure to mention missing timestamp, got: {}",
        stderr
    );
}

// Interactive mode only triggers on a real TTY. When stdin is piped/redirected
// and empty with no files given, kelora reads stdin, hits immediate EOF, and
// would otherwise exit 0 in silence. These tests pin the one-line hint that
// explains the empty-input case so it doesn't look like a crash.
#[test]
fn test_empty_stdin_emits_no_input_hint() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(&[], "");

    assert_eq!(exit_code, 0, "empty input is success, not an error");
    assert!(stdout.is_empty(), "no events means no stdout: {}", stdout);
    assert!(
        stderr.contains("No input") && stderr.contains("-h"),
        "empty stdin should nudge toward the options: {}",
        stderr
    );
}

#[test]
fn test_stdin_with_data_does_not_emit_no_input_hint() {
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-j"], "{\"level\":\"info\",\"message\":\"hi\"}\n");

    assert_eq!(exit_code, 0);
    assert!(
        stdout.contains("hi"),
        "input should be processed: {}",
        stdout
    );
    assert!(
        !stderr.contains("No input"),
        "real input must not trigger the no-input hint: {}",
        stderr
    );
}

#[test]
fn test_unparseable_stdin_does_not_emit_no_input_hint() {
    // Regression: lines that are read but fail to parse (e.g. plain text fed
    // with -j) produce zero events. The hint used to fire here because
    // `lines_read` is only tracked under --stats, so it claimed "stdin is
    // empty" while simultaneously reporting parse errors. `lines_errors` is the
    // signal that input did arrive.
    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["-j"], "garbage\nmore garbage\n");

    assert_eq!(exit_code, 1, "all-unparseable input is an error");
    assert!(
        stderr.contains("Parse errors"),
        "parse failures should be reported: {}",
        stderr
    );
    assert!(
        !stderr.contains("No input"),
        "input that failed to parse is not empty input: {}",
        stderr
    );
}

#[test]
fn test_no_input_flag_suppresses_no_input_hint() {
    // --no-input is an explicit begin/end-only opt-out, not an accident.
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["--no-input", "--begin", "print(\"ran\")"], "");

    assert_eq!(exit_code, 0);
    assert!(
        !stderr.contains("No input"),
        "--no-input is intentional; no hint expected: {}",
        stderr
    );
}

#[test]
fn test_no_diagnostics_suppresses_no_input_hint() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["--no-diagnostics"], "");

    assert_eq!(exit_code, 0);
    assert!(
        !stderr.contains("No input"),
        "--no-diagnostics should suppress the advisory hint: {}",
        stderr
    );
}
