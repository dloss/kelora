//! `--unwrap tap`: read log data out of captured TAP output (Perl's
//! `prove -v`, `perl t/foo.t`).
//!
//! Test code that logs through `note()`/`diag()` leaves its log lines in TAP
//! comments (`# ...`). This module takes the TAP structure off each raw line:
//!
//! - a comment's text is handed to the normal parser (the inner log format);
//! - `ok N` / `not ok N` result lines become `line` events carrying the test
//!   fields, and close the test: every comment and other line buffered since
//!   the previous result is released with that result's fields, since a test's
//!   log output comes *before* its result line;
//! - prove's per-file header (`t/foo.t .. `) sets `test_file` and is dropped,
//!   like the plan (`1..N`), `TAP version N`, blank lines, and Test::More's own
//!   diagnostics (`#   Failed test ...` with its indented details, `# Looks
//!   like you failed ...`);
//! - any other line (prove's summary, stray output, `Bail out!`) becomes a
//!   `line` event.
//!
//! Subtests are not unwrapped: their indented result lines are other lines,
//! and their comments count as comments of the enclosing test.

use crate::pipeline::ChunkLine;
use rhai::Dynamic;
use std::sync::Arc;

/// Fields a TAP result attaches to the events of its test.
pub type TapFields = Arc<Vec<(String, Dynamic)>>;

/// Lines held while waiting for a result line before they are released
/// without test fields. Bounds memory on input that never reaches a result
/// (not TAP, or a test that hangs before reporting).
pub const MAX_PENDING_LINES: usize = 10_000;

/// A parsed `ok N` / `not ok N` line.
#[derive(Debug, Clone, PartialEq)]
pub struct TestResult {
    pub ok: bool,
    pub num: u64,
    pub name: String,
    /// `TODO` or `SKIP`, when the result carries that directive.
    pub directive: Option<&'static str>,
}

/// What one raw line is, once the TAP structure is taken off.
#[derive(Debug, PartialEq)]
pub enum TapLine<'a> {
    /// Text inside a `#` comment: the embedded log line.
    Comment(&'a str),
    Result(TestResult),
    /// prove's per-file header, carrying the test file name.
    Header(&'a str),
    /// Anything else; emitted as a `line` event.
    Other,
    /// TAP structure or Test::More boilerplate with no log content.
    Drop,
}

/// Line classifier. Stateful only to recognise the indented detail lines that
/// follow a `#   Failed test` diagnostic.
#[derive(Debug, Default)]
pub struct TapClassifier {
    in_failure_block: bool,
}

impl TapClassifier {
    pub fn classify<'a>(&mut self, line: &'a str) -> TapLine<'a> {
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix('#') {
            let text = rest.strip_prefix(' ').unwrap_or(rest);
            if self.in_failure_block && text.starts_with([' ', '\t']) {
                return TapLine::Drop;
            }
            self.in_failure_block = false;
            if text.starts_with("  Failed test") || text.starts_with("  Failed (TODO) test") {
                self.in_failure_block = true;
                return TapLine::Drop;
            }
            if text.starts_with("Looks like you")
                || text.starts_with("Subtest: ")
                || text == "No tests run!"
            {
                return TapLine::Drop;
            }
            return TapLine::Comment(text);
        }

        if trimmed.is_empty() {
            return TapLine::Drop;
        }
        self.in_failure_block = false;

        if line.starts_with("TAP version ") || is_plan(line) {
            return TapLine::Drop;
        }
        if let Some(result) = parse_result(line) {
            return TapLine::Result(result);
        }
        if let Some(file) = parse_prove_header(line) {
            return TapLine::Header(file);
        }
        TapLine::Other
    }
}

/// `1..N`, optionally followed by a `# SKIP` reason.
fn is_plan(line: &str) -> bool {
    line.strip_prefix("1..")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Parse `ok N - name # DIRECTIVE reason`. The test number is required:
/// prove prints a bare `ok` as the per-file verdict, and Test::More always
/// numbers its results.
fn parse_result(line: &str) -> Option<TestResult> {
    let (ok, rest) = if let Some(rest) = line.strip_prefix("not ok") {
        (false, rest)
    } else if let Some(rest) = line.strip_prefix("ok") {
        (true, rest)
    } else {
        return None;
    };
    let rest = rest.strip_prefix([' ', '\t'])?.trim_start();
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if digits == 0 {
        return None;
    }
    let num = rest[..digits].parse().ok()?;
    let rest = &rest[digits..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }

    let (desc, directive) = split_directive(rest);
    let desc = desc.trim();
    let desc = desc.strip_prefix('-').map(str::trim_start).unwrap_or(desc);
    Some(TestResult {
        ok,
        num,
        name: desc.replace("\\#", "#"),
        directive,
    })
}

/// Split a result's description from a trailing `# TODO ...` / `# SKIP ...`
/// directive. An escaped `\#` belongs to the description.
fn split_directive(rest: &str) -> (&str, Option<&'static str>) {
    let bytes = rest.as_bytes();
    for (idx, _) in rest.match_indices('#') {
        if idx > 0 && bytes[idx - 1] == b'\\' {
            continue;
        }
        let after = rest[idx + 1..].trim_start();
        let word = after.get(..4).map(str::to_ascii_uppercase);
        let directive = match word.as_deref() {
            Some("TODO") => "TODO",
            Some("SKIP") => "SKIP",
            _ => continue,
        };
        return (&rest[..idx], Some(directive));
    }
    (rest, None)
}

/// prove's per-file header in verbose mode: `t/foo.t .. ` (the dots are padded
/// to align several files). The name must look like a test path, so stray
/// output such as `Loading ...` stays an ordinary line.
fn parse_prove_header(line: &str) -> Option<&str> {
    let line = line.trim_end();
    let (name, dots) = line.split_once(' ')?;
    let looks_like_test = name.ends_with(".t") || name.contains('/');
    (looks_like_test && dots.len() >= 2 && dots.bytes().all(|b| b == b'.')).then_some(name)
}

/// One line on its way to the pipeline.
#[derive(Debug)]
pub enum TapItem {
    /// Comment text, for the inner parser (and the multiline chunker).
    Inner(ChunkLine),
    /// A non-comment line, emitted as a `line` event.
    Raw(ChunkLine),
}

/// Buffered lines released together with the fields of the test they belong
/// to, optionally followed by that test's result line.
#[derive(Debug)]
pub struct TapRelease {
    pub fields: Option<TapFields>,
    pub items: Vec<TapItem>,
    pub result: Option<ChunkLine>,
}

/// Outcome of feeding one raw line.
#[derive(Debug, Default)]
pub struct TapStep {
    pub releases: Vec<TapRelease>,
    /// The line itself was TAP structure or boilerplate.
    pub dropped: bool,
    /// The buffer hit `MAX_PENDING_LINES` and was released without test fields.
    pub overflowed: bool,
}

/// Stateful unwrapper: classifies raw lines and holds each test's lines until
/// its result line arrives.
#[derive(Debug, Default)]
pub struct TapUnwrapper {
    classifier: TapClassifier,
    pending: Vec<TapItem>,
    test_file: Option<String>,
    last_filename: Option<Option<String>>,
}

impl TapUnwrapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: String, line_num: usize, filename: Option<String>) -> TapStep {
        let mut step = TapStep::default();

        // A test never spans input files.
        if self.last_filename.as_ref() != Some(&filename) {
            if self.last_filename.is_some() {
                step.releases.extend(self.finish());
                self.test_file = None;
                self.classifier = TapClassifier::default();
            }
            self.last_filename = Some(filename.clone());
        }

        let chunk_line = |text: String| ChunkLine {
            text,
            line_num,
            filename: filename.clone(),
        };

        match self.classifier.classify(&text) {
            TapLine::Drop => step.dropped = true,
            TapLine::Comment(inner) => {
                let inner = inner.to_string();
                self.pending.push(TapItem::Inner(chunk_line(inner)));
            }
            TapLine::Other => self.pending.push(TapItem::Raw(chunk_line(text))),
            TapLine::Header(file) => {
                let file = file.to_string();
                step.releases.extend(self.finish());
                self.test_file = Some(file);
                step.dropped = true;
            }
            TapLine::Result(result) => {
                let fields = self.result_fields(&result);
                step.releases.push(TapRelease {
                    fields: Some(fields),
                    items: std::mem::take(&mut self.pending),
                    result: Some(chunk_line(text)),
                });
            }
        }

        if self.pending.len() >= MAX_PENDING_LINES {
            step.overflowed = true;
            step.releases.extend(self.finish());
        }
        step
    }

    /// Release lines that no result line followed (end of input, end of a
    /// file). They keep `test_file` but get no test fields.
    pub fn finish(&mut self) -> Option<TapRelease> {
        if self.pending.is_empty() {
            return None;
        }
        let fields = self
            .test_file
            .as_ref()
            .map(|file| Arc::new(vec![("test_file".to_string(), Dynamic::from(file.clone()))]));
        Some(TapRelease {
            fields,
            items: std::mem::take(&mut self.pending),
            result: None,
        })
    }

    fn result_fields(&self, result: &TestResult) -> TapFields {
        let mut fields = Vec::with_capacity(5);
        if let Some(file) = &self.test_file {
            fields.push(("test_file".to_string(), Dynamic::from(file.clone())));
        }
        fields.push(("test_num".to_string(), Dynamic::from(result.num as i64)));
        if !result.name.is_empty() {
            fields.push(("test_name".to_string(), Dynamic::from(result.name.clone())));
        }
        fields.push(("test_ok".to_string(), Dynamic::from(result.ok)));
        if let Some(directive) = result.directive {
            fields.push(("test_directive".to_string(), Dynamic::from(directive)));
        }
        Arc::new(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(line: &str) -> Option<TestResult> {
        parse_result(line)
    }

    #[test]
    fn parses_result_lines() {
        assert_eq!(
            result("ok 1 - server starts"),
            Some(TestResult {
                ok: true,
                num: 1,
                name: "server starts".into(),
                directive: None
            })
        );
        let r = result("not ok 6 - refund restores stock # TODO refunds not implemented").unwrap();
        assert!(!r.ok);
        assert_eq!(r.num, 6);
        assert_eq!(r.name, "refund restores stock");
        assert_eq!(r.directive, Some("TODO"));

        let r = result("ok 7 # skip no database").unwrap();
        assert_eq!(r.name, "");
        assert_eq!(r.directive, Some("SKIP"));

        let r = result("ok 8 issue \\# 12 fixed").unwrap();
        assert_eq!(r.name, "issue # 12 fixed");
        assert_eq!(r.directive, None);

        let r = result("ok 9 - total # computed").unwrap();
        assert_eq!(r.name, "total # computed");
    }

    #[test]
    fn rejects_non_results() {
        assert_eq!(result("ok"), None);
        assert_eq!(result("ok - no number"), None);
        assert_eq!(result("okay 1"), None);
        assert_eq!(result("ok 1x"), None);
        assert_eq!(result("    ok 1 - subtest"), None);
    }

    #[test]
    fn classifies_structure_and_boilerplate() {
        let mut c = TapClassifier::default();
        assert_eq!(c.classify("1..6"), TapLine::Drop);
        assert_eq!(c.classify("TAP version 13"), TapLine::Drop);
        assert_eq!(c.classify(""), TapLine::Drop);
        assert_eq!(
            c.classify("t/checkout.t .. "),
            TapLine::Header("t/checkout.t")
        );
        assert_eq!(c.classify("t/a.t ...... "), TapLine::Header("t/a.t"));
        assert_eq!(c.classify("Result: PASS"), TapLine::Other);
        assert_eq!(c.classify("ok"), TapLine::Other);
        assert_eq!(c.classify("Loading ..."), TapLine::Other);

        assert_eq!(
            c.classify("# level=info msg=hi"),
            TapLine::Comment("level=info msg=hi")
        );
        assert_eq!(c.classify("#no space"), TapLine::Comment("no space"));

        assert_eq!(c.classify("#   Failed test 'x'"), TapLine::Drop);
        assert_eq!(c.classify("#   at t/x.t line 27."), TapLine::Drop);
        assert_eq!(c.classify("#          got: '43.1'"), TapLine::Drop);
        assert_eq!(c.classify("#     expected: '38.79'"), TapLine::Drop);
        assert_eq!(
            c.classify("# rolling back"),
            TapLine::Comment("rolling back")
        );
        // The block ended; indented comments are log content again.
        assert_eq!(
            c.classify("#   at com.example.Foo"),
            TapLine::Comment("  at com.example.Foo")
        );

        assert_eq!(c.classify("#   Failed (TODO) test 'y'"), TapLine::Drop);
        assert_eq!(
            c.classify("# Looks like you failed 2 tests of 6."),
            TapLine::Drop
        );
        assert_eq!(c.classify("# Subtest: inner"), TapLine::Drop);
    }

    fn texts(release: &TapRelease) -> Vec<(&'static str, &str)> {
        release
            .items
            .iter()
            .map(|item| match item {
                TapItem::Inner(l) => ("inner", l.text.as_str()),
                TapItem::Raw(l) => ("raw", l.text.as_str()),
            })
            .collect()
    }

    fn field<'a>(release: &'a TapRelease, key: &str) -> Option<&'a Dynamic> {
        release
            .fields
            .as_ref()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    #[test]
    fn comments_take_the_following_result() {
        let mut tap = TapUnwrapper::new();
        let mut releases = Vec::new();
        for (i, line) in [
            "t/checkout.t .. ",
            "1..2",
            "# first",
            "ok 1 - one",
            "# second",
            "stray output",
            "not ok 2 - two",
            "#   Failed test 'two'",
            "#   at t/checkout.t line 3.",
            "# trailing",
        ]
        .iter()
        .enumerate()
        {
            let step = tap.push(line.to_string(), i + 1, Some("run.t".into()));
            releases.extend(step.releases);
        }
        releases.extend(tap.finish());

        assert_eq!(releases.len(), 3);
        assert_eq!(texts(&releases[0]), vec![("inner", "first")]);
        assert_eq!(
            field(&releases[0], "test_num").unwrap().as_int().unwrap(),
            1
        );
        assert_eq!(
            field(&releases[0], "test_file")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "t/checkout.t"
        );
        assert_eq!(releases[0].result.as_ref().unwrap().line_num, 4);

        assert_eq!(
            texts(&releases[1]),
            vec![("inner", "second"), ("raw", "stray output")]
        );
        assert!(!field(&releases[1], "test_ok").unwrap().as_bool().unwrap());

        assert_eq!(texts(&releases[2]), vec![("inner", "trailing")]);
        assert!(releases[2].result.is_none());
        assert!(field(&releases[2], "test_num").is_none());
        assert!(field(&releases[2], "test_file").is_some());
    }

    #[test]
    fn new_file_releases_and_resets() {
        let mut tap = TapUnwrapper::new();
        assert!(tap
            .push("# a".into(), 1, Some("one.t".into()))
            .releases
            .is_empty());
        let step = tap.push("# b".into(), 1, Some("two.t".into()));
        assert_eq!(step.releases.len(), 1);
        assert_eq!(texts(&step.releases[0]), vec![("inner", "a")]);
        assert!(step.releases[0].fields.is_none());
    }

    #[test]
    fn overflow_releases_without_fields() {
        let mut tap = TapUnwrapper::new();
        let mut overflowed = false;
        for i in 0..MAX_PENDING_LINES {
            let step = tap.push(format!("# line {i}"), i + 1, None);
            overflowed |= step.overflowed;
        }
        assert!(overflowed);
        assert!(tap.finish().is_none());
    }
}
