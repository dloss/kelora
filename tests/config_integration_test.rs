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
        .env("HOME", dir) // Set HOME to temp dir for isolated config testing
        .env("XDG_CONFIG_HOME", dir.join(".config"))
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

fn run_kelora_in_dir_with_env(
    dir: &std::path::Path,
    args: &[&str],
    input: &str,
    envs: &[(&str, &str)],
) -> (String, String, i32) {
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let mut cmd = Command::new(&binary_path);
    cmd.current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in envs {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn().expect("Failed to start kelora");

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to read output");

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
    fs::write(&config_path, "defaults = -f json --with-stats\n").unwrap();

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
fn test_default_config_path_is_not_printed_on_normal_runs() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");
    fs::write(&config_path, "defaults = -f line\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "hello\n").unwrap();

    let (_stdout, stderr, exit_code) =
        run_kelora_in_dir(temp_dir.path(), &[log_file.to_str().unwrap()], "");

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        !stderr.contains("Config:"),
        "default config path should be suppressed on normal runs: {}",
        stderr
    );
    assert!(
        !stderr.contains("Defaults:"),
        "default config expansion details should be suppressed on normal runs: {}",
        stderr
    );
}

#[test]
fn test_explicit_config_file_path_is_still_printed() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("custom.ini");
    fs::write(&config_path, "defaults = -f line\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "hello\n").unwrap();

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
        stderr.contains("Config:"),
        "explicit config file path should still be shown: {}",
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
    fs::write(&config_path, "defaults = --with-stats\n").unwrap();

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
fn test_ignore_config_env_skips_ambient_project_and_user_config() {
    let temp_dir = TempDir::new().unwrap();
    let project_config = temp_dir.path().join(".kelora.ini");
    let user_config_dir = temp_dir.path().join(".config").join("kelora");
    let user_config = user_config_dir.join("kelora.ini");

    fs::write(&project_config, "defaults = --with-stats\n").unwrap();
    fs::create_dir_all(&user_config_dir).unwrap();
    fs::write(&user_config, "defaults = --with-stats\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    let (_stdout, stderr, exit_code) = run_kelora_in_dir_with_env(
        temp_dir.path(),
        &[log_file.to_str().unwrap()],
        "",
        &[("KELORA_IGNORE_CONFIG", "1")],
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        !stderr.contains("Config:"),
        "Should not show config expansion info, got: {}",
        stderr
    );
    assert!(
        !stderr.contains("Events processed") && !stderr.contains("Events created"),
        "Should not apply ambient config defaults, got: {}",
        stderr
    );
}

#[test]
fn test_ignore_config_env_still_allows_explicit_config_file() {
    let temp_dir = TempDir::new().unwrap();
    let ambient_project_config = temp_dir.path().join(".kelora.ini");
    let explicit_config = temp_dir.path().join("explicit.ini");

    fs::write(&ambient_project_config, "defaults = -q\n").unwrap();
    fs::write(&explicit_config, "defaults = --with-stats\n").unwrap();

    let log_file = temp_dir.path().join("test.log");
    fs::write(&log_file, "test log line\n").unwrap();

    let (_stdout, stderr, exit_code) = run_kelora_in_dir_with_env(
        temp_dir.path(),
        &[
            "--config-file",
            explicit_config.to_str().unwrap(),
            log_file.to_str().unwrap(),
        ],
        "",
        &[("KELORA_IGNORE_CONFIG", "1")],
    );

    assert_eq!(exit_code, 0, "kelora should exit successfully");
    assert!(
        stderr.contains("Config:"),
        "Expected explicit config file to load, got: {}",
        stderr
    );
    assert!(
        stderr.contains("Events") || stderr.contains("processed"),
        "Expected explicit config defaults to apply, got: {}",
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
        "defaults = --with-stats\n[aliases]\ntest-alias = -q\n",
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
    fs::write(&custom_config, "defaults = --with-stats\n").unwrap();

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
        "[aliases]\ncomplex = --filter \"e.level == \\\"error\\\" && e.msg.matches(\\\"critical\\\")\"\n").unwrap();

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
        "# This is a comment\ndefaults = --with-stats\n; Another comment\n[aliases]\n# Alias comment\nerrors = -l error\n").unwrap();

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

    let config_content = "defaults = -f json\n[aliases]\ntest = --with-stats\n";
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
    assert!(stdout.contains("test = --with-stats"), "Should show alias");
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

#[test]
fn test_save_alias_update_with_self_reference() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create initial alias
    fs::write(&config_path, "[aliases]\nerrors = -l error\n").unwrap();

    // Update it with self-reference + new flag
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let output = Command::new(&binary_path)
        .args([
            "--save-alias",
            "errors",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "errors",
            "--stats",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "kelora should exit successfully, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let config_content = fs::read_to_string(&config_path).unwrap();
    // Should resolve -a errors to -l error, then add --stats
    assert!(
        config_content.contains("errors = -l error --stats"),
        "Config should contain flattened alias, got: {}",
        config_content
    );
    // Should NOT contain -a reference
    assert!(
        !config_content.contains("-a errors") && !config_content.contains("--alias errors"),
        "Config should not contain alias reference, got: {}",
        config_content
    );
}

#[test]
fn test_save_alias_strips_positional_input_files() {
    // An alias is a reusable bundle of options; the positional input file that
    // happened to be on the command line when saving should not be baked in.
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let output = Command::new(&binary_path)
        .args([
            "--save-alias",
            "errs",
            "--config-file",
            config_path.to_str().unwrap(),
            "examples/simple_json.jsonl",
            "-j",
            "-l",
            "error",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "kelora should exit successfully, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(
        config_content.contains("errs = -j -l error"),
        "alias should keep the options, got: {}",
        config_content
    );
    assert!(
        !config_content.contains("simple_json.jsonl"),
        "alias should not bake in the input file, got: {}",
        config_content
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Not saving input file") && stderr.contains("simple_json.jsonl"),
        "user should be told the file was dropped, stderr: {}",
        stderr
    );
}

#[test]
fn test_save_alias_compose_with_reference() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create base alias
    fs::write(&config_path, "[aliases]\nerrors = -l error\n").unwrap();

    // Create new alias referencing the base
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let output = Command::new(&binary_path)
        .args([
            "--save-alias",
            "json-errors",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "errors",
            "-f",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "kelora should exit successfully, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let config_content = fs::read_to_string(&config_path).unwrap();
    // Should preserve the -a reference
    assert!(
        config_content.contains("json-errors = -a errors -f json"),
        "Config should preserve alias reference, got: {}",
        config_content
    );
}

#[test]
fn test_save_alias_error_on_unknown_reference() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create config with one alias
    fs::write(&config_path, "[aliases]\nknown = -l error\n").unwrap();

    // Try to create alias referencing unknown alias
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let output = Command::new(&binary_path)
        .args([
            "--save-alias",
            "new-alias",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "unknown",
            "--stats",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "kelora should fail when referencing unknown alias"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Referenced alias 'unknown' does not exist") || stderr.contains("unknown"),
        "Error message should mention unknown alias, got: {}",
        stderr
    );
}

#[test]
fn test_save_alias_nested_resolution_in_update() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    // Create nested aliases
    fs::write(
        &config_path,
        "[aliases]\nbase = -f json\nerrors = -a base -l error\n",
    )
    .unwrap();

    // Update errors with self-reference (should fully resolve)
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    let output = Command::new(&binary_path)
        .args([
            "--save-alias",
            "errors",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "errors",
            "--stats",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "kelora should exit successfully, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let config_content = fs::read_to_string(&config_path).unwrap();
    // Should fully resolve: -a errors -> -a base -l error -> -f json -l error
    // Then add --stats
    assert!(
        config_content.contains("errors = -f json -l error --stats"),
        "Config should contain fully resolved alias, got: {}",
        config_content
    );
    assert!(
        !config_content.contains("json-errors")
            || !config_content[config_content.find("errors = ").unwrap()..].contains("-a"),
        "Errors alias should not contain -a reference, got: {}",
        config_content
    );
}

#[test]
fn test_save_alias_full_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".kelora.ini");

    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/kelora")
    } else {
        std::env::current_dir()
            .unwrap()
            .join("target/release/kelora")
    };

    // Step 1: Create base alias
    Command::new(&binary_path)
        .args([
            "--save-alias",
            "json",
            "--config-file",
            config_path.to_str().unwrap(),
            "-f",
            "json",
        ])
        .output()
        .unwrap();

    // Step 2: Compose new alias referencing base
    Command::new(&binary_path)
        .args([
            "--save-alias",
            "json-errors",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "json",
            "-l",
            "error",
        ])
        .output()
        .unwrap();

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("json = -f json"),
        "Should have json alias, got: {}",
        content
    );
    assert!(
        content.contains("json-errors = -a json -l error"),
        "Should have json-errors with reference, got: {}",
        content
    );

    // Step 3: Update json-errors with self-reference
    Command::new(&binary_path)
        .args([
            "--save-alias",
            "json-errors",
            "--config-file",
            config_path.to_str().unwrap(),
            "-a",
            "json-errors",
            "--stats",
        ])
        .output()
        .unwrap();

    let content = fs::read_to_string(&config_path).unwrap();
    // Should resolve: -a json-errors -> -a json -l error -> -f json -l error
    // Then add --stats
    assert!(
        content.contains("json-errors = -f json -l error --stats"),
        "Should have flattened json-errors, got: {}",
        content
    );
}
