mod common;
use common::*;

/// chrono's `%Y` accepts a 2-digit year, so the Spark/log4j default layout
/// (`yy/MM/dd`) used to be auto-detected as year 17 rather than 2017 — reported
/// as a 100% successful parse.
#[test]
fn test_two_digit_year_slash_format_resolves_to_current_century() {
    let input = "17/06/09 20:10:40 INFO executor.Executor: running task";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "cols:ts(2) level mod *msg", "-s", "--no-diagnostics"],
        input,
    );
    assert_eq!(exit_code, 0, "stats run should succeed: {}", stderr);
    assert!(
        stdout.contains("2017-06-09"),
        "2-digit year should resolve to 2017, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("0017-06-09"),
        "2-digit year must not be read as year 17, got: {}",
        stdout
    );
}

/// The 4-digit nginx-style format must keep working: `%y` consumes "20" and then
/// fails on the following "17", so the `%Y` candidate still claims the line.
#[test]
fn test_four_digit_year_slash_format_still_parses() {
    let input = "2017/06/09 20:10:40 INFO executor.Executor: running task";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "cols:ts(2) level mod *msg", "-s", "--no-diagnostics"],
        input,
    );
    assert_eq!(exit_code, 0, "stats run should succeed: {}", stderr);
    assert!(
        stdout.contains("2017-06-09"),
        "4-digit year should parse unchanged, got: {}",
        stdout
    );
}

/// Two-digit years follow the POSIX pivot, so 69-99 land in the 1900s.
#[test]
fn test_two_digit_year_uses_posix_pivot() {
    let input = "99/12/31 23:59:59 INFO legacy.App: last log of the millennium";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "cols:ts(2) level mod *msg", "-s", "--no-diagnostics"],
        input,
    );
    assert_eq!(exit_code, 0, "stats run should succeed: {}", stderr);
    assert!(
        stdout.contains("1999-12-31"),
        "99 should pivot to 1999, got: {}",
        stdout
    );
}

/// An implausibly wide span is usually one mis-parsed format, not real data, so
/// stats should flag it instead of reporting it as a fact.
#[test]
fn test_implausible_time_span_is_flagged() {
    // Two well-formed but decades-apart timestamps.
    let input = "1995-01-01 00:00:00 INFO a.B: ancient\n2024-01-01 00:00:00 INFO a.B: recent";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "cols:ts(2) level mod *msg", "-s", "--no-diagnostics"],
        input,
    );
    assert_eq!(exit_code, 0, "stats run should succeed: {}", stderr);

    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("mixed timestamp formats"),
        "a multi-decade span should be flagged, got: {}",
        combined
    );
}

/// A normal span must stay quiet — the warning is only useful if it is rare.
#[test]
fn test_normal_time_span_is_not_flagged() {
    let input = "2024-01-01 00:00:00 INFO a.B: first\n2024-01-01 06:00:00 INFO a.B: last";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "cols:ts(2) level mod *msg", "-s", "--no-diagnostics"],
        input,
    );
    assert_eq!(exit_code, 0, "stats run should succeed: {}", stderr);

    let combined = format!("{}{}", stdout, stderr);
    assert!(
        !combined.contains("mixed timestamp formats"),
        "a 6-hour span must not be flagged, got: {}",
        combined
    );
}
