use std::fs;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Helper to run kelora in a specific directory with config files
fn run_kelora_in_dir(dir: &std::path::Path, args: &[&str], input: &str) -> (String, String, i32) {
    // Get absolute path to binary from current working directory before changing
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let mut cmd = Command::new(&binary_path)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    if let Some(stdin) = cmd.stdin.as_mut() {
        use std::io::Write;
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = cmd.wait_with_output().expect("Failed to read output");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn test_config_file_with_defaults() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create a config file with defaults
    fs::write(&config_path, "defaults = -f json --stats\n").unwrap();

    // Create a simple JSON log file
    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, r#"{"level": "info", "msg": "test"}"#).unwrap();

    // Run kelora with --config-file pointing to our test config
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("Events") || stderr.contains("processed"),
        "Expected stats in output, got: {}",
        stderr
    );
}

#[test]
fn test_config_file_with_alias() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create a config file with an alias
    fs::write(
        &config_path,
        "[aliases]\nerrors = --filter \"e.level == \\\"error\\\"\"\n",
    )
    .unwrap();

    // Create a JSON log file with mixed levels
    let log_file = temp_dir.path().join("test.log");
    fs::write(
        &log_file,
        r#"{"level": "info", "msg": "info message"}
{"level": "error", "msg": "error message"}
{"level": "info", "msg": "another info"}"#,
    )
    .unwrap();

    // Run kelora with the alias
    let (stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "-f",
            "json",
            "--alias",
            "errors",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully, stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("error message"),
        "Expected filtered error message, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("info message"),
        "Should not contain info messages"
    );
}

#[test]
fn test_ignore_config_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create a config file with defaults that would affect output
    fs::write(&config_path, "defaults = --stats\n").unwrap();

    // Create a simple log file
    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    // Run kelora with --ignore-config (should NOT show stats)
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &["--ignore-config", log_file.to_str().unwrap()],
        "",
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Should NOT show stats because config was ignored
    assert!(
        !stderr.contains("Events processed") && !stderr.contains("Events created"),
        "Should not show stats when config ignored, got: {}",
        stderr
    );
}

#[test]
fn test_project_config_precedence() {
    let temp_dir = TempDir::new().unwrap();

    // Create a project config in the current directory
    let project_config = temp_dir.path().join(".kelora.ini");
    fs::write(
        &project_config,
        "defaults = --stats\n[aliases]\ntest-alias = -q\n",
    )
    .unwrap();

    // Create a log file
    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    // Run kelora from temp_dir (should pick up .kelora.ini)
    let (_stdout, stderr, exit_code) =
        run_kelora_in_dir(temp_dir.path(), &[log_file.to_str().unwrap()], "");

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Should show stats from project config
    assert!(
        stderr.contains("Events") || stderr.contains("Stats"),
        "Expected stats from project config, got: {}",
        stderr
    );
}

#[test]
fn test_custom_config_file_path() {
    let temp_dir = TempDir::new().unwrap();

    // Create a config file in a non-standard location
    let custom_config = temp_dir.path().join("custom.ini");
    fs::write(&custom_config, "defaults = --stats\n").unwrap();

    // Create a log file
    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    // Run kelora with --config-file pointing to custom location
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            custom_config.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("Events") || stderr.contains("Stats"),
        "Expected stats from custom config, got: {}",
        stderr
    );
}

#[test]
fn test_invalid_config_produces_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create an invalid config file (unparseable defaults)
    fs::write(&config_path, "defaults = --filter 'unclosed quote\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    // Run kelora with the invalid config
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should fail with error code
    assert_ne!(exit_code, 0, "Should fail with invalid config");
    // Should mention the parsing error
    assert!(
        stderr.to_lowercase().contains("fail")
            || stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("invalid"),
        "Expected error message, got: {}",
        stderr
    );
}

#[test]
fn test_nonexistent_config_file_error() {
    let temp_dir = TempDir::new().unwrap();
    let nonexistent_config = temp_dir.path().join("does-not-exist.ini");

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    // Run kelora with --config-file pointing to nonexistent file
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            nonexistent_config.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should fail with error code
    assert_ne!(exit_code, 0, "Should fail with nonexistent config");
    assert!(
        stderr.contains("Failed to read config file") || stderr.contains("No such file"),
        "Expected file not found error, got: {}",
        stderr
    );
}

#[test]
fn test_recursive_alias_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with nested aliases
    fs::write(
        &config_path,
        "[aliases]\nbase = -f json\nerrors = --alias base --filter \"e.level == \\\"error\\\"\"\n",
    )
    .unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, r#"{"level": "error", "msg": "test"}"#).unwrap();

    // Run with nested alias
    let (_stdout, _stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--alias",
            "errors",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should successfully resolve nested aliases and process
    assert_eq!(exit_code, 0, "Should resolve nested aliases");
}

#[test]
fn test_circular_alias_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with circular aliases
    fs::write(
        &config_path,
        "[aliases]\nalias1 = --alias alias2\nalias2 = --alias alias1\n",
    )
    .unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test\n").unwrap();

    // Run with circular alias
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--alias",
            "alias1",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should fail with circular dependency error
    assert_ne!(exit_code, 0, "Should fail on circular alias");
    assert!(
        stderr.to_lowercase().contains("circular"),
        "Expected circular dependency error, got: {}",
        stderr
    );
}

#[test]
fn test_unknown_alias_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with one alias
    fs::write(&config_path, "[aliases]\nknown = -f json\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test\n").unwrap();

    // Run with unknown alias
    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--alias",
            "unknown",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should fail with unknown alias error
    assert_ne!(exit_code, 0, "Should fail on unknown alias");
    assert!(
        stderr.contains("Unknown alias") || stderr.to_lowercase().contains("unknown"),
        "Expected unknown alias error, got: {}",
        stderr
    );
}

#[test]
fn test_defaults_with_cli_override() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Config sets format to json
    fs::write(&config_path, "defaults = -f json\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "plain text log\n").unwrap();

    // CLI explicitly specifies line format after config defaults
    let (_stdout, _stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "-f",
            "line",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should succeed - CLI args should work alongside defaults
    assert_eq!(exit_code, 0, "CLI override should work with defaults");
}

#[test]
fn test_alias_with_quoted_args() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create alias with quoted filter expression
    fs::write(&config_path,
        "[aliases]\ncomplex = --filter \"e.level == \\\"error\\\" && e.msg.has_matches(\\\"critical\\\")\"\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(
        &log_file,
        r#"{"level": "error", "msg": "critical error"}
{"level": "error", "msg": "normal error"}
{"level": "info", "msg": "critical info"}"#,
    )
    .unwrap();

    let (stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "-f",
            "json",
            "--alias",
            "complex",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    assert_eq!(
        exit_code, 0,
        "kelora should exit successfully, stderr: {}",
        stderr
    );
    // Should only match the critical error
    assert!(
        stdout.contains("critical error"),
        "Should match critical error, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("normal error"),
        "Should not match normal error"
    );
    assert!(
        !stdout.contains("critical info"),
        "Should not match info level"
    );
}

#[test]
fn test_empty_config_file() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create empty config file
    fs::write(&config_path, "").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should succeed with empty config
    assert_eq!(exit_code, 0, "Empty config should not cause errors");
}

#[test]
fn test_config_with_comments() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with comments
    fs::write(&config_path,
        "# This is a comment\ndefaults = --stats\n; Another comment\n[aliases]\n# Alias comment\nerrors = -l error\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test\n").unwrap();

    let (_stdout, stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should parse config correctly, ignoring comments
    assert_eq!(exit_code, 0, "Config with comments should parse");
    assert!(
        stderr.contains("Events") || stderr.contains("Stats"),
        "Should apply defaults from commented config, got: {}",
        stderr
    );
}

#[test]
fn test_show_config_displays_content() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    let config_content = "defaults = -f json\n[aliases]\ntest = --stats\n";
    fs::write(&config_path, config_content).unwrap();

    let (stdout, _stderr, exit_code) = run_kelora_in_dir(temp_dir.path(), &["--show-config"], "");

    assert_eq!(
        exit_code, 0,
        "kelora --show-config should exit successfully"
    );
    // Should display the config file path and contents
    assert!(
        stdout.contains("defaults = -f json"),
        "Should show defaults, got: {}",
        stdout
    );
    assert!(stdout.contains("[aliases]"), "Should show aliases section");
    assert!(stdout.contains("test = --stats"), "Should show alias");
}

#[test]
fn test_show_config_no_file_found() {
    let temp_dir = TempDir::new().unwrap();

    // No config file in temp directory
    let (stdout, _stderr, exit_code) = run_kelora_in_dir(temp_dir.path(), &["--show-config"], "");

    assert_eq!(
        exit_code, 0,
        "kelora --show-config should exit successfully even without config"
    );
    // Should show message about no config found and example
    assert!(
        stdout.contains("No config found") || stdout.contains("Example"),
        "Should show helpful message when no config found, got: {}",
        stdout
    );
}

#[test]
fn test_multiple_aliases_in_one_command() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with multiple aliases
    fs::write(&config_path, "[aliases]\njson-fmt = -f json\nquiet = -q\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, r#"{"level": "info", "msg": "test"}"#).unwrap();

    let (_stdout, _stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--alias",
            "json-fmt",
            "--alias",
            "quiet",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    // Should successfully expand multiple aliases
    assert_eq!(exit_code, 0, "Should handle multiple aliases");
}

#[test]
fn test_defaults_and_alias_combination() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Defaults set format, alias adds filter
    fs::write(
        &config_path,
        "defaults = -f json\n[aliases]\nerrors = --filter \"e.level == \\\"error\\\"\"\n",
    )
    .unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(
        &log_file,
        r#"{"level": "info", "msg": "info"}
{"level": "error", "msg": "error"}"#,
    )
    .unwrap();

    let (stdout, _stderr, exit_code) = run_kelora_in_dir(
        temp_dir.path(),
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--alias",
            "errors",
            log_file.to_str().unwrap(),
        ],
        "",
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    // Should apply both defaults (json format) and alias (error filter)
    assert!(
        stdout.contains("error"),
        "Should show error level, got: {}",
        stdout
    );
    assert!(!stdout.contains("info"), "Should not show info level");
}
