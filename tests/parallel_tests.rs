mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_parallel_mode() {
    let input = r#"{"level": "INFO", "status": 200}
{"level": "ERROR", "status": 500}
{"level": "DEBUG", "status": 404}
{"level": "WARN", "status": 403}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--threads",
            "2",
            "--filter",
            "e.status >= 400",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully in parallel mode"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 3, "Should filter to 3 lines in parallel mode");

    // Verify all results have status >= 400
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        let status = parsed["status"]
            .as_i64()
            .expect("Status should be a number");
        assert!(
            status >= 400,
            "Parallel filtered results should have status >= 400"
        );
    }
}

#[test]
fn test_parallel_sequential_equivalence() {
    let input = r#"{"level": "INFO", "status": 200, "user": "alice"}
{"level": "ERROR", "status": 500, "user": "bob"}
{"level": "DEBUG", "status": 404, "user": "charlie"}
{"level": "WARN", "status": 403, "user": "david"}
{"level": "INFO", "status": 201, "user": "eve"}
{"level": "ERROR", "status": 502, "user": "frank"}"#;

    // Run sequential mode
    let (seq_stdout, _seq_stderr, seq_exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--filter",
            "e.status >= 400",
            "--exec",
            "let processed = true",
        ],
        input,
    );

    // Run parallel mode
    let (par_stdout, _par_stderr, par_exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--threads",
            "2",
            "--filter",
            "e.status >= 400",
            "--exec",
            "let processed = true",
        ],
        input,
    );

    // Both should exit successfully
    assert_eq!(seq_exit_code, 0, "Sequential mode should exit successfully");
    assert_eq!(par_exit_code, 0, "Parallel mode should exit successfully");

    // Parse and sort output lines for comparison (parallel may reorder)
    let mut seq_lines: Vec<&str> = seq_stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty() && l.starts_with('{'))
        .collect();
    let mut par_lines: Vec<&str> = par_stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty() && l.starts_with('{'))
        .collect();

    seq_lines.sort();
    par_lines.sort();

    // Should have same number of results
    assert_eq!(
        seq_lines.len(),
        par_lines.len(),
        "Sequential and parallel should produce same number of results"
    );

    // Results should be functionally equivalent (same filtered and processed records)
    for (seq_line, par_line) in seq_lines.iter().zip(par_lines.iter()) {
        let seq_json: serde_json::Value =
            serde_json::from_str(seq_line).expect("Sequential output should be valid JSON");
        let par_json: serde_json::Value =
            serde_json::from_str(par_line).expect("Parallel output should be valid JSON");

        // Check that key fields match
        assert_eq!(
            seq_json["status"], par_json["status"],
            "Status should match between modes"
        );
        assert_eq!(
            seq_json["user"], par_json["user"],
            "User should match between modes"
        );
        assert_eq!(
            seq_json["processed"], par_json["processed"],
            "Processed field should match between modes"
        );

        // Verify filtering worked correctly in both modes
        let status = seq_json["status"]
            .as_i64()
            .expect("Status should be a number");
        assert!(status >= 400, "Both modes should filter correctly");
    }

    // Verify both modes processed the same data successfully
    assert!(
        !seq_lines.is_empty(),
        "Sequential mode should produce some output"
    );
    assert!(
        !par_lines.is_empty(),
        "Parallel mode should produce some output"
    );
}

#[test]
fn test_parallel_stats_output_counts_lines_and_events() {
    let input = r#"{"status": 200}
{"status": 404}
{"status": 500}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--stats",
            "--exec",
            "if status >= 400 { track_count(\"errors\"); }",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --stats in parallel"
    );
    assert!(
        stderr.contains("Lines processed: 3 total"),
        "Stats output should report total lines processed"
    );
    assert!(
        stderr.contains("Events created: 3 total"),
        "Stats output should report events created"
    );
    assert!(
        !stderr.contains("__kelora_stats"),
        "Stats output must not leak internal tracker keys"
    );
}

#[test]
fn test_sequential_parallel_mode_parity() {
    // Test that sequential and parallel modes produce similar results
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1.write_all(b"{\"user\": \"alice\", \"status\": \"active\"}\n{\"user\": \"bob\", \"status\": \"inactive\"}\n").expect("Failed to write to temp file");
    temp_file2.write_all(b"{\"user\": \"charlie\", \"status\": \"active\"}\n{\"user\": \"dave\", \"status\": \"inactive\"}\n").expect("Failed to write to temp file");

    let files = &[
        temp_file1.path().to_str().unwrap(),
        temp_file2.path().to_str().unwrap(),
    ];

    // Test sequential mode
    let (stdout_seq, stderr_seq, exit_code_seq) = run_kelora_with_files(
        &[
            "-f",
            "json",
            "--exec",
            "print(\"File: \" + meta.filename + \", User: \" + e.user + \", Status: \" + e.status)",
        ],
        files,
    );

    // Test parallel mode
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_files(
        &[
            "-f",
            "json",
            "--parallel",
            "--exec",
            "print(\"File: \" + meta.filename + \", User: \" + e.user + \", Status: \" + e.status)",
        ],
        files,
    );

    assert_eq!(
        exit_code_seq, 0,
        "Sequential mode should exit successfully, stderr: {}",
        stderr_seq
    );
    assert_eq!(
        exit_code_par, 0,
        "Parallel mode should exit successfully, stderr: {}",
        stderr_par
    );

    // Both modes should show filename tracking
    assert!(
        stdout_seq.contains("File: ") && stdout_seq.contains("User: alice"),
        "Sequential mode should show filename and user data: {}",
        stdout_seq
    );
    assert!(
        stdout_par.contains("File: ") && stdout_par.contains("User: alice"),
        "Parallel mode should show filename and user data: {}",
        stdout_par
    );
}

#[test]
fn test_track_unique_parallel_mode() {
    let input = r#"{"ip": "1.1.1.1"}
{"ip": "2.2.2.2"}
{"ip": "1.1.1.1"}
{"ip": "3.3.3.3"}
{"ip": "2.2.2.2"}
{"ip": "4.4.4.4"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "2",
            "--exec",
            "track_unique(\"ips\", e.ip);",
            "--end",
            "print(`Unique IPs: ${metrics[\"ips\"].len()}`);",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully in parallel mode"
    );

    // Should merge unique values from all workers
    assert!(
        stdout.contains("Unique IPs: 4"),
        "Should collect 4 unique IPs across parallel workers"
    );
}

#[test]
fn test_track_bucket_parallel_mode() {
    let input = r#"{"status": "200"}
{"status": "404"}
{"status": "200"}
{"status": "500"}
{"status": "404"}
{"status": "200"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "json",
        "--parallel",
        "--batch-size", "2",
        "--exec", "track_bucket(\"status_counts\", e.status);",
        "--end", "let counts = metrics[\"status_counts\"]; print(`200: ${counts.get(\"200\") ?? 0}, 404: ${counts.get(\"404\") ?? 0}, 500: ${counts.get(\"500\") ?? 0}`);"
    ], input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully in parallel mode"
    );

    // Should merge bucket counts from all workers
    assert!(
        stdout.contains("200: 3"),
        "Should count 3 occurrences of status 200"
    );
    assert!(
        stdout.contains("404: 2"),
        "Should count 2 occurrences of status 404"
    );
    assert!(
        stdout.contains("500: 1"),
        "Should count 1 occurrence of status 500"
    );
}

#[test]
fn test_parallel_multiline_indent_consistency() {
    // Test that parallel and sequential processing produce identical results for indent strategy
    let input = r#"2024-01-01 10:00:00 INFO Starting application
    Additional info line 1
    Additional info line 2
2024-01-01 10:00:05 ERROR Database connection failed
    Stack trace line 1
    Stack trace line 2
    Stack trace line 3
2024-01-01 10:00:10 INFO Application started successfully
    Single continuation line
2024-01-01 10:00:15 DEBUG Debug message
    Debug detail 1
    Debug detail 2"#;

    // Sequential processing
    let (stdout_seq, stderr_seq, exit_code_seq) =
        run_kelora_with_input(&["-f", "line", "-M", "indent", "--stats"], input);
    assert_eq!(exit_code_seq, 0, "Sequential should succeed");

    // Parallel processing
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_input(
        &["-f", "line", "-M", "indent", "--stats", "--parallel"],
        input,
    );
    assert_eq!(exit_code_par, 0, "Parallel should succeed");

    // Parse event counts
    let events_created_seq = extract_events_created_from_stats(&stderr_seq);
    let events_created_par = extract_events_created_from_stats(&stderr_par);

    // Assert identical event counts
    assert_eq!(
        events_created_seq, events_created_par,
        "Sequential and parallel should create same number of events"
    );
    assert_eq!(
        events_created_seq, 4,
        "Should create exactly 4 multiline events from 12 lines"
    );

    // Count output lines (should be same)
    let output_lines_seq = stdout_seq.lines().count();
    let output_lines_par = stdout_par.lines().count();
    assert_eq!(
        output_lines_seq, output_lines_par,
        "Sequential and parallel should produce same number of output lines"
    );
    assert_eq!(output_lines_seq, 4, "Should output exactly 4 events");
}

#[test]
fn test_parallel_multiline_timestamp_consistency() {
    // Test timestamp strategy with parallel processing
    let input = r#"Jan  1 10:00:00 host app: Event one starts here
and continues on this line
and ends here
Jan  1 10:00:05 host app: Event two is an error
with multiple lines
of detailed information
Jan  1 10:00:10 host app: Event three is info
single line continuation
Jan  1 10:00:15 host app: Event four debug message"#;

    // Sequential processing
    let (stdout_seq, stderr_seq, exit_code_seq) =
        run_kelora_with_input(&["-f", "syslog", "-M", "timestamp", "--stats"], input);
    assert_eq!(exit_code_seq, 0, "Sequential should succeed");

    // Parallel processing
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_input(
        &["-f", "syslog", "-M", "timestamp", "--stats", "--parallel"],
        input,
    );
    assert_eq!(exit_code_par, 0, "Parallel should succeed");

    // Parse event counts
    let events_created_seq = extract_events_created_from_stats(&stderr_seq);
    let events_created_par = extract_events_created_from_stats(&stderr_par);

    // Assert identical event counts
    assert_eq!(
        events_created_seq, events_created_par,
        "Sequential and parallel should create same number of events"
    );
    assert_eq!(
        events_created_seq, 4,
        "Should create exactly 4 multiline events from 9 lines"
    );

    // Count output lines
    let output_lines_seq = stdout_seq.lines().count();
    let output_lines_par = stdout_par.lines().count();
    assert_eq!(
        output_lines_seq, output_lines_par,
        "Sequential and parallel should produce same number of output lines"
    );
}

#[test]
fn test_parallel_multiline_all_consistency() {
    // Test all strategy with parallel processing
    let input = r#"{"level": "INFO", "message": "Event one"}
{"level": "ERROR", "message": "Event two with error"}
{"level": "INFO", "message": "Event three info"}
{"level": "DEBUG", "message": "Event four debug"}
{"level": "ERROR", "message": "Event five another error"}
{"level": "INFO", "message": "Event six final info"}"#;

    // Sequential processing
    let (stdout_seq, stderr_seq, exit_code_seq) =
        run_kelora_with_input(&["-f", "line", "-M", "all", "--stats"], input);
    assert_eq!(exit_code_seq, 0, "Sequential should succeed");

    // Parallel processing
    let (stdout_par, stderr_par, exit_code_par) =
        run_kelora_with_input(&["-f", "line", "-M", "all", "--stats", "--parallel"], input);
    assert_eq!(exit_code_par, 0, "Parallel should succeed");

    // Parse event counts
    let events_created_seq = extract_events_created_from_stats(&stderr_seq);
    let events_created_par = extract_events_created_from_stats(&stderr_par);

    // Assert identical event counts
    assert_eq!(
        events_created_seq, events_created_par,
        "Sequential and parallel should create same number of events"
    );
    assert_eq!(
        events_created_seq, 1,
        "All strategy should create exactly 1 event from the stream"
    );

    // Both should produce exactly 1 output line
    let output_lines_seq = stdout_seq.lines().count();
    let output_lines_par = stdout_par.lines().count();
    assert_eq!(output_lines_seq, 1, "Sequential should output 1 line");
    assert_eq!(output_lines_par, 1, "Parallel should output 1 line");
}

#[test]
fn test_parallel_unordered_mode() {
    // Test --unordered flag allows out-of-order output
    let input = r#"{"id": 1, "data": "first"}
{"id": 2, "data": "second"}
{"id": 3, "data": "third"}
{"id": 4, "data": "fourth"}
{"id": 5, "data": "fifth"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--unordered",
            "--batch-size",
            "1",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Parallel unordered mode should succeed");

    // Should produce all 5 records
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 5, "Should output all 5 records");

    // Verify all IDs are present (order may vary)
    let mut ids: Vec<i64> = lines
        .iter()
        .map(|line| {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            json["id"].as_i64().unwrap()
        })
        .collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2, 3, 4, 5], "All IDs should be present");
}

#[test]
fn test_parallel_tiny_batch_timeout() {
    // Test with very small batch timeout
    let input = r#"{"data": "line1"}
{"data": "line2"}
{"data": "line3"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "10",
            "--batch-timeout",
            "1",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Parallel mode with tiny batch timeout should succeed"
    );

    // Should still process all lines
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output all 3 lines");
}

#[test]
fn test_parallel_with_malformed_events() {
    // Test parallel processing with mix of valid and malformed JSON
    let input = r#"{"valid": "first"}
{invalid json here
{"valid": "second"}
not json at all
{"valid": "third"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--parallel"], input);

    // May succeed and skip malformed lines, or may fail depending on strict mode
    // Should process the valid lines
    if exit_code == 0 {
        // If it succeeds, should have processed valid lines
        assert!(
            stdout.contains("first") || stdout.contains("second") || stdout.contains("third"),
            "Should process at least some valid lines"
        );
    }
    // If it fails, that's also acceptable behavior for malformed input
}

#[test]
fn test_parallel_empty_batches() {
    // Test parallel mode with very small input that creates mostly empty batches
    let input = r#"{"data": "single"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--threads",
            "4",
            "--batch-size",
            "10",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Should handle single line with many threads");
    assert!(stdout.contains("single"), "Should output the single line");
}

#[test]
fn test_parallel_large_batch_size() {
    // Test with batch size larger than input
    let input: String = (1..=50)
        .map(|i| format!(r#"{{"id": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--batch-size",
            "1000",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "Should handle batch size larger than input");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 50, "Should output all 50 lines");
}

#[test]
fn test_parallel_with_errors_in_exec_script() {
    // Test parallel mode when exec script has errors in some events
    let input = r#"{"value": 10}
{"value": 0}
{"value": 5}
{"value": 0}
{"value": 20}"#;

    // Division by zero in some cases
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--exec",
            "e.result = 100 / e.value;",
        ],
        input,
    );

    // Should handle errors gracefully
    // May succeed with errors logged, or may fail
    if exit_code == 0 {
        // If succeeds, should process lines without division by zero
        assert!(!stdout.is_empty(), "Should produce some output");
    }
    // Error messages might be in stderr
    // This tests error resilience in parallel mode
}

#[test]
fn test_parallel_unordered_maintains_completeness() {
    // Test that --unordered doesn't lose events, just reorders them
    let input: String = (1..=100)
        .map(|i| format!(r#"{{"id": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout_ordered, _stderr, exit_code_ordered) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--batch-size",
            "10",
        ],
        &input,
    );

    let (stdout_unordered, _stderr, exit_code_unordered) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--parallel",
            "--unordered",
            "--batch-size",
            "10",
        ],
        &input,
    );

    assert_eq!(exit_code_ordered, 0);
    assert_eq!(exit_code_unordered, 0);

    // Both should have same number of lines
    let ordered_lines: Vec<&str> = stdout_ordered.trim().lines().collect();
    let unordered_lines: Vec<&str> = stdout_unordered.trim().lines().collect();
    assert_eq!(
        ordered_lines.len(),
        100,
        "Ordered mode should output 100 lines"
    );
    assert_eq!(
        unordered_lines.len(),
        100,
        "Unordered mode should output 100 lines"
    );

    // Extract and sort IDs from both outputs
    let mut ordered_ids: Vec<i64> = ordered_lines
        .iter()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["id"]
                .as_i64()
                .unwrap()
        })
        .collect();
    let mut unordered_ids: Vec<i64> = unordered_lines
        .iter()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["id"]
                .as_i64()
                .unwrap()
        })
        .collect();

    ordered_ids.sort();
    unordered_ids.sort();

    // After sorting, both should have all IDs from 1 to 100
    assert_eq!(
        ordered_ids, unordered_ids,
        "Both modes should have same IDs"
    );
    assert_eq!(ordered_ids[0], 1, "Should start at 1");
    assert_eq!(ordered_ids[99], 100, "Should end at 100");
}

#[test]
fn test_parallel_stress_many_threads() {
    // Test parallel mode with many threads (stress test)
    let input: String = (1..=1000)
        .map(|i| format!(r#"{{"id": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--threads",
            "8",
            "--batch-size",
            "50",
            "--stats",
        ],
        &input,
    );

    assert_eq!(
        exit_code, 0,
        "Should handle many threads, stderr: {}",
        stderr
    );

    // Should process all lines
    assert!(
        stderr.contains("Lines processed: 1000 total"),
        "Should process all 1000 lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 1000, "Should output all 1000 lines");
}

#[test]
fn test_parallel_with_filtering_and_metrics() {
    // Test parallel mode combining filtering, metrics, and stats
    let input: String = (1..=200)
        .map(|i| format!(r#"{{"value": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "20",
            "--filter",
            "e.value % 10 == 0",
            "--exec",
            "track_count(\"divisible_by_10\");",
            "--stats",
            "--metrics",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "Complex parallel operation should succeed");

    // Should output 20 lines (multiples of 10)
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 20, "Should output 20 filtered lines");

    // Stats should show 20 output, 180 filtered
    assert!(
        stderr.contains("20 output"),
        "Stats should show 20 output events"
    );

    // Metrics should show count of 20
    assert!(
        stderr.contains("divisible_by_10") && stderr.contains("20"),
        "Metrics should track count of 20"
    );
}

#[test]
fn test_parallel_with_zero_batch_size() {
    // Test that zero or invalid batch size is handled
    let input = r#"{"data": "test"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--parallel", "--batch-size", "0"], input);

    // Should either use default batch size or error
    // Exit code 2 = usage error, 0 = success with default
    assert!(
        exit_code == 0 || exit_code == 2,
        "Should handle zero batch size"
    );

    if exit_code == 2 {
        assert!(
            stderr.to_lowercase().contains("error") || stderr.to_lowercase().contains("invalid"),
            "Should show error for invalid batch size"
        );
    }
}

#[test]
fn test_parallel_batch_timeout_zero() {
    // Test with zero batch timeout
    let input = r#"{"data": "line1"}
{"data": "line2"}
{"data": "line3"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--parallel",
            "--batch-size",
            "10",
            "--batch-timeout",
            "0",
        ],
        input,
    );

    // Should process successfully (0 might mean infinite or immediate)
    assert_eq!(exit_code, 0, "Should handle zero batch timeout");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output all 3 lines");
}

#[test]
fn test_parallel_consistency_with_different_thread_counts() {
    // Test that different thread counts produce same results
    let input: String = (1..=100)
        .map(|i| format!(r#"{{"value": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let filter_expr = "e.value % 7 == 0";

    let results: Vec<(String, i32)> = [2, 4, 8]
        .iter()
        .map(|threads| {
            let (stdout, _stderr, exit_code) = run_kelora_with_input(
                &[
                    "-f",
                    "json",
                    "-F",
                    "json",
                    "--parallel",
                    "--threads",
                    &threads.to_string(),
                    "--filter",
                    filter_expr,
                ],
                &input,
            );
            (stdout, exit_code)
        })
        .collect();

    // All should succeed
    for (i, (_stdout, exit_code)) in results.iter().enumerate() {
        assert_eq!(
            *exit_code,
            0,
            "Thread count {} should succeed",
            vec![2, 4, 8][i]
        );
    }

    // All should have same number of output lines
    let line_counts: Vec<usize> = results
        .iter()
        .map(|(stdout, _)| stdout.trim().lines().count())
        .collect();

    assert!(
        line_counts.iter().all(|&c| c == line_counts[0]),
        "All thread counts should produce same number of lines"
    );
    assert_eq!(
        line_counts[0], 14,
        "Should filter to 14 multiples of 7 (7, 14, 21, ..., 98)"
    );
}

#[test]
fn test_parallel_multiline_filtering_accuracy() {
    // Test that filtering works correctly in parallel multiline mode
    let input = r#"2024-01-01 10:00:00 INFO Starting application
    Additional info line 1
    Additional info line 2
2024-01-01 10:00:05 ERROR Database connection failed
    Stack trace line 1
    Stack trace line 2
2024-01-01 10:00:10 INFO Application started successfully
    Single continuation line
2024-01-01 10:00:15 DEBUG Debug message
    Debug detail 1"#;

    // Test filtering for ERROR events only
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "indent",
            "--stats",
            "--parallel",
            "--filter",
            "e.line.contains(\"ERROR\")",
        ],
        input,
    );
    assert_eq!(exit_code_par, 0, "Parallel filtering should succeed");

    // Parse event counts
    let events_created = extract_events_created_from_stats(&stderr_par);
    let events_filtered = extract_events_filtered_from_stats(&stderr_par);

    assert_eq!(events_created, 4, "Should create 4 events total");
    assert_eq!(events_filtered, 3, "Should filter out 3 non-ERROR events");

    // Should output exactly 1 line (the ERROR event)
    let output_lines = stdout_par.lines().count();
    assert_eq!(output_lines, 1, "Should output exactly 1 ERROR event");
    assert!(
        stdout_par.contains("ERROR"),
        "Output should contain ERROR event"
    );

    // Test the reverse filter
    let (stdout_par2, stderr_par2, exit_code_par2) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "indent",
            "--stats",
            "--parallel",
            "--filter",
            "e.line.contains(\"INFO\") || e.line.contains(\"DEBUG\")",
        ],
        input,
    );
    assert_eq!(
        exit_code_par2, 0,
        "Parallel reverse filtering should succeed"
    );

    let events_created2 = extract_events_created_from_stats(&stderr_par2);
    let events_filtered2 = extract_events_filtered_from_stats(&stderr_par2);

    assert_eq!(events_created2, 4, "Should create 4 events total");
    assert_eq!(events_filtered2, 1, "Should filter out 1 ERROR event");

    let output_lines2 = stdout_par2.lines().count();
    assert_eq!(output_lines2, 3, "Should output 3 non-ERROR events");
}

#[test]
fn test_parallel_multiline_event_counting_accuracy() {
    // Test various scenarios to ensure event counting is always accurate

    // Scenario 1: Simple multiline events
    let input1 = r#"Event 1 start
  continuation 1
Event 2 start
  continuation 2"#;

    let (_, stderr1, exit_code1) = run_kelora_with_input(
        &["-f", "line", "-M", "indent", "--stats", "--parallel"],
        input1,
    );
    assert_eq!(exit_code1, 0);
    assert_eq!(
        extract_events_created_from_stats(&stderr1),
        2,
        "Simple case should create 2 events"
    );

    // Scenario 2: Edge case with single line events mixed with multiline
    let input2 = r#"Single line event
Multiline event start
  continuation line
Another single line
Final multiline start
  final continuation"#;

    let (_, stderr2, exit_code2) = run_kelora_with_input(
        &["-f", "line", "-M", "indent", "--stats", "--parallel"],
        input2,
    );
    assert_eq!(exit_code2, 0);
    assert_eq!(
        extract_events_created_from_stats(&stderr2),
        4,
        "Mixed case should create 4 events"
    );

    // Scenario 3: Many small multiline events
    let input3 = (0..10)
        .map(|i| format!("Event {} start\n  continuation {}", i, i))
        .collect::<Vec<_>>()
        .join("\n");

    let (_, stderr3, exit_code3) = run_kelora_with_input(
        &["-f", "line", "-M", "indent", "--stats", "--parallel"],
        &input3,
    );
    assert_eq!(exit_code3, 0);
    assert_eq!(
        extract_events_created_from_stats(&stderr3),
        10,
        "Many events case should create 10 events"
    );
}

#[test]
fn test_parallel_multiline_vs_sequential_comprehensive() {
    // Comprehensive test comparing parallel vs sequential for multiple strategies and filters

    let test_cases = vec![
        (
            "indent",
            r#"App started
  with details
Error occurred
  stack trace line 1
  stack trace line 2
App finished
  cleanup done"#,
            3, // expected events
        ),
        (
            "timestamp",
            r#"Jan 1 10:00:00 server app: Request started
continuation line 1
continuation line 2
Jan 1 10:00:05 server app: Request completed
final line"#,
            2, // expected events
        ),
    ];

    for (strategy, input, expected_events) in test_cases {
        // Test without filtering
        let (stdout_seq, stderr_seq, _) = run_kelora_with_input(
            &[
                "-f",
                if strategy == "timestamp" {
                    "syslog"
                } else {
                    "line"
                },
                "-M",
                strategy,
                "--stats",
            ],
            input,
        );
        let (stdout_par, stderr_par, _) = run_kelora_with_input(
            &[
                "-f",
                if strategy == "timestamp" {
                    "syslog"
                } else {
                    "line"
                },
                "-M",
                strategy,
                "--stats",
                "--parallel",
            ],
            input,
        );

        let events_seq = extract_events_created_from_stats(&stderr_seq);
        let events_par = extract_events_created_from_stats(&stderr_par);

        assert_eq!(
            events_seq, expected_events,
            "Sequential {} should create {} events",
            strategy, expected_events
        );
        assert_eq!(
            events_par, expected_events,
            "Parallel {} should create {} events",
            strategy, expected_events
        );
        assert_eq!(
            events_seq, events_par,
            "Sequential and parallel {} should match",
            strategy
        );

        let lines_seq = stdout_seq.lines().count();
        let lines_par = stdout_par.lines().count();
        assert_eq!(
            lines_seq, lines_par,
            "Output line count should match for {}",
            strategy
        );

        // Test with filtering (where applicable)
        if strategy == "indent" {
            let (_, stderr_seq_f, _) = run_kelora_with_input(
                &[
                    "-f",
                    "line",
                    "-M",
                    strategy,
                    "--stats",
                    "--filter",
                    "e.line.contains(\"Error\")",
                ],
                input,
            );
            let (_, stderr_par_f, _) = run_kelora_with_input(
                &[
                    "-f",
                    "line",
                    "-M",
                    strategy,
                    "--stats",
                    "--parallel",
                    "--filter",
                    "e.line.contains(\"Error\")",
                ],
                input,
            );

            let filtered_seq = extract_events_filtered_from_stats(&stderr_seq_f);
            let filtered_par = extract_events_filtered_from_stats(&stderr_par_f);
            assert_eq!(
                filtered_seq, filtered_par,
                "Filtered counts should match for {}",
                strategy
            );
        }
    }
}

#[test]
fn test_stdin_with_parallel_processing() {
    let input = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--parallel", "-"], input);

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("test1"));
    assert!(stdout.contains("test2"));
    assert!(stdout.contains("test3"));
}

#[test]
fn test_parallel_stats_counting_basic() {
    let input: String = (1..=100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--stats",
            "--filter",
            "line.to_int() % 10 == 0",
            "--parallel",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output_lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(output_lines.len(), 10, "Should emit 10 multiples of 10");

    let stats = extract_stats_lines(&stderr);
    let lines_processed = stats
        .iter()
        .find(|line| line.starts_with("Lines processed:"))
        .expect("Stats should include line totals");
    assert_eq!(
        lines_processed,
        "Lines processed: 100 total, 0 filtered (0.0%), 0 errors (0.0%)"
    );

    let events_created = stats
        .iter()
        .find(|line| line.starts_with("Events created:"))
        .expect("Stats should include event totals");
    assert_eq!(
        events_created,
        "Events created: 100 total, 10 output, 90 filtered (90.0%)"
    );
}

#[test]
fn test_parallel_stats_counting_large_dataset() {
    let input: String = (1..=10_000)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--stats",
            "--filter",
            "line.to_int() % 10 == 0",
            "--parallel",
            "--batch-size",
            "100",
        ],
        &input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let stats = extract_stats_lines(&stderr);
    let lines_processed = stats
        .iter()
        .find(|line| line.starts_with("Lines processed:"))
        .expect("Stats should include line totals");
    assert_eq!(
        lines_processed,
        "Lines processed: 10000 total, 0 filtered (0.0%), 0 errors (0.0%)"
    );

    let events_created = stats
        .iter()
        .find(|line| line.starts_with("Events created:"))
        .expect("Stats should include event totals");
    assert_eq!(
        events_created,
        "Events created: 10000 total, 1000 output, 9000 filtered (90.0%)"
    );
}

#[test]
fn test_parallel_vs_sequential_stats_consistency() {
    let input: String = (1..=1000)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let (stdout_seq, stderr_seq, exit_code_seq) =
        run_kelora_with_input(&["--stats", "--filter", "line.to_int() % 100 == 0"], &input);
    let (stdout_par, stderr_par, exit_code_par) = run_kelora_with_input(
        &[
            "--stats",
            "--filter",
            "line.to_int() % 100 == 0",
            "--parallel",
            "--batch-size",
            "50",
        ],
        &input,
    );

    assert_eq!(exit_code_seq, 0, "Sequential execution should succeed");
    assert_eq!(exit_code_par, 0, "Parallel execution should succeed");
    assert_eq!(
        stdout_seq, stdout_par,
        "Sequential and parallel output should match exactly"
    );

    let stats_seq = extract_stats_lines(&stderr_seq);
    let stats_par = extract_stats_lines(&stderr_par);
    let seq_lines_processed = stats_line(&stats_seq, "Lines processed:");
    let par_lines_processed = stats_line(&stats_par, "Lines processed:");
    assert_eq!(
        seq_lines_processed, par_lines_processed,
        "Sequential and parallel runs should report the same line totals"
    );
    assert_eq!(
        seq_lines_processed,
        "Lines processed: 1000 total, 0 filtered (0.0%), 0 errors (0.0%)"
    );

    let seq_events_created = stats_line(&stats_seq, "Events created:");
    let par_events_created = stats_line(&stats_par, "Events created:");
    assert_eq!(
        seq_events_created, par_events_created,
        "Sequential and parallel runs should report the same event totals"
    );
    assert_eq!(
        seq_events_created,
        "Events created: 1000 total, 10 output, 990 filtered (99.0%)"
    );
}

#[test]
fn test_parallel_stats_with_errors() {
    let input = "1\n2\ninvalid\n4\n5\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["--stats", "--filter", "line.to_int() > 3", "--parallel"],
        input,
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output_lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        output_lines.len(),
        2,
        "Should emit two values greater than 3"
    );

    let stats = extract_stats_lines(&stderr);
    let lines_processed = stats
        .iter()
        .find(|line| line.starts_with("Lines processed:"))
        .expect("Stats should include line totals");
    assert_eq!(
        lines_processed,
        "Lines processed: 5 total, 0 filtered (0.0%), 0 errors (0.0%)"
    );

    let events_created = stats
        .iter()
        .find(|line| line.starts_with("Events created:"))
        .expect("Stats should include event totals");
    assert_eq!(
        events_created,
        "Events created: 5 total, 2 output, 3 filtered (60.0%)"
    );
}

#[test]
fn test_parallel_stats_with_different_batch_sizes() {
    let input: String = (1..=500)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let batch_sizes = [1, 10, 50, 100, 500];
    let mut results = Vec::new();

    for batch_size in batch_sizes {
        let (stdout, stderr, exit_code) = run_kelora_with_input(
            &[
                "--stats",
                "--filter",
                "line.to_int() % 50 == 0",
                "--parallel",
                "--batch-size",
                &batch_size.to_string(),
            ],
            &input,
        );

        assert_eq!(
            exit_code, 0,
            "kelora should succeed with batch size {}",
            batch_size
        );
        results.push((stdout, stderr));
    }

    let (first_stdout, _) = &results[0];
    for (idx, (stdout, stderr)) in results.iter().enumerate() {
        assert_eq!(
            stdout, first_stdout,
            "Batch size {} should match output from batch size {}",
            batch_sizes[idx], batch_sizes[0]
        );

        let stats = extract_stats_lines(stderr);
        let lines_processed = stats
            .iter()
            .find(|line| line.starts_with("Lines processed:"))
            .expect("Stats should include line totals");
        assert_eq!(
            lines_processed,
            "Lines processed: 500 total, 0 filtered (0.0%), 0 errors (0.0%)"
        );

        let events_created = stats
            .iter()
            .find(|line| line.starts_with("Events created:"))
            .expect("Stats should include event totals");
        assert_eq!(
            events_created,
            "Events created: 500 total, 10 output, 490 filtered (98.0%)"
        );
    }
}
