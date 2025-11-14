mod common;
use common::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_truncate_file_without_allow_fs_writes_flag() {
    // Test that truncate file operations fail without --allow-fs-writes
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("output.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            &format!(r#"truncate_file("{}");"#, output_path),
        ],
        input,
    );

    // Should warn about needing --allow-fs-writes
    assert!(
        stderr.contains("--allow-fs-writes") || stderr.contains("enable --allow-fs-writes"),
        "Should show warning about needing --allow-fs-writes flag, got: {}",
        stderr
    );

    // File should not be created
    assert!(
        !output_file.exists(),
        "File should not be created without --allow-fs-writes"
    );
}

#[test]
fn test_truncate_and_append_with_allow_fs_writes_flag() {
    // Test that file operations succeed with --allow-fs-writes
    // Use truncate_file + append_file to simulate write behavior
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("output.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(
                r#"truncate_file("{}"); append_file("{}", "test data");"#,
                output_path, output_path
            ),
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should succeed with --allow-fs-writes flag, stderr: {}",
        stderr
    );

    // File should be created with correct content
    assert!(output_file.exists(), "File should be created");
    let content = fs::read_to_string(&output_file).unwrap();
    // append_file adds a newline after the text
    assert_eq!(
        content, "test data\n",
        "File should contain written data with newline"
    );
}

#[test]
fn test_file_append_without_allow_fs_writes_flag() {
    // Test that file append operations fail without --allow-fs-writes
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("append.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            &format!(r#"append_file("{}", "appended data");"#, output_path),
        ],
        input,
    );

    // Should warn about needing --allow-fs-writes
    assert!(
        stderr.contains("--allow-fs-writes") || stderr.contains("enable --allow-fs-writes"),
        "Should show warning about needing --allow-fs-writes flag, got: {}",
        stderr
    );
}

#[test]
fn test_file_append_with_allow_fs_writes_flag() {
    // Test that file append operations succeed with --allow-fs-writes
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("append.txt");
    let output_path = output_file.to_str().unwrap();

    // Create initial file
    fs::write(&output_file, "initial data\n").unwrap();

    let input = r#"{"message": "test1"}
{"message": "test2"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(r#"append_file("{}", e.message + "\n");"#, output_path),
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should succeed with --allow-fs-writes flag, stderr: {}",
        stderr
    );

    // File should contain initial data plus appended data
    let content = fs::read_to_string(&output_file).unwrap();
    assert!(
        content.contains("initial data"),
        "Should preserve initial data"
    );
    assert!(content.contains("test1"), "Should append first message");
    assert!(content.contains("test2"), "Should append second message");
}

#[test]
fn test_file_append_with_strict_mode() {
    // Test --allow-fs-writes with --strict mode on filesystem errors
    let invalid_path = "/invalid/path/that/does/not/exist/file.txt";

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--strict",
            "--exec",
            &format!(r#"append_file("{}", "data");"#, invalid_path),
        ],
        input,
    );

    // Should fail with strict mode on invalid path
    assert_ne!(
        exit_code, 0,
        "Should fail with --strict on filesystem error"
    );
    assert!(
        stderr.to_lowercase().contains("error") || stderr.to_lowercase().contains("failed"),
        "Should show filesystem error"
    );
}

#[test]
fn test_file_append_without_strict_mode() {
    // Test that filesystem errors are tolerated without --strict
    let invalid_path = "/invalid/path/that/does/not/exist/file.txt";

    let input = r#"{"message": "test"}"#;

    let (_stdout, _stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(r#"append_file("{}", "data");"#, invalid_path),
        ],
        input,
    );

    // Without strict mode, might succeed (error is logged but not fatal)
    // Or might still fail - both are acceptable
    // The key is that --strict makes it definitively fail
}

#[test]
fn test_multiple_file_appends() {
    // Test multiple file append operations in single run
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("file1.txt");
    let file2 = temp_dir.path().join("file2.txt");

    let input = r#"{"id": 1, "message": "first"}
{"id": 2, "message": "second"}"#;

    let exec_script = format!(
        r#"if e.id == 1 {{ append_file("{}", e.message); }} else {{ append_file("{}", e.message); }}"#,
        file1.to_str().unwrap(),
        file2.to_str().unwrap()
    );

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--allow-fs-writes", "--exec", &exec_script],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should append to multiple files, stderr: {}",
        stderr
    );

    // Both files should exist with correct content
    assert!(file1.exists(), "First file should exist");
    assert!(file2.exists(), "Second file should exist");

    let content1 = fs::read_to_string(&file1).unwrap();
    let content2 = fs::read_to_string(&file2).unwrap();

    // append_file adds newlines
    assert_eq!(
        content1, "first\n",
        "First file should have correct content"
    );
    assert_eq!(
        content2, "second\n",
        "Second file should have correct content"
    );
}

#[test]
fn test_file_write_with_filtering() {
    // Test file writes combined with filtering
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("filtered.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"level": "info", "message": "info msg"}
{"level": "error", "message": "error msg"}
{"level": "info", "message": "another info"}
{"level": "error", "message": "another error"}"#;

    let exec_script = format!(
        r#"if e.level == "error" {{ append_file("{}", e.message + "\n"); }}"#,
        output_path
    );

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--allow-fs-writes", "--exec", &exec_script],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should succeed with filtering, stderr: {}",
        stderr
    );

    // File should contain only error messages
    let content = fs::read_to_string(&output_file).unwrap();
    assert!(content.contains("error msg"), "Should contain first error");
    assert!(
        content.contains("another error"),
        "Should contain second error"
    );
    assert!(
        !content.contains("info msg"),
        "Should not contain info messages"
    );
}

#[test]
fn test_file_operations_with_parallel_mode() {
    // Test file operations in parallel mode (should work or have clear error)
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("parallel_output.txt");
    let output_path = output_file.to_str().unwrap();

    let input: String = (1..=10)
        .map(|i| format!(r#"{{"id": {}}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let exec_script = format!(
        r#"append_file("{}", e.id.to_string() + "\n");"#,
        output_path
    );

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--parallel",
            "--batch-size",
            "2",
            "--exec",
            &exec_script,
        ],
        &input,
    );

    // Should either succeed or fail with clear message
    // Parallel file writes might have race conditions
    if exit_code == 0 {
        // If it succeeds, file should exist
        assert!(
            output_file.exists(),
            "Output file should exist if operation succeeded"
        );
        let content = fs::read_to_string(&output_file).unwrap();
        // Should have all IDs (order may vary due to parallel execution)
        for i in 1..=10 {
            assert!(content.contains(&i.to_string()), "Should contain ID {}", i);
        }
    } else {
        // If it fails, should have informative error
        assert!(
            stderr.to_lowercase().contains("error")
                || stderr.to_lowercase().contains("parallel")
                || stderr.to_lowercase().contains("concurrent"),
            "Should explain why file operations failed in parallel mode"
        );
    }
}

#[test]
fn test_truncate_file_overwrites_existing() {
    // Test that truncate_file overwrites existing files
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("overwrite.txt");
    let output_path = output_file.to_str().unwrap();

    // Create initial file
    fs::write(&output_file, "old content").unwrap();

    let input = r#"{"message": "new content"}"#;

    let (_stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(
                r#"truncate_file("{}"); append_file("{}", e.message);"#,
                output_path, output_path
            ),
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Should overwrite successfully");

    // File should contain only new content
    let content = fs::read_to_string(&output_file).unwrap();
    assert_eq!(content, "new content\n", "Should overwrite old content");
    assert!(
        !content.contains("old content"),
        "Old content should be gone"
    );
}

#[test]
fn test_read_file_allowed_without_flag() {
    // Test that read_file works without --allow-fs-writes (reads are always allowed)
    // Note: read_file can only be called during --begin phase
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.txt");
    fs::write(&input_file, "file content here\nline 2\n").unwrap();

    let input = r#"{"id": 1}"#;

    let begin_script = format!(
        r#"let lines = read_lines("{}"); print("Read " + lines.len() + " lines");"#,
        input_file.to_str().unwrap()
    );

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--begin", &begin_script], input);

    assert_eq!(
        exit_code, 0,
        "Read operations should work without --allow-fs-writes flag, stderr: {}",
        stderr
    );

    // Output should show that read_lines succeeded
    assert!(
        stdout.contains("Read 2 lines") || stdout.contains("Read 3 lines"),
        "Should read file without --allow-fs-writes, stdout: {}",
        stdout
    );
}

#[test]
fn test_file_operations_with_special_characters() {
    // Test file operations with filenames containing special characters
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("file with spaces.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(r#"append_file("{}", "content");"#, output_path),
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should handle filenames with spaces, stderr: {}",
        stderr
    );

    assert!(
        output_file.exists(),
        "File with spaces in name should be created"
    );
    let content = fs::read_to_string(&output_file).unwrap();
    assert_eq!(content, "content\n", "Should write correct content");
}

#[test]
fn test_truncate_file_creates_empty_file() {
    // Test that truncate_file creates empty file
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("empty.txt");
    let output_path = output_file.to_str().unwrap();

    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--allow-fs-writes",
            "--exec",
            &format!(r#"truncate_file("{}");"#, output_path),
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Should create empty file successfully, stderr: {}",
        stderr
    );

    assert!(output_file.exists(), "Empty file should be created");
    let content = fs::read_to_string(&output_file).unwrap();
    assert!(content.is_empty(), "File should be empty");
}
