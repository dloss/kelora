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
    // Every concrete input format should have a dedicated description, including
    // the easily-forgotten 'raw' format (regression guard for the harmonized list).
    for token in [
        "Concrete formats",
        "Meta formats",
        "\ncef\n",
        "\ncols:<spec>\n",
        "\ncombined\n",
        "csv / tsv / csvnh / tsvnh",
        "\njson (-j)\n",
        "\nline\n",
        "\nlogfmt\n",
        "\nraw\n",
        "\nregex:<pattern>\n",
        "\nsyslog\n",
        "Built-in application-log formats",
        "auto (default)",
        "auto-per-file",
    ] {
        assert!(
            stdout.contains(token),
            "--help-formats should describe {token:?}"
        );
    }
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
fn test_help_keyword_filters_cli_reference() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help", "since"]);
    assert_eq!(exit_code, 0, "--help KEYWORD should exit successfully");
    assert!(stdout.contains("Options matching \"since\":"));
    // The matched flag and its section heading are kept for context.
    assert!(stdout.contains("Filtering Options:"));
    assert!(stdout.contains("--since <TIME>"));
    // A long-only flag (indent 6) must not be merged into the preceding
    // short-aliased entry, so unrelated options stay filtered out.
    assert!(!stdout.contains("--exclude-keys"));
}

#[test]
fn test_help_keyword_equals_form_matches_literal_flag() {
    // The =KEYWORD form is an alternative to the space form for flag queries.
    let (stdout, _stderr, exit_code) = run_kelora(&["--help=--since"]);
    assert_eq!(exit_code, 0, "--help=KEYWORD should exit successfully");
    assert!(stdout.contains("Options matching \"--since\":"));
    assert!(stdout.contains("--since <TIME>"));
}

#[test]
fn test_help_short_flag_query_is_precise() {
    // A short flag is a whole-token, case-sensitive match: `-j` finds only the
    // `-j` option, not `-J` (case) and not the `-j` buried in `--multiline-join`
    // (substring). This is the payoff of special-casing flag queries.
    let (stdout, _stderr, exit_code) = run_kelora(&["--help", "-j"]);
    assert_eq!(exit_code, 0, "--help -j should exit successfully");
    assert!(stdout.contains("Options matching \"-j\":"));
    assert!(stdout.contains("Shortcut for -f json"));
    assert!(
        !stdout.contains("multiline-join"),
        "-j must not match the substring inside --multiline-join: {stdout}"
    );
    assert!(
        !stdout.contains("Shortcut for -F json"),
        "-j must not case-fold into the -J option: {stdout}"
    );
}

#[test]
fn test_help_short_flag_query_is_case_sensitive() {
    // The uppercase counterpart resolves to its own distinct option.
    let (stdout, _stderr, exit_code) = run_kelora(&["--help", "-J"]);
    assert_eq!(exit_code, 0, "--help -J should exit successfully");
    assert!(stdout.contains("Shortcut for -F json"));
    assert!(
        !stdout.contains("Shortcut for -f json"),
        "-J must not match the lowercase -j option: {stdout}"
    );
}

#[test]
fn test_help_keyword_no_match() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help", "nonexistentxyz"]);
    assert_eq!(
        exit_code, 0,
        "--help with no match should still exit cleanly"
    );
    assert!(stdout.contains("No options matching \"nonexistentxyz\""));
}

#[test]
fn test_bare_help_still_renders_full_reference() {
    // A keyword routes to the filtered search; bare --help must stay on clap's
    // full renderer (regression guard for the interception heuristic).
    let (stdout, _stderr, exit_code) = run_kelora(&["--help"]);
    assert_eq!(exit_code, 0, "--help should exit successfully");
    assert!(!stdout.contains("Options matching"));
    assert!(stdout.contains("Usage: kelora"));
    assert!(stdout.contains("Filtering Options:"));
}

#[test]
fn test_help_examples_topic() {
    let (stdout, _stderr, exit_code) = run_kelora(&["--help-examples"]);
    assert_eq!(exit_code, 0, "--help-examples should exit successfully");
    assert!(stdout.contains("Common Log Analysis Patterns:"));
    assert!(stdout.contains("SECURITY & DATA PRIVACY:"));
}
