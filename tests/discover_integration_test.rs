mod common;

use common::run_kelora_with_input;

#[test]
fn test_discover_json_profiles_nested_input_fields() {
    let input = r#"{"level":"info","user":{"name":"alice","roles":["admin","ops"]},"bytes":1536}
{"level":"error","user":{"name":"bob","roles":["ops"]},"bytes":2048,"extra":{"nested":{"x":1}}}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--discover=json"], input);

    assert_eq!(exit_code, 0, "discover should succeed: {}", stderr);
    assert!(
        stderr.is_empty(),
        "discover json mode should not emit stderr on success: {}",
        stderr
    );

    let doc: serde_json::Value =
        serde_json::from_str(&stdout).expect("discover output should be valid json");
    assert_eq!(doc["total_events"], 2);
    assert_eq!(doc["flatten_depth_limit"], 3);
    assert_eq!(doc["flatten_depth_capped"], false);
    assert_eq!(doc["truncated"], false);

    let fields = doc["fields"].as_array().expect("fields should be an array");

    let names: Vec<&str> = fields
        .iter()
        .map(|field| {
            field["name"]
                .as_str()
                .expect("field name should be a string")
        })
        .collect();

    assert!(
        names.contains(&"level"),
        "should include top-level fields: {:?}",
        names
    );
    assert!(
        names.contains(&"user.name"),
        "should flatten nested maps: {:?}",
        names
    );
    assert!(
        names.contains(&"user.roles[]"),
        "should flatten array elements: {:?}",
        names
    );
    assert!(
        names.contains(&"extra.nested.x"),
        "should flatten nested fields up to depth limit: {:?}",
        names
    );

    let roles = fields
        .iter()
        .find(|field| field["name"] == "user.roles[]")
        .expect("user.roles[] field should exist");
    assert_eq!(
        roles["seen"], 2,
        "seen is per-event: the array is present in both events"
    );
    assert_eq!(
        roles["observations"], 3,
        "observations counts array entries individually (2 + 1)"
    );
    assert_eq!(roles["cardinality"]["count"], 2);

    let bytes = fields
        .iter()
        .find(|field| field["name"] == "bytes")
        .expect("bytes field should exist");
    assert_eq!(bytes["samples"][0], 1536);
}

#[test]
fn test_discover_final_profiles_post_filter_post_exec_fields() {
    let input = r#"{"level":"info","keep":true,"bytes":1536}
{"level":"error","keep":false,"bytes":2048}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.keep",
            "--exec",
            r#"e.pretty = human_bytes(e.bytes); e = e.keep(["level","pretty"])"#,
            "--discover-final=json",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "discover final should succeed: {}", stderr);
    assert!(
        stderr.is_empty(),
        "discover final should not emit stderr on success: {}",
        stderr
    );

    let doc: serde_json::Value =
        serde_json::from_str(&stdout).expect("discover output should be valid json");
    assert_eq!(
        doc["total_events"], 1,
        "filter should run before output discovery"
    );

    let fields = doc["fields"].as_array().expect("fields should be an array");
    let names: Vec<&str> = fields
        .iter()
        .map(|field| {
            field["name"]
                .as_str()
                .expect("field name should be a string")
        })
        .collect();

    assert_eq!(names.len(), 2, "only projected output fields should remain");
    assert!(
        names.contains(&"level"),
        "output discovery should keep level"
    );
    assert!(
        names.contains(&"pretty"),
        "output discovery should see exec output"
    );
    assert!(
        !names.contains(&"bytes") && !names.contains(&"keep"),
        "output discovery should not report filtered/projection-only input fields: {:?}",
        names
    );

    let pretty = fields
        .iter()
        .find(|field| field["name"] == "pretty")
        .expect("pretty field should exist");
    assert_eq!(pretty["samples"][0], "1.5 KiB");
}

#[test]
fn test_discover_rejects_parallel_mode_at_cli_validation() {
    let input = r#"{"level":"info"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--parallel", "--discover"], input);

    assert_eq!(
        exit_code, 2,
        "parallel discover should be a CLI usage error"
    );
    assert!(
        stdout.is_empty(),
        "validation errors should not emit stdout"
    );
    assert!(
        stderr.contains("--discover and --discover-final are not supported with --parallel"),
        "expected clear validation message, got: {}",
        stderr
    );
}

#[test]
fn test_discover_and_discover_final_conflict() {
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--discover", "--discover-final"],
        r#"{"x":1}"#,
    );

    assert_eq!(
        exit_code, 2,
        "conflicting discover flags should be usage errors"
    );
    assert!(
        stdout.is_empty(),
        "validation errors should not emit stdout"
    );
    assert!(
        stderr.contains("--discover") && stderr.contains("--discover-final"),
        "expected conflict message to mention both flags, got: {}",
        stderr
    );
}

#[test]
fn test_discover_hints_at_discover_final_when_pipeline_transforms() {
    let input = r#"{"level":"info","status":200}
{"level":"error","status":500}"#;

    // Plain --discover with a field-mutating --exec: the user's computed field
    // won't appear (input is profiled before scripts run), so the footer should
    // point them at --discover-final.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--discover",
            "--exec",
            "e.slow = e.status >= 500",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(
        stdout.contains("--discover-final"),
        "exec pipeline should hint at --discover-final, got: {}",
        stdout
    );

    // Any filter also fires the hint: the surviving events' schema can differ
    // from the parsed input shown here.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &["-f", "json", "--discover", "--filter", "e.status >= 500"],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(
        stdout.contains("--discover-final"),
        "filter pipeline should hint at --discover-final, got: {}",
        stdout
    );

    // A bare probe with no filters or transforms stays uncluttered.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(&["-f", "json", "--discover"], input);
    assert_eq!(exit_code, 0);
    assert!(
        !stdout.contains("--discover-final"),
        "bare discover should not nag about --discover-final, got: {}",
        stdout
    );

    // --discover-final itself already shows post-script fields: no self-referential hint.
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--discover-final",
            "--exec",
            "e.slow = e.status >= 500",
        ],
        input,
    );
    assert_eq!(exit_code, 0);
    assert!(
        !stdout.contains("Use --discover-final"),
        "discover-final should not hint at itself, got: {}",
        stdout
    );
}
