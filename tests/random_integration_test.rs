// tests/random_integration_test.rs
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

/// Helper function to run kelora with a temporary file
fn run_kelora_with_file(args: &[&str], file_content: &str) -> (String, String, i32) {
    run_kelora_with_file_env(args, file_content, &[])
}

/// Like `run_kelora_with_file`, but with extra environment variables set on the
/// child process. Used to pin `KELORA_SEED` so randomness-dependent tests are
/// deterministic instead of probabilistically flaky.
fn run_kelora_with_file_env(
    args: &[&str],
    file_content: &str,
    envs: &[(&str, &str)],
) -> (String, String, i32) {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(file_content.as_bytes())
        .expect("Failed to write to temp file");

    let mut full_args = args.to_vec();
    full_args.push(temp_file.path().to_str().unwrap());

    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    let binary_path = env!("CARGO_BIN_EXE_kelora");

    let output = Command::new(binary_path)
        .args(&full_args)
        .envs(envs.iter().copied())
        .output()
        .expect("Failed to execute kelora");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn test_rand_function_basic() {
    let input = r#"{"message": "test1"}
{"message": "test2"}
{"message": "test3"}
{"message": "test4"}
{"message": "test5"}"#;

    // Filter using rand() - should pass some events through
    let (stdout, stderr, exit_code) =
        run_kelora_with_file(&["-f", "json", "--filter", "rand() < 0.8"], input);

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    // Should have some output (probabilistically)
    let lines: Vec<&str> = stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty())
        .collect();
    assert!(lines.len() <= 5, "Should not have more events than input");
}

#[test]
fn test_rand_int_function() {
    let input = r#"{"message": "test1"}
{"message": "test2"}
{"message": "test3"}"#;

    // Add random IDs using rand_int
    let (stdout, stderr, exit_code) = run_kelora_with_file(
        &[
            "-f",
            "json",
            "--output-format",
            "json",
            "-e",
            "e.random_id = rand_int(100, 999)",
        ],
        input,
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 3, "Should have 3 output events");

    // Check that random_id fields were added and are in range
    for line in lines {
        let json: serde_json::Value =
            serde_json::from_str(line).expect("Output should be valid JSON");

        let random_id = json["random_id"]
            .as_i64()
            .expect("Should have random_id field as integer");

        assert!(
            (100..=999).contains(&random_id),
            "Random ID {} should be in range [100, 999]",
            random_id
        );
    }
}

#[test]
fn test_rand_int_invalid_range() {
    let input = r#"{"message": "test"}"#;

    // Try to use rand_int with invalid range (min > max)
    let (_stdout, stderr, exit_code) = run_kelora_with_file(
        &["-f", "json", "-e", "e.bad_id = rand_int(999, 100)"],
        input,
    );

    // An invalid constant range errors on the exec stage, but exec is best-effort:
    // it rolls back to the original event and emits it, so default resilient mode
    // recovers and exits 0. (--strict fails — see the _strict variant below.)
    assert_eq!(
        exit_code, 0,
        "exec errors are recovered in resilient mode (best-effort)"
    );
    assert!(
        stderr.contains("Exec errors") || stderr.contains("Rhai error"),
        "Error message should mention Exec or Rhai error. stderr: {}",
        stderr
    );
}

#[test]
fn test_rand_int_invalid_range_strict() {
    let input = r#"{"message": "test"}"#;

    let (_stdout, stderr, exit_code) = run_kelora_with_file(
        &[
            "-f",
            "json",
            "--strict",
            "-e",
            "e.bad_id = rand_int(999, 100)",
        ],
        input,
    );

    assert_eq!(exit_code, 1, "strict exec errors should fail the process");
    assert!(
        stderr.contains("Pipeline error") || stderr.contains("exec error"),
        "Error message should mention the strict exec failure. stderr: {}",
        stderr
    );
}

#[test]
fn test_random_sampling_workflow() {
    // Create a larger dataset for sampling
    let input = (0..100)
        .map(|i| {
            format!(
                r#"{{"id": {}, "message": "event {}", "level": "INFO"}}"#,
                i, i
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Sample approximately 10% of events. A fixed KELORA_SEED makes rand()
    // deterministic so this test asserts an exact result instead of relying on
    // probability (an unseeded run can, very rarely, sample zero events and
    // flake on the "not empty" assertion).
    let (stdout, stderr, exit_code) = run_kelora_with_file_env(
        &[
            "-f",
            "json",
            "--output-format",
            "json",
            "--filter",
            "rand() < 0.1",
        ],
        &input,
        &[("KELORA_SEED", "42")],
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully. stderr: {}",
        stderr
    );

    let lines: Vec<&str> = stdout
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty())
        .collect();

    // With KELORA_SEED=42 the sampled count is deterministic (~10% of 100).
    assert_eq!(
        lines.len(),
        9,
        "Seeded rand() < 0.1 over 100 events should sample exactly 9. stderr: {}",
        stderr
    );

    // Verify each line is valid JSON
    for line in lines {
        let _: serde_json::Value = serde_json::from_str(line).expect("Output should be valid JSON");
    }
}

#[test]
fn test_invalid_seed_is_hard_error() {
    // An invalid KELORA_SEED must fail fast with a usage error rather than
    // silently falling back to a random seed (which would defeat reproducibility).
    for bad in ["notanumber", "-5", ""] {
        let (_stdout, stderr, exit_code) = run_kelora_with_file_env(
            &["-f", "json", "--filter", "rand() < 1.0"],
            r#"{"id": 1}"#,
            &[("KELORA_SEED", bad)],
        );

        assert_eq!(
            exit_code, 2,
            "KELORA_SEED='{}' should exit 2 (invalid usage). stderr: {}",
            bad, stderr
        );
        assert!(
            stderr.contains("KELORA_SEED"),
            "stderr should explain the invalid KELORA_SEED. stderr: {}",
            stderr
        );
    }
}
