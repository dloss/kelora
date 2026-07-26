mod common;
use common::*;

/// A malformed `cols:` spec used to raise the same runtime error on every input
/// line. It is a usage error, so it should fail once, before reading input.
#[test]
fn test_comma_separated_cols_spec_fails_once_as_usage_error() {
    let input = "2024-01-01 00:00:00 INFO hello\n2024-01-01 00:00:01 INFO world";

    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "cols:ts,level,msg"], input);

    assert_eq!(
        exit_code, 2,
        "a malformed spec is invalid CLI usage: {}",
        stderr
    );
    assert!(
        stderr.contains("separated by spaces, not commas"),
        "error should name the actual problem, got: {}",
        stderr
    );
    assert!(
        stderr.contains("cols:ts level msg"),
        "error should suggest the corrected spec, got: {}",
        stderr
    );
    // The old behaviour emitted one error per line; make sure we are not back to that.
    assert!(
        !stderr.contains("Parse errors"),
        "spec should be rejected before any line is parsed, got: {}",
        stderr
    );
}

/// An invalid field name should explain the naming rule rather than just echoing
/// the token back.
#[test]
fn test_invalid_field_name_reports_naming_rule() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:ts 9bad *msg"], "irrelevant\n");

    assert_eq!(exit_code, 2, "invalid field name is a usage error");
    assert!(
        stderr.contains("9bad") && stderr.contains("may not start with a digit"),
        "error should name the token and the rule, got: {}",
        stderr
    );
}

/// A zero field count is rejected up front too.
#[test]
fn test_invalid_field_count_rejected_before_parsing() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:ts(0) *msg"], "irrelevant\n");

    assert_eq!(exit_code, 2, "invalid count is a usage error");
    assert!(
        stderr.contains("count"),
        "error should mention the count, got: {}",
        stderr
    );
}

/// Valid specs, including the multi-token and rest forms, must be unaffected.
#[test]
fn test_valid_cols_specs_still_accepted() {
    let input = "2024-01-01 00:00:00 INFO some message here";

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:ts(2) level *msg", "-F", "json"], input);
    assert_eq!(exit_code, 0, "valid spec should parse: {}", stderr);

    let ev: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(ev["ts"].as_str().unwrap(), "2024-01-01 00:00:00");
    assert_eq!(ev["level"].as_str().unwrap(), "INFO");
    assert_eq!(ev["msg"].as_str().unwrap(), "some message here");
}

/// Type annotations are part of the cols spec, so validation must accept them
/// rather than mistaking `age:int` for a bad field name.
#[test]
fn test_cols_type_annotations_pass_validation() {
    let input = "alice 30 berlin";

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:name age:int city", "-F", "json"], input);
    assert_eq!(
        exit_code, 0,
        "type annotations are valid spec syntax: {}",
        stderr
    );

    let ev: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        ev["age"].as_i64().unwrap(),
        30,
        "age should be typed as int"
    );
}

/// An unrecognised annotation should name the valid types instead of reporting
/// the whole token as an invalid field name.
#[test]
fn test_unknown_type_annotation_lists_valid_types() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:name age:banana"], "alice 30\n");

    assert_eq!(exit_code, 2, "unknown annotation is a usage error");
    assert!(
        stderr.contains("banana") && stderr.contains("int, float, bool, string"),
        "error should list valid types, got: {}",
        stderr
    );
}

/// The skip token and explicit counts should also survive validation.
#[test]
fn test_cols_spec_with_skip_tokens_accepted() {
    let input = "2024-01-01 00:00:00 12345 INFO the message";

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:ts(2) - level *msg", "-F", "json"], input);
    assert_eq!(exit_code, 0, "skip token should be valid: {}", stderr);

    let ev: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(ev["level"].as_str().unwrap(), "INFO");
    assert_eq!(ev["msg"].as_str().unwrap(), "the message");
}
