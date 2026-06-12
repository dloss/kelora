mod common;
use common::*;

#[test]
fn test_help_time_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-time"]);
    assert_eq!(exit_code, 0, "--help-time should exit successfully");
    assert!(stdout.contains("Time Format Reference for --ts-format:"));
    assert!(stdout.contains("Timestamp filtering with --since and --until:"));
}

#[test]
fn test_help_formats_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-formats"]);
    assert_eq!(exit_code, 0, "--help-formats should exit successfully");
    assert!(stdout.contains("Format Reference:"));
    assert!(stdout.contains("tailmap"));
}

#[test]
fn test_main_help_describes_non_obvious_output_formats() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help"]);
    assert_eq!(exit_code, 0, "--help should exit successfully");
    assert!(stdout.contains("levelmap  Compact level timeline"));
    assert!(stdout.contains("keymap    First-character map for one selected field"));
    assert!(stdout.contains("tailmap   Percentile map for one numeric field"));
    assert!(stdout.contains("csv       Comma-separated with header row"));
    assert!(stdout.contains("tsvnh     TSV without header row"));
}

#[test]
fn test_main_help_documents_exit_codes() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help"]);
    assert_eq!(exit_code, 0, "--help should exit successfully");
    assert!(
        stdout.contains("Exit Codes:"),
        "--help should document the exit-code model: {}",
        stdout
    );
    // The resilient-vs-strict distinction is the non-obvious part users hit.
    assert!(stdout.contains("never once succeeded"));
    assert!(stdout.contains("--strict"));
    assert!(stdout.contains("134"));
}

#[test]
fn test_help_regex_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-regex"]);
    assert_eq!(exit_code, 0, "--help-regex should exit successfully");
    assert!(stdout.contains("Regex Format Parsing Reference for -f regex:PATTERN:"));
    assert!(stdout.contains("Named capture groups (REQUIRED):"));
}

#[test]
fn test_help_rhai_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-rhai"]);
    assert_eq!(exit_code, 0, "--help-rhai should exit successfully");
    assert!(stdout.contains("Rhai Language Guide:"));
    assert!(stdout.contains("KELORA PIPELINE STAGES:"));
}

#[test]
fn test_help_multiline_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-multiline"]);
    assert_eq!(exit_code, 0, "--help-multiline should exit successfully");
    assert!(stdout.contains("Multiline Strategy Reference for --multiline:"));
    assert!(stdout.contains("regex:match=REGEX[:end=REGEX]"));
}

#[test]
fn test_help_functions_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-functions"]);
    assert_eq!(exit_code, 0, "--help-functions should exit successfully");
    assert!(stdout.contains("Available Rhai Functions:"));
    assert!(stdout.contains("text.mask_ip([octets])"));
}

#[test]
fn test_help_functions_keyword_filter() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-functions", "mask_ip"]);
    assert_eq!(
        exit_code, 0,
        "--help-functions with a keyword should exit successfully"
    );
    assert!(stdout.contains("Functions matching \"mask_ip\":"));
    assert!(stdout.contains("text.mask_ip([octets])"));
    // The section header for a matched entry is preserved for context.
    assert!(stdout.contains("STRING FUNCTIONS:"));
    // Unrelated functions are filtered out.
    assert!(!stdout.contains("array.join(separator)"));
}

#[test]
fn test_help_functions_keyword_equals_form() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-functions=parse_json"]);
    assert_eq!(
        exit_code, 0,
        "--help-functions=KEYWORD should exit successfully"
    );
    assert!(stdout.contains("Functions matching \"parse_json\":"));
    assert!(stdout.contains("text.parse_json()"));
}

#[test]
fn test_help_functions_keyword_matches_multiline_entry() {
    // `text.normalized` spans several indented continuation lines; a keyword
    // that only appears in a continuation line must still surface the entry.
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-functions", "credit_card"]);
    assert_eq!(
        exit_code, 0,
        "--help-functions with a keyword should exit successfully"
    );
    assert!(stdout.contains("text.normalized([patterns])"));
    assert!(stdout.contains("credit_card"));
}

#[test]
fn test_help_functions_keyword_no_match() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-functions", "nonexistentxyz"]);
    assert_eq!(
        exit_code, 0,
        "--help-functions with no match should still exit successfully"
    );
    assert!(stdout.contains("No functions matching \"nonexistentxyz\""));
}

#[test]
fn test_help_examples_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-examples"]);
    assert_eq!(exit_code, 0, "--help-examples should exit successfully");
    assert!(stdout.contains("Common Log Analysis Patterns:"));
    assert!(stdout.contains("SECURITY & DATA PRIVACY:"));
}
