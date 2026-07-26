mod common;
use common::*;

/// Year-less layouts (syslog, glog, redis) carry no year, so kelora guesses one
/// from the wall clock and warns that it guessed. `--input-year` is what makes
/// that warning actionable (#341): it states the year instead.
///
/// The syslog sample is dated June/July, mirroring `Linux_2k.log` from
/// `logpai/loghub` — a 2005 capture that the heuristic dates to the current year.
const SYSLOG_2005: &str = "\
Jun 14 15:16:01 combo sshd(pam_unix)[19939]: authentication failure
Jun 14 15:16:02 combo sshd(pam_unix)[19940]: check pass; user unknown
Jul 27 14:42:00 combo ftpd[7045]: connection from 24.54.76.216
";

#[test]
fn input_year_dates_a_yearless_log_to_the_stated_year() {
    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["--input-year", "2005", "--normalize-ts"], SYSLOG_2005);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("2005-06-14T15:16:01"),
        "timestamps should resolve into 2005, got: {}",
        stdout
    );
    assert!(
        stdout.contains("2005-07-27T14:42:00"),
        "every year-less timestamp should resolve into 2005, got: {}",
        stdout
    );
}

#[test]
fn input_year_makes_the_time_window_usable_on_yearless_logs() {
    // The window filter runs before the script stages, so rewriting `ts` in
    // --exec cannot repair it; --input-year is the only thing that can.
    let (stdout, stderr, exit_code) = run_kelora_with_file(
        &[
            "--input-year",
            "2005",
            "--since",
            "2005-06-01",
            "--until",
            "2005-07-01",
        ],
        SYSLOG_2005,
    );

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("authentication failure") && stdout.contains("check pass"),
        "June events should be inside a June 2005 window, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("connection from"),
        "the July event should be outside a June 2005 window, got: {}",
        stdout
    );
}

// The year-less warning rides along with the --stats block, so these read stdout.
#[test]
fn input_year_silences_the_guessed_year_warning() {
    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["--input-year", "2005", "-s"], SYSLOG_2005);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        !stdout.contains("Year-less timestamps detected"),
        "a stated year is not a guessed year, so nothing should warn: {}",
        stdout
    );
}

#[test]
fn the_guessed_year_warning_names_the_flag_and_counts_timestamps_once() {
    let (stdout, stderr, exit_code) = run_kelora_with_file(&["-s"], SYSLOG_2005);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("Year-less timestamps detected"),
        "without --input-year the year is guessed and must warn: {}",
        stdout
    );
    assert!(
        stdout.contains("--input-year"),
        "the warning must name the way to act on it: {}",
        stdout
    );
    // Each timestamp is parsed more than once per run (parser, then the
    // post-script `parsed_ts` refresh); the count must still be 3, not 6.
    assert!(
        stdout.contains("(3 timestamps)"),
        "the count is per timestamp, not per parse attempt: {}",
        stdout
    );
}

#[test]
fn input_year_auto_keeps_the_wall_clock_heuristic() {
    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["--input-year", "auto", "-s"], SYSLOG_2005);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("Year-less timestamps detected"),
        "'auto' is the guessing mode, so it should still warn: {}",
        stdout
    );
}

#[test]
fn input_year_also_applies_to_displayed_timestamps() {
    // -Z re-parses the field for display. It has to agree with the year the
    // pipeline filtered on, or the printed timestamp names a year --since never saw.
    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["--input-year", "2005", "-Z"], SYSLOG_2005);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("2005-06-14T15:16:01"),
        "displayed timestamps should use the stated year, got: {}",
        stdout
    );
}

#[test]
fn input_year_leaves_timestamps_that_carry_a_year_alone() {
    let input = "2023-07-04T12:00:00Z INFO service started\n";

    let (stdout, stderr, exit_code) = run_kelora_with_file(
        &[
            "-f",
            "cols:ts level *msg",
            "--input-year",
            "2005",
            "--normalize-ts",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("2023-07-04T12:00:00"),
        "--input-year must not override a year present in the input, got: {}",
        stdout
    );
}

#[test]
fn a_yearless_log_crossing_new_years_eve_still_needs_auto() {
    // Documented limitation: the year is stated per run, not tracked per line,
    // so January lines following December ones land in the stated year. The
    // heuristic (the default) is what handles the boundary correctly.
    let input = "\
Dec 31 23:59:59 host app[1]: new year eve
Jan 01 00:00:05 host app[1]: happy new year
";

    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["--input-year", "2005", "--normalize-ts"], input);

    assert_eq!(exit_code, 0, "run should succeed: {}", stderr);
    assert!(
        stdout.contains("2005-12-31T23:59:59") && stdout.contains("2005-01-01T00:00:05"),
        "both lines resolve into the stated year, which is why --merge-sorted \
         rejects such a stream rather than merging it: {}",
        stdout
    );
}

#[test]
fn invalid_input_year_fails_before_reading_input() {
    for bad in ["205", "twentyfive"] {
        let (stdout, stderr, exit_code) =
            run_kelora_with_file(&["--input-year", bad, "-s"], SYSLOG_2005);

        assert_eq!(
            exit_code, 2,
            "a bad --input-year is invalid usage, got exit {} for '{}': {}",
            exit_code, bad, stderr
        );
        assert!(
            stderr.contains("Invalid --input-year"),
            "error should name the option, got: {}",
            stderr
        );
        assert!(
            stdout.is_empty(),
            "nothing should be processed, got: {}",
            stdout
        );
    }
}
