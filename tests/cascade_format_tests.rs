mod common;
use common::*;

/// Cascade mode: JSON lines parsed as JSON, everything else falls through to line.
#[test]
fn test_cascade_json_line_mixed() {
    let input = r#"{"level":"info","msg":"hello"}
plain text line
{"level":"error","msg":"oops"}
another plain line"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json,line", "-F", "json"], input);
    assert_eq!(exit_code, 0, "cascade parsing should succeed: {}", stderr);

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 4, "should emit all 4 events");

    // Event 1: JSON - should have _format=json and the msg field
    let ev1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ev1["_format"].as_str().unwrap(), "json");
    assert_eq!(ev1["msg"].as_str().unwrap(), "hello");

    // Event 2: plain text - should fall through to line
    let ev2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(ev2["_format"].as_str().unwrap(), "line");
    assert_eq!(ev2["line"].as_str().unwrap(), "plain text line");

    // Event 3: JSON again
    let ev3: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(ev3["_format"].as_str().unwrap(), "json");
    assert_eq!(ev3["msg"].as_str().unwrap(), "oops");

    // Event 4: plain text again
    let ev4: serde_json::Value = serde_json::from_str(lines[3]).unwrap();
    assert_eq!(ev4["_format"].as_str().unwrap(), "line");
}

/// Filter on _format in cascade mode.
#[test]
fn test_cascade_filter_by_format() {
    let input = r#"{"msg":"json event"}
plain event
{"msg":"another json"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json,line",
            "-F",
            "json",
            "--filter",
            "e._format == \"json\"",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "should only keep json events");
}

/// Cascade rejects schema-based formats.
#[test]
fn test_cascade_rejects_csv() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json,csv", "-F", "json"], "{}");
    assert_ne!(exit_code, 0, "cascade with csv should fail");
    assert!(
        stderr.contains("csv") && stderr.to_lowercase().contains("cascade"),
        "error should mention csv and cascade: {}",
        stderr
    );
}

/// Cascade rejects auto inside the list.
#[test]
fn test_cascade_rejects_auto() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "auto,json", "-F", "json"], "{}");
    assert_ne!(exit_code, 0, "cascade with auto should fail");
    assert!(
        stderr.to_lowercase().contains("auto"),
        "error should mention auto: {}",
        stderr
    );
}

/// Cascade requires at least two formats.
#[test]
fn test_cascade_rejects_duplicates() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json,json", "-F", "json"], "{}");
    assert_ne!(exit_code, 0, "cascade with duplicate should fail");
    assert!(
        stderr.to_lowercase().contains("duplicate"),
        "error should mention duplicate: {}",
        stderr
    );
}

/// Single format (no comma) still works as before — no _format field added.
#[test]
fn test_single_format_has_no_format_field() {
    let input = r#"{"msg":"hi"}"#;
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-F", "json"], input);
    assert_eq!(exit_code, 0);
    let ev: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        ev.get("_format").is_none(),
        "_format should not be added in single-format mode"
    );
}

/// Cascade diagnostics show per-format counts.
#[test]
fn test_cascade_diagnostic_counts() {
    let input = r#"{"a":1}
not json
{"b":2}"#;
    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "json,line", "-F", "json", "--stats"], input);
    assert_eq!(exit_code, 0);
    assert!(
        stdout.contains("Cascade formats:")
            && stdout.contains("json=2")
            && stdout.contains("line=1"),
        "stats should include per-format cascade counts: {}",
        stdout
    );
}

/// Repeated -f builds a cascade that includes a cols: member (which a comma
/// list can't express). JSON lines stay JSON; plain text is parsed by cols.
#[test]
fn test_repeated_f_cascade_with_cols() {
    let input = "{\"level\":\"warn\",\"msg\":\"slow\"}\n2026-06-11 09:14:02 INFO starting up\n";
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "-f", "cols:ts(2) level *msg", "-F", "json"],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "repeated -f cascade should succeed: {}",
        stderr
    );
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);

    let ev1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ev1["_format"].as_str().unwrap(), "json");
    assert_eq!(ev1["msg"].as_str().unwrap(), "slow");

    let ev2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(ev2["_format"].as_str().unwrap(), "cols");
    assert_eq!(ev2["level"].as_str().unwrap(), "INFO");
    assert_eq!(ev2["msg"].as_str().unwrap(), "starting up");
}

/// A selective regex: member declines non-matching lines so a later catch-all
/// ('line') can pick them up, instead of mangling them like cols would.
#[test]
fn test_repeated_f_regex_then_line_fallthrough() {
    let input = "2026-06-11 09:14:02 INFO hello\n\tat com.foo.Bar(Bar.java:1)\n";
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            r"regex:(?P<ts>\S+ \S+) (?P<level>INFO|WARN|ERROR) (?P<msg>.*)",
            "-f",
            "line",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "regex+line cascade should succeed: {}",
        stderr
    );
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);

    let ev1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ev1["_format"].as_str().unwrap(), "regex");
    assert_eq!(ev1["level"].as_str().unwrap(), "INFO");

    // Stack-trace line doesn't match the regex, so it falls through to line.
    let ev2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(ev2["_format"].as_str().unwrap(), "line");
}

/// A catch-all cols: member must be last; anything after it would never run.
#[test]
fn test_repeated_f_cols_not_last_is_rejected() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:ts level *msg", "-f", "json"], "{}");
    assert_ne!(exit_code, 0, "cols before another format should fail");
    assert!(
        stderr.contains("cols") && stderr.to_lowercase().contains("last"),
        "error should say cols must be last: {}",
        stderr
    );
}

/// Repeated -f still rejects schema-based formats as members.
#[test]
fn test_repeated_f_rejects_csv_member() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-f", "csv status:int"], "{}");
    assert_ne!(exit_code, 0, "csv as cascade member should fail");
    assert!(
        stderr.to_lowercase().contains("csv") && stderr.to_lowercase().contains("schema"),
        "error should mention csv schema restriction: {}",
        stderr
    );
}

/// The comma-list error for cols/regex points users at repeated -f.
#[test]
fn test_comma_cascade_with_cols_hints_repeated_f() {
    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json,cols:ts level *msg"], "{}");
    assert_ne!(exit_code, 0);
    assert!(
        stderr.contains("repeated -f"),
        "comma-list rejection should hint at repeated -f: {}",
        stderr
    );
}

/// Cascade works in parallel mode; per-worker counts merge into global stats.
#[test]
fn test_cascade_parallel_mode_merges_counts() {
    // Enough lines that parallel mode actually splits across workers.
    let mut input = String::new();
    for i in 0..200 {
        input.push_str(&format!("{{\"n\":{}}}\n", i));
        input.push_str("plain text line\n");
    }

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json,line", "-F", "json", "--parallel", "--stats"],
        &input,
    );
    assert_eq!(exit_code, 0);
    // 200 json + 200 line events merged across workers
    assert!(
        stdout.contains("Cascade formats:")
            && stdout.contains("json=200")
            && stdout.contains("line=200"),
        "parallel stats should merge per-format counts: {}",
        stdout
    );
}
