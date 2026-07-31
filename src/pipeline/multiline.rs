use super::trace_presets::{TraceMachine, TraceStep};
use super::{Chunk, ChunkLine, Chunker};
use crate::config::{MultilineConfig, MultilineJoin, MultilineStrategy};
use crate::timestamp::{AdaptiveTsParser, TsMatchKind};
use regex::Regex;

const MAX_TIMESTAMP_PREFIX_CHARS: usize = 64;
const MAX_TIMESTAMP_TOKENS: usize = 6;
/// After this many timestamped header lines have started events under a
/// preset strategy, the input evidently has reliable headers and the driver
/// is told to hint at `--multiline timestamp` once.
const PRESET_TS_HINT_HEADERS: u32 = 3;
/// Below this many fed lines, a run that found no event boundary is not worth
/// mentioning: a one-record input or a two-line smoke test legitimately has
/// nothing to split, and nagging there would be noise rather than a footgun.
const MIN_LINES_FOR_NO_BOUNDARY_HINT: u64 = 10;

/// Multi-line chunker that implements the reduced set of strategies for
/// detecting event boundaries.
///
/// Invariants the drivers rely on:
/// - Blank lines are *continuations* for `timestamp`/`indent` (real stack
///   traces contain them), separators for `blank`, and ordinary lines for
///   `regex`/`all`. Trailing blank lines are trimmed at flush (except under
///   `all`, which is byte-faithful), so a blanks-only buffer emits nothing.
/// - Every completed record is appended to the caller's `Vec`; nothing is
///   held back in a pending slot, so no record count is ever lost.
/// - Each record carries the provenance (line number, filename) of its first
///   physical line.
pub struct MultilineChunker {
    config: MultilineConfig,
    buffer: Vec<String>,
    buf_first_line_num: usize,
    buf_filename: Option<String>,
    start_regex: Option<Regex>,
    end_regex: Option<Regex>,
    timestamp_detector: Option<TimestampDetector>,
    preset: Option<PresetState>,
    /// Set when the line cap split an event; drained by `take_cap_hit`.
    cap_hit: bool,
    /// Lines fed so far, and the number of times the strategy's boundary rule
    /// fired. The cap and the end-of-input flush deliberately do not count: they
    /// are the safety net and EOF, not the rule matching. "Many lines, zero
    /// firings" is what `collapsed_without_boundary` reports.
    lines_fed: u64,
    boundaries: u64,
}

/// Runtime state for a language preset strategy: the trace rule machine plus
/// a timestamp guard. The guard makes the intuitive-but-suboptimal choice
/// safe: on input that *does* have timestamped headers, a locked header line
/// always starts a new event — overriding any trace continuation — so a
/// preset can never bleed a trace across a real event boundary. (Where
/// headers exist, `--multiline timestamp` is still the better tool; the
/// guard feeds the once-per-run hint that says so.)
struct PresetState {
    machine: TraceMachine,
    guard: TimestampDetector,
    /// Header lines the guard has turned into event starts.
    guard_boundaries: u32,
    /// Set once `guard_boundaries` reaches the hint threshold; drained by
    /// `take_preset_ts_hint`.
    ts_hint: bool,
}

impl MultilineChunker {
    pub fn new(config: MultilineConfig) -> Result<Self, String> {
        let mut start_regex = None;
        let mut end_regex = None;
        let mut timestamp_detector = None;
        let mut preset = None;

        match &config.strategy {
            MultilineStrategy::Regex { start, end } => {
                start_regex = Some(
                    Regex::new(start).map_err(|e| format!("Invalid regex start pattern: {}", e))?,
                );

                if let Some(end_pattern) = end {
                    end_regex = Some(
                        Regex::new(end_pattern)
                            .map_err(|e| format!("Invalid regex end pattern: {}", e))?,
                    );
                }
            }
            MultilineStrategy::Timestamp {
                chrono_format,
                loose,
            } => {
                timestamp_detector = Some(TimestampDetector::new(chrono_format.clone(), *loose));
            }
            MultilineStrategy::Preset(lang) => {
                preset = Some(PresetState {
                    machine: TraceMachine::new(*lang)?,
                    guard: TimestampDetector::new(None, false),
                    guard_boundaries: 0,
                    ts_hint: false,
                });
            }
            MultilineStrategy::Indent | MultilineStrategy::Blank | MultilineStrategy::All => {}
        }

        Ok(Self {
            config,
            buffer: Vec::new(),
            buf_first_line_num: 0,
            buf_filename: None,
            start_regex,
            end_regex,
            timestamp_detector,
            preset,
            cap_hit: false,
            lines_fed: 0,
            boundaries: 0,
        })
    }

    /// Check if this line starts a new event based on the current strategy
    fn starts_new_event(&mut self, line: &str) -> bool {
        match &self.config.strategy {
            MultilineStrategy::Timestamp { .. } => {
                if let Some(detector) = self.timestamp_detector.as_mut() {
                    detector.is_header(line)
                } else {
                    false
                }
            }
            // A blank line continues the current event (stack traces contain
            // blank lines); only a non-blank, non-indented line starts one.
            MultilineStrategy::Indent => !is_line_blank(line) && !is_line_indented(line),
            MultilineStrategy::Regex { .. } => {
                if let Some(regex) = &self.start_regex {
                    regex.is_match(line)
                } else {
                    false
                }
            }
            // Presets never reach here: `feed_line` classifies their lines
            // through the trace machine instead.
            MultilineStrategy::Blank | MultilineStrategy::All | MultilineStrategy::Preset(_) => {
                false
            }
        }
    }

    /// Check if this line ends the current event (only relevant for regex strategies with end=...)
    fn ends_current_event(&self, line: &str) -> bool {
        match (&self.config.strategy, &self.end_regex) {
            (MultilineStrategy::Regex { end: Some(_), .. }, Some(regex)) => regex.is_match(line),
            _ => false,
        }
    }

    fn push_line(&mut self, line: ChunkLine) {
        if self.buffer.is_empty() {
            self.buf_first_line_num = line.line_num;
            self.buf_filename = line.filename;
        }
        self.buffer.push(line.text);
    }

    /// Flush the current buffer as one record, if anything non-trivial is
    /// buffered. Trailing blank lines are trimmed (they belong to no event) —
    /// except under `all`, which reproduces the input byte-faithfully — so a
    /// buffer holding only blanks flushes to nothing.
    fn flush_buffer(&mut self, out: &mut Vec<Chunk>) {
        if self.buffer.is_empty() {
            return;
        }

        let line_count = self.buffer.len();

        if !matches!(self.config.strategy, MultilineStrategy::All) {
            while self.buffer.last().is_some_and(|l| is_line_blank(l)) {
                self.buffer.pop();
            }
        }

        if self.buffer.is_empty() {
            self.buf_filename = None;
            return;
        }

        let joiner = match self.config.join {
            MultilineJoin::Newline => "\n",
            MultilineJoin::Empty => "",
            MultilineJoin::Space => " ",
        };
        let mut joined = String::new();
        for (idx, line) in self.buffer.iter().enumerate() {
            if idx > 0 {
                joined.push_str(joiner);
            }
            joined.push_str(line.trim_end_matches(['\n', '\r']));
        }

        self.buffer.clear();
        out.push(Chunk {
            text: joined,
            first_line_num: self.buf_first_line_num,
            filename: self.buf_filename.take(),
            line_count,
        });
    }
}

struct TimestampDetector {
    parser: AdaptiveTsParser,
    chrono_format: Option<String>,
    loose: bool,
    /// The timestamp family the first detected header matched. Once locked,
    /// only that family marks headers, so a continuation line beginning with
    /// some *other* parseable prefix ("17:03 was the incident window") cannot
    /// split the event. `loose` disables locking for mixed-format files.
    locked: Option<TsMatchKind>,
}

impl TimestampDetector {
    fn new(chrono_format: Option<String>, loose: bool) -> Self {
        Self {
            parser: AdaptiveTsParser::new(),
            chrono_format,
            loose,
            locked: None,
        }
    }

    fn is_header(&mut self, line: &str) -> bool {
        let stripped = line.trim_end_matches(['\n', '\r']);

        if stripped.is_empty() {
            return false;
        }

        if stripped.starts_with(char::is_whitespace) {
            return false;
        }

        let candidates = timestamp_prefix_candidates(stripped);
        if candidates.is_empty() {
            return false;
        }

        // A pinned format is a contract, not a preference: only it detects
        // headers. (Falling back to adaptive detection here would reintroduce
        // exactly the false positives the hint exists to rule out.)
        if let Some(format) = self.chrono_format.as_deref() {
            return candidates
                .iter()
                .any(|c| AdaptiveTsParser::matches_custom_format(c, format));
        }

        for candidate in &candidates {
            if let Some(kind) = self.parser.parse_header_kind(candidate) {
                if self.loose {
                    return true;
                }
                match &self.locked {
                    None => {
                        self.locked = Some(kind);
                        return true;
                    }
                    Some(locked) if *locked == kind => return true,
                    // Wrong family — a longer candidate may still match the
                    // locked one (e.g. "Jan 2" date vs "Jan 2 03:04:05").
                    Some(_) => {}
                }
            }
        }

        false
    }
}

fn is_line_blank(line: &str) -> bool {
    line.trim_end_matches(['\n', '\r']).trim().is_empty()
}

fn is_line_indented(line: &str) -> bool {
    let stripped = line.trim_end_matches(['\n', '\r']);
    if stripped.is_empty() {
        return false;
    }

    stripped.starts_with(char::is_whitespace)
}

fn timestamp_prefix_candidates(line: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    if line.is_empty() {
        return candidates;
    }

    let mut tokens = Vec::new();
    let mut token_start: Option<usize> = None;
    let mut reached_limit = false;

    for (idx, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                tokens.push((start, idx));
                if tokens.len() == MAX_TIMESTAMP_TOKENS {
                    reached_limit = true;
                    break;
                }
            }
        } else if token_start.is_none() {
            token_start = Some(idx);
        }
    }

    if !reached_limit {
        if let Some(start) = token_start {
            tokens.push((start, line.len()));
        }
    }

    if tokens.is_empty() {
        let fallback = take_prefix_chars(line, MAX_TIMESTAMP_PREFIX_CHARS);
        push_candidate(&mut candidates, fallback.to_string());
        return candidates;
    }

    let max_tokens = tokens.len().min(MAX_TIMESTAMP_TOKENS);
    let start_idx = tokens[0].0;

    for count in 1..=max_tokens {
        let (_, end_idx) = tokens[count - 1];
        let slice = &line[start_idx..end_idx.min(line.len())];

        if slice.chars().count() > MAX_TIMESTAMP_PREFIX_CHARS {
            continue;
        }

        push_candidate(&mut candidates, slice.to_string());

        let trimmed_slice = slice.trim_end_matches([':', ',', ';', '-', '.']);
        if trimmed_slice.len() < slice.len() && trimmed_slice.chars().count() >= 4 {
            push_candidate(&mut candidates, trimmed_slice.to_string());
        }
    }

    let fallback = take_prefix_chars(line, MAX_TIMESTAMP_PREFIX_CHARS);
    push_candidate(&mut candidates, fallback.to_string());

    candidates
}

fn take_prefix_chars(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }

    for (count, (idx, _)) in s.char_indices().enumerate() {
        if count == max_chars {
            return &s[..idx];
        }
    }

    s
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if candidate.is_empty() {
        return;
    }

    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

impl Chunker for MultilineChunker {
    fn feed_line(&mut self, line: ChunkLine, out: &mut Vec<Chunk>) {
        self.lines_fed += 1;
        match &self.config.strategy {
            MultilineStrategy::All => {
                self.push_line(line);
                // `all` buffers the entire input; the cap intentionally does
                // not apply.
                return;
            }
            MultilineStrategy::Blank => {
                // Paragraph mode: a blank line terminates the record and is
                // itself part of no record.
                if is_line_blank(&line.text) {
                    self.boundaries += 1;
                    self.flush_buffer(out);
                    return;
                }
                self.push_line(line);
            }
            MultilineStrategy::Preset(_) => {
                let preset = self
                    .preset
                    .as_mut()
                    .expect("preset state exists for preset strategy");
                // The guard outranks the trace machine: a line starting with
                // a locked-format timestamp is always an event header, even
                // where a preset rule would read it as a continuation. (It
                // also locks on the file's first header, like the timestamp
                // strategy, so evaluate it on every line.)
                let boundary = if preset.guard.is_header(&line.text) {
                    preset.machine.reset();
                    preset.guard_boundaries += 1;
                    if preset.guard_boundaries == PRESET_TS_HINT_HEADERS {
                        preset.ts_hint = true;
                    }
                    true
                } else {
                    preset.machine.feed(&line.text) == TraceStep::Boundary
                };
                if boundary {
                    self.boundaries += 1;
                    self.flush_buffer(out);
                }
                self.push_line(line);
            }
            MultilineStrategy::Timestamp { .. }
            | MultilineStrategy::Indent
            | MultilineStrategy::Regex { .. } => {
                if self.buffer.is_empty() {
                    // Still evaluate the header check for the event's *first*
                    // line: the timestamp detector locks onto the first format
                    // that matches, and that must be the leading real header,
                    // not whichever parseable prefix shows up first later.
                    // It splits nothing (there is nothing to split yet), but it
                    // is the rule firing, so it counts: that keeps
                    // `collapsed_without_boundary` meaning "the predicate never
                    // matched any line", which is what the hint claims.
                    if self.starts_new_event(&line.text) {
                        self.boundaries += 1;
                    }
                } else if self.starts_new_event(&line.text) {
                    self.boundaries += 1;
                    self.flush_buffer(out);
                }

                let ends = self.ends_current_event(&line.text);
                self.push_line(line);
                if ends {
                    // A terminator is the rule firing too, so a run split only
                    // by `end=` does not report a collapse.
                    self.boundaries += 1;
                    self.flush_buffer(out);
                }
            }
        }

        if self.config.max_lines > 0 && self.buffer.len() >= self.config.max_lines {
            self.flush_buffer(out);
            self.cap_hit = true;
        }
    }

    fn flush(&mut self, out: &mut Vec<Chunk>) {
        self.flush_buffer(out);
        // A flush is a hard boundary (file change, idle timeout, EOF): a
        // trace never continues across one, so forget any in-trace state.
        if let Some(preset) = self.preset.as_mut() {
            preset.machine.reset();
        }
    }

    fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    fn take_cap_hit(&mut self) -> bool {
        std::mem::take(&mut self.cap_hit)
    }

    fn take_preset_ts_hint(&mut self) -> bool {
        self.preset
            .as_mut()
            .is_some_and(|p| std::mem::take(&mut p.ts_hint))
    }

    fn collapsed_without_boundary(&self) -> bool {
        // `all` is asked for exactly this, and a preset reporting no boundary
        // means the whole input was one stack trace (a piped crash dump) —
        // neither is a mistake. Mirrored by `config::no_boundary_hint_text`,
        // which has no message for these.
        if matches!(
            self.config.strategy,
            MultilineStrategy::All | MultilineStrategy::Preset(_)
        ) {
            return false;
        }
        self.boundaries == 0 && self.lines_fed >= MIN_LINES_FOR_NO_BOUNDARY_HINT
    }
}

/// Create a chunker based on multiline configuration
pub fn create_multiline_chunker(config: &MultilineConfig) -> Result<Box<dyn Chunker>, String> {
    let chunker = MultilineChunker::new(config.clone())?;
    Ok(Box::new(chunker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(strategy: MultilineStrategy, join: MultilineJoin) -> MultilineConfig {
        MultilineConfig {
            strategy,
            join,
            max_lines: 0,
            idle_timeout: None,
            idle_timeout_explicit: false,
        }
    }

    fn timestamp_strategy() -> MultilineStrategy {
        MultilineStrategy::Timestamp {
            chrono_format: None,
            loose: false,
        }
    }

    /// Feed `text` line by line (1-based line numbers, no filename) and return
    /// all records including the final flush.
    fn chunk_all(chunker: &mut MultilineChunker, text: &str) -> Vec<Chunk> {
        let mut out = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            chunker.feed_line(
                ChunkLine {
                    text: line.to_string(),
                    line_num: idx + 1,
                    filename: None,
                },
                &mut out,
            );
        }
        chunker.flush(&mut out);
        out
    }

    fn texts(chunks: &[Chunk]) -> Vec<&str> {
        chunks.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn timestamp_detection_with_format_hint() {
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Timestamp {
                chrono_format: Some("%b %e %H:%M:%S".to_string()),
                loose: false,
            },
            MultilineJoin::Space,
        ))
        .expect("chunker should build");

        let chunks = chunk_all(
            &mut chunker,
            "Jan  2 03:04:05 host app: one\n  stack frame line\nJan  3 03:04:05 host app: two\n",
        );
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("one"));
        assert!(chunks[0].text.contains("stack frame"));
        assert_eq!(chunks[0].first_line_num, 1);
        assert_eq!(chunks[0].line_count, 2);
        assert_eq!(chunks[1].first_line_num, 3);
    }

    #[test]
    fn format_hint_is_exclusive() {
        // With a pinned format, other recognizable timestamps are NOT headers:
        // the ISO line joins the current event instead of splitting it.
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Timestamp {
                chrono_format: Some("%b %e %H:%M:%S".to_string()),
                loose: false,
            },
            MultilineJoin::Newline,
        ))
        .unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "Jan  2 03:04:05 one\n2024-01-01T10:00:00 not a header here\nJan  3 03:04:05 two\n",
        );
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("not a header here"));
    }

    #[test]
    fn timestamp_locks_to_first_matching_family() {
        let mut chunker =
            MultilineChunker::new(config(timestamp_strategy(), MultilineJoin::Newline)).unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "2024-01-01T10:00:00 ERROR boom\n17:03 was the incident window\nJan 5 10:00:00 syslog-looking line\n2024-01-01T10:00:01 INFO ok\n",
        );
        // Time-only and syslog prefixes are not ISO headers -> continuations.
        assert_eq!(
            texts(&chunks),
            vec![
                "2024-01-01T10:00:00 ERROR boom\n17:03 was the incident window\nJan 5 10:00:00 syslog-looking line",
                "2024-01-01T10:00:01 INFO ok"
            ]
        );
    }

    #[test]
    fn timestamp_loose_disables_lock_in() {
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Timestamp {
                chrono_format: None,
                loose: true,
            },
            MultilineJoin::Newline,
        ))
        .unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "2024-01-01T10:00:00 ERROR boom\n17:03 split here\n",
        );
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn special_values_and_relative_times_are_not_headers() {
        let mut chunker =
            MultilineChunker::new(config(timestamp_strategy(), MultilineJoin::Newline)).unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "2024-01-01T10:00:00 ERROR boom\nnow retrying with backoff\ntoday was expected\n+30m elapsed since start\n2024-01-01T10:00:01 INFO ok\n",
        );
        assert_eq!(chunks.len(), 2, "prose lines must not split the event");
        assert!(chunks[0].text.contains("now retrying"));
        assert!(chunks[0].text.contains("today was expected"));
        assert!(chunks[0].text.contains("+30m elapsed"));
    }

    #[test]
    fn test_indent_strategy_basic() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Space)).unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "Header line\n  continued line\n\tmore continuation\nNew header\n",
        );
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("Header"));
        assert_eq!(chunks[0].line_count, 3);
        assert!(chunks[1].text.contains("New header"));
        assert_eq!(chunks[1].first_line_num, 4);
    }

    #[test]
    fn indent_blank_line_is_a_continuation() {
        // A blank line inside an indented block (Python tracebacks, Java
        // "Caused by" sections) must not split the event or create junk events.
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Newline))
                .unwrap();

        let chunks = chunk_all(&mut chunker, "Header\n  at foo\n\n  at bar\nNext\n");
        assert_eq!(texts(&chunks), vec!["Header\n  at foo\n\n  at bar", "Next"]);
    }

    #[test]
    fn trailing_blank_lines_are_trimmed() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Newline))
                .unwrap();

        let chunks = chunk_all(&mut chunker, "Header\n  cont\n\n\nNext\n\n");
        assert_eq!(texts(&chunks), vec!["Header\n  cont", "Next"]);
        // line_count still reflects the physical lines the record consumed.
        assert_eq!(chunks[0].line_count, 4);
    }

    #[test]
    fn blanks_only_buffer_flushes_to_nothing() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Newline))
                .unwrap();
        let chunks = chunk_all(&mut chunker, "\n\n\n");
        assert!(chunks.is_empty());
    }

    #[test]
    fn blank_strategy_splits_paragraphs() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Blank, MultilineJoin::Newline))
                .unwrap();

        let chunks = chunk_all(&mut chunker, "a\nb\n\nc\n\n\nd\ne\n");
        assert_eq!(texts(&chunks), vec!["a\nb", "c", "d\ne"]);
        assert_eq!(chunks[1].first_line_num, 4);
        assert_eq!(chunks[2].first_line_num, 7);
    }

    #[test]
    fn test_regex_strategy_start_only() {
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: r"^\d{4}-\d{2}-\d{2}".to_string(),
                end: None,
            },
            MultilineJoin::Space,
        ))
        .unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "2024-01-01 First event\ncontinuation\n2024-01-02 Second event\n",
        );
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("2024-01-01"));
        assert!(chunks[1].text.contains("2024-01-02"));
    }

    #[test]
    fn test_regex_strategy_with_end() {
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: r"^START".to_string(),
                end: Some(r"^END".to_string()),
            },
            MultilineJoin::Space,
        ))
        .unwrap();

        let chunks = chunk_all(&mut chunker, "START event 1\nmiddle line\nEND\n");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("START"));
        assert!(chunks[0].text.contains("END"));
    }

    #[test]
    fn line_matching_start_and_end_completes_two_records_without_loss() {
        // Regression for the pending_output one-slot buffer: a line that both
        // flushes the previous event (start match) and completes itself (end
        // match) produces two records in one feed; the third-in-a-row case
        // used to be silently dropped.
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: "A".to_string(),
                end: Some("E".to_string()),
            },
            MultilineJoin::Space,
        ))
        .unwrap();

        let chunks = chunk_all(&mut chunker, "Ax\nAy\nAE\nAz\nAE\n");
        assert_eq!(texts(&chunks), vec!["Ax", "Ay", "AE", "Az", "AE"]);
    }

    #[test]
    fn cap_splits_oversized_events_and_reports_it() {
        let mut cfg = config(MultilineStrategy::Indent, MultilineJoin::Space);
        cfg.max_lines = 3;
        let mut chunker = MultilineChunker::new(cfg).unwrap();

        let chunks = chunk_all(&mut chunker, "H\n a\n b\n c\n d\n");
        assert_eq!(chunks.len(), 2, "capped buffer splits into two records");
        assert_eq!(chunks[0].line_count, 3);
        assert!(chunker.take_cap_hit());
        assert!(!chunker.take_cap_hit(), "cap-hit flag is take-once");
    }

    /// 12 lines, no line matching the start pattern: the rule never fired, so
    /// the whole input is one event and the driver must be told (#361).
    #[test]
    fn no_boundary_is_reported_when_the_rule_never_matches() {
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: "^NEVER".to_string(),
                end: None,
            },
            MultilineJoin::Newline,
        ))
        .unwrap();

        let input = (0..12).map(|i| format!("line {}\n", i)).collect::<String>();
        let chunks = chunk_all(&mut chunker, &input);
        assert_eq!(chunks.len(), 1, "everything collapsed into one event");
        assert!(chunker.collapsed_without_boundary());
    }

    #[test]
    fn one_matching_line_is_enough_to_stay_quiet() {
        // The predicate fired on the first line. It split nothing (there was
        // nothing to split yet), but "no line began with a timestamp" would be
        // a false claim, so the hint must not fire.
        let mut chunker =
            MultilineChunker::new(config(timestamp_strategy(), MultilineJoin::Newline)).unwrap();

        let mut input = String::from("2024-01-01T10:00:00 header\n");
        for i in 0..12 {
            input.push_str(&format!("continuation {}\n", i));
        }
        let chunks = chunk_all(&mut chunker, &input);
        assert_eq!(chunks.len(), 1, "still one event");
        assert!(
            !chunker.collapsed_without_boundary(),
            "the rule did match a line"
        );
    }

    #[test]
    fn regex_end_only_split_is_not_a_collapse() {
        // `end=` matched even though `match=` never did, so events *were* split
        // and "nothing split the input" would be wrong.
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: "^NEVER".to_string(),
                end: Some("^END".to_string()),
            },
            MultilineJoin::Newline,
        ))
        .unwrap();

        let mut input = String::new();
        for i in 0..6 {
            input.push_str(&format!("body {}\nEND\n", i));
        }
        let chunks = chunk_all(&mut chunker, &input);
        assert_eq!(chunks.len(), 6, "end= terminated each event");
        assert!(!chunker.collapsed_without_boundary());
    }

    #[test]
    fn tiny_input_without_boundaries_stays_quiet() {
        // A one-record input has nothing to split; nagging there is noise.
        let mut chunker =
            MultilineChunker::new(config(timestamp_strategy(), MultilineJoin::Newline)).unwrap();
        let chunks = chunk_all(&mut chunker, "no timestamp here\nnor here\n");
        assert_eq!(chunks.len(), 1);
        assert!(
            !chunker.collapsed_without_boundary(),
            "below the line floor"
        );
    }

    #[test]
    fn all_and_presets_never_report_a_collapse() {
        // `all` is asked for exactly one event, and a preset legitimately finds
        // no boundary when the whole input is a single stack trace.
        let long = (0..20).map(|i| format!("line {}\n", i)).collect::<String>();

        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::All, MultilineJoin::Newline)).unwrap();
        let _ = chunk_all(&mut chunker, &long);
        assert!(!chunker.collapsed_without_boundary(), "`all` is exempt");

        let mut trace = String::from("Traceback (most recent call last):\n");
        for i in 0..12 {
            trace.push_str(&format!("  File \"m.py\", line {}, in f\n", i));
        }
        trace.push_str("ValueError: boom\n");
        let mut chunker = MultilineChunker::new(config(
            MultilineStrategy::Preset(crate::config::TracePreset::Python),
            MultilineJoin::Newline,
        ))
        .unwrap();
        let chunks = chunk_all(&mut chunker, &trace);
        assert_eq!(chunks.len(), 1, "one traceback, one event");
        assert!(
            !chunker.collapsed_without_boundary(),
            "presets are exempt: a piped crash dump is not a mistake"
        );
    }

    /// The cap is the safety net, not the rule: splitting on it must not mask a
    /// pattern that never matched. Both advisories fire, and together they
    /// diagnose the never-matching pattern.
    #[test]
    fn cap_splits_do_not_count_as_boundaries() {
        let mut cfg = config(
            MultilineStrategy::Regex {
                start: "^NEVER".to_string(),
                end: None,
            },
            MultilineJoin::Newline,
        );
        cfg.max_lines = 4;
        let mut chunker = MultilineChunker::new(cfg).unwrap();

        let input = (0..20).map(|i| format!("line {}\n", i)).collect::<String>();
        let chunks = chunk_all(&mut chunker, &input);
        assert_eq!(chunks.len(), 5, "cap split the run into fixed slices");
        assert!(chunker.take_cap_hit());
        assert!(
            chunker.collapsed_without_boundary(),
            "cap splits are not the rule firing"
        );
    }

    #[test]
    fn all_strategy_ignores_cap() {
        let mut cfg = config(MultilineStrategy::All, MultilineJoin::Newline);
        cfg.max_lines = 2;
        let mut chunker = MultilineChunker::new(cfg).unwrap();

        let chunks = chunk_all(&mut chunker, "a\nb\nc\nd\n");
        assert_eq!(chunks.len(), 1);
        assert!(!chunker.take_cap_hit());
    }

    #[test]
    fn test_all_strategy_joins_with_newlines() {
        // Default join for `all` is newline (set by config parsing); here the
        // config is constructed directly, so pass it explicitly.
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::All, MultilineJoin::Newline)).unwrap();

        let chunks = chunk_all(&mut chunker, "line1\nline2\nline3\n");
        assert_eq!(texts(&chunks), vec!["line1\nline2\nline3"]);
    }

    #[test]
    fn all_strategy_honors_explicit_join() {
        // --multiline all --multiline-join=space used to be silently ignored.
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::All, MultilineJoin::Space)).unwrap();

        let chunks = chunk_all(&mut chunker, "line1\nline2\n");
        assert_eq!(texts(&chunks), vec!["line1 line2"]);
    }

    #[test]
    fn all_strategy_keeps_trailing_blanks() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::All, MultilineJoin::Newline)).unwrap();
        let chunks = chunk_all(&mut chunker, "a\n\n");
        assert_eq!(texts(&chunks), vec!["a\n"]);
    }

    #[test]
    fn test_flush_empty_buffer() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Space)).unwrap();
        let mut out = Vec::new();
        chunker.flush(&mut out);
        chunker.flush(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn test_has_pending() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Space)).unwrap();
        assert!(!chunker.has_pending());

        let mut out = Vec::new();
        chunker.feed_line(
            ChunkLine {
                text: "test".to_string(),
                line_num: 1,
                filename: None,
            },
            &mut out,
        );
        assert!(chunker.has_pending());

        chunker.flush(&mut out);
        assert!(!chunker.has_pending());
    }

    #[test]
    fn provenance_tracks_first_line_and_filename() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Space)).unwrap();
        let mut out = Vec::new();
        chunker.feed_line(
            ChunkLine {
                text: "Header".to_string(),
                line_num: 41,
                filename: Some("a.log".to_string()),
            },
            &mut out,
        );
        chunker.feed_line(
            ChunkLine {
                text: "  cont".to_string(),
                line_num: 42,
                filename: Some("a.log".to_string()),
            },
            &mut out,
        );
        chunker.flush(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].first_line_num, 41);
        assert_eq!(out[0].filename.as_deref(), Some("a.log"));
        assert_eq!(out[0].line_count, 2);
    }

    #[test]
    fn test_very_large_multiline_event() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Space)).unwrap();

        let mut input = String::from("Header\n");
        for i in 0..1000 {
            input.push_str(&format!("  line {}\n", i));
        }
        let chunks = chunk_all(&mut chunker, &input);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("Header"));
        assert!(chunks[0].text.contains("line 999"));
    }

    #[test]
    fn test_timestamp_strategy_without_format_hint() {
        let mut chunker =
            MultilineChunker::new(config(timestamp_strategy(), MultilineJoin::Space)).unwrap();

        let chunks = chunk_all(
            &mut chunker,
            "2024-01-01T10:00:00 First\ncontinuation\n2024-01-01T10:00:01 Second\n",
        );
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_is_line_indented() {
        assert!(is_line_indented("  indented\n"));
        assert!(is_line_indented("\tindented\n"));
        assert!(!is_line_indented("not indented\n"));
        assert!(!is_line_indented(""));
        assert!(!is_line_indented("\n"));
    }

    #[test]
    fn test_timestamp_prefix_candidates() {
        let line = "2024-01-01 10:00:00 INFO message";
        let candidates = timestamp_prefix_candidates(line);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.contains("2024-01-01")));
    }

    #[test]
    fn test_timestamp_prefix_candidates_empty() {
        let candidates = timestamp_prefix_candidates("");
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_timestamp_prefix_candidates_no_whitespace() {
        let candidates = timestamp_prefix_candidates("singletoken");
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_timestamp_prefix_candidates_long_line() {
        let long_line = format!("{}start", "x".repeat(100));
        let candidates = timestamp_prefix_candidates(&long_line);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_take_prefix_chars() {
        assert_eq!(take_prefix_chars("hello world", 5), "hello");
        assert_eq!(take_prefix_chars("hello", 10), "hello");
        assert_eq!(take_prefix_chars("hello", 0), "");
        assert_eq!(take_prefix_chars("", 5), "");
    }

    #[test]
    fn test_take_prefix_chars_unicode() {
        assert_eq!(take_prefix_chars("日本語test", 3), "日本語");
    }

    #[test]
    fn test_push_candidate_deduplication() {
        let mut candidates = Vec::new();
        push_candidate(&mut candidates, "test".to_string());
        push_candidate(&mut candidates, "test".to_string());
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn test_push_candidate_empty() {
        let mut candidates = Vec::new();
        push_candidate(&mut candidates, "".to_string());
        assert_eq!(candidates.len(), 0);
    }

    #[test]
    fn test_invalid_regex_pattern() {
        let result = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: r"[invalid(".to_string(),
                end: None,
            },
            MultilineJoin::Space,
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_end_regex_pattern() {
        let result = MultilineChunker::new(config(
            MultilineStrategy::Regex {
                start: r"^START".to_string(),
                end: Some(r"[invalid(".to_string()),
            },
            MultilineJoin::Space,
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_timestamp_detector_empty_line() {
        let mut detector = TimestampDetector::new(None, false);
        assert!(!detector.is_header(""));
        assert!(!detector.is_header("   \n"));
    }

    #[test]
    fn test_timestamp_detector_indented_line() {
        let mut detector = TimestampDetector::new(None, false);
        assert!(!detector.is_header("  2024-01-01 test"));
    }

    #[test]
    fn test_timestamp_detector_valid_timestamp() {
        let mut detector = TimestampDetector::new(None, false);
        assert!(detector.is_header("2024-01-01T10:00:00 message"));
    }

    #[test]
    fn test_multiline_join_empty_removes_line_breaks() {
        let mut chunker =
            MultilineChunker::new(config(MultilineStrategy::Indent, MultilineJoin::Empty)).unwrap();

        let chunks = chunk_all(&mut chunker, "Header\n  continuation\n");
        assert_eq!(texts(&chunks), vec!["Header  continuation"]);
    }

    #[test]
    fn test_multiline_join_space_inserts_separator() {
        // Regression: `Space` must join lines with a single space between
        // them, not silently concatenate like `Empty`. Lines reach the chunker
        // with trailing newlines already stripped by the reader.
        let join_indented = |join| {
            let mut chunker =
                MultilineChunker::new(config(MultilineStrategy::Indent, join)).unwrap();
            let chunks = chunk_all(&mut chunker, "Header\n  continuation\n");
            chunks.into_iter().next().expect("buffered event").text
        };

        // One joiner space, then the continuation's own two-space indent.
        assert_eq!(join_indented(MultilineJoin::Space), "Header   continuation");
        // Space must differ from Empty (the bug made them identical).
        assert_eq!(join_indented(MultilineJoin::Empty), "Header  continuation");
    }

    #[test]
    fn test_create_multiline_chunker_function() {
        let result =
            create_multiline_chunker(&config(MultilineStrategy::Indent, MultilineJoin::Space));
        assert!(result.is_ok());
    }

    mod presets {
        use super::*;
        use crate::config::TracePreset;

        fn preset_chunker(preset: TracePreset) -> MultilineChunker {
            MultilineChunker::new(config(
                MultilineStrategy::Preset(preset),
                MultilineJoin::Newline,
            ))
            .expect("preset chunker builds")
        }

        #[test]
        fn python_traceback_attaches_to_logging_header() {
            // `logger.exception(...)` output: the header line is followed by
            // the traceback; both belong to one event, the neighbors stay
            // single-line events.
            let mut chunker = preset_chunker(TracePreset::Python);
            let chunks = chunk_all(
                &mut chunker,
                "starting worker\nrequest failed\nTraceback (most recent call last):\n  File \"/app/m.py\", line 3, in run\n    handle()\nValueError: boom\nnext request\n",
            );
            assert_eq!(
                texts(&chunks),
                vec![
                    "starting worker",
                    "request failed\nTraceback (most recent call last):\n  File \"/app/m.py\", line 3, in run\n    handle()\nValueError: boom",
                    "next request"
                ]
            );
            assert_eq!(chunks[1].first_line_num, 2);
            assert_eq!(chunks[1].line_count, 5);
        }

        #[test]
        fn go_blank_separated_goroutine_blocks_stay_one_event() {
            let mut chunker = preset_chunker(TracePreset::Go);
            let chunks = chunk_all(
                &mut chunker,
                "panic: boom\n\ngoroutine 1 [running]:\nmain.main()\n\t/app/main.go:5 +0x20\n\ngoroutine 7 [select]:\nmain.worker()\n\t/app/w.go:9 +0x11\nrestarted\n",
            );
            assert_eq!(chunks.len(), 2);
            assert!(chunks[0].text.contains("goroutine 7"));
            assert_eq!(chunks[1].text, "restarted");
        }

        #[test]
        fn timestamped_headers_split_events_and_trace_still_attaches() {
            // The intuitive-but-suboptimal case from the design discussion:
            // a preset applied to a timestamped log. Locked headers always
            // start events, the trace still glues to the header that logged
            // it, and after enough headers the hint flag is raised so the
            // driver can point at --multiline timestamp.
            let mut chunker = preset_chunker(TracePreset::Java);
            let chunks = chunk_all(
                &mut chunker,
                "2024-01-01T10:00:00 INFO started\n2024-01-01T10:00:01 ERROR sync failed\njava.lang.IllegalStateException: boom\n\tat com.example.Foo.bar(Foo.java:10)\nCaused by: java.lang.NullPointerException\n\t... 3 more\n2024-01-01T10:00:02 INFO recovered\n2024-01-01T10:00:03 INFO steady\n",
            );
            assert_eq!(chunks.len(), 4);
            assert!(chunks[1].text.contains("ERROR sync failed"));
            assert!(chunks[1].text.contains("... 3 more"));
            assert_eq!(chunks[2].text, "2024-01-01T10:00:02 INFO recovered");
            assert!(
                chunker.take_preset_ts_hint(),
                "three headers reached the hint threshold"
            );
            assert!(!chunker.take_preset_ts_hint(), "hint flag is take-once");
        }

        #[test]
        fn headerless_input_never_raises_the_ts_hint() {
            let mut chunker = preset_chunker(TracePreset::Python);
            let _ = chunk_all(
                &mut chunker,
                "one plain line\nanother\nTraceback (most recent call last):\n  File \"x.py\", line 1, in <module>\nValueError: x\nmore prose\n",
            );
            assert!(!chunker.take_preset_ts_hint());
        }

        #[test]
        fn flush_resets_trace_state_across_boundaries() {
            // A file boundary flush must not let a trace continue into the
            // next file: the blank line after the flush is no longer a Go
            // trace continuation and produces no junk grouping.
            let mut chunker = preset_chunker(TracePreset::Go);
            let mut out = Vec::new();
            for (idx, line) in ["panic: boom", "goroutine 1 [running]:"].iter().enumerate() {
                chunker.feed_line(
                    ChunkLine {
                        text: line.to_string(),
                        line_num: idx + 1,
                        filename: Some("a.log".to_string()),
                    },
                    &mut out,
                );
            }
            chunker.flush(&mut out); // file boundary
            chunker.feed_line(
                ChunkLine {
                    text: "\t/app/main.go:5 +0x20".to_string(),
                    line_num: 1,
                    filename: Some("b.log".to_string()),
                },
                &mut out,
            );
            chunker.feed_line(
                ChunkLine {
                    text: "plain".to_string(),
                    line_num: 2,
                    filename: Some("b.log".to_string()),
                },
                &mut out,
            );
            chunker.flush(&mut out);
            let texts: Vec<&str> = out.iter().map(|c| c.text.as_str()).collect();
            assert_eq!(
                texts,
                vec![
                    "panic: boom\ngoroutine 1 [running]:",
                    "\t/app/main.go:5 +0x20",
                    "plain"
                ],
                "the tab line after the boundary is not glued to the flushed trace"
            );
        }

        #[test]
        fn preset_respects_line_cap() {
            let mut cfg = config(
                MultilineStrategy::Preset(TracePreset::Go),
                MultilineJoin::Newline,
            );
            cfg.max_lines = 3;
            let mut chunker = MultilineChunker::new(cfg).unwrap();
            let chunks = chunk_all(
                &mut chunker,
                "panic: boom\n\ngoroutine 1 [running]:\nmain.main()\n\t/app/main.go:5 +0x20\n",
            );
            assert!(chunks.len() >= 2, "cap splits the oversized trace");
            assert!(chunker.take_cap_hit());
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::config::TracePreset;
    use proptest::prelude::*;

    /// A soup of the line shapes that exercise every boundary rule: headers,
    /// indented continuations, blanks, prose, start/end markers, and
    /// stack-trace fragments for the language presets.
    fn arb_line() -> impl Strategy<Value = String> {
        prop_oneof![
            (1u8..9, 0u8..9).prop_map(|(d, s)| format!("2024-01-0{}T10:00:0{} msg", d, s)),
            (0u32..50).prop_map(|n| format!("  at frame{}", n)),
            Just(String::new()),
            Just("   ".to_string()),
            (0u32..50).prop_map(|n| format!("word{} plain 17:03", n)),
            Just("START block".to_string()),
            Just("middle END".to_string()),
            Just("Traceback (most recent call last):".to_string()),
            (0u32..9).prop_map(|n| format!("  File \"m.py\", line {}, in f", n)),
            Just("ValueError: boom".to_string()),
            Just("java.lang.IllegalStateException: boom".to_string()),
            (0u32..9).prop_map(|n| format!("\tat com.example.A.b(A.java:{})", n)),
            Just("Caused by: java.lang.Error: e".to_string()),
            Just("panic: kaboom".to_string()),
            (1u32..9).prop_map(|n| format!("goroutine {} [running]:", n)),
            Just("main.main()".to_string()),
            Just("\t/app/main.go:5 +0x20".to_string()),
        ]
    }

    fn strategies_under_test() -> Vec<MultilineStrategy> {
        vec![
            MultilineStrategy::Indent,
            MultilineStrategy::Blank,
            MultilineStrategy::Timestamp {
                chrono_format: None,
                loose: false,
            },
            MultilineStrategy::Timestamp {
                chrono_format: None,
                loose: true,
            },
            MultilineStrategy::Regex {
                start: "^START".to_string(),
                end: None,
            },
            MultilineStrategy::Regex {
                start: "^START".to_string(),
                end: Some("END$".to_string()),
            },
            MultilineStrategy::All,
            MultilineStrategy::Preset(TracePreset::Java),
            MultilineStrategy::Preset(TracePreset::Python),
            MultilineStrategy::Preset(TracePreset::Go),
        ]
    }

    fn chunk_lines(cfg: MultilineConfig, lines: &[String]) -> Vec<Chunk> {
        let mut chunker = MultilineChunker::new(cfg).expect("chunker builds");
        let mut out = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            chunker.feed_line(
                ChunkLine {
                    text: line.clone(),
                    line_num: idx + 1,
                    filename: None,
                },
                &mut out,
            );
        }
        chunker.flush(&mut out);
        out
    }

    proptest! {
        /// Losslessness: no strategy (at any cap) may drop, duplicate, or
        /// reorder a non-blank input line. Blank lines are the one documented
        /// exception (trailing-blank trim, blank-strategy separators), so the
        /// invariant compares the non-blank sequences.
        #[test]
        fn no_nonblank_line_lost_duplicated_or_reordered(
            lines in proptest::collection::vec(arb_line(), 0..60),
            cap in 0usize..6,
        ) {
            for strategy in strategies_under_test() {
                let cfg = MultilineConfig {
                    strategy: strategy.clone(),
                    join: MultilineJoin::Newline,
                    max_lines: cap,
                    idle_timeout: None,
                    idle_timeout_explicit: false,
                };
                let got: Vec<String> = chunk_lines(cfg, &lines)
                    .iter()
                    .flat_map(|c| c.text.split('\n'))
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.to_string())
                    .collect();
                let expected: Vec<String> = lines
                    .iter()
                    .filter(|l| !l.trim().is_empty())
                    .cloned()
                    .collect();
                prop_assert_eq!(got, expected, "strategy {:?} cap {}", strategy, cap);
            }
        }

        /// `all` is byte-faithful: one record reproducing every line,
        /// blanks included, regardless of the cap.
        #[test]
        fn all_strategy_is_lossless_bytewise(
            lines in proptest::collection::vec(arb_line(), 1..40),
            cap in 0usize..6,
        ) {
            let cfg = MultilineConfig {
                strategy: MultilineStrategy::All,
                join: MultilineJoin::Newline,
                max_lines: cap,
                idle_timeout: None,
                idle_timeout_explicit: false,
            };
            let chunks = chunk_lines(cfg, &lines);
            prop_assert_eq!(chunks.len(), 1);
            prop_assert_eq!(&chunks[0].text, &lines.join("\n"));
        }

        /// Provenance: every record's first_line_num sequence is strictly
        /// increasing and each record's line_count sums to the total number of
        /// consumed physical lines (except blank-strategy separators, which
        /// belong to no record).
        #[test]
        fn provenance_is_ordered_and_consistent(
            lines in proptest::collection::vec(arb_line(), 0..60),
        ) {
            for strategy in strategies_under_test() {
                let is_blank_strategy = matches!(strategy, MultilineStrategy::Blank);
                let cfg = MultilineConfig {
                    strategy,
                    join: MultilineJoin::Newline,
                    max_lines: 0,
                    idle_timeout: None,
                    idle_timeout_explicit: false,
                };
                let chunks = chunk_lines(cfg, &lines);
                let mut prev_first = 0usize;
                let mut consumed = 0usize;
                for chunk in &chunks {
                    prop_assert!(chunk.first_line_num > prev_first);
                    prev_first = chunk.first_line_num;
                    consumed += chunk.line_count;
                }
                if !is_blank_strategy {
                    // Every non-blank line is consumed by exactly one record;
                    // only blank lines may fall between records (blanks-only
                    // buffers flush to nothing).
                    let nonblank = lines.iter().filter(|l| !l.trim().is_empty()).count();
                    prop_assert!(consumed >= nonblank && consumed <= lines.len());
                }
            }
        }
    }
}
