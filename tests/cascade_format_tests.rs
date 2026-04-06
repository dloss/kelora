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
