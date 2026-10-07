mod common;
use common::*;

#[test]
fn test_field_span_basic() {
    let input = r#"{"request_id":"req-1","msg":"a"}
{"request_id":"req-1","msg":"b"}
{"request_id":"req-2","msg":"c"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "request_id",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("req-1:2"));
    assert!(stdout.contains("req-2:1"));
}

#[test]
fn test_field_span_interleaved_creates_multiple_spans() {
    let input = r#"{"request_id":"req-1","msg":"a"}
{"request_id":"req-2","msg":"b"}
{"request_id":"req-1","msg":"c"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "request_id",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);

    let mut seen = stdout
        .lines()
        .filter(|l| l.contains("req-1:") || l.contains("req-2:"))
        .collect::<Vec<_>>();
    seen.sort();
    assert_eq!(seen, vec!["req-1:1", "req-1:1", "req-2:1"]);
}

#[test]
fn test_field_span_missing_field_strict_errors() {
    let input = r#"{"msg":"missing id"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--span", "request_id", "--strict"], input);

    assert_eq!(exit_code, 1);
    assert!(stderr.contains("missing required field 'request_id'"));
}

#[test]
fn test_idle_span_forward_only_gaps() {
    let input = r#"{"ts":"2025-01-15T10:00:10Z","msg":"first"}
{"ts":"2025-01-15T10:00:05Z","msg":"out_of_order"}
{"ts":"2025-01-15T10:00:20Z","msg":"after_gap"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span-idle",
            "5s",
            "--span-close",
            "print(span.id + ':' + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains(":2"), "first span should have 2 events");
    assert!(stdout.contains(":1"), "second span should have 1 event");
}

// Regression: a --span duration that fits in i64 milliseconds but exceeds
// chrono's representable datetime range used to abort the process (panic in
// ms_to_datetime, exit 134 under the release panic=abort profile). The window
// boundary must now clamp to the representable range and complete cleanly.
#[test]
fn test_time_span_huge_duration_does_not_panic() {
    let input = "ts=2024-01-01T00:00:00Z a=1\nts=2024-01-02T00:00:00Z a=2\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--span",
            "1000000000d",
            "--span-close",
            "print(\"closed:\" + span.size.to_string());",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "should not abort; stderr: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "must not panic; stderr: {stderr}"
    );
    // Both events fall into the single (clamped) window.
    assert!(stdout.contains("closed:2"), "stdout: {stdout}");
}

// --- --span-summary ---------------------------------------------------------

/// Timestamped input spanning four one-minute windows with an uneven split, so
/// a row's count cannot accidentally match the window index.
const SUMMARY_INPUT: &str = r#"{"ts":"2024-01-15T10:00:00Z","level":"INFO"}
{"ts":"2024-01-15T10:00:30Z","level":"DEBUG"}
{"ts":"2024-01-15T10:00:45Z","level":"INFO"}
{"ts":"2024-01-15T10:01:00Z","level":"ERROR"}
{"ts":"2024-01-15T10:03:00Z","level":"WARN"}"#;

#[test]
fn span_summary_text_is_one_key_value_line_per_span() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=text"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z  events=3",
            "2024-01-15T10:01:00Z  events=1",
            "2024-01-15T10:03:00Z  events=1",
        ]
    );
}

#[test]
fn span_summary_tsv_is_long_form_with_an_empty_key_column() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=tsv"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z\tevents\t\t3",
            "2024-01-15T10:01:00Z\tevents\t\t1",
            "2024-01-15T10:03:00Z\tevents\t\t1",
        ]
    );
}

#[test]
fn span_summary_json_carries_window_bounds_and_nested_metrics() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-summary=json",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    let first = stdout.lines().next().expect("a row");
    assert_eq!(
        first,
        r#"{"span":"2024-01-15T10:00:00Z/1m","start":"2024-01-15T10:00:00Z","end":"2024-01-15T10:01:00Z","first_ts":"2024-01-15T10:00:00Z","last_ts":"2024-01-15T10:00:45Z","events":3,"metrics":{"level":{"DEBUG":1,"INFO":2}}}"#
    );
}

#[test]
fn span_summary_labels_time_spans_by_window_start() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=text"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.starts_with("2024-01-15T10:00:00Z  "), "{stdout}");
}

#[test]
fn span_summary_labels_count_spans_by_index() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "2", "--span-summary=text"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    // Five events at --span 2: two full spans plus the trailing partial one,
    // which still closes (and so still reports) at the end of the run.
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["#0  events=2", "#1  events=2", "#2  events=1"]
    );
}

#[test]
fn span_summary_labels_field_spans_by_field_value() {
    let input = r#"{"svc":"api","n":1}
{"svc":"api","n":2}
{"svc":"db","n":3}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "svc", "--span-summary=text"],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["api  events=2", "db  events=1"]
    );
}

#[test]
fn span_summary_labels_idle_spans_by_session_start() {
    let input = r#"{"ts":"2024-01-15T10:00:00Z","n":1}
{"ts":"2024-01-15T10:00:10Z","n":2}
{"ts":"2024-01-15T11:00:00Z","n":3}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span-idle", "5m", "--span-summary=text"],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z  events=2",
            "2024-01-15T11:00:00Z  events=1"
        ]
    );
}

#[test]
fn span_summary_carries_per_window_freq_deltas_and_hides_the_global_table() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-summary=text",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z  events=3  level.DEBUG=1 level.INFO=2",
            "2024-01-15T10:01:00Z  events=1  level.ERROR=1",
            "2024-01-15T10:03:00Z  events=1  level.WARN=1",
        ]
    );
    // The cumulative table --freq would otherwise imply yields to the rollup.
    assert!(!stdout.contains("level\tINFO\t3"), "{stdout}");
}

#[test]
fn explicit_metrics_flag_still_prints_the_cumulative_table_after_the_rows() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-summary=text",
            "--metrics=tsv",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    let rows = stdout
        .find("2024-01-15T10:00:00Z  events=3")
        .expect("a row");
    let table = stdout.find("level\tINFO\t2").expect("the global table");
    assert!(rows < table, "table must follow the rows: {stdout}");
}

#[test]
fn span_summary_without_a_span_mode_is_a_usage_error() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--span-summary"], SUMMARY_INPUT);

    assert_eq!(exit_code, 2);
    assert!(
        stderr.contains("--span-summary requires --span"),
        "{stderr}"
    );
}

#[test]
fn span_summary_emits_nothing_under_silent() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--span-summary=text",
            "--silent",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty(), "stdout: {stdout}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn span_summary_emits_no_rows_for_an_empty_run() {
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--span", "1m", "--span-summary=text"], "");

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty(), "stdout: {stdout}");
}

#[test]
fn span_summary_skips_windows_with_no_events_rather_than_emitting_zeroes() {
    // 10:02 has no events; the rollup is sparse, not gap-filled.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=text"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(!stdout.contains("10:02:00Z"), "{stdout}");
    assert_eq!(stdout.trim_end().lines().count(), 3);
}

#[test]
fn span_summary_rows_survive_no_script_output() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--span-summary=text",
            "--no-script-output",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(
        stdout.contains("2024-01-15T10:00:00Z  events=3"),
        "{stdout}"
    );
}

#[test]
fn span_summary_runs_the_close_hook_before_each_row() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--span-summary=text",
            "--span-close",
            "print(`HOOK ${span.label}`)",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().take(2).collect::<Vec<_>>(),
        vec![
            "HOOK 2024-01-15T10:00:00Z",
            "2024-01-15T10:00:00Z  events=3"
        ]
    );
}

#[test]
fn tabs_and_newlines_in_a_field_label_cannot_corrupt_a_tsv_record() {
    let input = "{\"rid\":\"a\\tb\",\"n\":1}\n{\"rid\":\"c\\nd\",\"n\":2}";

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "rid", "--span-summary=tsv"],
        input,
    );

    assert_eq!(exit_code, 0);
    let lines: Vec<&str> = stdout.trim_end().lines().collect();
    assert_eq!(lines, vec!["a b\tevents\t\t1", "c d\tevents\t\t1"]);
    for line in lines {
        assert_eq!(line.matches('\t').count(), 3, "column count: {line}");
    }
}

#[test]
fn span_summary_warns_that_late_events_are_in_no_row() {
    let input = r#"{"ts":"2024-01-15T10:00:00Z","n":1}
{"ts":"2024-01-15T10:02:00Z","n":2}
{"ts":"2024-01-15T10:00:30Z","n":3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=text"],
        input,
    );

    assert_eq!(exit_code, 0);
    // The late event is in neither row: 1 + 1, not 3.
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z  events=1",
            "2024-01-15T10:02:00Z  events=1"
        ]
    );
    assert!(
        stderr.contains("arrived after their window had closed"),
        "{stderr}"
    );
}

#[test]
fn internal_metric_keys_never_reach_span_rows_or_span_metrics() {
    // #418: --card's internal values-seen counter leaked as a user metric.
    for format in ["text", "tsv", "json"] {
        let summary = format!("--span-summary={format}");
        let (stdout, _stderr, exit_code) = run_kelora_with_input(
            &["-f", "json", "--span", "2", "--card", "level", &summary],
            SUMMARY_INPUT,
        );
        assert_eq!(exit_code, 0);
        assert!(!stdout.contains("__kelora"), "{format}: {stdout}");
        assert!(stdout.contains("events"), "{format}: {stdout}");
    }

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--exec",
            r#"track_cardinality("lv", e.level)"#,
            "--span-close",
            "print(span.metrics.keys())",
        ],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0);
    assert!(!stdout.contains("__kelora"), "{stdout}");
}

#[test]
fn span_close_warns_that_late_events_reached_no_span() {
    // #417: the hook loses late events exactly like the summary does.
    let input = r#"{"ts":"2024-01-15T10:00:00Z","n":1}
{"ts":"2024-01-15T10:02:00Z","n":2}
{"ts":"2024-01-15T10:00:30Z","n":3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--span-close",
            "print(span.size)",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["1", "1"]
    );
    assert!(
        stderr.contains("1 event(s) arrived after their window had closed and reached no span"),
        "{stderr}"
    );

    // Tagging alone assigns each event its own window, so nothing is lost.
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--exec", "e.w = meta.span_id"],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("arrived after"), "{stderr}");
}

#[test]
fn span_summary_warns_when_no_event_can_be_placed_in_a_window() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--span-summary=text"],
        "{\"a\":1}\n{\"a\":2}",
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty(), "stdout: {stdout}");
    assert!(stderr.contains("no usable timestamp"), "{stderr}");
}

#[test]
fn span_summary_warns_that_interleaved_field_values_split_into_runs() {
    let input = r#"{"rid":"r1","n":1}
{"rid":"r2","n":2}
{"rid":"r1","n":3}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "rid", "--span-summary=text"],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stdout.trim_end().lines().count(), 3);
    assert!(stderr.contains("more than one row for 'r1'"), "{stderr}");
}

#[test]
fn span_summary_collapses_the_non_additive_warning_into_one_line() {
    let input = r#"{"ts":"2024-01-15T10:00:00Z","lat":10}
{"ts":"2024-01-15T10:01:00Z","lat":20}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--describe",
            "lat",
            "--span-summary=text",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    let omitted = stderr
        .lines()
        .filter(|line| line.contains("non-additive"))
        .count();
    assert_eq!(omitted, 1, "one collapsed line, not one per key: {stderr}");
    assert!(stderr.contains("lat_p95"), "{stderr}");
}

#[test]
fn span_summary_warns_when_a_metric_shadows_the_events_column() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--exec",
            r#"track_sum("events", 1)"#,
            "--span-summary=text",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.contains("built-in event-count column"), "{stderr}");
}

#[test]
fn json_rows_keep_a_shadowing_metric_separate_and_stay_quiet() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--exec",
            r#"track_sum("events", 1)"#,
            "--span-summary=json",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("built-in event-count column"), "{stderr}");
    assert!(
        stdout.contains(r#""events":3,"metrics":{"events":3}"#),
        "{stdout}"
    );
}

// --- span hints ------------------------------------------------------------

#[test]
fn hint_names_span_summary_when_a_span_only_tags_events() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--span", "1m"], SUMMARY_INPUT);

    assert_eq!(exit_code, 0);
    assert!(stderr.contains("only tagging events"), "{stderr}");
    assert!(stderr.contains("--span-summary"), "{stderr}");
}

#[test]
fn no_inert_span_hint_when_a_stage_reads_the_span_metadata() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--exec", "e.w = meta.span_id"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("only tagging events"), "{stderr}");
}

#[test]
fn hint_says_freq_ignores_the_window_without_span_summary() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1m", "--freq", "level"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(
        stderr.contains("--freq aggregates the whole run"),
        "{stderr}"
    );
    // The generic inert-span hint would only repeat this, less specifically.
    assert!(!stderr.contains("only tagging events"), "{stderr}");
}

#[test]
fn hint_says_span_close_output_is_discarded_under_a_data_only_mode() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-close",
            "print(1)",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(
        stderr.contains("--span-close output is discarded"),
        "{stderr}"
    );
    // --script-output wins against data-only modes (#379), so the hint offers
    // it as the route that keeps the hook's own output.
    assert!(stderr.contains("--script-output"), "{stderr}");
}

#[test]
fn span_close_print_survives_a_data_only_mode_with_explicit_script_output() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-close",
            "print(\"HOOK\")",
            "--script-output",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.contains("HOOK"), "{stdout}");
    // The problem the discarded-output hint flags no longer exists.
    assert!(
        !stderr.contains("--span-close output is discarded"),
        "{stderr}"
    );
}

#[test]
fn span_hints_go_quiet_once_span_summary_is_present() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--span-summary=text",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("--span-summary"), "{stderr}");
}

#[test]
fn no_hints_silences_the_span_hints() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--freq",
            "level",
            "--no-hints",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("aggregates the whole run"), "{stderr}");
}

// --- hook accessors --------------------------------------------------------

#[test]
fn span_metric_returns_zero_for_a_window_with_no_delta() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--exec",
            r#"track_freq("level", e.level)"#,
            "--span-close",
            r#"print(`${span.label} ${span.metric("level.ERROR")} ${span.metric("absent")}`)"#,
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z 0 0",
            "2024-01-15T10:01:00Z 1 0",
            "2024-01-15T10:03:00Z 0 0",
        ]
    );
}

#[test]
fn span_metric_returns_unit_for_an_omitted_non_additive_metric() {
    // #419: a structurally unavailable metric must not read as a 0.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--exec",
            r#"track_percentiles("p", e.ts.len()); track_cardinality("c", e.level); track_inc("n")"#,
            "--span-close",
            r#"print(`${span.metric("n")} ${type_of(span.metric("p_p95"))} ${type_of(span.metric("c"))} ${span.metric("absent")}`)"#,
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().next(),
        Some("3 () () 0"),
        "{stdout}"
    );
}

#[test]
fn first_ts_and_last_ts_are_observed_bounds_in_every_mode() {
    // #383: count and field spans have no window grid, but their events do
    // have timestamps.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--span-close",
            r#"print(`${span.id} ${span.first_ts.to_iso()} ${span.last_ts.to_iso()} ${type_of(span.start)}`)"#,
        ],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0);
    let first = stdout.trim_end().lines().next().unwrap_or_default();
    assert!(first.starts_with("#0 2024-01-15T10:00:00"), "{stdout}");
    assert!(first.contains(" 2024-01-15T10:00:30"), "{stdout}");
    assert!(first.ends_with(" ()"), "{stdout}");

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "2", "--span-summary=json"],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0);
    assert!(
        stdout.starts_with(
            r##"{"span":"#0","first_ts":"2024-01-15T10:00:00Z","last_ts":"2024-01-15T10:00:30Z","events":2"##
        ),
        "{stdout}"
    );

    // No timestamps at all: () in the hook, keys absent from the row.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--span-close",
            "print(type_of(span.first_ts))",
        ],
        "{\"a\":1}\n{\"a\":2}",
    );
    assert_eq!(exit_code, 0);
    assert_eq!(stdout.trim_end(), "()");
}

#[test]
fn min_and_max_are_per_window_values() {
    // #380: each window reports its own extremes, not the run's, while -m
    // still reports the run's.
    let input = r#"{"ts":"2024-01-15T10:00:00Z","v":50}
{"ts":"2024-01-15T10:00:10Z","v":7}
{"ts":"2024-01-15T10:01:00Z","v":3}
{"ts":"2024-01-15T10:01:10Z","v":9}
{"ts":"2024-01-15T10:02:00Z"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--exec",
            r#"if e.has("v") { track_max("hi", e.v); track_min("lo", e.v) }"#,
            "--span-summary=text",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("non-additive"), "{stderr}");
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec![
            "2024-01-15T10:00:00Z  events=2  hi=50  lo=7",
            "2024-01-15T10:01:00Z  events=2  hi=9  lo=3",
            "2024-01-15T10:02:00Z  events=1",
        ]
    );

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--exec",
            r#"if e.has("v") { track_max("hi", e.v) }"#,
            "--span-close",
            r#"print(`${span.metric("hi")} ${type_of(span.metric("hi"))}`)"#,
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    // The last window recorded no value: (), not 0.
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["50 i64", "9 i64", " ()"]
    );
}

#[test]
fn idle_span_ids_use_z_like_time_span_ids() {
    // #421: idle ids used `+00:00` where time ids and labels use `Z`.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span-idle",
            "30s",
            "--span-close",
            "print(span.id)",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().next(),
        Some("idle-#0-2024-01-15T10:00:00Z"),
        "{stdout}"
    );
}

#[test]
fn span_label_falls_back_to_the_id_when_there_is_no_start() {
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "2",
            "--span-close",
            "print(span.label)",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["#0", "#1", "#2"]
    );
}

#[test]
fn span_size_is_the_included_count_not_the_retained_event_count() {
    // Guards the split between event retention and metric baselines: a hook
    // reads span.events, so both are on and the two must agree.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--span",
            "1m",
            "--span-close",
            "print(`${span.size} ${span.events.len()}`)",
        ],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        stdout.trim_end().lines().collect::<Vec<_>>(),
        vec!["3 3", "1 1", "1 1"]
    );
}

#[test]
fn time_span_ignores_events_outside_since_until() {
    // Events the --since/--until window drops must not open spans: no rows for
    // windows outside the selected range, not even empty ones.
    let input = r#"{"ts":"2024-01-01T09:50:00Z","level":"ERROR"}
{"ts":"2024-01-01T10:05:00Z","level":"ERROR"}
{"ts":"2024-01-01T10:20:00Z","level":"INFO"}
{"ts":"2024-01-01T10:40:00Z","level":"ERROR"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-l",
            "error",
            "--since",
            "2024-01-01T10:00:00Z",
            "--until",
            "2024-01-01T10:29:59Z",
            "--span",
            "15m",
            "--span-summary=text",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout, "2024-01-01T10:00:00Z  events=1\n2024-01-01T10:15:00Z  events=0\n",
        "only windows inside the range; the filtered INFO event keeps its window"
    );
}

// --- duration validation -----------------------------------------------------

#[test]
fn sub_millisecond_span_is_a_usage_error_not_a_panic() {
    // #414: a sub-ms duration truncated to 0 ms and divided by zero during
    // window alignment, aborting the process under the release profile.
    for spec in ["999us", "500us", "1us", "1ns"] {
        let (_stdout, stderr, exit_code) = run_kelora_with_input(
            &["-f", "json", "--span", spec, "--span-summary=text"],
            SUMMARY_INPUT,
        );
        assert_eq!(exit_code, 2, "{spec}: {stderr}");
        assert!(
            stderr.contains("--span duration must be at least 1ms"),
            "{spec}: {stderr}"
        );
        assert!(!stderr.contains("panicked"), "{spec}: {stderr}");
    }

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1ms", "--span-summary=text"],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0, "{stderr}");
}

#[test]
fn hint_names_a_span_field_that_no_event_carries() {
    // #416: a misspelled --span FIELD puts every event in one '(unset)' span;
    // the hint must survive --span-summary's hint hush and suggest the field.
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "levle", "--span-summary=text"],
        SUMMARY_INPUT,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stdout.trim_end(), "(unset)  events=5");
    assert!(
        stderr.contains("--span field 'levle' was not present on any event"),
        "{stderr}"
    );
    assert!(stderr.contains("Did you mean 'level'?"), "{stderr}");
}

#[test]
fn no_span_field_hint_when_the_field_is_on_some_events_or_hints_are_off() {
    let input = "{\"a\":1}\n{\"svc\":\"x\",\"a\":2}";
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "svc", "--span-summary=text"],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("not present on any event"), "{stderr}");

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "levle",
            "--span-summary=text",
            "--no-hints",
        ],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0);
    assert!(!stderr.contains("not present on any event"), "{stderr}");
}

#[test]
fn no_timestamp_warning_mentions_a_field_the_duration_shadows() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "5m", "--span-summary=text"],
        "{\"5m\":\"a\"}\n{\"5m\":\"b\"}",
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.contains("no usable timestamp"), "{stderr}");
    assert!(
        stderr.contains("reads '5m' as a duration first"),
        "{stderr}"
    );
}

#[test]
fn calendar_span_units_are_rejected_with_a_fixed_length_suggestion() {
    // #415: epoch-modulo alignment starts weeks on Thursday and lets months and
    // years drift, so these units are refused rather than silently misaligned.
    for spec in ["1w", "2weeks", "1M", "1month", "1y", "1year"] {
        let (_stdout, stderr, exit_code) = run_kelora_with_input(
            &["-f", "json", "--span", spec, "--span-summary=text"],
            SUMMARY_INPUT,
        );
        assert_eq!(exit_code, 2, "{spec}: {stderr}");
        assert!(stderr.contains("epoch-aligned in UTC"), "{spec}: {stderr}");
        assert!(stderr.contains("--span 7d"), "{spec}: {stderr}");
    }

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1d", "--span-summary=text"],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 0, "{stderr}");
}

#[test]
fn a_digit_led_span_spec_that_is_no_duration_names_the_units() {
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--span", "1mo", "--span-summary=text"],
        SUMMARY_INPUT,
    );
    assert_eq!(exit_code, 2, "{stderr}");
    assert!(
        stderr.contains("Unrecognised --span duration '1mo'"),
        "{stderr}"
    );
    assert!(stderr.contains("ms, s, m, h, d"), "{stderr}");
    assert!(!stderr.contains("field name"), "{stderr}");
}
