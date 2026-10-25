mod common;
use common::*;
use serde_json::Value;

const TAP: &str = "t/checkout.t ..
1..3
# ts=2026-07-01T08:10:01Z level=info msg=\"starting test server\" port=18080
ok 1 - server starts
# ts=2026-07-01T08:10:02Z level=error msg=\"coupon lookup failed\" error=timeout
not ok 2 - coupon reduces total

#   Failed test 'coupon reduces total'
#   at t/checkout.t line 27.
#          got: '43.1'
#     expected: '38.79'
# ts=2026-07-01T08:10:03Z level=warn msg=\"refund endpoint missing\"
not ok 3 - refund restores stock # TODO refunds not implemented
#   Failed (TODO) test 'refund restores stock'
#   at t/checkout.t line 48.
# ts=2026-07-01T08:10:04Z level=info msg=\"stopping test server\"
# Looks like you failed 1 test of 3.
Result: FAIL
";

fn events(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("json output"))
        .collect()
}

#[test]
fn comments_get_the_following_test_fields() {
    let (stdout, stderr, code) = run_kelora_with_input(&["--unwrap", "tap", "-F", "json"], TAP);
    assert_eq!(code, 0, "stderr: {stderr}");
    let events = events(&stdout);
    assert_eq!(events.len(), 8, "stdout: {stdout}");

    assert_eq!(events[0]["msg"], "starting test server");
    assert_eq!(events[0]["port"], 18080);
    assert_eq!(events[0]["test_file"], "t/checkout.t");
    assert_eq!(events[0]["test_num"], 1);
    assert_eq!(events[0]["test_ok"], true);
    assert_eq!(events[1]["line"], "ok 1 - server starts");
    assert_eq!(events[1]["test_name"], "server starts");

    assert_eq!(events[2]["msg"], "coupon lookup failed");
    assert_eq!(events[2]["test_num"], 2);
    assert_eq!(events[2]["test_ok"], false);
    assert_eq!(events[3]["line"], "not ok 2 - coupon reduces total");

    // Test::More's failure diagnostics are dropped.
    assert_eq!(events[4]["msg"], "refund endpoint missing");
    assert_eq!(events[4]["test_directive"], "TODO");
    assert_eq!(events[5]["test_num"], 3);

    // Trailing lines with no result after them keep the file, not a test.
    assert_eq!(events[6]["msg"], "stopping test server");
    assert_eq!(events[6]["test_file"], "t/checkout.t");
    assert!(events[6].get("test_num").is_none());
    assert_eq!(events[7]["line"], "Result: FAIL");
}

#[test]
fn format_is_detected_from_the_comments() {
    let (stdout, stderr, code) =
        run_kelora_with_file(&["--unwrap", "tap", "-F", "json", "-v"], TAP);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.contains("logfmt"), "stderr: {stderr}");
    assert!(!stdout.contains("_format"), "stdout: {stdout}");
}

#[test]
fn line_filters_see_raw_lines() {
    let (stdout, _stderr, code) = run_kelora_with_input(
        &[
            "--unwrap",
            "tap",
            "--keep-lines",
            "^not ok",
            "-k",
            "test_num",
        ],
        TAP,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "test_num=2\ntest_num=3\n");
}

#[test]
fn test_boundary_ends_a_multiline_event() {
    let input = "1..2
# 2026-07-01 08:00:01 ERROR request failed
#\tat com.example.Foo.bar(Foo.java:12)
ok 1 - first
#\tat com.example.Main.main(Main.java:5)
ok 2 - second
";
    let (stdout, _stderr, code) = run_kelora_with_input(
        &[
            "--unwrap",
            "tap",
            "-f",
            "line",
            "-M",
            "timestamp",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(code, 0);
    let events = events(&stdout);
    let lines: Vec<(&str, i64)> = events
        .iter()
        .map(|e| (e["line"].as_str().unwrap(), e["test_num"].as_i64().unwrap()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (
                "2026-07-01 08:00:01 ERROR request failed \tat com.example.Foo.bar(Foo.java:12)",
                1
            ),
            ("ok 1 - first", 1),
            ("\tat com.example.Main.main(Main.java:5)", 2),
            ("ok 2 - second", 2),
        ]
    );
}

#[test]
fn filter_selects_failing_tests() {
    let (stdout, _stderr, code) = run_kelora_with_input(
        &[
            "--unwrap",
            "tap",
            "-f",
            "logfmt",
            "--filter",
            "e.test_ok == false && e.test_directive != \"TODO\" && e.has(\"msg\")",
            "-k",
            "msg",
        ],
        TAP,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "msg='coupon lookup failed'\n");
}

#[test]
fn drain_runs_on_the_embedded_messages() {
    let (stdout, stderr, code) =
        run_kelora_with_input(&["--unwrap", "tap", "--drain", "-k", "msg"], TAP);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("coupon lookup failed"), "stdout: {stdout}");
    assert!(!stdout.contains("Failed test"), "stdout: {stdout}");
}

#[test]
fn rejects_parallel_and_csv() {
    let (_stdout, stderr, code) = run_kelora_with_input(&["--unwrap", "tap", "--parallel"], TAP);
    assert_eq!(code, 2);
    assert!(stderr.contains("--unwrap is not supported with --parallel"));

    let (_stdout, stderr, code) = run_kelora_with_input(&["--unwrap", "tap", "-f", "csv"], TAP);
    assert_eq!(code, 2);
    assert!(stderr.contains("csv/tsv"));
}

#[test]
fn drain_diff_does_not_warn_about_tap_lines() {
    let dir = tempfile::TempDir::new().unwrap();
    let pass = dir.path().join("pass.t");
    let fail = dir.path().join("fail.t");
    std::fs::write(&pass, TAP.replace("coupon lookup failed", "coupon applied")).unwrap();
    std::fs::write(&fail, TAP).unwrap();
    let args = [
        "--unwrap",
        "tap",
        "-f",
        "logfmt",
        "--drain-diff=json",
        "-k",
        "msg",
    ];

    let (stdout, stderr, code) =
        run_kelora_with_files(&args, &[pass.to_str().unwrap(), fail.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("coupon lookup failed"), "stdout: {stdout}");
    assert!(!stderr.contains("excluded"), "stderr: {stderr}");

    let mut verbose = args.to_vec();
    verbose.push("-v");
    let (_stdout, stderr, _code) =
        run_kelora_with_files(&verbose, &[pass.to_str().unwrap(), fail.to_str().unwrap()]);
    assert!(
        stderr.contains("left out 8 --unwrap wrapper line(s)"),
        "stderr: {stderr}"
    );
}
