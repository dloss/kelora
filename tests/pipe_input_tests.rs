//! Inputs that can only be read once: named FIFOs and `<(...)` process
//! substitution. Format detection reads their head, so processing must reuse
//! the detection reader instead of reopening the path.
#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const JSON: &str = "{\"msg\":\"a\"}\n{\"msg\":\"b\"}\n";

/// Run kelora, killing it if it hangs (reopening a drained FIFO blocks).
fn run_with_timeout(args: &[&str]) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kelora"))
        .args(args)
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .env("KELORA_IGNORE_CONFIG", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kelora");
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().expect("wait").is_none() {
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("kelora hung on {args:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("output");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn make_fifo(dir: &tempfile::TempDir, name: &str, content: &'static str) -> String {
    let path = dir.path().join(name);
    let status = Command::new("mkfifo").arg(&path).status().expect("mkfifo");
    assert!(status.success());
    let writer_path = path.clone();
    std::thread::spawn(move || {
        if let Ok(mut fifo) = std::fs::OpenOptions::new().write(true).open(writer_path) {
            let _ = fifo.write_all(content.as_bytes());
        }
    });
    path.to_str().unwrap().to_string()
}

#[test]
fn auto_detection_reuses_a_fifo() {
    let dir = tempfile::TempDir::new().unwrap();
    let fifo = make_fifo(&dir, "in.fifo", JSON);
    let (stdout, code) = run_with_timeout(&["-k", "msg", &fifo]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "msg='a'\nmsg='b'\n");
}

#[test]
fn merge_sorted_reuses_a_fifo() {
    let dir = tempfile::TempDir::new().unwrap();
    let fifo = make_fifo(
        &dir,
        "in.fifo",
        "{\"ts\":\"2026-01-01T00:00:01Z\",\"msg\":\"a\"}\n",
    );
    let (stdout, code) = run_with_timeout(&["--merge-sorted", "-k", "msg", &fifo]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "msg='a'\n");
}

#[test]
fn process_substitution_inputs_are_read() {
    let kelora = env!("CARGO_BIN_EXE_kelora");
    let script = format!(
        "KELORA_IGNORE_CONFIG=1 '{kelora}' -k msg <(printf '{{\"msg\":\"a\"}}\\n') <(printf '{{\"msg\":\"b\"}}\\n')"
    );
    let output = Command::new("bash")
        .args(["-c", &script])
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .output()
        .expect("bash");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "msg='a'\nmsg='b'\n"
    );
}
