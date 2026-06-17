mod common;
use common::*;

#[test]
fn test_skip_lines_functionality() {
    // Test with headers in CSV-style data
    let input = r#"header1,header2,header3
description,more info,extra
alice,user,200
bob,admin,404
charlie,guest,500"#;

    // Test skipping first 2 lines (headers)
    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "--skip-lines",
            "2",
            "--filter",
            "line.contains(\"user\") || line.contains(\"admin\")",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "Should have 2 lines after skipping headers and filtering"
    );
    assert!(
        stdout.contains("alice,user,200"),
        "Should contain alice line"
    );
    assert!(stdout.contains("bob,admin,404"), "Should contain bob line");
    assert!(!stdout.contains("header1"), "Should not contain header1");
    assert!(
        !stdout.contains("description"),
        "Should not contain description line"
    );

    // Test with parallel processing
    let (stdout_parallel, _stderr_parallel, exit_code_parallel) = run_kelora_with_input(
        &[
            "-f",
            "line",
            "--skip-lines",
            "2",
            "--parallel",
            "--filter",
            "line.contains(\"user\") || line.contains(\"admin\")",
        ],
        input,
    );
    assert_eq!(
        exit_code_parallel, 0,
        "kelora should exit successfully in parallel mode"
    );

    let lines_parallel: Vec<&str> = stdout_parallel.trim().lines().collect();
    assert_eq!(
        lines_parallel.len(),
        2,
        "Parallel processing should give same result"
    );
}

#[test]
fn test_skip_lines_with_zero() {
    let input = r#"line1
line2
line3"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--skip-lines", "0"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should process all lines when skip-lines is 0"
    );
}

#[test]
fn test_skip_lines_greater_than_input() {
    let input = r#"line1
line2"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--skip-lines", "5"], input);
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        0,
        "Should produce no output when skipping more lines than available"
    );
}

#[test]
fn test_ignore_lines_functionality() {
    let input = r#"{"level": "INFO", "message": "This is an info message"}
# This is a comment line
{"level": "ERROR", "message": "This is an error message"}

{"level": "DEBUG", "message": "This is a debug message"}
# Another comment
{"level": "WARN", "message": "This is a warning"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "^#.*|^$", // Ignore comments and empty lines
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "Should output 4 lines (comments and empty lines ignored)"
    );

    // Verify all lines are valid JSON (no comments or empty lines)
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
    }
}

#[test]
fn test_ignore_lines_with_specific_pattern() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}
{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "systemd", // Ignore lines containing "systemd"
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines pattern"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (systemd lines ignored)"
    );

    // Verify systemd lines are not present
    for line in lines {
        assert!(
            !line.contains("systemd"),
            "Output should not contain systemd lines"
        );
    }
}

#[test]
fn test_keep_lines_functionality() {
    let input = r#"{"level": "INFO", "message": "This is an info message"}
# This is a comment line
{"level": "ERROR", "message": "This is an error message"}

{"level": "DEBUG", "message": "This is a debug message"}
# Another comment
{"level": "WARN", "message": "This is a warning"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            r#"^\{"#, // Keep only lines starting with JSON (curly brace)
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with keep-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "Should output 4 lines (only JSON lines kept)"
    );

    // Verify all lines are valid JSON (no comments or empty lines)
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
    }
}

#[test]
fn test_zero_results_with_unseen_filter_field_emits_hint() {
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.levle == "INFO""#], input);

    assert_eq!(
        exit_code, 0,
        "missing filter fields should remain non-fatal in resilient mode"
    );
    assert!(
        stderr.contains("0 events matched"),
        "stderr should explain empty results: {}",
        stderr
    );
    assert!(
        stderr.contains("levle"),
        "stderr should include the unseen field name: {}",
        stderr
    );
}

#[test]
fn test_bare_field_reference_suggests_e_prefix() {
    // The most common newcomer mistake: referencing a field without the `e.`
    // prefix. The hint should point straight at `e.<field>` rather than a
    // string-similar scope variable.
    let input = r#"{"level": "INFO", "status": 200}"#;

    let (_stdout, stderr, _exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", "status >= 500"], input);

    assert!(
        stderr.contains("e.status"),
        "stderr should suggest the e.-prefixed field for a bare reference: {}",
        stderr
    );
}

#[test]
fn test_near_miss_bare_field_suggests_closest_e_prefixed_field() {
    let input = r#"{"level": "INFO", "status": 200}"#;

    let (_stdout, stderr, _exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", "statuss >= 500"], input);

    assert!(
        stderr.contains("e.status"),
        "stderr should suggest the closest e.-prefixed field for a near-miss: {}",
        stderr
    );
}

#[test]
fn test_zero_results_with_existing_filter_field_does_not_emit_typo_hint() {
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.level == "ERROR""#], input);

    assert_eq!(
        exit_code, 0,
        "non-matching filters should remain successful"
    );
    assert!(
        !stderr.contains("0 events matched"),
        "stderr should not suggest a typo for a legitimate filter miss: {}",
        stderr
    );
}

#[test]
fn test_zero_results_valid_field_stays_silent() {
    // A `--filter` on a real field whose value simply never occurs is a
    // legitimate miss with no specific culprit (not a typo, not a numeric/string
    // mismatch). Under the Rule of Silence it produces no hint — empty output
    // after your own filter is self-evident — not even with --diagnostics, which
    // only forces the advisory tiers on, it does not invent advice.
    let input = "{\"level\": \"INFO\"}\n{\"level\": \"DEBUG\"}";

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--diagnostics",
            "--filter",
            r#"e.level == "NOPE""#,
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "a non-matching filter should remain successful"
    );
    assert!(
        !stderr.contains("events matched"),
        "a legitimate filter miss with no detectable culprit should stay silent: {}",
        stderr
    );
}

#[test]
fn test_keys_typo_hints_with_nearest_field_suggestion() {
    // `-k levle` is a typo for the `level` field. Every event is emptied and
    // dropped, producing silent empty output + exit 0. The hint should name the
    // unseen key and suggest the closest real field.
    let input = r#"{"ts": "x", "level": "INFO", "msg": "hi"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-k", "levle"], input);

    assert_eq!(exit_code, 0, "a key typo should remain non-fatal");
    assert!(
        stdout.trim().is_empty(),
        "no events should be output: {}",
        stdout
    );
    assert!(
        stderr.contains("levle") && stderr.contains("never present"),
        "stderr should name the unseen key: {}",
        stderr
    );
    assert!(
        stderr.contains("Did you mean 'level'?"),
        "stderr should suggest the closest field: {}",
        stderr
    );
}

#[test]
fn test_keys_rename_lists_present_fields_when_no_near_match() {
    // `-k timestamp` against a `ts`-keyed log is a rename, not a typo: the names
    // are too lexically distant for a nearest-match suggestion, so the hint lists
    // the fields actually present (which surfaces the real `ts` name).
    let input = r#"{"ts": "x", "level": "INFO", "msg": "hi"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "timestamp"], input);

    assert_eq!(exit_code, 0, "a key rename miss should remain non-fatal");
    assert!(
        stderr.contains("timestamp") && stderr.contains("never present"),
        "stderr should name the unseen key: {}",
        stderr
    );
    assert!(
        stderr.contains("Present fields:") && stderr.contains("ts"),
        "stderr should list the fields actually present: {}",
        stderr
    );
}

#[test]
fn test_keys_typo_among_valid_keys_hints_even_with_output() {
    // `-k ts,levle`: `ts` exists so output is non-empty (exit looks fine), but
    // `levle` was silently dropped. The hint must still fire on the unseen key.
    let input = r#"{"ts": "x", "level": "INFO", "msg": "hi"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "ts,levle"], input);

    assert_eq!(exit_code, 0, "a partial key typo should remain non-fatal");
    assert!(
        stdout.contains("ts="),
        "the valid key should still be emitted: {}",
        stdout
    );
    assert!(
        stderr.contains("levle") && stderr.contains("Did you mean 'level'?"),
        "stderr should flag the unseen key even when output is non-empty: {}",
        stderr
    );
}

#[test]
fn test_exclude_keys_typo_hints_silent_redaction_failure() {
    // The quiet failure: `--exclude-keys passwrd` (typo for `passwd`) drops
    // nothing, so the field meant to be scrubbed stays in the output. Output is
    // non-empty and exit is 0, so only a hint reveals the mistake.
    let input = r#"{"ts": "x", "passwd": "secret", "msg": "hi"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--exclude-keys", "passwrd"], input);

    assert_eq!(exit_code, 0, "an exclude-key typo should remain non-fatal");
    assert!(
        stdout.contains("passwd="),
        "the field stays in output because the exclude did not match: {}",
        stdout
    );
    assert!(
        stderr.contains("passwrd")
            && stderr.contains("not removed")
            && stderr.contains("Did you mean 'passwd'?"),
        "stderr should explain the silent exclude failure: {}",
        stderr
    );
}

#[test]
fn test_keys_nested_map_path_points_to_get_path() {
    // A dotted path copied from --discover (a value nested in a map) can't be
    // selected by -k; the hint should point at get_path, not guess the parent.
    let input = r#"{"user": {"name": "alice"}}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "user.name"], input);

    assert_eq!(exit_code, 0);
    assert!(
        stderr.contains("'user' is present")
            && stderr.contains("get_path(\"user.name\")")
            && !stderr.contains("Did you mean 'user'?"),
        "nested map path should suggest get_path, not the bare parent: {stderr}"
    );
}

#[test]
fn test_keys_array_element_path_points_to_whole_field() {
    // `field[]` is discover's notation for array elements; the array itself is a
    // selectable top-level field, so the hint should suggest `-k tags`.
    let input = r#"{"tags": ["a", "b"]}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "tags[]"], input);

    assert_eq!(exit_code, 0);
    assert!(
        stderr.contains("top-level field 'tags'") && stderr.contains("-k tags"),
        "array-element path should point at selecting the whole field: {stderr}"
    );
}

#[test]
fn test_keys_literal_dotted_field_is_selectable_without_hint() {
    // A top-level field whose literal name contains a dot must remain selectable;
    // because it is present, the "never present" hint never fires for it.
    let input = r#"{"user.name": "alice", "status": "ok"}"#;

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-F", "json", "-k", "user.name"], input);

    assert_eq!(exit_code, 0);
    assert_eq!(stdout.trim(), r#"{"user.name":"alice"}"#);
    assert!(
        !stderr.contains("never present"),
        "a present literal-dotted field must not be flagged: {stderr}"
    );
}

#[test]
fn test_keys_present_in_some_rows_does_not_hint() {
    // Heterogeneous logs legitimately have fields missing from some rows. As long
    // as a key appears somewhere in the stream, it is not a typo — no hint.
    let input = "{\"ts\": \"x\", \"level\": \"INFO\"}\n{\"ts\": \"y\", \"detail\": \"d\"}";

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "ts,detail"], input);

    assert_eq!(exit_code, 0, "selecting present keys should succeed");
    assert!(
        !stderr.contains("never present"),
        "stderr should not flag a key that appears in at least one row: {}",
        stderr
    );
}

#[test]
fn test_keys_typo_hint_suppressed_by_silent() {
    // The hint is a diagnostic; --silent must suppress it like the others.
    let input = r#"{"ts": "x"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-k", "bogus", "--silent"], input);

    assert_eq!(exit_code, 0, "a key typo should remain non-fatal");
    assert!(
        stderr.trim().is_empty(),
        "--silent should suppress the key typo hint: {}",
        stderr
    );
}

#[test]
fn test_zero_results_numeric_string_comparison_emits_hint() {
    // The most common beginner mistake on typed data: comparing a numeric field
    // to a quoted number. In Rhai a number never equals a string, so the filter is
    // silently always false. The hint should point at dropping the quotes.
    let input = r#"{"status": 404}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.status == "404""#], input);

    assert_eq!(exit_code, 0, "a non-matching filter stays non-fatal");
    assert!(
        stderr.contains("0 events matched") && stderr.contains("e.status == 404"),
        "stderr should suggest dropping the quotes: {}",
        stderr
    );
}

#[test]
fn test_zero_results_non_numeric_string_comparison_no_numeric_hint() {
    // Comparing a string field to a non-numeric literal that simply matched
    // nothing is a legitimate miss — no numeric-mismatch hint (and no nagging).
    let input = r#"{"name": "bob"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "--filter", r#"e.name == "alice""#], input);

    assert_eq!(exit_code, 0);
    assert!(
        !stderr.contains("0 events matched"),
        "a legitimate string miss should stay silent: {}",
        stderr
    );
}

#[test]
fn test_level_filter_on_unstructured_input_hints_missing_level_field() {
    // With `-f line` forced, the whole record stays in `line` and there is no
    // level field, so the level filter drops everything. Instead of a silent
    // empty result, point at the structural cause and offer a workaround.
    // (Under auto-detect this same shape now matches `iso8601-level` and gains a
    // real level field — see lnav_formats; this test pins the explicit-line path.)
    let input = "[2025-01-15 10:00:00] INFO started\n[2025-01-15 10:00:05] ERROR boom";

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "line", "-l", "error"], input);

    assert_eq!(exit_code, 0, "a level miss should remain non-fatal");
    assert!(stdout.is_empty(), "no events should be output: {}", stdout);
    assert!(
        stderr.contains("0 events matched") && stderr.contains("no level field"),
        "stderr should explain the missing level field: {}",
        stderr
    );
}

#[test]
fn test_level_filter_with_present_level_field_does_not_hint_missing_field() {
    // When a level field exists but no value matches, the empty result is a
    // legitimate mismatch, not a structural problem — no "missing level" hint.
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "json", "-l", "error"], input);

    assert_eq!(
        exit_code, 0,
        "a non-matching level should remain successful"
    );
    assert!(
        !stderr.contains("no level field"),
        "stderr should not claim a missing level field when one exists: {}",
        stderr
    );
}

#[test]
fn test_level_filter_vocabulary_mismatch_lists_levels_present() {
    // The dangerous operator case: glog logs use single-letter levels (I/W/E/F),
    // so `-l ERROR` matches nothing even though errors exist. A silent empty
    // result reads as "cluster healthy". Surface the levels actually present so
    // the dialect mismatch is visible — without claiming a missing level field.
    let input = "I0102 15:04:05.123456 1234 server.go:42] starting controller\n\
                 E0612 09:10:11.000001 7 reflector.go:138] failed to watch";

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "glog", "-l", "ERROR"], input);

    assert_eq!(exit_code, 0, "a level miss should remain non-fatal");
    assert!(stdout.is_empty(), "no events should be output: {}", stdout);
    assert!(
        stderr.contains("0 events matched") && stderr.contains("levels present:"),
        "stderr should list the levels actually present: {}",
        stderr
    );
    assert!(
        stderr.contains('E') && stderr.contains('I'),
        "the hint should show the glog levels seen (E, I): {}",
        stderr
    );
    assert!(
        !stderr.contains("no level field"),
        "a level field exists, so do not claim it is missing: {}",
        stderr
    );
}

#[test]
fn test_level_filter_partial_match_does_not_hint_vocabulary() {
    // When a requested level IS among those seen, an empty-ish result is a
    // legitimate "none of those right now" and must stay quiet — the vocabulary
    // hint only fires when the requested levels are entirely absent.
    let input = r#"{"level": "INFO", "message": "ok"}"#;

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "json", "-l", "INFO,ERROR"], input);

    assert_eq!(exit_code, 0);
    assert!(
        !stderr.contains("matched none of the levels present"),
        "INFO was present and matched, so no vocabulary hint should fire: {}",
        stderr
    );
}

#[test]
fn test_time_filter_without_timestamps_hints_missing_timestamp() {
    // `--since` on input with no parseable timestamp silently drops everything;
    // surface the structural cause and point at --ts-field/--ts-format.
    let input = "just some text\nmore text without a timestamp";

    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "line", "--since", "2025-01-01T00:00:00Z"], input);

    assert_eq!(exit_code, 0, "a time miss should remain non-fatal");
    assert!(stdout.is_empty(), "no events should be output: {}", stdout);
    assert!(
        stderr.contains("0 events matched") && stderr.contains("no timestamps were parsed"),
        "stderr should explain the missing timestamps: {}",
        stderr
    );
}

#[test]
fn test_keep_lines_with_specific_pattern() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}
{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            "ERROR|WARN", // Keep only ERROR and WARN level lines
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with keep-lines pattern"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "Should output 2 lines (only ERROR and WARN lines kept)"
    );

    // Verify only ERROR and WARN lines are present
    for line in lines {
        assert!(
            line.contains("ERROR") || line.contains("WARN"),
            "Output should only contain ERROR or WARN lines"
        );
    }
}

#[test]
fn test_combined_keep_lines_and_ignore_lines() {
    let input = r#"{"level": "INFO", "message": "User login successful"}
# This is a comment line
{"level": "DEBUG", "message": "systemd startup complete"}
{"level": "ERROR", "message": "Failed to connect to database"}

{"level": "DEBUG", "message": "systemd service started"}
{"level": "WARN", "message": "High memory usage detected"}
# Another comment"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--keep-lines",
            r#"^\{"#, // Keep only lines starting with JSON (curly brace)
            "--ignore-lines",
            "systemd", // Then ignore lines containing "systemd"
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with both keep-lines and ignore-lines"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "Should output 3 lines (JSON lines kept, then systemd lines ignored)"
    );

    // Verify lines are valid JSON and don't contain systemd
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
        assert!(
            !line.contains("systemd"),
            "Output should not contain systemd lines"
        );
    }

    // Verify specific levels are present
    let content = stdout.trim();
    assert!(content.contains("INFO"));
    assert!(content.contains("ERROR"));
    assert!(content.contains("WARN"));
    assert!(!content.contains("DEBUG")); // DEBUG lines contain systemd
}

#[test]
fn test_ignore_lines_with_stats() {
    let input = r#"{"level": "INFO", "message": "Valid message 1"}
# Comment to ignore
{"level": "ERROR", "message": "Valid message 2"}
# Another comment
{"level": "WARN", "message": "Valid message 3"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--ignore-lines",
            "^#",
            "--with-stats",
        ],
        input,
    );
    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully with ignore-lines and stats enabled"
    );

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should output 3 non-comment lines");

    let stats = extract_stats_lines(&stderr);
    let lines_processed = stats
        .iter()
        .find(|line| line.starts_with("Lines processed:"))
        .expect("Stats should report line counts");
    assert_eq!(
        lines_processed,
        "Lines processed: 5 total, 2 filtered (40.0%), 0 errors (0.0%)"
    );

    let events_created = stats
        .iter()
        .find(|line| line.starts_with("Events created:"))
        .expect("Stats should report event counts");
    assert_eq!(
        events_created,
        "Events created: 3 total, 3 output, 0 filtered (0.0%)"
    );
}

#[test]
fn test_multiple_filters() {
    let input = r#"{"level": "INFO", "status": 200, "response_time": 50}
{"level": "ERROR", "status": 500, "response_time": 100}
{"level": "WARN", "status": 404, "response_time": 200}"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--filter",
            "e.status >= 400",
            "--filter",
            "e.response_time > 150",
        ],
        input,
    );
    assert_eq!(exit_code, 0, "kelora should exit successfully");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        1,
        "Should filter to 1 line matching both conditions"
    );

    let parsed: serde_json::Value =
        serde_json::from_str(lines[0]).expect("Line should be valid JSON");
    assert_eq!(parsed["level"], "WARN");
    assert_eq!(parsed["status"], 404);
    assert_eq!(parsed["response_time"], 200);
}

#[test]
fn test_ordered_filter_exec_stages() {
    // Test that filter and exec stages execute in the exact CLI order
    let input = r#"{"status": "200", "message": "OK"}"#;

    // Test correct order: exec (convert) -> filter -> filter -> exec (add field)
    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--ignore-config",
            "--exec",
            "e.status=e.status.to_int()",
            "--filter",
            "e.status > 100",
            "--filter",
            "e.status < 400",
            "--exec",
            "e.level=\"info\"",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("status=200"));
    assert!(stdout.contains("level='info'"));

    // Test wrong order: filter before conversion should fail
    let (stdout2, _stderr2, _exit_code2) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--filter",
            "e.status > 100", // This will fail on string "200"
            "--exec",
            "e.status=e.status.to_int()",
            "--filter",
            "e.status < 400",
            "--exec",
            "e.level=\"info\"",
        ],
        input,
    );

    // Should produce no output because string "200" > 100 comparison doesn't work as expected
    assert!(stdout2.trim().is_empty());
}

#[test]
fn test_complex_ordered_pipeline() {
    // Test a more complex pipeline with transformations and filtering
    let input = r#"{"value": 5}
{"value": 15}
{"value": 25}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--ignore-config",
            "--exec",
            "e.doubled = e.value * 2",
            "--filter",
            "e.doubled > 20",
            "--exec",
            "e.status = if e.doubled > 30 { \"high\" } else { \"medium\" }",
        ],
        input,
    );

    assert_eq!(exit_code, 0);
    assert_eq!(stderr, "");

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2); // Should filter out value=5 (doubled=10)

    // Check first line (value=15, doubled=30, status="medium")
    assert!(lines[0].contains("value=15"));
    assert!(lines[0].contains("doubled=30"));
    assert!(lines[0].contains("status='medium'"));

    // Check second line (value=25, doubled=50, status="high")
    assert!(lines[1].contains("value=25"));
    assert!(lines[1].contains("doubled=50"));
    assert!(lines[1].contains("status='high'"));
}

#[test]
fn test_levels_before_exec_limits_exec_work() {
    let input = r#"{"level":"ERROR","message":"fail"}
{"level":"INFO","message":"ok"}
{"level":"WARN","message":"heads-up"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--levels",
            "error",
            "--exec",
            "track_sum(\"exec_runs\", 1)",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Filtering by --levels before --exec should succeed"
    );

    let exec_metric_line = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("exec_runs"))
        .expect("Metrics output should list exec_runs");
    assert!(
        exec_metric_line.contains("= 1"),
        "Exec stage should run once when --levels precedes it (saw `{}`)",
        exec_metric_line.trim()
    );
}

#[test]
fn test_exec_before_levels_observes_all_events() {
    let input = r#"{"level":"ERROR","message":"fail"}
{"level":"INFO","message":"ok"}
{"level":"WARN","message":"heads-up"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            "track_sum(\"exec_runs\", 1)",
            "--levels",
            "error",
            "--with-metrics",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "Filtering by --levels after --exec should succeed"
    );

    let exec_metric_line = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("exec_runs"))
        .expect("Metrics output should list exec_runs");
    assert!(
        exec_metric_line.contains("= 3"),
        "Exec stage should run on all three events when it precedes --levels (saw `{}`)",
        exec_metric_line.trim()
    );
}
