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
// writes something to stdout.
//
// No shell is involved: each line is split into arguments here and the kelora
// binary is launched directly, so nothing but kelora can run. Lines must start
// with `kelora`; pipes, `;`, `&&` and the like are rejected rather than executed.
// A trailing `> file` redirect is dropped (stdout is captured either way).

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

/// Split one example line into kelora's arguments, the way sh would for the
/// subset the skill uses: words, '...' and "..." quoting, `#` comments, and an
/// optional trailing `> file`.
fn parse_command(line: &str) -> Result<Vec<String>, String> {
    // Each word with whether any part of it was quoted (a quoted `>` is data).
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else { break };
        if first == '#' {
            break;
        }
        let mut word = String::new();
        let mut quoted = false;
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            chars.next();
            match c {
                '\'' => {
                    quoted = true;
                    loop {
                        match chars.next() {
                            Some('\'') => break,
                            Some(c) => word.push(c),
                            None => return Err("unterminated single quote".into()),
                        }
                    }
                }
                '"' => {
                    quoted = true;
                    loop {
                        match chars.next() {
                            Some('"') => break,
                            Some('\\') => match chars.next() {
                                Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                                Some(c) => {
                                    word.push('\\');
                                    word.push(c);
                                }
                                None => return Err("dangling backslash".into()),
                            },
                            Some(c @ ('$' | '`')) => {
                                return Err(format!("shell expansion `{c}` in double quotes"))
                            }
                            Some(c) => word.push(c),
                            None => return Err("unterminated double quote".into()),
                        }
                    }
                }
                '|' | ';' | '&' | '<' | '$' | '`' | '(' | ')' | '*' | '?' => {
                    return Err(format!("shell syntax `{c}` outside quotes"))
                }
                c => word.push(c),
            }
        }
        words.push((word, quoted));
    }

    if let Some(pos) = words.iter().position(|(w, q)| !q && w == ">") {
        if pos + 2 != words.len() {
            return Err("`>` must be followed by exactly one file name at the end".into());
        }
        words.truncate(pos);
    }
    if words.iter().any(|(w, q)| !q && w.contains('>')) {
        return Err("redirection other than a trailing `> file`".into());
    }
    match words.first() {
        Some((w, false)) if w == "kelora" => {
            Ok(words.into_iter().skip(1).map(|(w, _)| w).collect())
        }
        _ => Err("only `kelora ...` lines can be run".into()),
    }
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

/// Plain-text Java service output with one multi-frame trace (`-M java`).
fn java_trace_log() -> String {
    [
        "Starting OrderService v2.3.1",
        "Failed to process order batch 7712",
        "org.springframework.dao.DataAccessResourceFailureException: could not execute statement",
        "\tat com.example.orders.BatchWriter.flush(BatchWriter.java:88)",
        "Caused by: java.sql.SQLTransientConnectionException: Connection is not available",
        "\tat com.zaxxer.hikari.pool.HikariPool.getConnection(HikariPool.java:181)",
        "\t... 4 more",
        "Batch 7712 processed on retry",
    ]
    .join("\n")
}

/// Plain-text Python output with two tracebacks (`-M python`).
fn python_trace_log() -> String {
    let trace = |n: u32| {
        format!(
            "Traceback (most recent call last):\n  File \"worker.py\", line {n}, in run\n    \
             handle(job)\nKeyError: 'job_{n}'"
        )
    };
    [
        "worker started".to_string(),
        trace(10),
        trace(20),
        "worker stopped".to_string(),
    ]
    .join("\n")
}

/// One minute of JSON service events; `after` adds a new failure template and
/// drops the pool-recycle one, so `--drain-diff` has `+`, `-` and `*` rows.
fn deploy_events(minute_prefix: &str, after: bool) -> String {
    (0..30)
        .map(|i| {
            let msg = match (i % 3, after) {
                (0, false) => format!("connection pool recycled for db{}.internal", i % 4),
                (0, true) => format!("worker {i} restarted after heartbeat timeout 30s"),
                (1, true) if i % 2 == 1 => {
                    format!("upstream auth.internal returned 503 for request r-{i}")
                }
                _ => format!("request r-{i} served in {} ms", 10 + i),
            };
            format!(
                r#"{{"ts":"{minute_prefix}{:02}:{:02}Z","level":"INFO","msg":"{msg}"}}"#,
                if after { 1 } else { 0 },
                i
            ) + "\n"
        })
        .collect()
}

fn write_fixtures(dir: &Path) {
    let files: [(&str, String); 12] = [
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
        ("trace.log", java_trace_log()),
        ("app_py.log", python_trace_log()),
        ("before.log", deploy_events("2024-01-15T09:", false)),
        ("after.log", deploy_events("2024-01-15T10:", true)),
        (
            "deploy.log",
            deploy_events("2024-01-15T09:", false)
                + r#"{"ts":"2024-01-15T10:00:30Z","level":"INFO","msg":"deploy v2.4.0 started"}"#
                + "\n"
                + &deploy_events("2024-01-15T10:", true),
        ),
    ];
    for (name, content) in files {
        // Exactly one trailing newline: a blank last line is not part of any example.
        fs::write(dir.join(name), content.trim_end().to_string() + "\n").expect("write fixture");
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
fn parse_command_runs_only_kelora() {
    assert_eq!(
        parse_command(r#"kelora --filter 'e.n > 1 && e.s == "a|b"' app.log # note"#).unwrap(),
        ["--filter", r#"e.n > 1 && e.s == "a|b""#, "app.log"]
    );
    assert_eq!(
        parse_command("kelora -F json access.log > out.jsonl").unwrap(),
        ["-F", "json", "access.log"]
    );
    for bad in [
        "rm -rf app.log",
        "kelora app.log; rm app.log",
        "kelora app.log && rm app.log",
        "kelora app.log | sh",
        "kelora $(rm app.log)",
        "kelora \"$(rm app.log)\"",
        "kelora app.log > a > b",
        "'kelora' app.log",
    ] {
        assert!(parse_command(bad).is_err(), "should refuse: {bad}");
    }
}

#[test]
fn skill_md_bash_examples_run_cleanly() {
    let skill = fs::read_to_string(SKILL_PATH).expect("read SKILL.md");
    let dir = tempfile::tempdir().expect("temp dir");
    write_fixtures(dir.path());

    let kelora = env!("CARGO_BIN_EXE_kelora");
    let mut failures = Vec::new();
    for cmd in bash_commands(&skill) {
        let args = match parse_command(&cmd) {
            Ok(parsed) => parsed,
            Err(why) => {
                failures.push(format!("  $ {cmd}\n    not runnable: {why}"));
                continue;
            }
        };
        let output = Command::new(kelora)
            .args(&args)
            .current_dir(dir.path())
            .env("LLVM_PROFILE_FILE", "/dev/null")
            .env_remove("KELORA_NO_WARNINGS")
            .env_remove("KELORA_NO_HINTS")
            .output()
            .expect("run kelora");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut problems = Vec::new();
        if !output.status.success() {
            problems.push(format!("exit {:?}", output.status.code()));
        }
        if !stderr.trim().is_empty() {
            problems.push(format!("stderr: {}", stderr.trim()));
        }
        if stdout.trim().is_empty() {
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
