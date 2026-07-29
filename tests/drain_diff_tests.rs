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
        "--cut-at",
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
        "single file with --cut-at must equal the same file pre-split"
    );
}

#[test]
fn test_cut_mode_warns_on_events_without_timestamps() {
    let content = "{\"msg\": \"no timestamp at all\"}\n{\"ts\": \"2026-07-24T15:00:00Z\", \"msg\": \"fine\"}\n{\"ts\": \"2026-07-24T10:00:00Z\", \"msg\": \"fine\"}\n";
    let file = temp_log(content);
    let (_stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
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

/// A cut that lands outside the log leaves one side empty, which makes every
/// template NEW or VANISHED by construction — a report that reads as a dramatic
/// finding when it only means the boundary missed. It must fail instead, and the
/// message must hand over the log's real span so the next attempt can work.
#[test]
fn test_cut_outside_the_log_is_refused_with_the_observed_span() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let path = combined.path().to_str().unwrap();

    // Past every event: the target side is starved.
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2030-01-01T00:00:00Z",
        path,
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("the target side is empty"),
        "stderr: {}",
        stderr
    );
    assert!(stderr.contains("fall before it"), "stderr: {}", stderr);
    // The span is the actionable half — it is what a working --cut-at is picked from.
    assert!(
        stderr.contains("The input spans 2026-07-24T10:00:00Z .. 2026-07-24T15:39:00Z"),
        "stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("VANISHED"),
        "a refused comparison must not also print its report: {}",
        stdout
    );

    // Before every event: the baseline side is starved.
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2000-01-01T00:00:00Z",
        path,
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("the baseline side is empty"),
        "stderr: {}",
        stderr
    );
    assert!(stderr.contains("fall at or after it"), "stderr: {}", stderr);
}

/// The trap this refusal exists for: `--cut-at` shares `--since`'s vocabulary, so a
/// relative value resolves against wall-clock now and silently overshoots any
/// archived log. Those users get the clock caveat; someone who typed an absolute
/// stamp already knows it was absolute and does not need the sentence.
#[test]
fn test_now_relative_cut_explains_the_clock_only_when_relevant() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let path = combined.path().to_str().unwrap();

    let (_, stderr, code) = run_diff(&["--drain-diff", "--cut-at", "1h", path, "-k", "msg"]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("relative times resolve against the current time"),
        "stderr: {}",
        stderr
    );

    let (_, stderr, _) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2030-01-01T00:00:00Z",
        path,
        "-k",
        "msg",
    ]);
    assert!(
        !stderr.contains("relative times resolve"),
        "an absolute --cut-at needs no clock caveat: {}",
        stderr
    );
}

/// Same vacuity, reached the other way: two inputs where one contributes nothing.
#[test]
fn test_two_input_mode_refuses_an_empty_side() {
    let baseline = temp_log(&baseline_content());
    let empty = temp_log("");
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        baseline.path().to_str().unwrap(),
        empty.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("the target input contributed none"),
        "stderr: {}",
        stderr
    );
}

/// The split is the report's whole premise, so a successful run states where it
/// landed — otherwise "target 0 events" and "the log stops there" are
/// indistinguishable without a second command.
#[test]
fn test_report_echoes_the_span_of_each_side() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2026-07-24T14:00:00Z",
        combined.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stdout.contains("baseline spans 2026-07-24T10:00:00Z .. 2026-07-24T11:19:00Z"),
        "stdout: {}",
        stdout
    );
    assert!(
        stdout.contains("target   spans 2026-07-24T15:00:00Z .. 2026-07-24T15:39:00Z"),
        "stdout: {}",
        stdout
    );
}

#[test]
fn test_json_report_carries_each_side_span() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        "--cut-at",
        "2026-07-24T14:00:00Z",
        combined.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["baseline_span"]["first"], "2026-07-24T10:00:00Z");
    assert_eq!(json["baseline_span"]["last"], "2026-07-24T11:19:00Z");
    assert_eq!(json["target_span"]["first"], "2026-07-24T15:00:00Z");
    assert_eq!(json["target_span"]["last"], "2026-07-24T15:39:00Z");
}

/// `--cut-at` used to be resolved by calling the `--since` parser, so its failures
/// named a flag that was never on the command line.
#[test]
fn test_cut_diagnostics_name_cut_and_not_since() {
    let file = temp_log("{\"ts\": \"2026-07-24T10:00:00Z\", \"msg\": \"x\"}\n");
    let path = file.path().to_str().unwrap();

    for value in ["not-a-time", "until-1h", "since+5m"] {
        let (_, stderr, code) = run_diff(&["--drain-diff", "--cut-at", value, path, "-k", "msg"]);
        assert_eq!(code, 2, "value {}: stderr {}", value, stderr);
        assert!(
            stderr.contains(&format!("--cut-at '{}'", value)),
            "value {} must be quoted back: {}",
            value,
            stderr
        );
        assert!(
            !stderr.contains("--since"),
            "value {} must not blame --since: {}",
            value,
            stderr
        );
    }
}

/// The whole point of the pair, and the only thing that separates them: which
/// side the matching event lands on. Same predicate, same log, one event moves.
/// Mirrors the --section-from / --section-after distinction.
#[test]
fn test_cut_before_and_cut_after_place_the_matching_event_differently() {
    let marker = "{\"ts\": \"2026-07-24T14:30:00Z\", \"msg\": \"deploy started: v1.4.2\"}\n";
    let content = format!("{}{}{}", baseline_content(), marker, target_content());
    let before_file = temp_log(&content);
    let after_file = temp_log(&content);
    let predicate = "e.msg.contains(\"deploy started\")";

    let side_counts = |args: &[&str]| -> (u64, u64, String) {
        let (stdout, stderr, code) = run_diff(args);
        assert_eq!(code, 0, "stderr: {}", stderr);
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
        (
            json["baseline_events"].as_u64().unwrap(),
            json["target_events"].as_u64().unwrap(),
            stdout,
        )
    };

    let (before_b, before_t, before_out) = side_counts(&[
        "--drain-diff=json",
        "--cut-before",
        predicate,
        before_file.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    let (after_b, after_t, after_out) = side_counts(&[
        "--drain-diff=json",
        "--cut-after",
        predicate,
        after_file.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);

    // Exactly one event moves across the boundary, and the totals are unchanged.
    assert_eq!(after_b, before_b + 1, "--cut-after keeps the match behind");
    assert_eq!(
        after_t,
        before_t - 1,
        "--cut-before hands the match forward"
    );
    assert_eq!(before_b + before_t, after_b + after_t);

    // --cut-before puts the marker in the target, so its template is NEW there;
    // --cut-after leaves it in the baseline, where it is the only occurrence and
    // so falls under the vanished noise floor rather than being reported.
    assert!(
        before_out.contains("deploy started: <version>"),
        "--cut-before must report the marker template: {}",
        before_out
    );
    assert!(
        !after_out.contains("deploy started: <version>"),
        "--cut-after must not report the marker as target-side change: {}",
        after_out
    );
}

/// --cut-after adds a degenerate case --cut-before cannot produce: a marker on
/// the final event starves the target even though the predicate did match. That
/// needs a different message from "never matched", since the expression is right
/// and the log is what's wrong.
#[test]
fn test_cut_after_distinguishes_a_final_match_from_no_match() {
    let content = format!(
        "{}{}",
        baseline_content(),
        "{\"ts\": \"2026-07-24T23:59:00Z\", \"msg\": \"shutdown complete: drain-diff-final\"}\n"
    );
    let matched = temp_log(&content);
    let unmatched = temp_log(&content);

    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-after",
        "e.msg.contains(\"drain-diff-final\")",
        matched.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("matched the last of"),
        "a final match must not be reported as never matching: {}",
        stderr
    );
    assert!(!stderr.contains("never matched"), "stderr: {}", stderr);

    // Contrast: a genuinely absent marker still gets the "never matched" wording.
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-after",
        "e.msg.contains(\"no-such-marker\")",
        unmatched.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(stderr.contains("never matched"), "stderr: {}", stderr);
}

/// A first-event match is degenerate for --cut-before but perfectly fine for
/// --cut-after, which keeps that event on the baseline side.
#[test]
fn test_cut_after_accepts_a_first_event_match() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        "--cut-after",
        "true",
        combined.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["baseline_events"], 1);
    assert_eq!(json["target_events"], 152);
}

/// The point of --cut-before: split on what the change looks like, with no
/// timestamp to look up and no clock to reason about. Must agree exactly with the
/// equivalent --cut-at, so the two splitters are interchangeable when both are
/// expressible.
#[test]
fn test_cut_predicate_matches_the_equivalent_timestamp_split() {
    // A marker sits exactly on the boundary the 14:00 cut would draw.
    let marked = format!(
        "{}{}{}",
        baseline_content(),
        "{\"ts\": \"2026-07-24T14:30:00Z\", \"msg\": \"deploy started: v1.4.2\"}\n",
        target_content()
    );
    let by_predicate = temp_log(&marked);
    let by_time = temp_log(&marked);

    let (pred_out, pred_err, pred_code) = run_diff(&[
        "--drain-diff",
        "--cut-before",
        "e.msg.contains(\"deploy started\")",
        by_predicate.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    let (time_out, _, time_code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2026-07-24T14:30:00Z",
        by_time.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(pred_code, 0, "stderr: {}", pred_err);
    assert_eq!(time_code, 0);
    assert_eq!(
        pred_out, time_out,
        "--cut-before and the equivalent --cut-at must draw the same boundary"
    );
    // The matching event belongs to the target, not the baseline.
    assert!(
        pred_out.contains("deploy started: <version>"),
        "stdout: {}",
        pred_out
    );
}

/// The boundary latches, so a predicate matching most of the log still splits it
/// once. Without the latch every match would re-cross and the sides would be
/// interleaved rather than split.
#[test]
fn test_cut_predicate_latches_on_the_first_match() {
    // 3 baseline events, then 5 that all match: the split must be 3/5, not 3/1.
    let mut content = String::new();
    for i in 0..3 {
        content.push_str(&format!(
            "{{\"ts\": \"2026-07-24T10:0{}:00Z\", \"level\": \"INFO\", \"msg\": \"request {} served\"}}\n",
            i, i
        ));
    }
    for i in 0..5 {
        content.push_str(&format!(
            "{{\"ts\": \"2026-07-24T11:0{}:00Z\", \"level\": \"ERROR\", \"msg\": \"upstream {} failed\"}}\n",
            i, i
        ));
    }
    let file = temp_log(&content);
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff=json",
        "--cut-before",
        "e.level == \"ERROR\"",
        file.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["baseline_events"], 3);
    assert_eq!(
        json["target_events"], 5,
        "all five matching events belong to the target, not just the first"
    );
}

#[test]
fn test_cut_predicate_refuses_both_degenerate_predicates() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let path = combined.path().to_str().unwrap();

    // Never matches: nothing reaches the target.
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-before",
        "e.msg.contains(\"no such line anywhere\")",
        path,
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(stderr.contains("never matched"), "stderr: {}", stderr);
    assert!(
        !stdout.contains("VANISHED"),
        "a refused comparison must not also print its report: {}",
        stdout
    );

    // Always matches: nothing stays in the baseline.
    let (_, stderr, code) = run_diff(&["--drain-diff", "--cut-before", "true", path, "-k", "msg"]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("matched the very first"),
        "stderr: {}",
        stderr
    );
}

/// Stricter than --filter on purpose: the boundary is one decision, so a failed
/// evaluation before it is found leaves every later event's side unknown.
#[test]
fn test_cut_predicate_evaluation_failure_invalidates_the_report() {
    let combined = temp_log(&format!("{}{}", baseline_content(), target_content()));
    let (stdout, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-before",
        "e.nosuchfield.bogus_method()",
        combined.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 1, "stderr: {}", stderr);
    assert!(
        stderr.contains("--cut-before failed to evaluate"),
        "stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("NEW in target"),
        "an unknown split must not print a report: {}",
        stdout
    );
}

#[test]
fn test_cut_predicate_usage_errors() {
    let file = temp_log("{\"ts\": \"2026-07-24T10:00:00Z\", \"msg\": \"x\"}\n");
    let path = file.path().to_str().unwrap();

    // Without --drain-diff.
    let (_, stderr, code) = run_diff(&["--cut-before", "true", path, "-k", "msg"]);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(
        stderr.contains("--cut-before requires --drain-diff"),
        "stderr: {}",
        stderr
    );

    // Multiple splitters at once: the rules would contradict each other.
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
        "2026-07-24T14:00:00Z",
        "--cut-before",
        "true",
        path,
        "-k",
        "msg",
    ]);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("cannot be used with"), "stderr: {}", stderr);

    // Two inputs plus a predicate: the split happens inside one input.
    let other = temp_log("{\"ts\": \"2026-07-24T10:00:00Z\", \"msg\": \"y\"}\n");
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-before",
        "true",
        path,
        other.path().to_str().unwrap(),
        "-k",
        "msg",
    ]);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("--cut-before"), "stderr: {}", stderr);
    assert!(stderr.contains("single input"), "stderr: {}", stderr);
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

    // One input without --cut-at.
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

    // --cut-at without --drain-diff.
    let (_, stderr, code) = run_diff(&["--cut-at", "14:00", path, "-k", "msg"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--cut-at requires --drain-diff"));

    // --cut-at with two inputs.
    let other = temp_log("{\"msg\": \"y\"}\n");
    let (_, stderr, code) = run_diff(&[
        "--drain-diff",
        "--cut-at",
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

    // Invalid --cut-at timestamp.
    let (_, stderr, code) =
        run_diff(&["--drain-diff", "--cut-at", "not-a-time", path, "-k", "msg"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--cut-at"));
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
