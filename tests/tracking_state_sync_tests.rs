//! Behavior lock-in for the per-event tracking-state sync.
//!
//! Every `track_*` metric lives in a thread-local map while a script stage
//! runs and in `PipelineContext` between stages, so the pipeline hands the map
//! to the engine before each stage and takes it back afterwards. These tests
//! pin the observable consequences of that hand-off — final metric values,
//! sequential/parallel agreement, batch boundaries, span windows, error paths —
//! so the sync can be reimplemented without silently losing counts.

mod common;
use common::*;

/// Metrics as JSON on stdout: `--metrics=json` in a data-only mode gives one
/// object with every user metric, which is far easier to assert on than the
/// human-readable table.
fn metrics_json(args: &[&str], input: &str) -> serde_json::Value {
    let (stdout, stderr, exit_code) = run_kelora_with_input(args, input);
    assert_eq!(
        exit_code, 0,
        "kelora should exit 0; stderr was:\n{}",
        stderr
    );
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("metrics should be JSON ({e}); stdout was:\n{stdout}"))
}

fn sample_events() -> String {
    let mut input = String::new();
    for (i, level) in ["INFO", "ERROR", "INFO", "WARN", "INFO", "ERROR"]
        .iter()
        .enumerate()
    {
        input.push_str(&format!(
            r#"{{"ts":"2026-01-01T10:0{}:00Z","level":"{}","u":"u{}","lat":{}}}"#,
            i / 3,
            level,
            i % 4,
            (i + 1) * 10
        ));
        input.push('\n');
    }
    input
}

#[test]
fn freq_accumulates_across_a_multi_stage_pipeline() {
    // A filter stage and an exec stage both hand the tracking state back and
    // forth before --freq's stage sees it; the counts must survive all three.
    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "--filter",
            "e.lat > 0",
            "--exec",
            r#"track_sum("lat_total", e.lat)"#,
            "--freq",
            "level",
            "--metrics=json",
        ],
        &sample_events(),
    );

    assert_eq!(metrics["level"]["INFO"], 3);
    assert_eq!(metrics["level"]["ERROR"], 2);
    assert_eq!(metrics["level"]["WARN"], 1);
    assert_eq!(metrics["lat_total"], 210);
}

#[test]
fn every_track_family_survives_the_per_event_sync() {
    // One run per storage shape: a map (freq), an array (unique, top), a
    // serialized sketch (cardinality, percentiles) and plain scalars.
    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--exec",
            r#"track_freq("lvl", e.level);
               track_unique("users", e.u);
               track_top("busiest", e.u, 2);
               track_cardinality("u_card", e.u);
               track_stats("lat", e.lat);
               track_sum("lat_running", e.lat);
               track_min("lat_lo", e.lat);
               track_max("lat_hi", e.lat);"#,
        ],
        &sample_events(),
    );

    assert_eq!(metrics["lvl"]["INFO"], 3);
    assert_eq!(metrics["lvl"]["ERROR"], 2);
    assert_eq!(metrics["lvl"]["WARN"], 1);

    let users = metrics["users"]
        .as_array()
        .expect("track_unique should produce an array");
    assert_eq!(users.len(), 4, "u0..u3 are the distinct users: {users:?}");

    assert_eq!(metrics["u_card"], 4, "HLL is clamped to a whole number");
    assert_eq!(metrics["lat_count"], 6);
    assert_eq!(metrics["lat_min"], 10);
    assert_eq!(metrics["lat_max"], 60);
    assert_eq!(metrics["lat_avg"], 35);
    assert_eq!(metrics["lat_sum"], 210, "track_stats keeps its own sum");
    assert_eq!(metrics["lat_running"], 210.0);
    assert_eq!(metrics["lat_lo"], 10);
    assert_eq!(metrics["lat_hi"], 60);

    let busiest = metrics["busiest"]
        .as_array()
        .expect("track_top should produce an array");
    assert!(
        !busiest.is_empty(),
        "track_top should rank something: {busiest:?}"
    );
}

#[test]
fn sequential_and_parallel_agree_on_every_metric() {
    let input = sample_events();
    let base = [
        "-f",
        "json",
        "-m",
        "--metrics=json",
        "--exec",
        r#"track_freq("lvl", e.level);
           track_cardinality("u_card", e.u);
           track_stats("lat", e.lat);
           track_sum("lat_sum", e.lat);"#,
    ];

    let sequential = metrics_json(&base, &input);

    let mut parallel_args = base.to_vec();
    parallel_args.push("--parallel");
    let parallel = metrics_json(&parallel_args, &input);

    assert_eq!(
        sequential, parallel,
        "sequential and --parallel must report identical metrics"
    );
}

#[test]
fn parallel_metrics_are_complete_at_every_batch_size() {
    // Each worker batch harvests the tracking state once at its boundary; a
    // batch size of 1 makes every event a boundary, so a harvest that missed
    // either half of the state (thread-local or context) shows up here.
    let input = sample_events();

    for batch_size in ["1", "2", "5", "64"] {
        let metrics = metrics_json(
            &[
                "-f",
                "json",
                "--parallel",
                "--batch-size",
                batch_size,
                "--freq",
                "level",
                "--card",
                "u",
                "--metrics=json",
            ],
            &input,
        );

        assert_eq!(
            metrics["level"]["INFO"], 3,
            "batch size {batch_size} lost INFO counts: {metrics}"
        );
        assert_eq!(
            metrics["level"]["ERROR"], 2,
            "batch size {batch_size} lost ERROR counts: {metrics}"
        );
        assert_eq!(
            metrics["level"]["WARN"], 1,
            "batch size {batch_size} lost WARN counts: {metrics}"
        );
        assert_eq!(
            metrics["u"], 4,
            "batch size {batch_size} lost cardinality: {metrics}"
        );
    }
}

#[test]
fn freq_counts_every_event_of_a_high_cardinality_field() {
    // The frequency map grows to one entry per event here, which is the shape
    // that makes a per-event copy of the tracking state expensive. Correctness
    // first: nothing may be dropped as the map grows.
    const EVENTS: usize = 2_000;

    let mut input = String::new();
    for i in 0..EVENTS {
        input.push_str(&format!("{{\"id\":\"v{i}\"}}\n"));
    }

    let metrics = metrics_json(&["-f", "json", "--freq", "id", "--metrics=json"], &input);

    let table = metrics["id"]
        .as_object()
        .expect("--freq should produce a value→count map");
    assert_eq!(table.len(), EVENTS, "one entry per distinct value");
    assert!(
        table.values().all(|count| count == &serde_json::json!(1)),
        "every distinct value was seen exactly once"
    );
}

#[test]
fn metrics_from_healthy_events_survive_a_failing_script() {
    // An exec error on one event must not cost the other events their counts.
    let input = concat!(
        "{\"level\":\"INFO\",\"n\":1}\n",
        "{\"level\":\"ERROR\",\"n\":2}\n",
        "{\"level\":\"INFO\",\"n\":3}\n",
        "{\"level\":\"WARN\",\"n\":4}\n",
    );

    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--exec",
            r#"track_freq("lvl", e.level); if e.n == 2 { throw "boom" }"#,
        ],
        input,
    );

    assert_eq!(metrics["lvl"]["INFO"], 2);
    assert_eq!(metrics["lvl"]["WARN"], 1);
    assert!(
        metrics["lvl"]["ERROR"].is_null(),
        "the failing event's own contribution is rolled back with the rest of \
         its stage: {metrics}"
    );
}

/// One case per `track_*` function: the script records a value and then throws,
/// so the metric must come out exactly as if the event had never been seen.
///
/// A stage that errors falls back to the state it had before it ran — the event
/// keeps its old fields, `emit_each` output and queued file writes are dropped.
/// Metrics are part of that fallback, and this is the list that keeps a newly
/// added `track_*` from quietly opting out of it.
const ATOMIC_TRACK_CALLS: &[(&str, &str)] = &[
    ("track_freq", r#"track_freq("m", e.level)"#),
    ("track_sum", r#"track_sum("m", e.n)"#),
    ("track_inc", r#"track_inc("m")"#),
    ("track_avg", r#"track_avg("m", e.n)"#),
    ("track_min", r#"track_min("m", e.n)"#),
    ("track_max", r#"track_max("m", e.n)"#),
    ("track_unique", r#"track_unique("m", e.level)"#),
    ("track_cardinality", r#"track_cardinality("m", e.level)"#),
    ("track_percentiles", r#"track_percentiles("m", e.n)"#),
    ("track_stats", r#"track_stats("m", e.n)"#),
    ("track_top", r#"track_top("m", e.level, 3)"#),
    ("track_bottom", r#"track_bottom("m", e.level, 3)"#),
    ("track_top_by", r#"track_top_by("m", e.level, e.n, 3)"#),
    (
        "track_bottom_by",
        r#"track_bottom_by("m", e.level, e.n, 3)"#,
    ),
];

#[test]
fn every_track_function_is_undone_when_its_stage_fails() {
    // Two events: the first records normally, the second records and then
    // throws. Every metric must read exactly as it does for the first event
    // alone — which is what the no-throw baseline in the same run measures.
    let input = "{\"level\":\"A\",\"n\":1}\n{\"level\":\"B\",\"n\":2}\n";

    for (name, call) in ATOMIC_TRACK_CALLS {
        // Stages run in the order they appear, so the filter has to come first
        // for the baseline to mean "only the first event was ever recorded".
        let baseline = metrics_json(
            &[
                "-f",
                "json",
                "-m",
                "--metrics=json",
                "--no-diagnostics",
                "--filter",
                "e.n == 1",
                "--exec",
                call,
            ],
            input,
        );

        let throwing = format!(r#"{call}; if e.n == 2 {{ throw "boom" }}"#);
        let rolled_back = metrics_json(
            &[
                "-f",
                "json",
                "-m",
                "--metrics=json",
                "--no-diagnostics",
                "--exec",
                &throwing,
            ],
            input,
        );

        assert_eq!(
            baseline, rolled_back,
            "{name}: a stage that throws after recording must leave the metric \
             as if the event had not been seen\n  expected (first event only): \
             {baseline}\n  got (second event threw):    {rolled_back}"
        );
    }
}

#[test]
fn a_rolled_back_metric_does_not_linger_as_an_empty_shell() {
    // The failing event is the only one that ever records, so undoing it has to
    // remove the metric outright rather than leave an empty table behind.
    let input = "{\"level\":\"A\",\"n\":1}\n";

    for (name, call) in ATOMIC_TRACK_CALLS {
        let throwing = format!(r#"{call}; throw "boom""#);
        let metrics = metrics_json(
            &[
                "-f",
                "json",
                "-m",
                "--metrics=json",
                "--no-diagnostics",
                "--exec",
                &throwing,
            ],
            input,
        );

        assert_eq!(
            metrics,
            serde_json::json!({}),
            "{name}: rolling back the only recorded value should leave no metric"
        );
    }
}

#[test]
fn rollback_is_the_same_wherever_the_failing_event_sits() {
    // The previous implementation rolled back by letting the next event
    // overwrite the failed one's state, so a throw on the *last* event kept its
    // contribution while the same throw one line earlier lost it.
    let three = "{\"level\":\"A\"}\n{\"level\":\"B\"}\n{\"level\":\"C\"}\n";

    let fails_first = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--exec",
            r#"track_freq("lvl", e.level); if e.level == "A" { throw "x" }"#,
        ],
        three,
    );
    let fails_last = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--exec",
            r#"track_freq("lvl", e.level); if e.level == "C" { throw "x" }"#,
        ],
        three,
    );

    assert!(
        fails_first["lvl"]["A"].is_null(),
        "a throw on the first event drops its count: {fails_first}"
    );
    assert!(
        fails_last["lvl"]["C"].is_null(),
        "a throw on the last event must drop its count too: {fails_last}"
    );
    assert_eq!(fails_first["lvl"]["B"], 1);
    assert_eq!(fails_last["lvl"]["B"], 1);
}

#[test]
fn rollback_does_not_depend_on_how_input_is_batched() {
    // The old rollback happened by letting the next event overwrite the failed
    // one, so under --parallel it stopped at the batch boundary: the same log
    // reported the failing event's count with --batch-size 1 or 2 and dropped
    // it with 4. The answer must not depend on batching at all.
    let input = concat!(
        "{\"level\":\"A\",\"n\":1}\n",
        "{\"level\":\"B\",\"n\":2}\n",
        "{\"level\":\"C\",\"n\":3}\n",
        "{\"level\":\"D\",\"n\":4}\n",
    );
    let script = r#"track_freq("lvl", e.level); if e.n == 2 { throw "x" }"#;

    let sequential = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--exec",
            script,
        ],
        input,
    );
    assert!(
        sequential["lvl"]["B"].is_null(),
        "sequential drops the failing event's count: {sequential}"
    );

    for batch_size in ["1", "2", "4", "64"] {
        let parallel = metrics_json(
            &[
                "-f",
                "json",
                "-m",
                "--metrics=json",
                "--no-diagnostics",
                "--parallel",
                "--batch-size",
                batch_size,
                "--exec",
                script,
            ],
            input,
        );
        assert_eq!(
            parallel, sequential,
            "--batch-size {batch_size} must not change which events counted"
        );
    }
}

#[test]
fn rollback_covers_only_the_stage_that_failed() {
    // Stages are independent transactions: an earlier stage that completed
    // keeps its record of the event, exactly as it keeps the event mutations it
    // made before a later stage threw.
    let input = "{\"level\":\"A\"}\n{\"level\":\"B\"}\n";

    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--exec",
            r#"track_freq("first", e.level)"#,
            "--exec",
            r#"track_freq("second", e.level); if e.level == "B" { throw "x" }"#,
        ],
        input,
    );

    assert_eq!(metrics["first"]["A"], 1);
    assert_eq!(
        metrics["first"]["B"], 1,
        "the completed stage keeps its count"
    );
    assert_eq!(metrics["second"]["A"], 1);
    assert!(
        metrics["second"]["B"].is_null(),
        "the failing stage's own count is undone: {metrics}"
    );
}

#[test]
fn a_failing_filter_stage_also_rolls_its_metrics_back() {
    // Filters are gates rather than transforms, but a failed one is still a
    // stage that did not complete. A filter is a single expression, so it
    // records through `&&` and fails on a division by zero for the second
    // event only.
    let input = "{\"level\":\"A\",\"n\":1}\n{\"level\":\"B\",\"n\":2}\n";

    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "-m",
            "--metrics=json",
            "--no-diagnostics",
            "--filter",
            r#"track_freq("lvl", e.level) == () && (e.n == 1 || (1/(e.n - 2)) > 0)"#,
        ],
        input,
    );

    assert_eq!(metrics["lvl"]["A"], 1);
    assert!(
        metrics["lvl"]["B"].is_null(),
        "the failing filter's own count is undone: {metrics}"
    );
}

#[test]
fn rollback_leaves_the_error_counters_alone() {
    // The internal half of the tracking state records that the failure
    // happened, so it is never part of the rollback.
    let input = "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n";

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--exec",
            r#"track_inc("seen"); throw "always""#,
        ],
        input,
    );

    assert_eq!(exit_code, 0, "exec errors are non-fatal");
    assert!(
        stderr.contains("Exec errors: 3 total"),
        "every failure is still counted; stderr was:\n{stderr}"
    );
}

#[test]
fn error_counters_survive_an_error_on_every_event() {
    // The error tally lives in the internal half of the tracking state and is
    // written on the path that skips the normal hand-off, so a run where every
    // event fails must still report every failure, not just the last one.
    let mut input = String::new();
    for i in 0..12 {
        input.push_str(&format!("{{\"n\":{i}}}\n"));
    }

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-q", "--exec", r#"throw "always""#], &input);

    assert_eq!(exit_code, 0, "exec errors are non-fatal");
    assert!(
        stderr.contains("Exec errors: 12 total"),
        "every failing event should be counted; stderr was:\n{stderr}"
    );
}

#[test]
fn span_windows_report_their_own_metric_deltas() {
    // Per-window metrics are a diff of the tracking state at span open against
    // its state at span close, so both snapshots have to be the live map.
    let input = concat!(
        "{\"ts\":\"2026-01-01T10:00:00Z\",\"level\":\"INFO\"}\n",
        "{\"ts\":\"2026-01-01T10:00:30Z\",\"level\":\"ERROR\"}\n",
        "{\"ts\":\"2026-01-01T10:01:10Z\",\"level\":\"INFO\"}\n",
        "{\"ts\":\"2026-01-01T10:01:40Z\",\"level\":\"WARN\"}\n",
    );

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--span",
            "1m",
            "--span-summary",
            "--freq",
            "level",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "stderr was:\n{stderr}");
    let rows: Vec<&str> = stdout.lines().collect();
    assert!(
        rows.contains(&"2026-01-01T10:00:00Z\tlevel\tINFO\t1"),
        "first window counts only its own INFO event: {rows:?}"
    );
    assert!(
        rows.contains(&"2026-01-01T10:00:00Z\tlevel\tERROR\t1"),
        "first window counts its ERROR event: {rows:?}"
    );
    assert!(
        rows.contains(&"2026-01-01T10:01:00Z\tlevel\tINFO\t1"),
        "second window must not inherit the first window's INFO count: {rows:?}"
    );
    assert!(
        rows.contains(&"2026-01-01T10:01:00Z\tlevel\tWARN\t1"),
        "second window counts its WARN event: {rows:?}"
    );
}

#[test]
fn end_script_sees_the_final_metric_state() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-q",
            "--exec",
            r#"track_freq("lvl", e.level); track_sum("total", 1)"#,
            "--end",
            r#"print(`${metrics.lvl.INFO}/${metrics.total}`)"#,
        ],
        &sample_events(),
    );

    assert_eq!(exit_code, 0, "stderr was:\n{stderr}");
    assert_eq!(
        stdout.trim(),
        "3/6",
        "--end should see every event's contribution"
    );
}

#[test]
fn time_window_narrows_the_aggregate_not_just_the_output() {
    let metrics = metrics_json(
        &[
            "-f",
            "json",
            "--since",
            "2026-01-01T10:01:00Z",
            "--freq",
            "level",
            "--metrics=json",
        ],
        &sample_events(),
    );

    let table = metrics["level"]
        .as_object()
        .expect("--freq should produce a value→count map");
    let total: i64 = table.values().filter_map(|v| v.as_i64()).sum();
    assert_eq!(
        total, 3,
        "only the three events inside the window may be counted: {metrics}"
    );
}
