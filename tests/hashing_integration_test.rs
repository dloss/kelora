use std::path::PathBuf;
use std::process::Command;

fn kelora_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_kelora") {
        return PathBuf::from(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let exe_name = if cfg!(windows) {
        "kelora.exe"
    } else {
        "kelora"
    };

    let debug_path = manifest_dir.join("target").join("debug").join(exe_name);
    if debug_path.exists() {
        return debug_path;
    }

    manifest_dir.join("target").join("release").join(exe_name)
}

#[test]
fn test_bucket_function() {
    // Feed input via stdin
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "line", "--exec", "e.bucket = bucket(e.line)"])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"test\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bucket="));
}

#[test]
fn test_hash_function_default() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "line", "--exec", "e.hash = hash(e.line)"])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"hello\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // SHA-256 hash of "hello"
    assert!(stdout.contains("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"));
}

#[test]
fn test_hash_function_with_algorithm() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.hash_xxh3 = hash(e.line, "xxh3")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"hello\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // xxh3 hash should be present and be 16 hex characters
    assert!(stdout.contains("hash_xxh3="));
}

#[test]
fn test_pseudonym_with_env_secret() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test_secret_12345")
        .args([
            "-vv",
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should log stable mode (only visible with -vv)
    assert!(stderr.contains("pseudonym: ON (stable; KELORA_SECRET)"));
    // Should produce pseudonym
    assert!(stdout.contains("pseudo="));
}

#[test]
fn test_pseudonym_ephemeral_mode() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env_remove("KELORA_SECRET")
        .args([
            "-vv",
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should log ephemeral mode (only visible with -vv)
    assert!(stderr.contains("pseudonym: ON (ephemeral; not stable)"));
    // Should produce pseudonym
    assert!(stdout.contains("pseudo="));
}

#[test]
fn test_pseudonym_ephemeral_warning_at_default_verbosity() {
    // Without KELORA_SECRET and without -vv, the safety warning must still appear
    // on stderr so users aren't silently surprised by non-correlating pseudonyms.
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env_remove("KELORA_SECRET")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Warning shown without -vv
    assert!(
        stderr.contains("KELORA_SECRET not set"),
        "expected ephemeral-key warning on stderr, got: {}",
        stderr
    );
    // Pseudonymization still works
    assert!(stdout.contains("pseudo="));
}

#[test]
fn test_pseudonym_ephemeral_warning_suppressed_when_silent() {
    // --silent (and --no-diagnostics) should suppress the ephemeral-key warning.
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env_remove("KELORA_SECRET")
        .args([
            "--no-diagnostics",
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("KELORA_SECRET not set"),
        "ephemeral-key warning should be suppressed with --no-diagnostics, got: {}",
        stderr
    );
}

#[test]
fn test_pseudonym_domain_separation() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test_secret_12345")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo_email = pseudonym(e.line, "kelora:v1:email"); e.pseudo_ip = pseudonym(e.line, "kelora:v1:ip")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Both pseudonyms should be present
    assert!(stdout.contains("pseudo_email="));
    assert!(stdout.contains("pseudo_ip="));

    // Extract the values to check they're different (not easily done in shell)
    // For now, just verify both are present
}

#[test]
fn test_pseudonym_deterministic() {
    // Run twice with same secret
    let output1 = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test_secret_12345")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let output2 = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test_secret_12345")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);

    // Should produce identical output (deterministic)
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_pseudonym_empty_domain_error() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test_secret_12345")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("domain must be non-empty"));
}

#[test]
fn test_pseudonym_empty_secret_error() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "")
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.pseudo = pseudonym(e.line, "kelora:v1:user")"#,
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"user123\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should fail with message about empty secret
    assert!(stderr.contains("KELORA_SECRET must not be empty"));
    // Process should exit with error
    assert!(!output.status.success());
}

fn run_pseudonymize_ips_prefix(secret: &str, input: &[u8]) -> String {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", secret)
        .args([
            "-f",
            "line",
            "--exec",
            r#"e.p = e.line.pseudonymize_ips("prefix")"#,
            "-k",
            "p",
        ])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input).ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_pseudonymize_ips_prefix_keyed_by_secret() {
    let input = b"10.0.5.23\n10.0.5.24\n2001:db8::1\nnot-an-ip\n";
    let first = run_pseudonymize_ips_prefix("secret-a", input);
    assert_eq!(first, run_pseudonymize_ips_prefix("secret-a", input));
    assert_ne!(first, run_pseudonymize_ips_prefix("secret-b", input));

    let lines: Vec<&str> = first.lines().collect();
    // Two IPv4 pseudonyms sharing the /24, one IPv6; text without an
    // address passes through
    assert_eq!(lines.len(), 4, "{first}");
    let net = |l: &str| l.rsplit_once('.').unwrap().0.to_string();
    assert_eq!(net(lines[0]), net(lines[1]));
    assert!(lines[2].contains(':'));
    assert_eq!(lines[3], "p='not-an-ip'");
}

fn run_kelora_with_secret(args: &[&str], input: &[u8]) -> std::process::Output {
    Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env("KELORA_SECRET", "test-secret")
        .args(args)
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input).ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora")
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_pseudonymize_ips_matches_single_address_functions() {
    let script = r#"
        e.token = e.line.pseudonymize_ips();
        e.token_ok = e.token == "from <ip:" + pseudonym("10.0.0.1", "ip") + ">:80 and <ip:" + pseudonym("10.0.0.11", "ip") + ">";
        e.prefix = e.line.pseudonymize_ips("prefix");
        e.prefix_ok = e.prefix == "from " + "10.0.0.1".pseudonymize_ips("prefix") + ":80 and " + "10.0.0.11".pseudonymize_ips("prefix");
        e.prefix_ok = e.prefix_ok && e.prefix.extract_ip() != "10.0.0.1";
    "#;
    let out = stdout_of(&run_kelora_with_secret(
        &["-f", "line", "-e", script, "-k", "token_ok,prefix_ok"],
        b"from 10.0.0.1:80 and 10.0.0.11\n",
    ));
    assert_eq!(out.trim(), "token_ok=true prefix_ok=true");
}

#[test]
fn test_pseudonymize_flag_hides_addresses_everywhere() {
    let input = b"{\"ip\":\"10.0.0.1\",\"msg\":\"from 2001:db8::1 port 22\",\"peers\":{\"192.168.1.5\":[\"192.168.1.6\"]}}\n";
    for kind in ["ip", "ip-prefix"] {
        let output = run_kelora_with_secret(
            &[
                "-f",
                "json",
                "--pseudonymize",
                kind,
                "-F",
                "json",
                "--script-output",
                "-e",
                "print(meta.line)",
            ],
            input,
        );
        let out = stdout_of(&output);
        for ip in ["10.0.0.1", "2001:db8::1", "192.168.1.5", "192.168.1.6"] {
            assert!(!out.contains(ip), "{kind}: {ip} leaked in {out}");
        }
        assert!(out.contains("port 22"), "{out}");
    }
}

#[test]
fn test_pseudonymize_flag_runs_before_filters() {
    let input = b"{\"ip\":\"10.0.0.1\"}\n{\"ip\":\"10.0.0.2\"}\n";
    let out = stdout_of(&run_kelora_with_secret(
        &[
            "-f",
            "json",
            "--pseudonymize",
            "ip",
            "--filter",
            r#"e.ip == "10.0.0.1".pseudonymize_ips()"#,
        ],
        input,
    ));
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[test]
fn test_pseudonymize_flag_parallel_matches_sequential() {
    let input = b"a 10.0.0.1\nb 10.0.0.2\nc 10.0.0.1\n";
    let args = ["-f", "line", "--pseudonymize", "ip-prefix"];
    let sequential = stdout_of(&run_kelora_with_secret(&args, input));
    let parallel = stdout_of(&run_kelora_with_secret(
        &[&args[..], &["--parallel"]].concat(),
        input,
    ));
    assert_eq!(sequential, parallel);
    assert!(!sequential.contains("10.0.0."), "{sequential}");
}

#[test]
fn test_pseudonymize_flag_hides_addresses_in_parse_errors() {
    let output = run_kelora_with_secret(
        &["-f", "json", "--pseudonymize", "ip", "-v"],
        b"not json from 10.0.0.1\n",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not json from"), "{stderr}");
    assert!(!stderr.contains("10.0.0.1"), "{stderr}");
}

#[test]
fn test_pseudonymize_flag_rejects_both_kinds() {
    let output = run_kelora_with_secret(&["--pseudonymize", "ip,ip-prefix"], b"");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn test_pseudonym_ephemeral_warning_obeys_no_warnings() {
    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env_remove("KELORA_SECRET")
        .args(["-f", "line", "--pseudonymize", "ip", "--no-warnings", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(b"10.0.0.1\n").ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success());
    assert!(!stderr.contains("KELORA_SECRET not set"), "{stderr}");
}

#[test]
fn test_pseudonymize_flag_hides_glued_addresses() {
    let out = stdout_of(&run_kelora_with_secret(
        &["-f", "line", "--pseudonymize", "ip"],
        b"user_10.0.0.1 host10.0.0.2 src:2001:db8::1\n",
    ));
    for ip in ["10.0.0.1", "10.0.0.2", "2001:db8::1"] {
        assert!(!out.contains(ip), "{ip} leaked in {out}");
    }
}

fn run_kelora_env(env: &[(&str, &str)], args: &[&str], input: &[u8]) -> std::process::Output {
    let mut command = Command::new(kelora_binary());
    command
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .env_remove("KELORA_SECRET")
        .env_remove("KELORA_SECRET_FILE");
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .args(args)
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input).ok();
            }
            child.wait_with_output()
        })
        .expect("Failed to execute kelora")
}

#[test]
fn test_secret_file_matches_secret_env() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    std::fs::write(&path, "file-secret\n").unwrap();
    let path = path.to_str().unwrap();
    let args = ["-f", "line", "--pseudonymize", "ip"];

    let from_env = run_kelora_env(&[("KELORA_SECRET", "file-secret")], &args, b"10.0.0.1\n");
    let from_file = run_kelora_env(&[("KELORA_SECRET_FILE", path)], &args, b"10.0.0.1\n");
    assert_eq!(stdout_of(&from_env), stdout_of(&from_file));
    assert!(!String::from_utf8_lossy(&from_file.stderr).contains("ephemeral"));

    let both = run_kelora_env(
        &[("KELORA_SECRET", "x"), ("KELORA_SECRET_FILE", path)],
        &args,
        b"10.0.0.1\n",
    );
    assert_eq!(both.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&both.stderr).contains("not both"));

    let empty = dir.path().join("empty");
    std::fs::write(&empty, "\n").unwrap();
    let output = run_kelora_env(
        &[("KELORA_SECRET_FILE", empty.to_str().unwrap())],
        &args,
        b"10.0.0.1\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is empty"));
}

#[test]
fn test_pseudonymize_flag_counts_addresses_in_stats() {
    for extra in [&[][..], &["--parallel"][..]] {
        let args = [
            &["-f", "line", "--pseudonymize", "ip", "--stats", "-q"][..],
            extra,
        ]
        .concat();
        let output = run_kelora_with_secret(&args, b"10.0.0.1 and ::1\nnone\n10.0.0.1\n");
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(report.contains("Addresses pseudonymized: 3"), "{report}");
    }
}
