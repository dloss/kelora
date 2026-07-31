//! An `absorb_*` call that can never work must say so.
//!
//! Every `absorb_*` function returns a status map, which is the right shape for
//! *data-dependent* outcomes — this line had no `key=value` pairs, that payload
//! was not JSON. It was the wrong shape for a bug in the script itself: an
//! unknown option key or an uncompilable pattern discarded the whole call, and
//! because no working script assigns the return value, the run printed nothing
//! and exited 0 (#364). Those two determinate failures now go through the
//! script-error channel instead; everything data-dependent still stays quiet.

mod common;
use common::*;

const INPUT: &str = "{\"a\":\"x=1 y=2\",\"n\":1}\n{\"a\":\"x=3 y=4\",\"n\":2}";

#[test]
fn unknown_absorb_option_is_reported_and_names_the_valid_keys() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.absorb_regex("a", "x=(?P<x>\\d+)", #{types: true})"#,
        ],
        INPUT,
    );

    // exec is a best-effort transform: the events still come out (rolled back,
    // so unextracted), and the run exits 0. The point is that it is no longer
    // silent about why nothing was extracted.
    assert_eq!(
        exit_code, 0,
        "an exec error rolls back; it does not fail the run: {stderr}"
    );
    assert_eq!(
        stdout.trim().lines().count(),
        2,
        "both events should still be emitted: {stdout}"
    );
    assert!(
        !stdout.contains("\"x\""),
        "the call was discarded, so nothing was extracted: {stdout}"
    );

    assert!(
        stderr.contains("Exec errors:") || stderr.contains("Mixed errors:"),
        "an option typo is a script bug and belongs in the exec-error channel: {stderr}"
    );
    assert!(
        stderr.contains("unknown absorb option: types"),
        "the existing diagnostic should now reach stderr: {stderr}"
    );
    assert!(
        stderr.contains("valid: sep, kv_sep, keep_source, overwrite"),
        "naming the valid keys saves a trip to --help-functions: {stderr}"
    );
}

#[test]
fn unknown_absorb_option_still_aborts_under_strict() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--strict",
            "--exec",
            r#"e.absorb_kv("a", #{keep_sorce: true})"#,
        ],
        INPUT,
    );

    assert_ne!(exit_code, 0, "--strict must fail on the first script bug");
    assert!(
        stderr.contains("unknown absorb option: keep_sorce"),
        "stderr should name the offending key: {stderr}"
    );
}

#[test]
fn uncompilable_pattern_is_reported_with_the_reason_on_one_line() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--exec", r#"e.absorb_regex("a", "x=(?P<x")"#],
        INPUT,
    );

    // Same class as the option typo: no input can make an invalid regex match.
    assert_eq!(exit_code, 0, "still a recovered exec error: {stderr}");
    assert!(
        stderr.contains("Exec errors:") || stderr.contains("Mixed errors:"),
        "an uncompilable pattern should not be silent either: {stderr}"
    );
    // The summary prints only the first line of a message, so the regex crate's
    // four-line error has to be condensed down to its reason.
    let reported = stderr
        .lines()
        .find(|line| line.contains("Invalid regex pattern"))
        .unwrap_or_else(|| panic!("no pattern error in stderr: {stderr}"));
    assert!(
        reported.contains("group name"),
        "the line shown should carry the reason, not just 'regex parse error:': {reported}"
    );
}

#[test]
fn data_dependent_absorb_outcomes_stay_quiet() {
    // A pattern that compiles but matches nothing on these lines, a field that is
    // absent, and a payload that is not JSON are all normal for a real log. None
    // of them is a script bug, so none of them may produce a diagnostic.
    for exec in [
        r#"e.absorb_regex("a", "zzz(?P<q>\\d+)")"#,
        r#"e.absorb_kv("no_such_field")"#,
        r#"e.absorb_json("a")"#,
    ] {
        let (_stdout, stderr, exit_code) =
            run_kelora_with_input(&["-f", "json", "-F", "json", "--exec", exec], INPUT);

        assert_eq!(exit_code, 0, "`{exec}` should succeed: {stderr}");
        assert!(
            !stderr.contains("errors:"),
            "`{exec}` is data-dependent and must not report an error: {stderr}"
        );
    }
}

#[test]
fn a_working_absorb_call_is_unaffected() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"e.absorb_regex("a", "x=(?P<x>\\d+)")"#,
        ],
        INPUT,
    );

    assert_eq!(exit_code, 0, "{stderr}");
    assert!(stderr.is_empty(), "a clean run says nothing: {stderr}");
    assert!(
        stdout.contains(r#""x":"1""#) && stdout.contains(r#""x":"3""#),
        "named captures should still be extracted: {stdout}"
    );
}
