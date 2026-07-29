// Integration tests for --drain's default masking (see issue #337): the CLI
// reproductions, end to end.

mod common;
use common::run_kelora_with_input;

fn drain_templates(input: &str) -> String {
    let (stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "cols:*msg", "--drain", "-k", "msg"], input);
    assert_eq!(exit_code, 0, "drain run failed: {}", stderr);
    stdout
}

#[test]
fn test_ctime_dates_do_not_split_one_message_into_several_templates() {
    // Three renderings of the same ftpd message. Before ctime/asctime dates
    // were masked, the weekday and month stayed literal, so this produced two
    // templates -- one of them labeled "Mon Jun" while covering a line that
    // said "Sun Jul".
    let input = "connection from 10.0.0.1 () at Mon Jun 13 03:55:15 2005\n\
                 connection from 10.0.0.2 () at Fri Jul  1 04:11:02 2005\n\
                 connection from 10.0.0.3 () at Sun Jul 10 05:00:00 2005\n";

    let stdout = drain_templates(input);
    assert!(
        stdout.contains("templates (1 items):"),
        "expected one template, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("3: connection from <ipv4> () at <timestamp>"),
        "expected a single <timestamp> placeholder, got:\n{}",
        stdout
    );
    for leaked in ["Mon", "Jun", "Fri", "Jul", "Sun"] {
        assert!(
            !stdout.contains(leaked),
            "weekday/month name {} leaked into the template:\n{}",
            leaked,
            stdout
        );
    }
}

#[test]
fn test_a_digit_inside_a_word_is_not_a_number() {
    // `ssh2` is a protocol name, not a number, but a pattern matching anywhere
    // in a token used to replace the whole token: this template read
    // "... port <num> <num>", with the word that names the message gone.
    let input = "Accepted publickey for root from 10.0.0.1 port 22 ssh2\n\
                 Accepted publickey for root from 10.0.0.2 port 4022 ssh2\n";

    let stdout = drain_templates(input);
    assert!(
        stdout.contains("2: Accepted publickey for root from <ipv4> port <num> ssh2"),
        "expected ssh2 to survive masking, got:\n{}",
        stdout
    );
}

#[test]
fn test_the_literal_part_of_a_token_survives_masking() {
    // Whole-token masking rendered these as "<num> ... <num> <version> <num>
    // <num>": the path, the query key, the protocol and the worker all gone.
    let input = "worker-3 GET /api/v1/users?id=5 HTTP/1.1 200 1234\n\
                 worker-11 GET /api/v1/users?id=97 HTTP/1.1 200 8899\n";

    let stdout = drain_templates(input);
    assert!(
        stdout.contains("2: worker-<num> GET <path>?id=<num> HTTP/<version> <num> <num>"),
        "expected the literal parts to survive, got:\n{}",
        stdout
    );
}

#[test]
fn test_key_value_masking_keeps_the_key() {
    // uid=0 used to mask to <num>, key included, so the template no longer
    // recorded which number was which -- while tty=ssh kept its key because
    // nothing matched its value.
    let input = "auth failure; logname= uid=0 euid=0 tty=ssh\n\
                 auth failure; logname= uid=500 euid=501 tty=ssh\n";

    let stdout = drain_templates(input);
    assert!(
        stdout.contains("2: auth failure; logname= uid=<num> euid=<num> tty=ssh"),
        "expected value-only masking, got:\n{}",
        stdout
    );
}

#[test]
fn test_key_value_masking_survives_placeholder_names_with_digits() {
    // `<ipv4>` contains a digit, so a second masking pass would collapse
    // `rhost=<ipv4>` to `<num>` and lose the key again.
    let input = "authentication failure; rhost=218.188.2.4 user=root\n\
                 authentication failure; rhost=10.0.0.7 user=root\n";

    let stdout = drain_templates(input);
    assert!(
        stdout.contains("2: authentication failure; rhost=<ipv4> user=root"),
        "expected rhost=<ipv4>, got:\n{}",
        stdout
    );
}
