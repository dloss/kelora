//! Keep the hand-written function reference (docs/reference/functions.md) in
//! sync with the built-in catalogue (`kelora --help-functions`, from
//! src/rhai_functions/docs.rs). Adding a function means documenting it in both.

mod common;
use common::*;
use regex::Regex;
use std::collections::BTreeSet;

/// The signature column of a `--help-functions` line: everything before the
/// description, which starts after two spaces or after `) ` + a capital letter.
fn signature_part(line: &str) -> &str {
    let two_spaces = line.find("  ").unwrap_or(line.len());
    let desc_start = Regex::new(r"\) [A-Z]")
        .unwrap()
        .find(line)
        .map(|m| m.start() + 1)
        .unwrap_or(line.len());
    &line[..two_spaces.min(desc_start)]
}

fn names_in(text: &str) -> BTreeSet<String> {
    Regex::new(r"(?:^|[\s,/.`(])([a-z_][a-z0-9_]*)\(")
        .unwrap()
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .collect()
}

fn help_function_names() -> BTreeSet<String> {
    let (stdout, _stderr, code) = run_kelora(&["--help-functions"]);
    assert_eq!(code, 0);
    stdout
        .lines()
        .filter(|line| line.starts_with(|c: char| c.is_ascii_lowercase()))
        .flat_map(|line| names_in(signature_part(line)))
        .collect()
}

fn doc_function_names() -> BTreeSet<String> {
    let doc = std::fs::read_to_string("docs/reference/functions.md")
        .expect("docs/reference/functions.md should exist");
    doc.lines()
        .filter(|line| line.starts_with("#### "))
        .flat_map(names_in)
        .collect()
}

#[test]
fn every_builtin_function_is_documented() {
    let documented = doc_function_names();
    let missing: Vec<_> = help_function_names()
        .into_iter()
        .filter(|name| !documented.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "functions listed by --help-functions but without a `####` entry in \
         docs/reference/functions.md: {missing:?}"
    );
}

#[test]
fn every_documented_function_exists() {
    let builtin = help_function_names();
    let stale: Vec<_> = doc_function_names()
        .into_iter()
        .filter(|name| !builtin.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "functions documented in docs/reference/functions.md but not listed by \
         --help-functions (renamed or removed?): {stale:?}"
    );
}
