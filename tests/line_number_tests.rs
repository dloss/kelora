//! `meta.line_num` and `file:line` in messages are the line within the event's
//! own file, in every reading mode.
mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn file(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f
}

const A: &str = "{\"ts\":\"2026-01-01T00:00:01Z\",\"m\":\"a1\"}\n{\"ts\":\"2026-01-01T00:00:03Z\",\"m\":\"a2\"}\n";
const B: &str = "{\"ts\":\"2026-01-01T00:00:02Z\",\"m\":\"b1\"}\n\n{\"ts\":\"2026-01-01T00:00:04Z\",\"m\":\"b2\"}\n";

fn numbers(extra: &[&str]) -> String {
    let (a, b) = (file(A), file(B));
    let mut args = vec!["-j", "-e", "e.n = meta.line_num", "-k", "m,n"];
    args.extend_from_slice(extra);
    let (stdout, stderr, code) = run_kelora_with_files(
        &args,
        &[a.path().to_str().unwrap(), b.path().to_str().unwrap()],
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    stdout.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn numbering_restarts_in_each_file() {
    let expected = "m='a1' n=1 m='a2' n=2 m='b1' n=1 m='b2' n=3";
    assert_eq!(numbers(&[]), expected);
    assert_eq!(numbers(&["--parallel", "--batch-size", "1"]), expected);
    assert_eq!(numbers(&["--parallel"]), expected);
}

#[test]
fn merge_sorted_numbers_lines_within_their_file() {
    assert_eq!(
        numbers(&["--merge-sorted"]),
        "m='a1' n=1 m='b1' n=1 m='a2' n=2 m='b2' n=3"
    );
}

#[test]
fn parse_errors_name_the_line_in_its_file() {
    let a = file(A);
    let b = file("{\"m\":\"b1\"}\nnot json\n");
    for extra in [&[][..], &["--parallel"][..]] {
        let mut args = vec!["-j"];
        args.extend_from_slice(extra);
        let (_stdout, stderr, _code) = run_kelora_with_files(
            &args,
            &[a.path().to_str().unwrap(), b.path().to_str().unwrap()],
        );
        let expected = format!("{}:2:", b.path().display());
        assert!(stderr.contains(&expected), "{extra:?}: {stderr}");
    }
}

#[test]
fn parallel_keeps_numbers_of_dropped_lines() {
    let input: String = (1..=6).map(|i| format!("{{\"m\":\"x{i}\"}}\n")).collect();
    for extra in [&[][..], &["--parallel"][..]] {
        let mut args = vec![
            "-j",
            "--keep-lines",
            "x[246]",
            "-e",
            "e.n = meta.line_num",
            "-k",
            "n",
        ];
        args.extend_from_slice(extra);
        let (stdout, _stderr, code) = run_kelora_with_file(&args, &input);
        assert_eq!(code, 0);
        assert_eq!(stdout, "n=2\nn=4\nn=6\n", "{extra:?}");
    }
}

#[test]
fn drain_full_names_files_when_there_are_several() {
    let a = file("{\"msg\":\"request took 15 ms\"}\n");
    let b = file("{\"msg\":\"other\"}\n{\"msg\":\"request took 230 ms\"}\n");
    let (stdout, _stderr, code) = run_kelora_with_files(
        &["-j", "--drain=full", "-k", "msg"],
        &[a.path().to_str().unwrap(), b.path().to_str().unwrap()],
    );
    assert_eq!(code, 0);
    let expected = format!("lines: {}:1 - {}:2", a.path().display(), b.path().display());
    assert!(stdout.contains(&expected), "{stdout}");

    let (stdout, _stderr, _code) = run_kelora_with_file(
        &["-j", "--drain=full", "-k", "msg"],
        "{\"msg\":\"request took 15 ms\"}\n{\"msg\":\"request took 230 ms\"}\n",
    );
    assert!(stdout.contains("lines: 1-2"), "{stdout}");
}
