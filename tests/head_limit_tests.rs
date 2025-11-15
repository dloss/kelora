mod common;
use common::*;

#[test]
fn test_head_limit_basic() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--head", "3"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully with --head");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --head 3 is specified"
    );

    // Check that it outputs the first 3 lines
    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
    assert!(stdout.contains("Line 3"), "Should include third line");
    assert!(!stdout.contains("Line 4"), "Should not include fourth line");
    assert!(!stdout.contains("Line 5"), "Should not include fifth line");
}

#[test]
fn test_head_limit_with_filter() {
    let input = r#"{"level": "INFO", "message": "Good line 1"}
{"level": "ERROR", "message": "Bad line 1"}
{"level": "INFO", "message": "Good line 2"}
{"level": "ERROR", "message": "Bad line 2"}
{"level": "INFO", "message": "Good line 3"}
{"level": "ERROR", "message": "Bad line 3"}
{"level": "INFO", "message": "Good line 4"}"#;

    // --head 4 means read only first 4 input lines, then filter
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"INFO\"",
            "--head",
            "4",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head and --filter"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    // First 4 lines are: Good line 1, Bad line 1, Good line 2, Bad line 2
    // Filter keeps only INFO lines: Good line 1, Good line 2
    assert_eq!(
        lines.len(),
        2,
        "Should output 2 lines (first 4 input lines filtered to INFO only)"
    );

    assert!(
        stdout.contains("Good line 1"),
        "Should include first INFO line"
    );
    assert!(
        stdout.contains("Good line 2"),
        "Should include second INFO line"
    );
    assert!(
        !stdout.contains("Good line 3"),
        "Should not include third INFO line (not in first 4 input lines)"
    );
    assert!(
        !stdout.contains("Bad line"),
        "Should not include any ERROR lines due to filter"
    );
}

#[test]
fn test_head_limit_with_skip_lines() {
    let input = r#"header1
header2
{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    // Skip 2 lines, then read 3 lines (lines 3, 4, 5)
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--skip-lines", "2", "--head", "5"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --skip-lines and --head"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    // Head 5 means read lines 1-5. Skip 2 means skip lines 1-2. So we process lines 3-5.
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (lines 3-5 after skipping first 2)"
    );

    assert!(!stdout.contains("header"), "Should not contain headers");
    assert!(stdout.contains("Line 1"), "Should include Line 1");
    assert!(stdout.contains("Line 2"), "Should include Line 2");
    assert!(stdout.contains("Line 3"), "Should include Line 3");
    assert!(!stdout.contains("Line 4"), "Should not include Line 4");
}

#[test]
fn test_head_limit_larger_than_input() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--head", "10"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head larger than input"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output all available lines when --head is larger than input"
    );

    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
}

#[test]
fn test_head_limit_parallel_mode() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}
{"level": "INFO", "message": "Line 6"}
{"level": "INFO", "message": "Line 7"}
{"level": "INFO", "message": "Line 8"}
{"level": "INFO", "message": "Line 9"}
{"level": "INFO", "message": "Line 10"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--head", "3", "--parallel"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head and --parallel"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --head 3 is specified in parallel mode"
    );

    // Check that it outputs the first 3 lines (order should be preserved by default)
    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
    assert!(stdout.contains("Line 3"), "Should include third line");
    assert!(!stdout.contains("Line 4"), "Should not include fourth line");
    assert!(!stdout.contains("Line 10"), "Should not include tenth line");
}

#[test]
fn test_head_limit_parallel_small_batches() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--head",
            "3",
            "--parallel",
            "--batch-size",
            "1",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head, --parallel, and small batch size"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --head 3 with batch-size 1 in parallel mode"
    );
}

#[test]
fn test_head_and_take_combined() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "ERROR", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "ERROR", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}
{"level": "ERROR", "message": "Line 6"}
{"level": "INFO", "message": "Line 7"}"#;

    // Read first 5 input lines, filter to INFO, take first 2 output events
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--head",
            "5",
            "--filter",
            "e.level == \"INFO\"",
            "--take",
            "2",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head and --take"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    // First 5 input lines: Line 1 (INFO), Line 2 (ERROR), Line 3 (INFO), Line 4 (ERROR), Line 5 (INFO)
    // Filter to INFO: Line 1, Line 3, Line 5
    // Take 2: Line 1, Line 3
    assert_eq!(
        lines.len(),
        2,
        "Should output 2 lines (first 5 input lines, filtered to INFO, take 2)"
    );

    assert!(stdout.contains("Line 1"), "Should include Line 1");
    assert!(stdout.contains("Line 3"), "Should include Line 3");
    assert!(
        !stdout.contains("Line 5"),
        "Should not include Line 5 (take 2)"
    );
    assert!(
        !stdout.contains("Line 7"),
        "Should not include Line 7 (head 5)"
    );
}

#[test]
fn test_head_limit_parallel_with_filter() {
    let input = r#"{"level": "INFO", "message": "Good line 1"}
{"level": "ERROR", "message": "Bad line 1"}
{"level": "INFO", "message": "Good line 2"}
{"level": "ERROR", "message": "Bad line 2"}
{"level": "INFO", "message": "Good line 3"}
{"level": "ERROR", "message": "Bad line 3"}
{"level": "INFO", "message": "Good line 4"}"#;

    // Read first 5 input lines in parallel
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"INFO\"",
            "--head",
            "5",
            "--parallel",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --head, --filter, and --parallel"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    // First 5 lines: Good line 1, Bad line 1, Good line 2, Bad line 2, Good line 3
    // Filter to INFO: Good line 1, Good line 2, Good line 3
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (first 5 input lines filtered to INFO in parallel mode)"
    );

    assert!(
        stdout.contains("Good line 1"),
        "Should include first INFO line"
    );
    assert!(
        stdout.contains("Good line 2"),
        "Should include second INFO line"
    );
    assert!(
        stdout.contains("Good line 3"),
        "Should include third INFO line"
    );
    assert!(
        !stdout.contains("Good line 4"),
        "Should not include fourth INFO line (not in first 5 input lines)"
    );
    assert!(
        !stdout.contains("Bad line"),
        "Should not include any ERROR lines due to filter"
    );
}
