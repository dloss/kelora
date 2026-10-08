mod common;
use common::*;

#[test]
fn parse_json_does_not_run_functions_from_log_data() {
    // Rhai's built-in parse_json evaluated its input, so this line stopped the
    // run with exit code 7 and silently dropped every later line.
    let input = "{\"a\": 1}\n{\"a\": exit(7)}\n{\"a\": print(\"INJECTED\")}\n{\"a\": 3}\n";
    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "--exec",
            "e += parse_json(e.line)",
            "-F",
            "json",
        ],
        input,
    );

    assert_eq!(code, 0, "stderr: {stderr}");
    // The raw line still appears as data; print() must not have run.
    assert!(stdout
        .lines()
        .chain(stderr.lines())
        .all(|l| l.trim() != "INJECTED"));
    assert!(stdout.contains(r#""a":1"#));
    assert!(stdout.contains(r#""a":3"#));
    assert!(
        stderr.contains("parse_json: invalid JSON"),
        "stderr: {stderr}"
    );
}

#[test]
fn parse_json_fans_out_a_json_array_file() {
    let input = "[\n{\"level\": \"info\", \"n\": 1},\n{\"level\": \"error\", \"n\": 2}\n]\n";
    let (stdout, stderr, code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "-M",
            "all",
            "--exec",
            "emit_each(e.line.parse_json())",
            "-F",
            "json",
        ],
        input,
    );

    assert_eq!(code, 0, "stderr: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![r#"{"level":"info","n":1}"#, r#"{"level":"error","n":2}"#],
        "stderr: {stderr}"
    );
}
