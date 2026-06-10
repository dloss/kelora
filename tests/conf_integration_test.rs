// tests/init_integration_test.rs
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn kelora_binary_path() -> &'static str {
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    env!("CARGO_BIN_EXE_kelora")
}

/// Helper function to run kelora with given arguments and input via stdin
fn run_kelora_with_input(args: &[&str], input: &str) -> (String, String, i32) {
    let mut cmd = Command::new(kelora_binary_path())
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start kelora");

    if let Some(stdin) = cmd.stdin.as_mut() {
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
fn test_init_map_basic_functionality() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test data file
    let data_file = temp_dir.path().join("whitelist.txt");
    fs::write(&data_file, "192.168.1.1\n10.0.0.1\n127.0.0.1\n").expect("Failed to write data file");

    let begin_script = format!(
        "conf.whitelist = read_lines(\"{}\")",
        data_file.to_str().unwrap()
    );

    let input = r#"{"ip": "192.168.1.1", "action": "connect"}
{"ip": "192.168.1.2", "action": "connect"}
{"ip": "10.0.0.1", "action": "connect"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--begin",
            &begin_script,
            "--filter",
            "conf.whitelist.contains(e.ip)",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Should only contain whitelisted IPs
    assert!(stdout.contains("192.168.1.1"));
    assert!(!stdout.contains("192.168.1.2")); // Not in whitelist
    assert!(stdout.contains("10.0.0.1"));
}

#[test]
fn test_read_file_function() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test banner file
    let banner_file = temp_dir.path().join("banner.txt");
    fs::write(
        &banner_file,
        "Welcome to the system!\nPlease follow security guidelines.",
    )
    .expect("Failed to write banner file");

    let begin_script = format!(
        "conf.banner = read_file(\"{}\")",
        banner_file.to_str().unwrap()
    );

    let input = r#"{"user": "alice", "action": "login"}
{"user": "bob", "action": "login"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--begin",
            &begin_script,
            "--exec",
            "if e.action == \"login\" { print(conf.banner) }",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);

    // Should print banner for each login
    assert_eq!(stdout.matches("Welcome to the system!").count(), 2);
    assert_eq!(
        stdout.matches("Please follow security guidelines.").count(),
        2
    );
}

#[test]
fn test_init_map_immutability() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test data file
    let data_file = temp_dir.path().join("config.txt");
    fs::write(&data_file, "admin\nuser\nguest\n").expect("Failed to write data file");

    let begin_script = format!(
        "conf.roles = read_lines(\"{}\")",
        data_file.to_str().unwrap()
    );

    let input = r#"{"user": "alice", "role": "admin"}"#;

    // Try to modify conf map after --begin phase
    let (_stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--begin",
            &begin_script,
            "--exec",
            "conf.roles.push(\"hacker\"); e.valid = conf.roles.contains(e.role)",
        ],
        input,
    );

    assert_ne!(
        exit_code, 0,
        "Conf mutations after --begin should be rejected"
    );
    assert!(
        stderr.contains("conf map is read-only"),
        "Should surface immutability error. stderr: {}",
        stderr
    );
}

#[test]
fn test_conf_read_in_exec_then_non_conf_filter() {
    // Regression: a --filter (or any stage) that does NOT reference `conf`
    // skips loading the real conf map into its scope. The conf-immutability
    // check must not treat that empty placeholder as a mutation. Previously
    // this raised a spurious "conf map is read-only" error.
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let data_file = temp_dir.path().join("blocked.txt");
    fs::write(&data_file, "192.168.1.100\n10.0.0.50\n").expect("Failed to write data file");

    let begin_script = format!(
        "conf.blocked = read_lines(\"{}\")",
        data_file.to_str().unwrap()
    );

    let input = r#"{"ip": "192.168.1.100", "user": "admin"}
{"ip": "192.168.1.200", "user": "alice"}
{"ip": "10.0.0.50", "user": "bob"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--begin",
            &begin_script,
            "--exec",
            "e.is_blocked = e.ip in conf.blocked",
            "--filter",
            "e.is_blocked",
            "-k",
            "ip,user,is_blocked",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);
    assert!(
        !stderr.contains("conf map is read-only"),
        "Reading conf in --exec then filtering on a non-conf field must not \
         trigger the immutability error. stderr: {}",
        stderr
    );
    // Only the two blocked IPs should pass the filter.
    assert!(stdout.contains("192.168.1.100"));
    assert!(stdout.contains("10.0.0.50"));
    assert!(!stdout.contains("192.168.1.200"));
}

#[test]
fn test_read_functions_only_in_begin() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test data file
    let data_file = temp_dir.path().join("test.txt");
    fs::write(&data_file, "test data\n").expect("Failed to write data file");

    let input = r#"{"test": "data"}"#;

    // Try to use read_file outside of --begin phase
    let (_stdout, stderr, _exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "--exec",
            &format!("e.data = read_file(\"{}\")", data_file.to_str().unwrap()),
        ],
        input,
    );

    assert!(
        stderr.contains("can only be called during --begin phase"),
        "Should surface phase restriction error. stderr: {}",
        stderr
    );
}

#[test]
fn test_empty_file_handling() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create empty files
    let empty_file = temp_dir.path().join("empty.txt");
    fs::write(&empty_file, "").expect("Failed to write empty file");

    let empty_lines_file = temp_dir.path().join("empty_lines.txt");
    fs::write(&empty_lines_file, "").expect("Failed to write empty lines file");

    let begin_script = format!(
        "conf.empty_content = read_file(\"{}\"); conf.empty_lines = read_lines(\"{}\")",
        empty_file.to_str().unwrap(),
        empty_lines_file.to_str().unwrap()
    );

    let input = r#"{"test": "data"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f", "json",
            "-F", "json",
            "--begin", &begin_script,
            "--exec", "e.empty_content_len = conf.empty_content.len(); e.empty_lines_len = conf.empty_lines.len()"
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);
    assert!(stdout.contains("\"empty_content_len\":0"));
    assert!(stdout.contains("\"empty_lines_len\":0"));
}

#[test]
fn test_save_alias_preserves_no_emoji_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("kelora.ini");

    let output = Command::new(kelora_binary_path())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args([
            "--save-alias",
            "noemoji",
            "--config-file",
            config_path.to_str().unwrap(),
            "-f",
            "json",
            "--no-emoji",
            "examples/simple_json.jsonl",
        ])
        .output()
        .expect("Failed to run kelora --save-alias command");

    assert!(
        output.status.success(),
        "kelora --save-alias exited with {:?}. stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Alias 'noemoji' saved to"),
        "Should print success message. stdout: {}",
        stdout
    );

    let config_contents =
        fs::read_to_string(&config_path).expect("Failed to read generated config file");

    assert!(
        config_contents.contains("--no-emoji"),
        "Alias should retain --no-emoji flag. Contents:\n{}",
        config_contents
    );
}

#[test]
fn test_utf8_bom_handling() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create file with UTF-8 BOM
    let bom_file = temp_dir.path().join("bom.txt");
    let content_with_bom = format!("{}{}", '\u{feff}', "line1\nline2\nline3");
    fs::write(&bom_file, content_with_bom).expect("Failed to write BOM file");

    let begin_script = format!(
        "conf.lines = read_lines(\"{}\")",
        bom_file.to_str().unwrap()
    );

    let input = r#"{"test": "data"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--begin",
            &begin_script,
            "--exec",
            "e.first_line = conf.lines[0]; e.lines_count = conf.lines.len()",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);
    // First line should not contain BOM character
    assert!(stdout.contains("\"first_line\":\"line1\""));
    assert!(stdout.contains("\"lines_count\":3"));
}

#[test]
fn test_newline_stripping() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create file with different newline styles
    let newline_file = temp_dir.path().join("newlines.txt");
    fs::write(&newline_file, "line1\nline2\r\nline3\n").expect("Failed to write newline file");

    let begin_script = format!(
        "conf.lines = read_lines(\"{}\")",
        newline_file.to_str().unwrap()
    );

    let input = r#"{"test": "data"}"#;

    let (stdout, stderr, exit_code) = run_kelora_with_input(
        &[
            "-f",
            "json",
            "-F",
            "json",
            "--begin",
            &begin_script,
            "--exec",
            "e.line1 = conf.lines[0]; e.line2 = conf.lines[1]; e.line3 = conf.lines[2]",
        ],
        input,
    );

    assert_eq!(exit_code, 0, "Command should succeed. stderr: {}", stderr);
    // Lines should not contain newline characters
    assert!(stdout.contains("\"line1\":\"line1\""));
    assert!(stdout.contains("\"line2\":\"line2\""));
    assert!(stdout.contains("\"line3\":\"line3\""));
}
