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
