mod common;
use common::*;

#[test]
fn test_extract_re_maps_basic_functionality() {
    let input = r#"{"log": "User alice@test.com logged in from 192.168.1.100"}
{"log": "User bob@example.org failed login from 10.0.0.50"}
{"log": "Error: no email addresses found in this line"}"#;

    // Test basic extract_re_maps usage with emit_each
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--exec", "let email_maps = extract_re_maps(e.log, \"\\\\w+@\\\\w+\\\\.\\\\w+\", \"email\"); emit_each(email_maps)",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should emit 2 email events");

    // Check first email
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("Should be valid JSON");
    assert_eq!(first["email"].as_str().unwrap(), "alice@test.com");

    // Check second email
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("Should be valid JSON");
    assert_eq!(second["email"].as_str().unwrap(), "bob@example.org");
}

#[test]
fn test_extract_re_maps_with_capture_groups() {
    let input = r#"{"message": "user=alice status=200 response_time=45ms"}
{"message": "user=bob status=404 response_time=12ms"}
{"message": "user=charlie status=500 response_time=234ms"}"#;

    // Extract usernames using capture groups
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--exec", "let user_maps = extract_re_maps(e.message, \"user=([\\\\w]+)\", \"username\"); emit_each(user_maps)",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should emit 3 username events");

    let users: Vec<String> = lines
        .iter()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            parsed["username"].as_str().unwrap().to_string()
        })
        .collect();

    assert_eq!(users, vec!["alice", "bob", "charlie"]);
}

#[test]
fn test_extract_re_maps_with_base_context() {
    let input = r#"{"timestamp": "2023-07-18T15:04:23Z", "source": "webapp", "message": "IPs detected: 192.168.1.1 and 10.0.0.1"}
{"timestamp": "2023-07-18T15:05:30Z", "source": "database", "message": "Connection from 172.16.0.100"}"#;

    // Extract IPs with base context preservation
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                let ip_maps = extract_re_maps(e.message, "\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b", "ip");
                let base = #{timestamp: e.timestamp, source: e.source};
                emit_each(ip_maps, base)
            "#,
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should emit 3 IP events (2 from first, 1 from second)"
    );

    // Check that all events have base context
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("Should be valid JSON");
        assert!(
            parsed["timestamp"].is_string(),
            "Should have timestamp from base"
        );
        assert!(parsed["source"].is_string(), "Should have source from base");
        assert!(parsed["ip"].is_string(), "Should have extracted IP");
    }

    // Check specific IPs and their sources
    let first_event: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first_event["ip"].as_str().unwrap(), "192.168.1.1");
    assert_eq!(first_event["source"].as_str().unwrap(), "webapp");

    let third_event: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(third_event["ip"].as_str().unwrap(), "172.16.0.100");
    assert_eq!(third_event["source"].as_str().unwrap(), "database");
}

#[test]
fn test_extract_re_maps_composability() {
    let input = r#"{"text": "Contact alice@test.com or call +1-555-123-4567"}
{"text": "Email bob@example.org for support"}"#;

    // Test composability: extract both emails and phone numbers, combine them
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            r#"
                let email_maps = extract_re_maps(e.text, "\\w+@\\w+\\.\\w+", "contact");
                let phone_maps = extract_re_maps(e.text, "\\+?1?-?\\d{3}-\\d{3}-\\d{4}", "contact");
                let all_contacts = email_maps + phone_maps;
                if all_contacts.len() > 0 {
                    emit_each(all_contacts, #{source: "contact_extraction", original_text: e.text})
                }
            "#,
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should emit 3 contact events (2 emails + 1 phone)"
    );

    let contacts: Vec<String> = lines
        .iter()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            parsed["contact"].as_str().unwrap().to_string()
        })
        .collect();

    assert!(contacts.contains(&"alice@test.com".to_string()));
    assert!(contacts.contains(&"bob@example.org".to_string()));
    assert!(contacts.contains(&"+1-555-123-4567".to_string()));

    // Verify all have the base context
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["source"].as_str().unwrap(), "contact_extraction");
        assert!(parsed["original_text"].is_string());
    }
}

#[test]
fn test_status_class_function() {
    let input = r#"{"status": 200}
{"status": 404}
{"status": 500}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "e.class = e.status.status_class();",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 3, "Should output 3 lines");

    let first: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first["class"], "2xx");

    let second: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second["class"], "4xx");

    let third: serde_json::Value =
        serde_json::from_str(lines[2]).expect("Third line should be valid JSON");
    assert_eq!(third["class"], "5xx");
}

#[test]
fn test_or_empty_with_empty_strings() {
    let input = r#"{"message": "prefix:found"}
{"message": "no prefix here"}
{"message": "prefix:also_found"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.extracted = e.message.after(\"prefix:\").or_empty();",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Parse output to check that empty strings were converted to Unit (missing field)
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output 3 lines");

    // First event should have extracted field
    assert!(
        lines[0].contains("\"extracted\":\"found\""),
        "First event should have extracted field with value"
    );

    // Second event should NOT have extracted field (Unit removes it)
    assert!(
        !lines[1].contains("extracted"),
        "Second event should not have extracted field (empty string became Unit)"
    );

    // Third event should have extracted field
    assert!(
        lines[2].contains("\"extracted\":\"also_found\""),
        "Third event should have extracted field with value"
    );
}

#[test]
fn test_or_empty_prevents_empty_field_assignment() {
    let input = r#"{"message": "prefix:value1"}
{"message": "no prefix"}
{"message": "prefix:value2"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.extracted = e.message.after(\"prefix:\").or_empty(); track_unique(\"values\", e.extracted);",
            "--end",
            "print(`Unique: ${metrics[\"values\"].len()}`);",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    // Should track only 2 unique values (empty string from missing prefix is skipped)
    assert!(
        stdout.contains("Unique: 2"),
        "Should track 2 unique values, skipping empty/Unit"
    );
}

#[test]
fn test_or_empty_with_empty_arrays() {
    let input = r#"{"id": 1, "tags": ["a", "b"]}
{"id": 2, "tags": []}
{"id": 3, "tags": ["c"]}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.tags = e.tags.or_empty();",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output 3 lines");

    // First event should have tags
    assert!(
        lines[0].contains("\"tags\":[\"a\",\"b\"]"),
        "First event should have tags array"
    );

    // Second event should NOT have tags field (empty array became Unit)
    assert!(
        !lines[1].contains("tags"),
        "Second event should not have tags field (empty array became Unit)"
    );
    assert!(
        lines[1].contains("\"id\":2"),
        "Second event should still have id field"
    );

    // Third event should have tags
    assert!(
        lines[2].contains("\"tags\":[\"c\"]"),
        "Third event should have tags array"
    );
}

#[test]
fn test_or_empty_with_empty_maps() {
    let input = r#"{"id": 1, "metadata": {"key": "value"}}
{"id": 2, "metadata": {}}
{"id": 3, "metadata": {"foo": "bar"}}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "e.metadata = e.metadata.or_empty();",
            "-F",
            "json",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output 3 lines");

    // First event should have metadata
    assert!(
        lines[0].contains("\"metadata\":{\"key\":\"value\"}"),
        "First event should have metadata map"
    );

    // Second event should NOT have metadata field (empty map became Unit)
    assert!(
        !lines[1].contains("metadata"),
        "Second event should not have metadata field (empty map became Unit)"
    );
    assert!(
        lines[1].contains("\"id\":2"),
        "Second event should still have id field"
    );

    // Third event should have metadata
    assert!(
        lines[2].contains("\"metadata\":{\"foo\":\"bar\"}"),
        "Third event should have metadata map"
    );
}

#[test]
fn test_extract_json_basic_object() {
    let input = r#"{"message": "User data: {\"name\":\"Alice\",\"age\":30,\"active\":true}"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--exec", "let user = e.message.extract_json(); e.user_name = user.name; e.user_age = user.age; e.active = user.active;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["user_name"].as_str().unwrap(), "Alice");
    assert_eq!(output["user_age"].as_i64().unwrap(), 30);
    assert!(output["active"].as_bool().unwrap());
}

#[test]
fn test_extract_json_array() {
    let input = r#"{"message": "Tags: [\"rust\",\"cli\",\"logs\"] are available"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let tags = e.message.extract_json(); e.tags = tags;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    let tags = output["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[0].as_str().unwrap(), "rust");
    assert_eq!(tags[1].as_str().unwrap(), "cli");
    assert_eq!(tags[2].as_str().unwrap(), "logs");
}

#[test]
fn test_extract_json_with_nth_parameter() {
    let input = r#"{"message": "First: {\"id\":1} and second: {\"id\":2} objects"}"#;

    // Extract first JSON object (default)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let first = e.message.extract_json(); e.first_id = first.id;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["first_id"].as_i64().unwrap(), 1);

    // Extract second JSON object (nth=1)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let second = e.message.extract_json(1); e.second_id = second.id;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["second_id"].as_i64().unwrap(), 2);
}

#[test]
fn test_extract_json_nested_structures() {
    let input = r#"{"message": "Config: {\"server\":{\"host\":\"localhost\",\"port\":8080},\"enabled\":true}"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--exec", "let cfg = e.message.extract_json(); e.host = cfg.server.host; e.port = cfg.server.port; e.enabled = cfg.enabled;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["host"].as_str().unwrap(), "localhost");
    assert_eq!(output["port"].as_i64().unwrap(), 8080);
    assert!(output["enabled"].as_bool().unwrap());
}

#[test]
fn test_extract_json_no_match() {
    let input = r#"{"message": "This text has no JSON objects or arrays"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let result = e.message.extract_json(); e.found = (result != \"\");",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert!(!output["found"].as_bool().unwrap());
}

#[test]
fn test_extract_json_mixed_objects_and_arrays() {
    let input = r#"{"message": "Found [1,2,3] items and {\"status\":\"ok\"} result"}"#;

    // Extract first (array)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let first = e.message.extract_json(0); e.items = first;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    let items = output["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);

    // Extract second (object)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let second = e.message.extract_json(1); e.status = second.status;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["status"].as_str().unwrap(), "ok");
}

#[test]
fn test_extract_json_with_escaped_characters() {
    let input = r#"{"message": "Data: {\"text\":\"Line 1\\nLine 2\",\"quote\":\"He said \\\"hello\\\"\"}"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let data = e.message.extract_json(); e.text = data.text; e.quote = data.quote;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["text"].as_str().unwrap(), "Line 1\nLine 2");
    assert_eq!(output["quote"].as_str().unwrap(), "He said \"hello\"");
}

#[test]
fn test_extract_jsons_multiple_objects() {
    let input = r#"{"message": "Found {\"id\":1,\"name\":\"Alice\"} and {\"id\":2,\"name\":\"Bob\"} users"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--exec", "let jsons = e.message.extract_jsons(); e.count = jsons.len(); e.first = jsons[0]; e.second = jsons[1];",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["count"].as_i64().unwrap(), 2);
    assert_eq!(
        output["first"].as_str().unwrap(),
        r#"{"id":1,"name":"Alice"}"#
    );
    assert_eq!(
        output["second"].as_str().unwrap(),
        r#"{"id":2,"name":"Bob"}"#
    );
}

#[test]
fn test_extract_jsons_mixed_types() {
    let input = r#"{"message": "Data: [1,2,3] and {\"status\":\"ok\"} and [\"a\",\"b\"]"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let jsons = e.message.extract_jsons(); e.count = jsons.len(); e.items = jsons;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["count"].as_i64().unwrap(), 3);

    let items = output["items"].as_array().unwrap();
    assert_eq!(items[0].as_str().unwrap(), "[1,2,3]");
    assert_eq!(items[1].as_str().unwrap(), r#"{"status":"ok"}"#);
    assert_eq!(items[2].as_str().unwrap(), r#"["a","b"]"#);
}

#[test]
fn test_extract_jsons_empty_result() {
    let input = r#"{"message": "No JSON structures here, just plain text"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let jsons = e.message.extract_jsons(); e.count = jsons.len();",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["count"].as_i64().unwrap(), 0);
}

#[test]
fn test_extract_jsons_nested_structures() {
    let input = r#"{"message": "Config: {\"server\":{\"host\":\"localhost\",\"port\":8080}} and metadata: {\"version\":\"1.0\"}"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let jsons = e.message.extract_jsons(); e.count = jsons.len(); e.configs = jsons;",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("Should be valid JSON");
    assert_eq!(output["count"].as_i64().unwrap(), 2);

    let configs = output["configs"].as_array().unwrap();
    assert_eq!(
        configs[0].as_str().unwrap(),
        r#"{"server":{"host":"localhost","port":8080}}"#
    );
    assert_eq!(configs[1].as_str().unwrap(), r#"{"version":"1.0"}"#);
}

#[test]
fn test_extract_jsons_with_emit_each() {
    let input = r#"{"message": "Items: {\"id\":1} and {\"id\":2} and {\"id\":3}"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--exec",
            "let jsons = e.message.extract_jsons(); emit_each(jsons.map(|j| #{raw: j}))",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should emit 3 events");

    for (i, line) in lines.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("Should be valid JSON");
        let expected = format!(r#"{{"id":{}}}"#, i + 1);
        assert_eq!(parsed["raw"].as_str().unwrap(), expected);
    }
}
