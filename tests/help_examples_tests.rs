// tests/help_examples_tests.rs
//
// Runs every `kelora ...` command printed by `kelora --help-examples`, so the
// built-in examples cannot drift from the CLI or from the fixtures they name.
// The help text uses bare file names (`api_logs.jsonl`), as users run it from
// examples/; here every argument that names a file in examples/ is rewritten to
// its absolute path and the command runs in a scratch dir (so `--metrics-file`
// output does not land in the repo).
//
// A command passes when it exits 0, writes to stdout (unless it is a --silent
// run), and its stderr carries no error, warning, hint, "Function not found" or
// "likely a field-name typo". The few intentional
// exceptions are listed explicitly below; each entry must still match an
// example, so stale entries fail too.
//
// No shell is involved: each command is split into arguments here and the
// kelora binary is launched directly. Pipelines are handled for the two shapes
// the help uses: `tail -f FILE | kelora ...` (FILE becomes kelora's stdin) and
// `kelora ... | tool` (kelora runs alone; the downstream tool is not run).

mod common;

use common::run_kelora;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Examples that need something the test cannot provide (a remote host).
/// Matched as a substring of the joined command.
const SKIP: &[(&str, &str)] = &[("ssh loghost.example.net", "needs ssh to a remote host")];

/// Examples that are meant to exit non-zero: assertion demos. Matched as a
/// substring of the joined command; the exit code must be exactly 1.
const EXPECT_EXIT_1: &[&str] = &["--assert 'e.has(\"user_id\")'"];

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Collect commands: a line that starts with `kelora ` or pipes into kelora,
/// joined with its `\` continuations and with the lines of a multi-line quoted
/// script.
fn extract_commands(help: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut pending = String::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if pending.is_empty() {
            let starts = trimmed.starts_with("kelora ")
                || (trimmed.contains("| kelora ") && !trimmed.starts_with('#'));
            if !starts {
                continue;
            }
        }
        if in_single_quote(&pending, trimmed) {
            pending.push_str(line);
            pending.push('\n');
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
    assert!(pending.is_empty(), "unterminated example: {pending}");
    commands
}

/// Whether a single quote is still open after `pending` + `line`.
fn in_single_quote(pending: &str, line: &str) -> bool {
    let mut open = false;
    let mut in_double = false;
    for c in pending.chars().chain(line.chars()) {
        match c {
            '\'' if !in_double => open = !open,
            '"' if !open => in_double = !in_double,
            _ => {}
        }
    }
    open
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Pipe,
}

/// Split a command the way sh would for the subset the help uses: words,
/// '...' and "..." quoting, and `|`. Anything else shell-ish is refused.
fn tokenize(cmd: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = cmd.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else { break };
        if first == '|' {
            chars.next();
            tokens.push(Token::Pipe);
            continue;
        }
        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == '|' {
                break;
            }
            chars.next();
            match c {
                '\'' => loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated single quote".into()),
                    }
                },
                '"' => loop {
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
                },
                ';' | '&' | '<' | '>' | '$' | '`' | '(' | ')' | '*' | '?' | '{' | '}' => {
                    return Err(format!("shell syntax `{c}` outside quotes"))
                }
                c => word.push(c),
            }
        }
        tokens.push(Token::Word(word));
    }
    Ok(tokens)
}

/// A runnable kelora invocation: its arguments and an optional stdin file.
#[derive(Debug)]
struct Invocation {
    args: Vec<String>,
    stdin: Option<String>,
}

fn plan(cmd: &str) -> Result<Invocation, String> {
    let tokens = tokenize(cmd)?;
    let mut segments: Vec<Vec<String>> = vec![Vec::new()];
    for token in tokens {
        match token {
            Token::Pipe => segments.push(Vec::new()),
            Token::Word(w) => segments.last_mut().unwrap().push(w),
        }
    }
    let Some(pos) = segments
        .iter()
        .position(|s| s.first().map(String::as_str) == Some("kelora"))
    else {
        return Err("no kelora segment".into());
    };
    if pos > 1 {
        return Err("kelora must be the first or second pipeline stage".into());
    }
    let stdin = if pos == 1 {
        match segments[0].iter().map(String::as_str).collect::<Vec<_>>()[..] {
            ["tail", "-f" | "-F", file] | ["cat", file] => Some(file.to_string()),
            _ => return Err(format!("unsupported producer `{}`", segments[0].join(" "))),
        }
    } else {
        None
    };
    Ok(Invocation {
        args: segments[pos][1..].to_vec(),
        stdin,
    })
}

/// Rewrite an argument naming a fixture in examples/ to its absolute path.
fn resolve(arg: &str) -> String {
    let path = examples_dir().join(arg);
    if !arg.starts_with('-') && !arg.contains('/') && path.is_file() {
        path.to_string_lossy().into_owned()
    } else {
        arg.to_string()
    }
}

/// Lines of stderr that indicate a broken example.
fn bad_stderr_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("error")
                || lower.contains("warning")
                || lower.contains("function not found")
                || lower.contains("field-name typo")
                || line.contains('⚠')
                || line.contains('🔸')
                || line.contains('💡')
        })
        .collect()
}

fn help_examples() -> String {
    let (stdout, stderr, code) = run_kelora(&["--help-examples"]);
    assert_eq!(code, 0, "--help-examples failed: {stderr}");
    stdout
}

#[test]
fn help_examples_extraction_finds_commands() {
    let commands = extract_commands(&help_examples());
    assert!(
        commands.len() >= 80,
        "expected many examples, found {}; did the help layout change?",
        commands.len()
    );
    // Continuations and multi-line scripts are joined into one command.
    assert!(commands.iter().any(
        |c| c.contains("--filter 'e.get_path(\"status\", 0) >= 400'")
            && c.contains("--filter 'e.get_path(\"response_time\", 0.0) > 0.2'")
    ));
    assert!(commands.iter().all(|c| !c.trim_end().ends_with('\\')));
}

#[test]
fn plan_handles_pipes_and_refuses_shell() {
    let inv = plan("tail -f app.jsonl | kelora -j --filter 'e.a | e.b'").unwrap();
    assert_eq!(inv.stdin.as_deref(), Some("app.jsonl"));
    assert_eq!(inv.args, ["-j", "--filter", "e.a | e.b"]);
    let inv = plan("kelora -j x.jsonl --span 1m --span-summary=tsv | duckdb").unwrap();
    assert!(inv.stdin.is_none());
    assert_eq!(inv.args.last().unwrap(), "--span-summary=tsv");
    for bad in [
        "kelora x; rm x",
        "kelora x && rm x",
        "kelora $(rm x)",
        "kelora logs/*.log",
        "kelora x > out",
        "rm x | kelora",
    ] {
        assert!(plan(bad).is_err(), "should refuse: {bad}");
    }
}

#[test]
fn help_examples_run_cleanly() {
    let commands = extract_commands(&help_examples());
    let scratch = tempfile::tempdir().expect("temp dir");
    let kelora = env!("CARGO_BIN_EXE_kelora");

    let mut failures = Vec::new();
    let mut skips_used = vec![false; SKIP.len()];
    let mut expect_used = vec![false; EXPECT_EXIT_1.len()];

    for cmd in &commands {
        if let Some(i) = SKIP.iter().position(|(pat, _)| cmd.contains(pat)) {
            skips_used[i] = true;
            continue;
        }
        let inv = match plan(cmd) {
            Ok(inv) => inv,
            Err(why) => {
                failures.push(format!("  $ {cmd}\n    not runnable: {why}"));
                continue;
            }
        };
        let args: Vec<String> = inv.args.iter().map(|a| resolve(a)).collect();
        let stdin = match &inv.stdin {
            Some(file) => {
                let path = examples_dir().join(file);
                match std::fs::File::open(&path) {
                    Ok(f) => Stdio::from(f),
                    Err(e) => {
                        failures.push(format!("  $ {cmd}\n    stdin {file}: {e}"));
                        continue;
                    }
                }
            }
            None => Stdio::null(),
        };
        let output = Command::new(kelora)
            .args(&args)
            .current_dir(scratch.path())
            .stdin(stdin)
            .env("LLVM_PROFILE_FILE", "/dev/null")
            .env("KELORA_IGNORE_CONFIG", "1")
            // Fixed zone so the local-time examples select the same events everywhere.
            .env("TZ", "UTC")
            .env_remove("KELORA_NO_WARNINGS")
            .env_remove("KELORA_NO_HINTS")
            .output()
            .expect("run kelora");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code();

        let mut problems = Vec::new();
        let expected_fail = EXPECT_EXIT_1.iter().position(|pat| cmd.contains(pat));
        if let Some(i) = expected_fail {
            expect_used[i] = true;
            if code != Some(1) {
                problems.push(format!("expected exit 1 (assert demo), got {code:?}"));
            }
            if !stderr.to_lowercase().contains("assert") {
                problems.push("expected an assertion report on stderr".to_string());
            }
        } else {
            if !output.status.success() {
                problems.push(format!("exit {code:?}"));
            }
            // An example that prints nothing usually filters on a field the
            // fixture lacks; only --silent runs are meant to be quiet.
            if output.stdout.is_empty() && !cmd.contains("--silent") {
                problems.push("no output on stdout".to_string());
            }
            let bad = bad_stderr_lines(&stderr);
            if !bad.is_empty() {
                problems.push(format!("stderr: {}", bad.join("\n            ")));
            }
        }
        if !problems.is_empty() {
            failures.push(format!("  $ {cmd}\n    {}", problems.join("\n    ")));
        }
    }

    for (used, (pat, why)) in skips_used.iter().zip(SKIP) {
        if !used {
            failures.push(format!("  stale SKIP entry `{pat}` ({why})"));
        }
    }
    for (used, pat) in expect_used.iter().zip(EXPECT_EXIT_1) {
        if !used {
            failures.push(format!("  stale EXPECT_EXIT_1 entry `{pat}`"));
        }
    }

    assert!(
        failures.is_empty(),
        "{} --help-examples command(s) failed (see {}):\n{}",
        failures.len(),
        file!(),
        failures.join("\n")
    );
}
