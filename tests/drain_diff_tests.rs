// Integration tests for --drain-diff (template-level baseline/target comparison)

mod common;
use common::run_kelora;

use std::io::Write;
use tempfile::NamedTempFile;

fn temp_log(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temp file");
    file.write_all(content.as_bytes()).expect("write temp file");
    file
}

/// A baseline corpus: login noise plus a pool-recycle template that will
/// vanish from the target.
fn baseline_content() -> String {
    let mut out = String::new();
    for i in 0..60 {
        out.push_str(&format!(
            "{{\"ts\": \"2026-07-24T10:{:02}:00Z\", \"msg\": \"client 10.0.0.{} authenticated ok\"}}\n",
            i % 60,
            i % 12
        ));
    }
    for i in 0..20 {
        out.push_str(&format!(
            "{{\"ts\": \"2026-07-24T11:{:02}:00Z\", \"msg\": \"connection pool recycled for host{}.example.com\"}}\n",
            i % 60,
            i % 3
        ));
    }
    out
}

/// A target corpus: the same login noise (fewer), a new upstream-error
/// template, and an injected 3x novel line.
fn target_content() -> String {
    let mut out = String::new();
    for i in 0..40 {
        out.push_str(&format!(
            "{{\"ts\": \"2026-07-24T15:{:02}:00Z\", \"msg\": \"client 10.0.0.{} authenticated ok\"}}\n",
            i % 60,
            i % 12
        ));
    }
    for i in 0..30 {
        out.push_str(&format!(
            "{{\"ts\": \"2026-07-24T15:{:02}:30Z\", \"msg\": \"upstream host{}.example.com returned 502 for request {}\"}}\n",
            i % 60,
            i % 4,
            i
        ));
    }
    for i in 0..3 {
        out.push_str(&format!(
            "{{\"ts\": \"2026-07-24T15:30:0{}Z\", \"msg\": \"OOM killer invoked for process 4242\"}}\n",
            i
        ));
    }
    out
}

fn run_diff(args: &[&str]) -> (String, String, i32) {
    run_kelora(args)
}

#[test]
fn test_two_file_diff_reports_three_sections() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(stdout.contains("NEW in target"), "stdout: {}", stdout);
    assert!(
        stdout.contains("VANISHED from target"),
        "stdout: {}",
        stdout
    );
    assert!(stdout.contains("VOLUME SHIFTS"), "stdout: {}", stdout);
    assert!(stdout.contains("totals: baseline 80 events, target 73 events"));
    // The vanished template keeps its baseline count.
    assert!(stdout.contains("20  connection pool recycled"));
    // No spurious pass-2 residue warning.
    assert!(
        !stderr.contains("matched no frozen template"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_injected_novel_line_is_new_with_exact_count() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, _stderr, code) = run_diff(&[
        "--drain-diff=json",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let oom: Vec<&serde_json::Value> = json["new"]
        .as_array()
        .expect("new array")
        .iter()
        .filter(|e| e["template"].as_str().unwrap_or("").contains("OOM killer"))
        .collect();
    assert_eq!(oom.len(), 1, "exactly one OOM template: {}", stdout);
    assert_eq!(oom[0]["count"], 3, "injected exactly 3 copies");
    assert_eq!(json["unmatched_events"], 0, "residue guard");
    assert_eq!(json["baseline_events"], 80);
    assert_eq!(json["target_events"], 73);
}

#[test]
fn test_self_diff_yields_no_changes() {
    // diff(A, A): identical content in two distinct files.
    let content = baseline_content();
    let a = temp_log(&content);
    let b = temp_log(&content);

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        a.path().to_str().unwrap(),
        b.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(stdout.contains("no new templates"), "stdout: {}", stdout);
    assert!(
        stdout.contains("no vanished templates"),
        "stdout: {}",
        stdout
    );
    assert!(stdout.contains("no volume shifts"), "stdout: {}", stdout);
}

#[test]
fn test_swapping_inputs_flips_deltas_but_keeps_template_set() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());
    let b_path = baseline.path().to_str().unwrap();
    let t_path = target.path().to_str().unwrap();

    let (fwd, _, code_fwd) = run_diff(&["--drain-diff=json", b_path, t_path, "-k", "msg"]);
    let (rev, _, code_rev) = run_diff(&["--drain-diff=json", t_path, b_path, "-k", "msg"]);
    assert_eq!(code_fwd, 0);
    assert_eq!(code_rev, 0);

    let fwd: serde_json::Value = serde_json::from_str(&fwd).expect("fwd JSON");
    let rev: serde_json::Value = serde_json::from_str(&rev).expect("rev JSON");

    let templates = |v: &serde_json::Value| -> Vec<String> {
        let mut all: Vec<String> = ["new", "vanished", "shifted"]
            .iter()
            .flat_map(|section| {
                v[*section]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["template_id"].as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        all.sort();
        all
    };
    assert_eq!(
        templates(&fwd),
        templates(&rev),
        "template set must be order-independent"
    );

    // NEW in one direction is VANISHED in the other, with the same count.
    let fwd_new = fwd["new"].as_array().unwrap();
    let rev_vanished = rev["vanished"].as_array().unwrap();
    assert_eq!(fwd_new.len(), rev_vanished.len());

    // Shifted deltas flip sign.
    for entry in fwd["shifted"].as_array().unwrap() {
        let id = entry["template_id"].as_str().unwrap();
        let mirrored = rev["shifted"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["template_id"] == id)
            .unwrap_or_else(|| panic!("template {} missing from reversed shift list", id));
        let d_fwd = entry["delta_pp"].as_f64().unwrap();
        let d_rev = mirrored["delta_pp"].as_f64().unwrap();
        assert!(
            (d_fwd + d_rev).abs() < 1e-9,
            "delta must flip sign: {} vs {}",
            d_fwd,
            d_rev
        );
    }
}

#[test]
fn test_repeated_runs_are_deterministic() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());
    let args = [
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ];
    let (first, _, _) = run_diff(&args);
    let (second, _, _) = run_diff(&args);
    assert_eq!(
        first, second,
        "identical runs must produce identical output"
    );
}

#[test]
fn test_proportionally_identical_sides_produce_no_shifts() {
    // Target is 3x the baseline volume with identical template proportions.
    let mut small = String::new();
    let mut large = String::new();
    for i in 0..30 {
        let line = format!(
            "{{\"msg\": \"client 10.0.0.{} authenticated ok\"}}\n",
            i % 12
        );
        small.push_str(&line);
        large.push_str(&line);
        large.push_str(&line);
        large.push_str(&line);
    }
    for i in 0..10 {
        let line = format!(
            "{{\"msg\": \"upstream host{}.example.com returned 502\"}}\n",
            i % 4
        );
        small.push_str(&line);
        large.push_str(&line);
        large.push_str(&line);
        large.push_str(&line);
    }
    let baseline = temp_log(&small);
    let target = temp_log(&large);

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stdout.contains("no volume shifts"),
        "share math must not flag proportionally identical sides: {}",
        stdout
    );
    assert!(stdout.contains("no new templates"));
    assert!(stdout.contains("no vanished templates"));
}

#[test]
fn test_cut_mode_equals_pre_split_files() {
    let baseline = baseline_content(); // timestamps 10:xx-11:xx
    let target = target_content(); // timestamps 15:xx
    let combined = temp_log(&format!("{}{}", baseline, target));
    let base_file = temp_log(&baseline);
    let target_file = temp_log(&target);

    let (cut_out, cut_err, cut_code) = run_diff(&[
        "--drain-diff",
        "--cut",
        "2026-07-24T14:00:00Z",
        combined.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    let (split_out, _, split_code) = run_diff(&[
        "--drain-diff",
        base_file.path().to_str().unwrap(),
        target_file.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(cut_code, 0, "stderr: {}", cut_err);
    assert_eq!(split_code, 0);
    assert_eq!(
        cut_out, split_out,
        "single file with --cut must equal the same file pre-split"
    );
}

#[test]
fn test_cut_mode_warns_on_events_without_timestamps() {
    let content = "{\"msg\": \"no timestamp at all\"}\n{\"ts\": \"2026-07-24T15:00:00Z\", \"msg\": \"fine\"}\n{\"ts\": \"2026-07-24T10:00:00Z\", \"msg\": \"fine\"}\n";
    let file = temp_log(content);
    let (_stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut",
        "2026-07-24T14:00:00Z",
        file.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0);
    assert!(
        stderr.contains("excluded 1 event(s) without a parseable timestamp"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_filters_run_before_diffing() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    // Filtering out the upstream template leaves only OOM as NEW.
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
        "--filter",
        "!e.msg.contains(\"upstream\")",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let new = json["new"].as_array().expect("new array");
    assert_eq!(new.len(), 1, "only OOM should remain NEW: {}", stdout);
    assert!(new[0]["template"].as_str().unwrap().contains("OOM killer"));
}

#[test]
fn test_usage_errors_exit_2() {
    let file = temp_log("{\"msg\": \"x\"}\n");
    let path = file.path().to_str().unwrap();

    // One input without --cut.
    let (_, stderr, code) = run_diff(&["--drain-diff", path, "-k", "msg"]);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("exactly 2 inputs"));

    // Missing -k.
    let (_, stderr, code) = run_diff(&["--drain-diff", path, path]);
    assert_eq!(code, 2);
    assert!(stderr.contains("exactly one effective field"));

    // Same file on both sides.
    let (_, stderr, code) = run_diff(&["--drain-diff", path, path, "-k", "msg"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("same file"));

    // --cut without --drain-diff.
    let (_, stderr, code) = run_diff(&["--cut", "14:00", path, "-k", "msg"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--cut requires --drain-diff"));

    // --cut with two inputs.
    let other = temp_log("{\"msg\": \"y\"}\n");
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut",
        "14:00",
        path,
        other.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 2);
    assert!(stderr.contains("single input"));

    // Parallel mode.
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--parallel",
        path,
        other.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 2);
    assert!(stderr.contains("not supported with --parallel"));

    // Combined with --drain.
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--drain",
        path,
        other.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 2);
    assert!(stderr.contains("pick one summary mode"));

    // Invalid --cut timestamp.
    let (_, stderr, code) = run_diff(&["--drain-diff", "--cut", "not-a-time", path, "-k", "msg"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--cut"));
}

#[test]
fn test_residue_guard_is_zero_on_example_corpus() {
    // Pass-2 unmatched events should be impossible by construction; assert it
    // on real example corpora (these messages exercise the digit-bearing mask
    // names like <ipv4> that broke drain_rs's own inference-mode lookup).
    let manifest = env!("CARGO_MANIFEST_DIR");
    let baseline = format!("{}/examples/app_monitoring.jsonl", manifest);
    let target = format!("{}/examples/api_logs.jsonl", manifest);

    let (stdout, stderr, code) =
        run_diff(&["--drain-diff=json", &baseline, &target, "-k", "message"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(
        json["unmatched_events"], 0,
        "pass-2 residue must be zero on the example corpus: {}",
        stdout
    );
    assert!(
        !stderr.contains("matched no frozen template"),
        "stderr: {}",
        stderr
    );
    let events =
        json["baseline_events"].as_u64().unwrap() + json["target_events"].as_u64().unwrap();
    assert!(events > 0, "corpus should produce events");
}

#[test]
fn test_typoed_key_fails_instead_of_reporting_no_change() {
    // A field name that matches nothing excludes every event, which used to
    // print an all-empty report ("no new templates", 0 events) and exit 0 — a
    // typo reading as a confident "nothing changed".
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "nosuchfield",
    ]);
    assert_eq!(code, 1, "stdout: {} stderr: {}", stdout, stderr);
    assert!(
        !stdout.contains("NEW in target") && !stdout.contains("no volume shifts"),
        "no report may be printed: {}",
        stdout
    );
    assert!(
        stderr.contains("never present in the input") && stderr.contains("nosuchfield"),
        "stderr: {}",
        stderr
    );
    // The run's real field names are surfaced so the fix is obvious.
    assert!(stderr.contains("msg"), "stderr: {}", stderr);
}

#[test]
fn test_typoed_key_suggests_the_nearest_field() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "mesg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(stderr.contains("Did you mean 'msg'?"), "stderr: {}", stderr);
}

#[test]
fn test_typoed_key_fails_in_json_mode_too() {
    // The JSON consumer is exactly who would act on a fabricated empty diff.
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "nosuchfield",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(stdout.trim().is_empty(), "stdout: {}", stdout);
}

#[test]
fn test_typoed_key_still_fails_under_silent() {
    // --silent suppresses the report and both advisory tiers, but the refusal is
    // fatal: the exit code is the only signal a scripted run has left, so it
    // must not read as success.
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "nosuchfield",
        "--silent",
    ]);
    assert_eq!(code, 1, "stdout: {} stderr: {}", stdout, stderr);
    assert!(stdout.trim().is_empty(), "stdout: {}", stdout);
    assert!(stderr.contains("nosuchfield"), "stderr: {}", stderr);
}

#[test]
fn test_silent_valid_run_stays_quiet_and_succeeds() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
        "--silent",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(stdout.trim().is_empty(), "stdout: {}", stdout);
    assert!(stderr.trim().is_empty(), "stderr: {}", stderr);
}

#[test]
fn test_field_empty_on_every_event_fails() {
    // The field exists, so it is not a typo — but an always-empty value yields
    // no templates either, and the report would be equally vacuous.
    let baseline = temp_log("{\"msg\": \"\"}\n{\"msg\": \"\"}\n");
    let target = temp_log("{\"msg\": \"\"}\n");

    let (stdout, stderr, code) = run_diff(&[
        "-f",
        "json",
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stdout: {} stderr: {}", stdout, stderr);
    assert!(
        stderr.contains("which was empty on all 3 event(s)"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn test_partially_missing_field_warns_but_still_reports() {
    // Heterogeneous logs legitimately carry the field on only some events; the
    // diff is still meaningful, but the shrunken corpus must not be silent.
    let baseline = temp_log(
        "{\"msg\": \"alpha connect 1\"}\n{\"other\": \"x\"}\n{\"msg\": \"alpha connect 2\"}\n",
    );
    let target = temp_log(
        "{\"msg\": \"alpha connect 3\"}\n{\"other\": \"y\"}\n{\"msg\": \"alpha connect 4\"}\n",
    );

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stderr.contains("excluded 2 event(s) with no 'msg' value"),
        "stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("only the 4 event(s) that had one"),
        "stderr: {}",
        stderr
    );
    assert!(stdout.contains("totals: baseline 2 events, target 2 events"));
}

#[test]
fn test_zero_compared_events_warns_that_the_report_is_vacuous() {
    // Nothing typo'd: the filter removed every event. The report is still
    // all-empty, so it must not read as "nothing changed".
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
        "--filter",
        "false",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stderr.contains("compared 0 events") && stderr.contains("missing data"),
        "stderr: {}",
        stderr
    );
    assert!(stdout.contains("no new templates"), "stdout: {}", stdout);
}

#[test]
fn test_json_reports_field_exclusion_count() {
    let baseline = temp_log("{\"msg\": \"alpha connect 1\"}\n{\"other\": \"x\"}\n");
    let target = temp_log("{\"msg\": \"alpha connect 2\"}\n");

    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(json["excluded_no_field"], 1);
}

#[test]
fn test_events_are_suppressed_in_diff_mode() {
    let baseline = temp_log(&baseline_content());
    let target = temp_log(&target_content());
    let (stdout, _, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        target.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0);
    assert!(
        !stdout.contains("authenticated ok\"") && !stdout.contains("ts="),
        "diff mode must not emit individual events: {}",
        stdout
    );
    // The report itself is the only stdout payload.
    assert!(stdout.trim_start().starts_with("NEW in target"));
}
