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
//! - a TAP 13 YAML block after a result (`  ---` ... `  ...`, as node's test
//!   runner writes) is dropped; its `duration_ms` becomes `test_duration_ms`;
//! - prove's per-file header (`t/foo.t .. `, or `t/foo.t .. ok` without `-v`)
//!   sets `test_file`. With `-v` it comes before the file's output; without,
//!   the file's stderr comes before it (prove writes the header to buffered
//!   stdout) and its verdict lines (`Dubious, ...`, `Failed 2/5 subtests`)
//!   follow it. A section without result lines is therefore taken to belong to
//!   the header after it. A bare header is dropped, like the plan (`1..N`), `TAP
//!   version N`, blank lines, Test::More's own diagnostics (`#   Failed test
//!   ...` with its indented details, `# Looks like you failed ...`) and node's
//!   closing tally (`# tests 12`, `# pass 12`, ...);
//! - any other line (prove's summary, a header's verdict, stray output, `Bail
//!   out!`) becomes a `line` event.
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
    /// Test2's buffered subtest: `ok 1 - name {`, its lines follow until `}`.
    pub opens_block: bool,
}

/// What one raw line is, once the TAP structure is taken off.
#[derive(Debug, PartialEq)]
pub enum TapLine<'a> {
    /// Text inside a `#` comment: the embedded log line.
    Comment(&'a str),
    Result(TestResult),
    /// prove's per-file header, carrying the test file name and whether a
    /// verdict follows it on the line (`t/foo.t .. ok`).
    Header {
        file: &'a str,
        verdict: bool,
    },
    /// A line of a YAML block after a result, with the block's `duration_ms`
    /// when this line holds it and the block follows a top-level result.
    Yaml(Option<f64>),
    /// A subtest's (indented) result.
    Nested(TestResult),
    /// `# Subtest: name`, announcing a subtest.
    Subtest(&'a str),
    /// `}`, closing a Test2 buffered subtest.
    BlockEnd,
    /// Anything else; emitted as a `line` event.
    Other,
    /// TAP structure or Test::More boilerplate with no log content.
    Drop,
}

/// Where a YAML block can start or is in progress.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Yaml {
    #[default]
    Outside,
    /// The previous line was a result; `top` if it was not indented.
    AfterResult {
        top: bool,
    },
    InBlock {
        top: bool,
    },
}

/// Line classifier. Stateful only to recognise multi-line structure: the
/// indented detail lines after a `#   Failed test` diagnostic, and YAML blocks.
#[derive(Debug, Default)]
pub struct TapClassifier {
    in_failure_block: bool,
    /// Inside a quoted `got:`/`expected:` value that spans lines.
    in_quoted_value: bool,
    yaml: Yaml,
}

impl TapClassifier {
    pub fn classify<'a>(&mut self, line: &'a str) -> TapLine<'a> {
        let trimmed = line.trim_start();

        // YAML content can hold anything, `#` included, so it is checked first.
        match std::mem::take(&mut self.yaml) {
            Yaml::InBlock { top } => {
                if trimmed.trim_end() == "..." {
                    return TapLine::Yaml(None);
                }
                self.yaml = Yaml::InBlock { top };
                let duration = trimmed
                    .strip_prefix("duration_ms:")
                    .and_then(|ms| ms.trim().parse().ok())
                    .filter(|_| top);
                return TapLine::Yaml(duration);
            }
            Yaml::AfterResult { top }
                if trimmed.len() < line.len() && trimmed.trim_end() == "---" =>
            {
                self.yaml = Yaml::InBlock { top };
                return TapLine::Yaml(None);
            }
            _ => {}
        }

        if let Some(rest) = trimmed.strip_prefix('#') {
            let text = rest.strip_prefix(' ').unwrap_or(rest);
            if self.in_failure_block {
                if self.in_quoted_value {
                    self.in_quoted_value = !text.ends_with('\'');
                    return TapLine::Drop;
                }
                if text.starts_with([' ', '\t']) {
                    self.in_quoted_value = opens_quoted_value(text);
                    return TapLine::Drop;
                }
            }
            self.in_failure_block = false;
            self.in_quoted_value = false;
            if text.starts_with("  Failed test") || text.starts_with("  Failed (TODO) test") {
                self.in_failure_block = true;
                return TapLine::Drop;
            }
            if let Some(name) = text.strip_prefix("Subtest: ") {
                return TapLine::Subtest(name.trim_end());
            }
            if text.starts_with("Looks like you")
                || text == "No tests run!"
                || text.starts_with("Tests were run but no plan was declared")
                || is_runner_tally(text)
            {
                return TapLine::Drop;
            }
            return TapLine::Comment(text);
        }

        if trimmed.is_empty() {
            return TapLine::Drop;
        }
        self.in_failure_block = false;
        self.in_quoted_value = false;

        if line.starts_with("TAP version ") || is_plan(trimmed) {
            return TapLine::Drop;
        }
        if trimmed.trim_end() == "}" {
            return TapLine::BlockEnd;
        }
        if let Some(result) = parse_result(line) {
            self.yaml = Yaml::AfterResult { top: true };
            return TapLine::Result(result);
        }
        if trimmed.len() < line.len() {
            if let Some(result) = parse_result(trimmed) {
                // A subtest's result; its YAML block has no duration for the test.
                self.yaml = Yaml::AfterResult { top: false };
                return TapLine::Nested(result);
            }
        }
        if let Some((file, verdict)) = parse_prove_header(line) {
            return TapLine::Header { file, verdict };
        }
        TapLine::Other
    }
}

/// A `got: '...` / `expected: '...` detail line whose quoted value continues
/// on the next lines (Test::More quotes values and splits them at newlines).
fn opens_quoted_value(text: &str) -> bool {
    let Some((_, value)) = text.split_once(": ") else {
        return false;
    };
    value.len() > 1 && value.starts_with('\'') && !value.ends_with('\'') || value == "'"
}

/// node's closing tally: `# tests 12`, `# pass 12`, `# duration_ms 351.2`.
fn is_runner_tally(text: &str) -> bool {
    const KEYS: [&str; 8] = [
        "tests",
        "suites",
        "pass",
        "fail",
        "cancelled",
        "skipped",
        "todo",
        "duration_ms",
    ];
    text.split_once(' ')
        .is_some_and(|(key, value)| KEYS.contains(&key) && value.parse::<f64>().is_ok())
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
    let trimmed = rest.trim_end();
    let (rest, opens_block) = match trimmed.strip_suffix(" {") {
        Some(rest) => (rest, true),
        None => (trimmed, false),
    };

    let (desc, directive) = split_directive(rest);
    let desc = desc.trim();
    let desc = desc.strip_prefix('-').map(str::trim_start).unwrap_or(desc);
    Some(TestResult {
        ok,
        num,
        name: desc.replace("\\#", "#"),
        directive,
        opens_block,
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

/// prove's per-file header: `t/foo.t .. ` with `-v`, `t/foo.t .. ok` or
/// `t/foo.t .. 1/?` without (the dots are padded to align several files).
/// Returns the file and whether a verdict follows the dots. The name must look
/// like a test path, so stray output such as `Loading ...` stays an ordinary
/// line.
fn parse_prove_header(line: &str) -> Option<(&str, bool)> {
    let line = line.trim_end();
    let (name, rest) = line.split_once(' ')?;
    let looks_like_test = name.ends_with(".t") || name.contains('/');
    let dots = rest.bytes().take_while(|&b| b == b'.').count();
    let after = &rest[dots..];
    (looks_like_test && dots >= 2 && (after.is_empty() || after.starts_with(' ')))
        .then_some((name, !after.is_empty()))
}

/// prove's verdict lines after a header, for a file that did not pass.
fn is_prove_verdict(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("Dubious, test returned")
        || trimmed == "No subtests run"
        || trimmed.starts_with("(less ")
        || trimmed.starts_with("Parse errors:")
        || (trimmed.starts_with("Failed ") && trimmed.ends_with("subtests"))
        || (trimmed.starts_with("All ") && trimmed.ends_with("subtests passed"))
}

/// Fields of one line beyond its test's (`test_subtest`, `test_time`).
pub type Extra = Vec<(String, Dynamic)>;

/// A prove line released at once with its test file.
fn file_release(file: &str, line: ChunkLine, extra: Extra) -> TapRelease {
    TapRelease {
        fields: Some(Arc::new(vec![(
            "test_file".to_string(),
            Dynamic::from(file.to_string()),
        )])),
        items: vec![TapItem::Raw(line, extra)],
        result: None,
    }
}

/// One line on its way to the pipeline, with its own extra fields.
#[derive(Debug)]
pub enum TapItem {
    /// Comment text, for the inner parser (and the multiline chunker).
    Inner(ChunkLine, Extra),
    /// A non-comment line, emitted as a `line` event.
    Raw(ChunkLine, Extra),
}

/// Buffered lines released together with the fields of the test they belong
/// to, optionally followed by that test's result line.
#[derive(Debug)]
pub struct TapRelease {
    pub fields: Option<TapFields>,
    pub items: Vec<TapItem>,
    pub result: Option<(ChunkLine, Extra)>,
}

impl TapRelease {
    /// The lines to feed, in order, each carrying its event's fields: `true`
    /// for comment text (parsed with the input format), `false` for a `line`
    /// event. The result line, if any, comes last.
    pub fn into_lines(self) -> Vec<(bool, ChunkLine)> {
        let fields = self.fields;
        let with_fields = |mut line: ChunkLine, extra: Extra| {
            line.extra_fields = if extra.is_empty() {
                fields.clone()
            } else {
                let mut all = fields.as_deref().cloned().unwrap_or_default();
                all.extend(extra);
                Some(Arc::new(all))
            };
            line
        };
        let mut lines: Vec<(bool, ChunkLine)> = self
            .items
            .into_iter()
            .map(|item| match item {
                TapItem::Inner(line, extra) => (true, with_fields(line, extra)),
                TapItem::Raw(line, extra) => (false, with_fields(line, extra)),
            })
            .collect();
        if let Some((line, extra)) = self.result {
            lines.push((false, with_fields(line, extra)));
        }
        lines
    }
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

/// Split off PostgreSQL's `[HH:MM:SS.mmm](0.123s) ` prefix, returning the
/// time and the rest of the line.
fn split_time_prefix(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let (time, rest) = rest.split_once("](")?;
    let (delta, rest) = rest.split_once("s) ")?;
    let is_time = !time.is_empty()
        && time
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b':' || b == b'.');
    let is_delta = !delta.is_empty() && delta.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    (is_time && is_delta).then_some((time, rest))
}

/// Number of leading spaces: a subtest's lines are indented 4 per level.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Stateful unwrapper: classifies raw lines and holds each test's lines until
/// its result line arrives.
#[derive(Debug, Default)]
pub struct TapUnwrapper {
    classifier: TapClassifier,
    /// The last result's release, held back until the line after it shows
    /// whether a YAML block adds `test_duration_ms`.
    held: Option<TapRelease>,
    pending: Vec<TapItem>,
    test_file: Option<String>,
    /// A result line was seen since the last header: `prove -v` output, whose
    /// header comes before the file's lines.
    results_since_header: bool,
    /// Set right after a header: prove's verdict lines that follow it belong
    /// to its file.
    verdict_file: Option<String>,
    /// Names of the open subtests, by the indentation of their lines.
    subtests: Vec<(usize, String)>,
    /// Fields of a Test2 buffered subtest (`ok 1 - name {`) whose lines are
    /// still coming: they belong to that result, which came first.
    block: Option<TapFields>,
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
                self.results_since_header = false;
                self.verdict_file = None;
                self.subtests.clear();
                self.classifier = TapClassifier::default();
            }
            self.last_filename = Some(filename.clone());
        }

        let (time, body) = match split_time_prefix(&text) {
            Some((time, body)) => (Some(time.to_string()), body),
            None => (None, text.as_str()),
        };
        let indent = indent_of(body);
        let line = self.classifier.classify(body);
        if let TapLine::Yaml(duration) = line {
            step.dropped = true;
            if let (Some(ms), Some(held)) = (duration, self.held.as_mut()) {
                if let Some(fields) = held.fields.as_mut() {
                    Arc::make_mut(fields).push(("test_duration_ms".to_string(), Dynamic::from(ms)));
                }
            }
            return step;
        }
        step.releases.extend(self.held.take());
        let verdict_file = self.verdict_file.take();

        let chunk_line = |text: String| ChunkLine {
            text,
            line_num,
            filename: filename.clone(),
            extra_fields: None,
        };
        let mut extra = Extra::new();
        if let Some(subtest) = self.subtest_path(indent) {
            extra.push(("test_subtest".to_string(), Dynamic::from(subtest)));
        }
        if let Some(time) = time {
            extra.push(("test_time".to_string(), Dynamic::from(time)));
        }

        match line {
            TapLine::Yaml(_) => unreachable!("handled above"),
            TapLine::Drop => step.dropped = true,
            TapLine::Comment(inner) => {
                let inner = inner.to_string();
                self.pending.push(TapItem::Inner(chunk_line(inner), extra));
            }
            TapLine::Other => match verdict_file {
                Some(file) if is_prove_verdict(body) => {
                    step.releases
                        .push(file_release(&file, chunk_line(text), extra));
                    self.verdict_file = Some(file);
                }
                _ => self.pending.push(TapItem::Raw(chunk_line(text), extra)),
            },
            TapLine::Subtest(name) => {
                self.open_subtest(indent, name);
                step.dropped = true;
            }
            TapLine::Nested(result) => {
                if result.opens_block {
                    self.open_subtest(indent + 4, &result.name);
                } else {
                    self.subtests.retain(|(level, _)| *level <= indent);
                }
                // A failing assertion says what went wrong; passing ones are
                // summarized by the test's own result.
                if result.ok {
                    step.dropped = true;
                } else {
                    self.pending.push(TapItem::Raw(chunk_line(text), extra));
                }
            }
            TapLine::BlockEnd => {
                step.dropped = true;
                self.subtests.retain(|(level, _)| *level <= indent);
                if indent == 0 {
                    if let Some(fields) = self.block.take() {
                        step.releases.push(TapRelease {
                            fields: Some(fields),
                            items: std::mem::take(&mut self.pending),
                            result: None,
                        });
                    }
                }
            }
            TapLine::Header { file, verdict } => {
                let file = file.to_string();
                if !self.results_since_header {
                    // No results since the last header: these lines came
                    // before this header and belong to its file.
                    self.test_file = Some(file.clone());
                }
                step.releases.extend(self.finish());
                self.test_file = Some(file.clone());
                self.results_since_header = false;
                self.subtests.clear();
                if verdict {
                    step.releases
                        .push(file_release(&file, chunk_line(text), extra));
                } else {
                    step.dropped = true;
                }
                self.verdict_file = Some(file);
            }
            TapLine::Result(result) => {
                self.results_since_header = true;
                self.subtests.clear();
                let fields = self.result_fields(&result);
                let release = TapRelease {
                    fields: Some(fields.clone()),
                    items: std::mem::take(&mut self.pending),
                    result: Some((chunk_line(text), extra)),
                };
                if result.opens_block {
                    // Test2 buffered subtest: its lines follow until `}`.
                    step.releases.push(release);
                    self.block = Some(fields);
                    self.open_subtest(4, &result.name);
                } else {
                    self.held = Some(release);
                }
            }
        }

        if self.pending.len() >= MAX_PENDING_LINES {
            step.overflowed = true;
            step.releases.extend(self.finish());
        }
        step
    }

    /// Record a subtest whose lines are indented by `indent`, closing any
    /// deeper ones.
    fn open_subtest(&mut self, indent: usize, name: &str) {
        self.subtests.retain(|(level, _)| *level < indent);
        self.subtests.push((indent, name.to_string()));
    }

    /// `test_subtest` for a line at `indent`: the open subtests below the top
    /// level (whose name is already `test_name`), outermost first.
    fn subtest_path(&self, indent: usize) -> Option<String> {
        let names: Vec<&str> = self
            .subtests
            .iter()
            .filter(|(level, _)| *level > 4 && *level <= indent)
            .map(|(_, name)| name.as_str())
            .collect();
        (!names.is_empty()).then(|| names.join(" / "))
    }

    /// Release everything still held (end of input, end of a file): the last
    /// result, then lines that no result line followed. Those keep `test_file`
    /// but get no test fields (or the fields of an unclosed Test2 subtest).
    pub fn finish(&mut self) -> Vec<TapRelease> {
        let mut releases: Vec<TapRelease> = self.held.take().into_iter().collect();
        let block = self.block.take();
        if !self.pending.is_empty() {
            let fields = block.or_else(|| {
                self.test_file.as_ref().map(|file| {
                    Arc::new(vec![("test_file".to_string(), Dynamic::from(file.clone()))])
                })
            });
            releases.push(TapRelease {
                fields,
                items: std::mem::take(&mut self.pending),
                result: None,
            });
        }
        releases
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
                directive: None,
                opens_block: false
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
        let r = result("ok 1 - Basics {").unwrap();
        assert_eq!(r.name, "Basics");
        assert!(r.opens_block);
    }

    #[test]
    fn classifies_structure_and_boilerplate() {
        let mut c = TapClassifier::default();
        assert_eq!(c.classify("1..6"), TapLine::Drop);
        assert_eq!(c.classify("TAP version 13"), TapLine::Drop);
        assert_eq!(c.classify(""), TapLine::Drop);
        assert_eq!(
            c.classify("t/checkout.t .. "),
            TapLine::Header {
                file: "t/checkout.t",
                verdict: false
            }
        );
        assert_eq!(
            c.classify("t/00-compile.t .......... 1/? "),
            TapLine::Header {
                file: "t/00-compile.t",
                verdict: true
            }
        );
        assert_eq!(
            c.classify("t/00-check-deps.t ....... ok"),
            TapLine::Header {
                file: "t/00-check-deps.t",
                verdict: true
            }
        );
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

        // A quoted value spanning lines stays in the block.
        assert_eq!(
            c.classify("#   Failed test 'can create a class'"),
            TapLine::Drop
        );
        assert_eq!(
            c.classify("#          got: ''has' is not supported at (eval 8) line 3, near \""),
            TapLine::Drop
        );
        assert_eq!(c.classify("#     has \""), TapLine::Drop);
        assert_eq!(c.classify("# '"), TapLine::Drop);
        assert_eq!(c.classify("#     expected: undef"), TapLine::Drop);
        assert_eq!(c.classify("# next log"), TapLine::Comment("next log"));

        assert_eq!(c.classify("#   Failed (TODO) test 'y'"), TapLine::Drop);
        assert_eq!(
            c.classify("# Looks like you failed 2 tests of 6."),
            TapLine::Drop
        );
        assert_eq!(c.classify("# Subtest: inner"), TapLine::Subtest("inner"));
        assert_eq!(
            c.classify(
                "# Tests were run but no plan was declared and done_testing() was not seen."
            ),
            TapLine::Drop
        );
        assert_eq!(c.classify("# tests 143"), TapLine::Drop);
        assert_eq!(c.classify("# duration_ms 32944.5483"), TapLine::Drop);
        assert_eq!(
            c.classify("# tests passed"),
            TapLine::Comment("tests passed")
        );
    }

    #[test]
    fn classifies_yaml_blocks() {
        let mut c = TapClassifier::default();
        assert!(matches!(c.classify("ok 1 - one"), TapLine::Result(_)));
        assert_eq!(c.classify("  ---"), TapLine::Yaml(None));
        assert_eq!(
            c.classify("  duration_ms: 39.9579"),
            TapLine::Yaml(Some(39.9579))
        );
        assert_eq!(c.classify("  # not a comment"), TapLine::Yaml(None));
        assert_eq!(c.classify("  ..."), TapLine::Yaml(None));
        assert_eq!(c.classify("# after"), TapLine::Comment("after"));
        // `---` only opens a block right after a result.
        assert_eq!(c.classify("  ---"), TapLine::Other);

        // A subtest's block is dropped, without a duration for the parent.
        assert!(matches!(c.classify("    ok 1 - inner"), TapLine::Nested(_)));
        assert_eq!(c.classify("    ---"), TapLine::Yaml(None));
        assert_eq!(c.classify("    duration_ms: 1.5"), TapLine::Yaml(None));
        assert_eq!(c.classify("    ..."), TapLine::Yaml(None));
    }

    #[test]
    fn yaml_duration_joins_the_result_fields() {
        let mut tap = TapUnwrapper::new();
        let mut releases = Vec::new();
        for (i, line) in [
            "# log",
            "ok 1 - one",
            "  ---",
            "  duration_ms: 12.5",
            "  ...",
            "ok 2 - two",
        ]
        .iter()
        .enumerate()
        {
            let step = tap.push(line.to_string(), i + 1, None);
            releases.extend(step.releases);
        }
        assert_eq!(releases.len(), 1, "the result waits for the line after it");
        assert_eq!(texts(&releases[0]), vec![("inner", "log")]);
        assert_eq!(
            field(&releases[0], "test_duration_ms")
                .unwrap()
                .as_float()
                .unwrap(),
            12.5
        );
        let last = tap.finish();
        assert_eq!(last.len(), 1);
        assert!(field(&last[0], "test_duration_ms").is_none());
    }

    fn file_of(release: &TapRelease) -> String {
        field(release, "test_file")
            .unwrap()
            .clone()
            .into_string()
            .unwrap()
    }

    /// Without `-v`, a file's stderr comes before its header and prove's
    /// verdict lines after it.
    #[test]
    fn non_verbose_lines_belong_to_the_header_after_them() {
        let mut tap = TapUnwrapper::new();
        let mut releases = Vec::new();
        for (i, line) in [
            "t/a.t ...... ok",
            "#   Failed test 'loads'",
            "#   at t/b.t line 3.",
            "# [warning] cannot connect",
            "Compilation failed in require at t/b.t line 11.",
            "t/b.t ...... ",
            "Dubious, test returned 255 (wstat 65280, 0xff00)",
            "Failed 1/1 subtests ",
            "t/c.t ...... ok",
        ]
        .iter()
        .enumerate()
        {
            releases.extend(tap.push(line.to_string(), i + 1, None).releases);
        }
        releases.extend(tap.finish());

        let lines: Vec<(String, Vec<(&str, &str)>)> =
            releases.iter().map(|r| (file_of(r), texts(r))).collect();
        assert_eq!(
            lines,
            vec![
                ("t/a.t".into(), vec![("raw", "t/a.t ...... ok")]),
                (
                    "t/b.t".into(),
                    vec![
                        ("inner", "[warning] cannot connect"),
                        ("raw", "Compilation failed in require at t/b.t line 11.")
                    ]
                ),
                (
                    "t/b.t".into(),
                    vec![("raw", "Dubious, test returned 255 (wstat 65280, 0xff00)")]
                ),
                ("t/b.t".into(), vec![("raw", "Failed 1/1 subtests ")]),
                ("t/c.t".into(), vec![("raw", "t/c.t ...... ok")]),
            ]
        );
    }

    /// With `-v`, the header comes first; lines after a file's last result
    /// stay with that file.
    #[test]
    fn verbose_lines_belong_to_the_header_before_them() {
        let mut tap = TapUnwrapper::new();
        let mut releases = Vec::new();
        for (i, line) in [
            "t/a.t .. ",
            "# log a",
            "ok 1 - one",
            "# trailing a",
            "ok",
            "t/b.t .. ",
            "# log b",
            "ok 1 - one",
        ]
        .iter()
        .enumerate()
        {
            releases.extend(tap.push(line.to_string(), i + 1, None).releases);
        }
        releases.extend(tap.finish());
        let files: Vec<(String, Vec<(&str, &str)>)> =
            releases.iter().map(|r| (file_of(r), texts(r))).collect();
        assert_eq!(
            files,
            vec![
                ("t/a.t".into(), vec![("inner", "log a")]),
                ("t/a.t".into(), vec![("inner", "trailing a"), ("raw", "ok")]),
                ("t/b.t".into(), vec![("inner", "log b")]),
            ]
        );
    }

    fn texts(release: &TapRelease) -> Vec<(&'static str, &str)> {
        release
            .items
            .iter()
            .map(|item| match item {
                TapItem::Inner(l, _) => ("inner", l.text.as_str()),
                TapItem::Raw(l, _) => ("raw", l.text.as_str()),
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
        assert_eq!(releases[0].result.as_ref().unwrap().0.line_num, 4);

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
        assert!(tap.finish().is_empty());
    }
}
