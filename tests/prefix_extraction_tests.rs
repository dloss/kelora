mod common;
use common::*;

#[test]
fn test_prefix_extraction_basic() {
    let input = r#"web_1    | GET /health 200
db_1     | Connection established
api_1    | Starting server on port 8080
cache_1  | Memory usage: 45%"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["--extract-prefix", "src", "-f", "line", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "Prefix extraction should succeed");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 4, "Should extract prefix from 4 lines");

    // Parse the JSON output
    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    // Check each line has extracted prefix and remaining content
    assert_eq!(parsed[0]["src"], "web_1");
    assert_eq!(parsed[0]["line"], "GET /health 200");

    assert_eq!(parsed[1]["src"], "db_1");
    assert_eq!(parsed[1]["line"], "Connection established");

    assert_eq!(parsed[2]["src"], "api_1");
    assert_eq!(parsed[2]["line"], "Starting server on port 8080");

    assert_eq!(parsed[3]["src"], "cache_1");
    assert_eq!(parsed[3]["line"], "Memory usage: 45%");
}

#[test]
fn test_prefix_extraction_custom_separator() {
    let input = r#"auth-service :: User login successful
payment-service :: Transaction completed
email-service :: Message sent"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "service",
            "--prefix-sep",
            " :: ",
            "-f",
            "line",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "Custom separator should work");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should extract prefix from 3 lines");

    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    assert_eq!(parsed[0]["service"], "auth-service");
    assert_eq!(parsed[0]["line"], "User login successful");

    assert_eq!(parsed[1]["service"], "payment-service");
    assert_eq!(parsed[1]["line"], "Transaction completed");

    assert_eq!(parsed[2]["service"], "email-service");
    assert_eq!(parsed[2]["line"], "Message sent");
}

#[test]
fn test_prefix_extraction_with_filtering() {
    let input = r#"web_1    | GET /health 200
db_1     | Connection established
web_1    | GET /api/users
api_1    | Starting server"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "src",
            "-f",
            "line",
            "-F",
            "json",
            "--filter",
            "e.src == \"web_1\"",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "Filtering with prefix extraction should work");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should filter to only web_1 entries");

    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    assert_eq!(parsed[0]["src"], "web_1");
    assert_eq!(parsed[0]["line"], "GET /health 200");

    assert_eq!(parsed[1]["src"], "web_1");
    assert_eq!(parsed[1]["line"], "GET /api/users");
}

#[test]
fn test_prefix_extraction_with_json_format() {
    let input = r#"web_1 | {"timestamp": "2024-01-01T10:00:00Z", "level": "INFO", "message": "Request processed"}
db_1  | {"timestamp": "2024-01-01T10:01:00Z", "level": "DEBUG", "message": "Query executed"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["--extract-prefix", "container", "-f", "json", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "Prefix extraction with JSON should work");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should parse both JSON lines with prefix");

    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    // First line should have both extracted prefix and parsed JSON fields
    assert_eq!(parsed[0]["container"], "web_1");
    assert_eq!(parsed[0]["level"], "INFO");
    assert_eq!(parsed[0]["message"], "Request processed");

    // Second line
    assert_eq!(parsed[1]["container"], "db_1");
    assert_eq!(parsed[1]["level"], "DEBUG");
    assert_eq!(parsed[1]["message"], "Query executed");
}

#[test]
fn test_prefix_extraction_edge_cases() {
    let input = r#" | Just a message
empty_prefix | Some content
no-separator-here
service-with-dashes | Another message"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["--extract-prefix", "src", "-f", "line", "-F", "json"],
        input,
    );
    assert_eq!(exit_code, 0, "Edge cases should be handled");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 4, "Should handle all edge cases");

    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    // Empty prefix should not be extracted
    assert!(parsed[0]["src"].is_null());
    assert_eq!(parsed[0]["line"], "Just a message");

    // Normal prefix
    assert_eq!(parsed[1]["src"], "empty_prefix");
    assert_eq!(parsed[1]["line"], "Some content");

    // No separator - no prefix extraction
    assert!(parsed[2]["src"].is_null());
    assert_eq!(parsed[2]["line"], "no-separator-here");

    // Service with dashes
    assert_eq!(parsed[3]["src"], "service-with-dashes");
    assert_eq!(parsed[3]["line"], "Another message");
}

#[test]
fn test_prefix_extraction_with_transformation() {
    let input = r#"web_1    | GET /api/users
api_1    | Starting server"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "src",
            "-f",
            "line",
            "-F",
            "json",
            "--exec",
            "e.service_type = if e.src.contains(\"web\") { \"frontend\" } else { \"backend\" }",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "Transformation with prefix extraction should work"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should transform both log lines");

    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("Should be valid JSON"))
        .collect();

    assert_eq!(parsed[0]["src"], "web_1");
    assert_eq!(parsed[0]["service_type"], "frontend");

    assert_eq!(parsed[1]["src"], "api_1");
    assert_eq!(parsed[1]["service_type"], "backend");
}

// Auto-detection classifies the line the parser will see, after the prefix is
// stripped; it used to look at `api-1  | {...}` and fall back to `line`.
#[test]
fn test_prefix_extraction_auto_detects_after_prefix_file() {
    let content = r#"api-1  | {"level":"INFO","msg":"request handled","status":200}
api-1  | {"level":"ERROR","msg":"db timeout","status":500}
"#;

    let (stdout, stderr, exit_code) = run_kelora_with_file(
        &["--extract-prefix", "service", "-k", "service,level,msg"],
        content,
    );

    assert_eq!(exit_code, 0, "{stderr}");
    assert!(
        stdout.contains("service='api-1' level='ERROR' msg='db timeout'"),
        "JSON after the prefix should be parsed: {stdout}\n{stderr}"
    );
    assert!(!stderr.contains("No input format detected"), "{stderr}");
}

#[test]
fn test_prefix_extraction_auto_detects_after_prefix_stdin() {
    // kubectl logs --timestamps: the prefix is the timestamp, separated by a space.
    let input = r#"2026-10-24T09:12:02.223456789Z {"level":"error","msg":"upstream failed","err":"connection refused"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "kube_ts",
            "--prefix-sep",
            " ",
            "-k",
            "kube_ts,level,err",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "{stderr}");
    assert!(
        stdout.contains(
            "kube_ts='2026-10-24T09:12:02.223456789Z' level='error' err='connection refused'"
        ),
        "{stdout}\n{stderr}"
    );
}

/// `docker compose logs` output: every line, trace lines included, carries
/// the service prefix.
const COMPOSE_TRACEBACK: &str = "web-1  | INFO started
web-1  | Traceback (most recent call last):
web-1  |   File \"/app/x.py\", line 3, in <module>
web-1  |     boom()
web-1  | ZeroDivisionError: division by zero
db-1   | LOG:  checkpoint";

#[test]
fn multiline_warns_when_continuation_lines_carry_the_prefix() {
    // --multiline joins raw lines before the parser removes the prefix, so it
    // never sees the traceback's indentation and leaves every line its own
    // event. Silent wrong output before; now a warning.
    for args in [
        vec![
            "--extract-prefix",
            "svc",
            "-f",
            "line",
            "-M",
            "python",
            "-q",
        ],
        vec![
            "--extract-prefix",
            "svc",
            "-f",
            "line",
            "-M",
            "indent",
            "-q",
            "--parallel",
        ],
    ] {
        let (_stdout, stderr, exit_code) = run_kelora_with_input(&args, COMPOSE_TRACEBACK);
        assert_eq!(exit_code, 0, "{args:?}: {stderr}");
        assert_eq!(
            stderr
                .matches("2 lines carry the --extract-prefix prefix in front of indented text")
                .count(),
            1,
            "{args:?}: {stderr}"
        );
        assert!(stderr.contains("--no-log-prefix"), "{args:?}: {stderr}");
    }

    // kubectl logs --timestamps, prefix separated by a space.
    let kubectl = "2026-11-14T09:00:00.1Z ERROR Request failed
2026-11-14T09:00:00.1Z java.lang.NullPointerException: boom
2026-11-14T09:00:00.1Z \tat com.acme.Foo.bar(Foo.java:10)
2026-11-14T09:00:01.2Z INFO next request";
    let (_stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "kts",
            "--prefix-sep",
            " ",
            "-f",
            "line",
            "-M",
            "java",
            "-q",
        ],
        kubectl,
    );
    assert!(
        stderr.contains("1 line carries the --extract-prefix prefix"),
        "{stderr}"
    );
}

#[test]
fn no_prefix_multiline_warning_without_evidence_or_with_warnings_off() {
    // Without --multiline there is nothing to warn about.
    let (_stdout, stderr, _) = run_kelora_with_input(
        &["--extract-prefix", "svc", "-f", "line", "-q"],
        COMPOSE_TRACEBACK,
    );
    assert!(!stderr.contains("--extract-prefix prefix"), "{stderr}");

    // Prefix only on record starts: continuation lines are plain, so the
    // combination works and must stay quiet.
    let (stdout, stderr, _) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "svc",
            "-f",
            "line",
            "-M",
            "indent",
            "-F",
            "json",
        ],
        "web|ERROR x\n    at a\nweb|INFO y",
    );
    assert!(!stderr.contains("--extract-prefix prefix"), "{stderr}");
    assert_eq!(stdout.lines().count(), 2, "{stdout}");

    let (_stdout, stderr, _) = run_kelora_with_input(
        &[
            "--extract-prefix",
            "svc",
            "-f",
            "line",
            "-M",
            "python",
            "-q",
            "--no-warnings",
        ],
        COMPOSE_TRACEBACK,
    );
    assert!(!stderr.contains("--extract-prefix prefix"), "{stderr}");
}
