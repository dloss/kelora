// Signal handling integration tests
//
// These tests verify that kelora properly handles Unix signals (SIGUSR1, SIGTERM, SIGINT)
// and that broken pipe scenarios are handled gracefully.

#![cfg(unix)] // Signal handling is Unix-specific

mod common;

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

/// Get the kelora binary path
fn kelora_binary() -> &'static str {
    // Use CARGO_BIN_EXE_kelora env var set by cargo during test runs
    // This works correctly for regular builds, coverage builds, and custom target dirs
    env!("CARGO_BIN_EXE_kelora")
}

#[test]
fn test_sigusr1_prints_stats_and_continues() {
    // SIGUSR1 should print stats to stderr and continue processing
    // Expected: Process continues, stats appear in stderr, exit code 0
    // Note: Stats must be enabled with -s flag

    let input = r#"{"level":"INFO","message":"line 1"}
{"level":"INFO","message":"line 2"}
{"level":"INFO","message":"line 3"}
{"level":"INFO","message":"line 4"}
{"level":"INFO","message":"line 5"}
"#;

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json", "--with-stats"]) // Enable stats alongside events
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Write input to stdin
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        // Close stdin to signal EOF (but don't drop the child yet)
        drop(child.stdin.take());
    }

    // Give it a moment to start processing
    thread::sleep(Duration::from_millis(100));

    // Send SIGUSR1 to trigger stats printing
    Command::new("kill")
        .args(["-USR1", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGUSR1");

    // Wait for process to complete
    let output = child.wait_with_output().expect("Failed to read output");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // Should complete successfully
    assert_eq!(exit_code, 0, "Should exit with 0 after SIGUSR1");

    // Should have processed all events
    assert_eq!(stdout.lines().count(), 5, "Should output all 5 events");

    // Stats should appear in stderr (triggered by SIGUSR1)
    assert!(
        stderr.contains("Stats:") || stderr.contains("📈 Stats:"),
        "stderr should contain stats after SIGUSR1. stderr:\n{}",
        stderr
    );
}

#[test]
fn test_sigterm_graceful_shutdown() {
    // SIGTERM should trigger graceful shutdown with stats
    // Expected: Stats printed, exit code 143 (128 + 15)

    // Use a slow-ish input with a filter to ensure processing takes time
    let input = (0..100)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Write input
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        drop(child.stdin.take());
    }

    // Give it a moment to start
    thread::sleep(Duration::from_millis(50));

    // Send SIGTERM
    Command::new("kill")
        .args(["-TERM", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGTERM");

    // Wait for process to complete
    let output = child.wait_with_output().expect("Failed to read output");

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // Should exit with SIGTERM code (143 = 128 + 15)
    // OR exit with 0 if it completed gracefully before signal was processed
    assert!(
        exit_code == 143 || exit_code == 0,
        "Should exit with 143 (SIGTERM) or 0 (graceful completion), got {}",
        exit_code
    );

    // Should print SIGTERM message to stderr
    if exit_code == 143 {
        assert!(
            stderr.contains("SIGTERM") || stderr.contains("shutting down"),
            "stderr should mention SIGTERM on signal exit. stderr:\n{}",
            stderr
        );
    }
}

#[test]
fn test_double_sigterm_immediate_exit() {
    // Sending SIGTERM twice should cause immediate exit
    // Expected: Exit code 143, process terminates quickly

    // Create a long-running input
    let input = (0..10000)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Write input
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        drop(child.stdin.take());
    }

    // Give it a moment to start
    thread::sleep(Duration::from_millis(50));

    // Send first SIGTERM
    Command::new("kill")
        .args(["-TERM", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGTERM");

    // Immediately send second SIGTERM
    thread::sleep(Duration::from_millis(10));
    Command::new("kill")
        .args(["-TERM", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGTERM");

    // Wait for process - should exit quickly
    let output = child.wait_with_output().expect("Failed to read output");
    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should exit with SIGTERM code (143)
    assert_eq!(
        exit_code, 143,
        "Should exit with 143 (SIGTERM) on graceful SIGTERM shutdown. stderr: {}",
        stderr
    );

    // Verify that SIGTERM was actually received (message in stderr confirms it)
    assert!(
        stderr.contains("SIGTERM"),
        "stderr should mention SIGTERM to confirm correct signal was received"
    );
}

#[test]
fn test_sigint_graceful_shutdown() {
    // SIGINT (Ctrl-C) should trigger graceful shutdown
    // Expected: Exit code 130 (128 + 2) or 0 if graceful

    let input = (0..100)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Write input
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        drop(child.stdin.take());
    }

    // Give it a moment to start
    thread::sleep(Duration::from_millis(50));

    // Send SIGINT
    Command::new("kill")
        .args(["-INT", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGINT");

    // Wait for process
    let output = child.wait_with_output().expect("Failed to read output");
    let exit_code = output.status.code().unwrap_or(-1);

    // Should exit with SIGINT code (130 = 128 + 2) or 0 if graceful
    assert!(
        exit_code == 130 || exit_code == 0,
        "Should exit with 130 (SIGINT) or 0 (graceful completion), got {}",
        exit_code
    );
}

#[test]
fn test_double_sigint_immediate_exit() {
    // Sending SIGINT twice should cause immediate exit with code 130

    let input = (0..10000)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Write input
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        drop(child.stdin.take());
    }

    // Give it a moment to start
    thread::sleep(Duration::from_millis(50));

    // Send first SIGINT
    Command::new("kill")
        .args(["-INT", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGINT");

    // Immediately send second SIGINT
    thread::sleep(Duration::from_millis(10));
    Command::new("kill")
        .args(["-INT", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGINT");

    // Wait for process
    let output = child.wait_with_output().expect("Failed to read output");
    let exit_code = output.status.code().unwrap_or(-1);

    // Should exit with SIGINT code
    assert_eq!(exit_code, 130, "Should exit with 130 after double SIGINT");
}

#[test]
fn test_signal_during_file_processing() {
    // Test that signals work correctly when processing files (not stdin)
    // Expected: SIGTERM during file processing should exit gracefully

    // Create a large temp file
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let content = (0..1000)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");
    temp_file
        .write_all(content.as_bytes())
        .expect("Failed to write temp file");

    let child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json", temp_file.path().to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Give it a moment to start processing
    thread::sleep(Duration::from_millis(50));

    // Send SIGTERM
    Command::new("kill")
        .args(["-TERM", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGTERM");

    // Wait for process
    let output = child.wait_with_output().expect("Failed to read output");
    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should exit with SIGTERM code (143) or 0 if processing completed before signal
    assert!(
        exit_code == 143 || exit_code == 0,
        "Should exit with 143 (SIGTERM) or 0 (completed). Got {}. stderr: {}",
        exit_code,
        stderr
    );
}

#[test]
fn test_sigint_flushes_metrics() {
    // Aggregation on an unbounded stream: `tail -f … --metrics` accumulates
    // track_* data and only emits it at end of input. On a never-ending stream
    // that end arrives via Ctrl-C, so a single SIGINT must flush the metrics
    // table rather than dropping it (regression for the !SHOULD_TERMINATE gate).
    //
    // We keep stdin open with a slow producer thread so the process is still
    // running when the signal lands, then assert the tracked counts appear.

    let mut child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args([
            "-f",
            "json",
            "--exec",
            r#"track_freq("level", e.level)"#,
            "--metrics",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let child_pid = child.id();

    // Feed events slowly from a background thread, leaving stdin open (mimics a
    // live stream). The thread is detached; the process is killed via SIGINT.
    let mut stdin = child.stdin.take().expect("stdin");
    let writer = thread::spawn(move || {
        for _ in 0..200 {
            if stdin
                .write_all(b"{\"level\":\"INFO\"}\n{\"level\":\"ERROR\"}\n")
                .is_err()
            {
                break;
            }
            let _ = stdin.flush();
            thread::sleep(Duration::from_millis(20));
        }
    });

    // Let it process a few events, then interrupt with a single SIGINT.
    thread::sleep(Duration::from_millis(300));
    Command::new("kill")
        .args(["-INT", &child_pid.to_string()])
        .output()
        .expect("Failed to send SIGINT");

    let output = child.wait_with_output().expect("Failed to read output");
    let _ = writer.join();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // Graceful SIGINT shutdown exits 130.
    assert_eq!(
        exit_code, 130,
        "Should exit with 130 (SIGINT). stdout:\n{}",
        stdout
    );

    // The accumulated metrics must reach stdout. In data-only --metrics mode
    // with a redirected (non-terminal) stdout, the table is emitted as TSV
    // rows: `level\t<value>\t<count>`.
    assert!(
        stdout.contains("level\tINFO\t") || stdout.contains("level\tERROR\t"),
        "metrics should be flushed on SIGINT, got stdout:\n{}",
        stdout
    );
}

#[test]
fn test_broken_pipe_exit_code() {
    // Test that broken pipe results in exit code 141 (128 + 13)
    // We simulate this by piping to `head -n 1` which closes the pipe early

    let input = (0..1000)
        .map(|i| format!(r#"{{"level":"INFO","message":"line {}"}}"#, i))
        .collect::<Vec<_>>()
        .join("\n");

    // Create a kelora process piped to `head -n 1`
    // head will close its stdin after reading 1 line, causing SIGPIPE
    let mut kelora_child = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn kelora");

    let head_child = Command::new("head")
        .args(["-n", "1"])
        .stdin(kelora_child.stdout.take().unwrap())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn head");

    // Write input to kelora's stdin
    if let Some(stdin) = kelora_child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
        drop(kelora_child.stdin.take());
    }

    // Wait for head to complete (it should exit after 1 line)
    let head_output = head_child
        .wait_with_output()
        .expect("Failed to wait for head");
    let head_stdout = String::from_utf8_lossy(&head_output.stdout);
    assert_eq!(head_stdout.lines().count(), 1);

    // Now wait for kelora - it should have received SIGPIPE and exited with 141
    let kelora_output = kelora_child
        .wait_with_output()
        .expect("Failed to wait for kelora");
    let exit_code = kelora_output.status.code().unwrap_or(-1);

    // Should exit with SIGPIPE code (141 = 128 + 13)
    assert_eq!(
        exit_code, 141,
        "Should exit with 141 (SIGPIPE) when pipe is broken, got {}",
        exit_code
    );
}

#[test]
fn test_stats_printed_on_normal_exit() {
    // Verify that stats are printed on normal exit (no signal)
    // This is a baseline to compare with signal-triggered stats
    // Note: Stats must be enabled with -s flag

    let input = r#"{"level":"INFO","message":"line 1"}
{"level":"INFO","message":"line 2"}
{"level":"INFO","message":"line 3"}
"#;

    let output = Command::new(kelora_binary())
        .env("LLVM_PROFILE_FILE", "/dev/null") // Disable profraw generation for subprocesses
        .args(["-f", "json", "--with-stats"]) // Enable stats alongside events
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input.as_bytes())?;
                drop(child.stdin.take());
            }
            child.wait_with_output()
        })
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert_eq!(stdout.lines().count(), 3, "Should output 3 events");
    assert!(
        stderr.contains("Stats:") || stderr.contains("📈 Stats:"),
        "Stats should be printed to stderr on normal exit"
    );
}
