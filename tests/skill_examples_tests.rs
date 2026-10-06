// tests/skill_examples_tests.rs
//
// Runs every command in the ```bash blocks of skills/log-analysis/SKILL.md, so the
// agent skill cannot drift from the CLI unnoticed. The skill uses generic file
// names (app.log, api.log, ...); each one is backed by a small fixture written
// into a temp dir below. A command that names a file with no fixture fails to
// open it, so adding an example means adding its fixture here.
//
// A command passes when it exits 0, prints nothing on stderr (no error, warning,
// or hint — a hint on a skill example usually means a wrong field name), and
// writes something to stdout unless it redirects stdout itself.

use chrono::{Duration, Local, Utc};
use std::fs;
use std::path::Path;
use std::process::Command;

const SKILL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/skills/log-analysis/SKILL.md");

/// Collect shell commands from the ```bash fences, joining `\` continuations.
fn bash_commands(markdown: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_bash = false;
    let mut pending = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_bash = !in_bash && trimmed == "```bash";
            continue;
        }
        if !in_bash || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(head) = trimmed.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        commands.push(std::mem::take(&mut pending));
    }
    commands
}

/// JSON app log with a level, message, latency and nested user. Timestamps sit in
/// the last hour (for `--since 1h`) and at 10:05 today in both local time and UTC
/// (for `--since 10:00`), sorted so window-based examples see ordered input.
fn app_log() -> String {
    let now = Utc::now();
    let ten_local = Local::now()
        .date_naive()
        .and_hms_opt(10, 5, 0)
        .unwrap()
        .and_local_timezone(Local)
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    let ten_utc = now.date_naive().and_hms_opt(10, 5, 0).unwrap().and_utc();
    let mut times = vec![ten_local, ten_utc];
    times.extend((0..10).map(|i| now - Duration::minutes(50 - i * 5)));
    times.sort();

    let rows = [
        ("INFO", "user 101 logged in", 12),
        ("INFO", "user 102 logged in", 15),
        ("WARN", "slow query on orders took 820 ms", 820),
        ("ERROR", "upstream payments returned 503", 1500),
        ("INFO", "user 103 logged in", 11),
        ("DEBUG", "cache hit for key session:42", 1),
        ("ERROR", "upstream payments returned 504", 3000),
        ("INFO", "user 104 logged in", 14),
        ("WARN", "slow query on users took 640 ms", 640),
        ("INFO", "user 105 logged in", 13),
        ("INFO", "user 106 logged in", 12),
        ("ERROR", "upstream payments returned 503", 1700),
    ];
    times
        .iter()
        .zip(rows.iter().cycle())
        .enumerate()
        .map(|(i, (ts, (level, msg, ms)))| {
            format!(
                r#"{{"timestamp":"{}","level":"{}","msg":"{}","duration_ms":{},"user":{{"id":{}}}}}"#,
                ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                level,
                msg,
                ms,
                100 + i % 4
            ) + "\n"
        })
        .collect()
}

fn write_fixtures(dir: &Path) {
    let files: [(&str, String); 7] = [
        ("app.log", app_log()),
        (
            "api.log",
            [
                r#"{"ts":"2024-01-15T10:00:00Z","path":"/api/users","status":200,"duration_ms":12}"#,
                r#"{"ts":"2024-01-15T10:00:01Z","path":"/api/orders","status":502,"duration_ms":3000}"#,
                r#"{"ts":"2024-01-15T10:00:02Z","path":"/api/orders","status":201,"duration_ms":45}"#,
            ]
            .join("\n"),
        ),
        (
            "events.log",
            [
                r#"{"ts":"2024-01-15T10:00:00Z","msg":"checkout","data":"{\"cart\":3,\"total\":42.5}"}"#,
                r#"{"ts":"2024-01-15T10:00:01Z","msg":"checkout","data":"{\"cart\":1,\"total\":9.9}"}"#,
            ]
            .join("\n"),
        ),
        (
            "events.jsonl",
            [
                r#"{"ts":"2024-01-15T10:00:00Z","level":"INFO","msg":"started"}"#,
                r#"{"ts":"2024-01-15T10:00:01Z","level":"WARN","msg":"retrying"}"#,
            ]
            .join("\n"),
        ),
        (
            "access.log",
            [
                r#"192.168.1.10 - alice [15/Jan/2024:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1024 "-" "curl/8.0""#,
                r#"192.168.1.11 - - [15/Jan/2024:10:00:01 +0000] "POST /api/login HTTP/1.1" 401 128 "-" "Mozilla/5.0""#,
            ]
            .join("\n"),
        ),
        (
            "syslog.log",
            [
                "<34>Jan 15 10:00:00 web01 sshd[4721]: Failed password for root from 10.0.0.5",
                "<30>Jan 15 10:00:02 web01 systemd[1]: Started nginx.service",
            ]
            .join("\n"),
        ),
        (
            "mixed.log",
            [
                r#"{"ts":"2024-01-15T10:00:00Z","level":"INFO","msg":"json line"}"#,
                "plain text banner line",
                r#"{"ts":"2024-01-15T10:00:01Z","level":"ERROR","msg":"another json line"}"#,
            ]
            .join("\n"),
        ),
    ];
    for (name, content) in files {
        fs::write(dir.join(name), content + "\n").expect("write fixture");
    }
}

#[test]
fn skill_md_has_bash_examples() {
    let skill = fs::read_to_string(SKILL_PATH).expect("read SKILL.md");
    assert!(
        bash_commands(&skill).len() >= 10,
        "expected the skill to carry runnable examples; did the ```bash fences change?"
    );
}

#[test]
fn skill_md_bash_examples_run_cleanly() {
    let skill = fs::read_to_string(SKILL_PATH).expect("read SKILL.md");
    let dir = tempfile::tempdir().expect("temp dir");
    write_fixtures(dir.path());

    let kelora = env!("CARGO_BIN_EXE_kelora");
    let mut failures = Vec::new();
    for cmd in bash_commands(&skill) {
        // `kelora` in the skill resolves to the binary under test, regardless of PATH.
        let script = format!("kelora() {{ \"$KELORA_BIN\" \"$@\"; }}\n{cmd}");
        let output = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(dir.path())
            .env("KELORA_BIN", kelora)
            .env("LLVM_PROFILE_FILE", "/dev/null")
            .env_remove("KELORA_NO_WARNINGS")
            .env_remove("KELORA_NO_HINTS")
            .output()
            .expect("run sh");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let redirects_stdout = cmd.contains(" > ");
        let mut problems = Vec::new();
        if !output.status.success() {
            problems.push(format!("exit {:?}", output.status.code()));
        }
        if !stderr.trim().is_empty() {
            problems.push(format!("stderr: {}", stderr.trim()));
        }
        if !redirects_stdout && stdout.trim().is_empty() {
            problems.push("no output on stdout".to_string());
        }
        if !problems.is_empty() {
            failures.push(format!("  $ {cmd}\n    {}", problems.join("\n    ")));
        }
    }

    assert!(
        failures.is_empty(),
        "SKILL.md examples failed against the fixtures in {}:\n{}",
        file!(),
        failures.join("\n")
    );
}
