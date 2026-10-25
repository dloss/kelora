mod common;
use common::*;

#[test]
fn test_take_limit_basic() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--take", "3"], input);

    assert_eq!(exit_code, 0, "kelora should exit successfully with --take");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --take 3 is specified"
    );

    // Check that it outputs the first 3 lines
    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
    assert!(stdout.contains("Line 3"), "Should include third line");
    assert!(!stdout.contains("Line 4"), "Should not include fourth line");
    assert!(!stdout.contains("Line 5"), "Should not include fifth line");
}

#[test]
fn test_take_limit_with_filter() {
    let input = r#"{"level": "INFO", "message": "Good line 1"}
{"level": "ERROR", "message": "Bad line 1"}
{"level": "INFO", "message": "Good line 2"}
{"level": "ERROR", "message": "Bad line 2"}
{"level": "INFO", "message": "Good line 3"}
{"level": "ERROR", "message": "Bad line 3"}
{"level": "INFO", "message": "Good line 4"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"INFO\"",
            "--take",
            "2",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take and --filter"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output exactly 2 lines when --take 2 is specified with filter"
    );

    // Check that it outputs the first 2 INFO lines
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
        "Should not include third INFO line due to --take 2"
    );
    assert!(
        !stdout.contains("Bad line"),
        "Should not include any ERROR lines due to filter"
    );
}

#[test]
fn test_take_limit_zero() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--take", "0"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take 0"
    );

    let output = stdout.trim();
    assert!(
        output.is_empty(),
        "Should output no lines when --take 0 is specified"
    );
}

#[test]
fn test_take_limit_larger_than_input() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--take", "10"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take larger than input"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output all available lines when --take is larger than input"
    );

    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
}

#[test]
fn test_take_limit_parallel_mode() {
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
        run_kelora_with_input(&["-f", "json", "--take", "3", "--parallel"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take and --parallel"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --take 3 is specified in parallel mode"
    );

    // Check that it outputs the first 3 lines (order should be preserved by default)
    assert!(stdout.contains("Line 1"), "Should include first line");
    assert!(stdout.contains("Line 2"), "Should include second line");
    assert!(stdout.contains("Line 3"), "Should include third line");
    assert!(!stdout.contains("Line 4"), "Should not include fourth line");
    assert!(!stdout.contains("Line 10"), "Should not include tenth line");
}

#[test]
fn test_take_limit_parallel_small_batches() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--take",
            "3",
            "--parallel",
            "--batch-size",
            "1",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take, --parallel, and small batch size"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --take 3 with batch-size 1 in parallel mode"
    );
}

#[test]
fn test_take_limit_parallel_with_filter() {
    let input = r#"{"level": "INFO", "message": "Good line 1"}
{"level": "ERROR", "message": "Bad line 1"}
{"level": "INFO", "message": "Good line 2"}
{"level": "ERROR", "message": "Bad line 2"}
{"level": "INFO", "message": "Good line 3"}
{"level": "ERROR", "message": "Bad line 3"}
{"level": "INFO", "message": "Good line 4"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.level == \"INFO\"",
            "--take",
            "2",
            "--parallel",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take, --filter, and --parallel"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output exactly 2 lines when --take 2 with filter in parallel mode"
    );

    // Check that it outputs the first 2 INFO lines
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
        "Should not include third INFO line due to --take 2"
    );
    assert!(
        !stdout.contains("Bad line"),
        "Should not include any ERROR lines due to filter"
    );
}

#[test]
fn test_take_limit_parallel_unordered() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}
{"level": "INFO", "message": "Line 3"}
{"level": "INFO", "message": "Line 4"}
{"level": "INFO", "message": "Line 5"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--take", "3", "--parallel", "--unordered"],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with --take, --parallel, and --unordered"
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output exactly 3 lines when --take 3 in unordered parallel mode"
    );

    // In unordered mode, we can't guarantee which 3 lines we get, but we should get exactly 3
    // and they should all be from our input
    for line in lines {
        assert!(
            line.contains("Line"),
            "Each output line should contain 'Line'"
        );
    }
}

#[test]
fn test_take_zero_processes_no_event() {
    let input = r#"{"level": "INFO", "message": "Line 1"}
{"level": "INFO", "message": "Line 2"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-n",
            "0",
            "-e",
            "track_inc(\"seen\")",
            "--end",
            "print(metrics.get(\"seen\") ?? 0)",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim(),
        "0",
        "-n 0 must not run any event through the stages"
    );
}
