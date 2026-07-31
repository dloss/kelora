//! Format auto-detection and detection notice handling
//!
//! This module handles detecting input format from file content
//! and displaying appropriate notices to users.

use anyhow::Result;
use std::fs;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{self, KeloraConfig};
use crate::decompression;
use crate::parsers;
use crate::pipeline;
use crate::readers;
use crate::stats;
use crate::stats::ProcessingStats;

/// Marker error returned by the auto-detection paths when *every* input path
/// failed to open and the per-file reasons (`Failed to open file '…'` /
/// `Input path '…' is a directory; skipping`) were already written to stderr in
/// detail. The top-level error handler recognizes it and skips the otherwise
/// redundant generic `Pipeline error: …` line, while still exiting non-zero.
/// Only returned once detail has actually been printed, so suppressing it never
/// leaves a silent failure; any other error still prints normally. Its `Display`
/// is a sensible fallback for any consumer that does print it.
#[derive(Debug)]
pub struct AllInputsUnopenable;

impl std::fmt::Display for AllInputsUnopenable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no input files could be opened")
    }
}

impl std::error::Error for AllInputsUnopenable {}

/// How much of an input's head informs sampled (multi-line) detection. Only
/// file inputs are sampled: a file delivers its head promptly and EOF bounds
/// short files, whereas stdin can be a live pipe where waiting for more than
/// the first line would stall the stream — stdin keeps first-line detection.
pub const SAMPLE_MAX_LINES: usize = 64;
/// Byte cap on the sampled head, so a file of enormous lines doesn't buffer
/// unbounded memory during detection.
pub const SAMPLE_MAX_BYTES: usize = 256 * 1024;

/// Multi-offset probing: in addition to the head sample, plain (uncompressed,
/// seekable) files get a few line windows sampled from deeper in the file —
/// at 1/4, 1/2, 3/4 of the size, plus a window near the end — so a format
/// change partway through (concatenated rotations, a service that switched
/// from plaintext to JSON mid-file) is still seen. Head-only sampling can't
/// catch those by construction. gzip/zstd inputs aren't seekable in the
/// decompressed domain, so they keep head-only sampling.
///
/// Files smaller than this aren't worth probing: their probe windows would
/// largely overlap the head sample.
const PROBE_MIN_FILE_BYTES: u64 = 32 * 1024;
/// Byte budget per probe window; also the amount by which a probe read is
/// hard-limited (`Read::take`), so one enormous line can't buffer unbounded.
const PROBE_WINDOW_BYTES: u64 = 64 * 1024;
/// Non-empty lines sampled per probe window.
const PROBE_MAX_LINES: usize = 16;
/// The tail probe starts this many bytes before EOF, so it samples the actual
/// last lines of the file rather than a window 64 KiB short of them.
const PROBE_TAIL_BYTES: u64 = 8 * 1024;

/// Result of format detection
#[derive(Debug, Clone)]
pub struct DetectedFormat {
    pub format: config::InputFormat,
    /// The reader produced at least one line — including a blank one.
    pub had_input: bool,
    /// Detection was based on a real non-empty line, rather than falling back to
    /// `line` because the input ran out. Multi-file detection uses this to keep
    /// scanning past inputs that hold nothing to detect from.
    pub saw_content: bool,
    /// How many non-empty lines from the input's head informed the decision
    /// (1 for first-line detection, up to `SAMPLE_MAX_LINES` for sampled
    /// detection). Only used to word the `-v` notice.
    pub sample_lines: usize,
    /// How many additional lines came from mid-file probe windows (0 when the
    /// input wasn't probed: stdin, compressed, or small files). Only used to
    /// word the `-v` notice.
    pub probe_lines: usize,
}

impl DetectedFormat {
    /// Returns true if a non-line format was detected
    pub fn detected_non_line(&self) -> bool {
        self.had_input && !matches!(self.format, config::InputFormat::Line)
    }

    /// Returns true if detection fell back to line format
    pub fn fell_back_to_line(&self) -> bool {
        self.had_input && matches!(self.format, config::InputFormat::Line)
    }
}

/// Detect format from a peekable reader
/// Returns the detected format without consuming the first line
pub fn detect_format_from_peekable_reader<R: std::io::BufRead>(
    reader: &mut readers::PeekableLineReader<R>,
) -> Result<DetectedFormat> {
    match reader.peek_first_non_empty_line()? {
        None => Ok(DetectedFormat {
            format: config::InputFormat::Line,
            had_input: reader.saw_any_input(),
            saw_content: false,
            sample_lines: 0,
            probe_lines: 0,
        }),
        Some(line) => {
            // Remove newline for detection
            let trimmed_line = line.trim_end_matches(&['\r', '\n'][..]);
            let detected = parsers::detect_format(trimmed_line)?;
            Ok(DetectedFormat {
                format: detected,
                had_input: true,
                saw_content: true,
                sample_lines: 1,
                probe_lines: 0,
            })
        }
    }
}

/// Detect format from a peekable reader by sampling up to `SAMPLE_MAX_LINES`
/// non-empty lines (capped at `SAMPLE_MAX_BYTES`) instead of just the first.
///
/// A homogeneous sample detects exactly like `detect_format_from_peekable_reader`;
/// a mixed sample yields a `Cascade` over the detected formats (see
/// `parsers::detect_format_from_sample`). Used for *file* inputs only — stdin
/// may be a live pipe, where blocking on a 64-line sample would stall
/// `tail -f`-style streaming, so it keeps first-line detection.
///
/// When `probe_path` names the underlying file, plain uncompressed files of at
/// least `PROBE_MIN_FILE_BYTES` additionally get mid-file probe windows (see
/// `probe_file_offsets`) appended to the sample, so a format change beyond the
/// head is still detected. Probe lines are appended *after* the head lines:
/// the whole-file csv/tsv rule keys off the file's true first line.
pub fn detect_format_from_peekable_reader_sampled<R: std::io::BufRead>(
    reader: &mut readers::PeekableLineReader<R>,
    probe_path: Option<&str>,
) -> Result<DetectedFormat> {
    let head_lines = reader.peek_nonempty_lines(SAMPLE_MAX_LINES, SAMPLE_MAX_BYTES)?;
    if head_lines.is_empty() {
        return Ok(DetectedFormat {
            format: config::InputFormat::Line,
            had_input: reader.saw_any_input(),
            saw_content: false,
            sample_lines: 0,
            probe_lines: 0,
        });
    }
    let probe_lines = probe_path.map(probe_file_offsets).unwrap_or_default();
    let trimmed: Vec<&str> = head_lines
        .iter()
        .chain(probe_lines.iter())
        .map(|line| line.trim_end_matches(&['\r', '\n'][..]))
        .collect();
    let detected = parsers::detect_format_from_sample(trimmed.iter().copied())?;
    Ok(DetectedFormat {
        format: detected,
        had_input: true,
        saw_content: true,
        sample_lines: head_lines.len(),
        probe_lines: probe_lines.len(),
    })
}

/// Sample a few line windows from deeper inside a plain file: at 1/4, 1/2 and
/// 3/4 of its size, plus a window covering the last `PROBE_TAIL_BYTES`.
///
/// Best-effort by design: probing only ever *adds* sample lines on top of an
/// already-successful head detection, so any condition that makes probing
/// unsafe or useless — not a regular file, too small, gzip/zstd magic bytes
/// (byte offsets in the compressed stream are meaningless for the decompressed
/// lines), or an I/O error mid-probe — degrades to fewer or no probe lines
/// rather than an error.
fn probe_file_offsets(path: &str) -> Vec<String> {
    use std::io::Read;

    let Ok(metadata) = fs::metadata(path) else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() < PROBE_MIN_FILE_BYTES {
        return Vec::new();
    }
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut magic = [0u8; 4];
    let Ok(n) = file.read(&mut magic) else {
        return Vec::new();
    };
    if decompression::looks_compressed(&magic[..n]) {
        return Vec::new();
    }

    let size = metadata.len();
    let offsets = [
        size / 4,
        size / 2,
        size / 4 * 3,
        size.saturating_sub(PROBE_TAIL_BYTES),
    ];
    let mut lines = Vec::new();
    for offset in offsets {
        match probe_lines_at(&mut file, offset) {
            Ok(mut probed) => lines.append(&mut probed),
            Err(_) => break,
        }
    }
    lines
}

/// Read up to `PROBE_MAX_LINES` non-empty lines from a window starting at
/// `offset`, hard-limited to `PROBE_WINDOW_BYTES` bytes.
///
/// The (almost certainly partial) line the offset lands in is skipped, and a
/// final line cut off by the window/EOF without its `\n` is dropped: a
/// truncated line could re-detect as a different format (a JSON line cut in
/// half reads as `line`) and pollute the cascade with a member the file
/// doesn't actually contain.
fn probe_lines_at(file: &mut fs::File, offset: u64) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};

    file.seek(SeekFrom::Start(offset))?;
    let mut window = std::io::BufReader::new(file.by_ref().take(PROBE_WINDOW_BYTES));

    let mut buf = Vec::new();
    if offset > 0 {
        // Skip the partial line the seek landed in. If no newline shows up in
        // the whole window (one enormous line), there is nothing to sample.
        buf.clear();
        if window.read_until(b'\n', &mut buf)? == 0 || !buf.ends_with(b"\n") {
            return Ok(Vec::new());
        }
    }

    let mut lines = Vec::new();
    while lines.len() < PROBE_MAX_LINES {
        buf.clear();
        if window.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        if !buf.ends_with(b"\n") {
            // Cut off by the window cap (or a missing trailing newline at
            // EOF); can't tell truncation from completeness, so drop it.
            break;
        }
        let line = String::from_utf8_lossy(&buf).into_owned();
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    Ok(lines)
}

/// Detect the input format by scanning `sorted_files` in order, using the first
/// file that actually contains a non-empty line.
///
/// A file that opens but holds nothing to detect from — completely empty, or
/// blank lines only — does *not* end the scan. `-f auto` is documented as
/// detecting "from the first non-empty line", so a leading empty file (a
/// freshly rotated log, say) must not pin every later file to `line`: that
/// silently reduced whole JSON files to `line='{"…"}'` with no diagnostic.
///
/// If no file has content the scan falls back to `line`, carrying `had_input`
/// forward from the files it did read so the "fell back to line" hint still
/// fires for blank-lines-only input.
///
/// Open failures and directories are collected and only reported if *no* file
/// could be read at all; otherwise the regular readers reopen those paths and
/// report them, and reporting here too would duplicate the message.
pub fn detect_format_from_files(sorted_files: &[String], strict: bool) -> Result<DetectedFormat> {
    let mut failed_opens: Vec<(String, String)> = Vec::new();
    let mut failed_dirs: Vec<String> = Vec::new();
    let mut detected: Option<DetectedFormat> = None;
    let mut empty_fallback: Option<DetectedFormat> = None;

    for file_path in sorted_files {
        if let Ok(metadata) = fs::metadata(file_path) {
            if metadata.is_dir() {
                if strict {
                    return Err(anyhow::anyhow!(
                        "Input path '{}' is a directory; only files are supported",
                        file_path
                    ));
                }
                failed_dirs.push(file_path.clone());
                continue;
            }
        }

        match decompression::DecompressionReader::new(file_path) {
            Ok(decompressed) => {
                let mut peekable_reader = readers::PeekableLineReader::new(decompressed);
                // Files get sampled detection: the head arrives promptly (unlike
                // a live stdin pipe) and the reader is discarded afterwards —
                // regular readers reopen the path for actual processing. The
                // path is passed along so plain files also get mid-file probes.
                let candidate = detect_format_from_peekable_reader_sampled(
                    &mut peekable_reader,
                    Some(file_path),
                )?;
                if candidate.saw_content {
                    detected = Some(candidate);
                    break;
                }
                // Nothing to detect from here — keep scanning, but remember that
                // we did read something so the hint stays accurate.
                match &mut empty_fallback {
                    Some(fallback) => fallback.had_input |= candidate.had_input,
                    None => empty_fallback = Some(candidate),
                }
            }
            Err(e) => {
                if strict {
                    return Err(anyhow::anyhow!(config::format_input_open_error(
                        file_path,
                        &e.to_string()
                    )));
                }
                failed_opens.push((file_path.clone(), e.to_string()));
            }
        }
    }

    if let Some(detected) = detected.or(empty_fallback) {
        return Ok(detected);
    }

    let printed_detail = !failed_dirs.is_empty() || !failed_opens.is_empty();
    for path in failed_dirs {
        eprintln!(
            "{}",
            config::format_error_message_auto(&format!(
                "Input path '{}' is a directory; skipping (input files only)",
                path
            ))
        );
        stats::stats_file_open_failed(&path);
    }
    for (path, err) in failed_opens {
        eprintln!(
            "{}",
            config::format_error_message_auto(&config::format_input_open_error(&path, &err))
        );
        stats::stats_file_open_failed(&path);
    }
    // The per-file reasons above already say which inputs failed and why, so
    // don't repeat a generic line. Fall back to the explicit message only if
    // nothing was printed (shouldn't happen — the loop routes every path to one
    // of the lists above).
    if printed_detail {
        return Err(anyhow::Error::new(AllInputsUnopenable));
    }
    Err(anyhow::anyhow!(
        "Failed to open any input files for detection"
    ))
}

/// Detect format for parallel mode processing
/// Returns the detected format and optionally a reader to reuse for stdin
pub fn detect_format_for_parallel_mode(
    files: &[String],
    no_input: bool,
    strict: bool,
) -> Result<(DetectedFormat, Option<Box<dyn BufRead + Send>>)> {
    use std::io;

    if no_input {
        // For --no-input mode, default to Line format
        return Ok((
            DetectedFormat {
                format: config::InputFormat::Line,
                had_input: false,
                saw_content: false,
                sample_lines: 0,
                probe_lines: 0,
            },
            None,
        ));
    }

    if files.is_empty() {
        // For stdin with potential gzip/zstd, handle decompression first
        let stdin_reader = readers::ChannelStdinReader::new()?;
        let processed_stdin = decompression::maybe_decompress(stdin_reader)?;
        let mut peekable_reader =
            readers::PeekableLineReader::new(io::BufReader::new(processed_stdin));

        let detected = detect_format_from_peekable_reader(&mut peekable_reader)?;

        // Reuse the peekable reader so we don't consume stdin twice
        Ok((detected, Some(Box::new(peekable_reader))))
    } else {
        // For files, detect from the first file that actually has content.
        let sorted_files = pipeline::builders::sort_files(files, &config::FileOrder::Cli)?;
        let detected = detect_format_from_files(&sorted_files, strict)?;

        // For files we can reopen them later, so we don't need to keep this reader
        Ok((detected, None))
    }
}

/// Format a notice about detected format
pub fn format_detected_format_notice(
    config: &KeloraConfig,
    detected: &DetectedFormat,
) -> Option<String> {
    if detected.detected_non_line() {
        // "What kelora did" status (🔹). A *confident* auto-detection is not
        // surprising, so a successful run stays silent (Rule of Silence) and this
        // line surfaces only under -v/--verbose. `verbose` is forced to 0 by
        // --silent / --no-diagnostics (see config.rs), so this single check also
        // covers those cases.
        if config.processing.verbose == 0 {
            return None;
        }
        let format_name = detected.format.to_display_string();
        let sampled = if detected.probe_lines > 0 {
            format!(
                "first {} + {} mid-file lines",
                detected.sample_lines, detected.probe_lines
            )
        } else {
            format!("first {} lines", detected.sample_lines)
        };
        let provenance = if detected.format.is_cascade() {
            format!("mixed formats in {}", sampled)
        } else if detected.sample_lines > 1 || detected.probe_lines > 0 {
            format!("from {}", sampled)
        } else {
            "from first line".to_string()
        };
        let message = config.format_info_message(&format!(
            "Auto-detected format: {} ({})",
            format_name, provenance
        ));
        Some(message)
    } else if detected.fell_back_to_line() {
        // Advisory hint (💡): obeys --no-hints / --silent like every other hint,
        // and surfaces even when stderr is redirected (no terminal gate) — the
        // "I fell back to whole-line parsing" notice is exactly what someone
        // exploring an unknown file in a pipe wants to see. `--discover` does
        // not hush it (#343); see `format_fallback_hint_allowed`.
        if !config.format_fallback_hint_allowed() {
            return None;
        }
        let message = config.format_hint_message(
            "No input format detected; keeping whole lines as 'line'. For 'timestamp LEVEL message' app logs, extract fields with -f 'cols:ts(2) level *msg' (or a regex:). Mixed file? Cascade with repeated -f, e.g. -f json -f 'cols:ts(2) level *msg'. See --help-formats.",
        );
        Some(message)
    } else {
        None
    }
}

/// Tracks whether the "no input format detected" hint already went out this run.
///
/// Detection is per-file in `auto-per-file` mode, so a run over N unstructured
/// files used to print N copies of the same paragraph. The advice is identical
/// every time and does not name the file, so once is enough. Only the hint is
/// deduplicated — the `-v` "Auto-detected format: X" status is per-file *by
/// design* (it says what each file was read as, which differs between files).
static FALLBACK_HINT_EMITTED: AtomicBool = AtomicBool::new(false);

/// Emit a notice about detected format to stderr
pub fn emit_detected_format_notice(config: &KeloraConfig, detected: &DetectedFormat) {
    if let Some(message) = format_detected_format_notice(config, detected) {
        if detected.fell_back_to_line() && FALLBACK_HINT_EMITTED.swap(true, Ordering::Relaxed) {
            return;
        }
        eprintln!("{}", message);
    }
}

/// Extract a counter value from tracking data
/// Format a warning message about parse failures
pub fn parse_failure_warning_message(
    config: &KeloraConfig,
    stats: Option<&ProcessingStats>,
    auto_detected_non_line: bool,
    events_were_output: bool,
) -> Option<String> {
    // A warning (🔸): obeys --no-warnings / --silent only. Unlike the info
    // notice it carries no terminal gate, so "parsing mostly failed" reaches a
    // stuck user even when stderr is redirected to a file or captured by CI.
    if !auto_detected_non_line || !config.warnings_allowed() {
        return None;
    }

    let stats = stats?;
    let parse_errors = stats.lines_errors as i64;
    let events_created = stats.events_created as i64;

    let seen = std::cmp::max(1, events_created + parse_errors);
    let should_warn = (parse_errors >= 10 && parse_errors * 3 >= seen)
        || (events_created == 0 && parse_errors >= 3);

    if should_warn {
        let text = mixed_format_suggestion(stats).unwrap_or_else(|| {
            "Parsing mostly failed. The input may use the wrong format, contain mixed formats, or require multiline parsing. Try -f line, specify -f <fmt>, or see --help-formats / --help-multiline.".to_string()
        });
        let mut message = config.format_warning_message(&text);
        if !events_were_output {
            message = message.trim_start_matches('\n').to_string();
        }
        Some(message)
    } else {
        None
    }
}

/// Build a format-specific "mixed formats" warning when auto-detection locked
/// onto one format but a sampled failing line looks like a *different* format.
///
/// This turns the otherwise generic "parsing mostly failed" notice into the
/// actionable hint the user actually needs, e.g.
/// `Detected mixed formats (json + line). Try: -f json,line`.
///
/// Returns `None` (so the caller falls back to the generic message) when we
/// can't confidently name a distinct secondary format.
fn mixed_format_suggestion(stats: &ProcessingStats) -> Option<String> {
    let primary = stats.detected_format.as_deref()?;
    let sample = stats.first_parse_error_sample.as_deref()?;

    // An auto-detected cascade is already the mixed-format remedy; suggesting
    // `-f cascade(json,line),X` would be invalid syntax. Let the generic
    // "parsing mostly failed" guidance handle any remaining failures.
    if primary.starts_with("cascade(") {
        return None;
    }

    // Re-detect the format of a line that the primary parser rejected.
    let secondary_fmt = parsers::detect_format(sample).ok()?;
    let secondary = secondary_fmt.cascade_name();

    // If the failing line re-detects as the same format, naming it adds nothing
    // (the line is just malformed) — let the generic message handle it.
    if secondary == primary {
        return None;
    }

    // The primary is an auto-detected non-line format; it's cascade-eligible
    // unless it is a schema-based format (csv/tsv variants).
    let primary_eligible = !matches!(primary, "csv" | "tsv" | "csvnh" | "tsvnh");

    if primary_eligible && secondary_fmt.is_cascade_eligible() {
        // Both fit in a comma cascade. A catch-all (line/raw) must come last;
        // the primary is never a catch-all here, so `primary,secondary` is
        // always validly ordered.
        Some(format!(
            "Detected mixed formats ({primary} + {secondary}). Try: -f {primary},{secondary} (see --help-formats)."
        ))
    } else {
        // One side can't go in a comma list (e.g. csv/tsv): suggest repeated -f.
        Some(format!(
            "Detected mixed formats ({primary} + {secondary}). These can't share a comma list; use repeated flags: -f {primary} -f {secondary} (see --help-formats)."
        ))
    }
}

/// Emit a warning about parse failures to stderr
pub fn emit_parse_failure_warning(
    config: &KeloraConfig,
    stats: Option<&ProcessingStats>,
    auto_detected_non_line: bool,
    events_were_output: bool,
) {
    if let Some(message) =
        parse_failure_warning_message(config, stats, auto_detected_non_line, events_were_output)
    {
        eprintln!("{}", message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorMode, EmojiMode};

    fn base_config() -> KeloraConfig {
        let mut cfg = KeloraConfig::default();
        cfg.output.emoji = EmojiMode::Never;
        cfg.output.color = ColorMode::Never;
        cfg.processing.quiet_events = false;
        cfg.processing.silent = false;
        cfg.processing.suppress_warnings = false;
        cfg.processing.suppress_hints = false;
        cfg
    }

    fn write_input(dir: &tempfile::TempDir, name: &str, contents: &str) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, contents).expect("failed to write test input");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn multi_file_detection_skips_files_without_content() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let empty = write_input(&dir, "empty.log", "");
        let blank = write_input(&dir, "blank.log", "\n\n");
        let json = write_input(&dir, "data.json", "{\"a\":1}\n");

        let detected = detect_format_from_files(&[empty, blank, json], false).expect("detection");

        assert!(
            matches!(detected.format, config::InputFormat::Json),
            "should detect from the first file with content, got {:?}",
            detected.format
        );
        assert!(detected.saw_content);
    }

    #[test]
    fn file_detection_samples_beyond_the_first_line() {
        // A mixed file (JSON with interleaved plain-text lines) must detect as
        // a cascade rather than pinning the whole file to the first line's
        // format — the head is sampled, not just line one.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mixed = write_input(
            &dir,
            "mixed.log",
            "{\"level\":\"info\",\"msg\":\"up\"}\nServer starting on port 8080\n{\"level\":\"error\",\"msg\":\"down\"}\n",
        );

        let detected = detect_format_from_files(&[mixed], false).expect("detection");

        match &detected.format {
            config::InputFormat::Cascade(members) => {
                assert_eq!(
                    members,
                    &vec![config::InputFormat::Json, config::InputFormat::Line]
                );
            }
            other => panic!("expected cascade(json,line), got {other:?}"),
        }
        assert_eq!(detected.sample_lines, 3);
        assert!(detected.detected_non_line());
    }

    /// Enough JSON lines to pass `PROBE_MIN_FILE_BYTES` on their own.
    fn json_block(lines: usize) -> String {
        (0..lines)
            .map(|i| {
                format!(
                    "{{\"level\":\"info\",\"msg\":\"padding padding padding padding\",\"seq\":{i}}}\n"
                )
            })
            .collect()
    }

    fn text_block(lines: usize) -> String {
        (0..lines)
            .map(|i| format!("plain text payload without any structure at all {i}\n"))
            .collect()
    }

    #[test]
    fn probing_detects_format_change_beyond_the_head() {
        // JSON for the first ~40 KiB, plain text after: the 64-line head
        // sample sees only JSON; the mid-file/tail probes must catch the
        // switch and turn detection into a cascade.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let contents = format!("{}{}", json_block(600), text_block(600));
        assert!(contents.len() as u64 >= PROBE_MIN_FILE_BYTES * 2);
        let path = write_input(&dir, "rotated.log", &contents);

        let detected = detect_format_from_files(&[path], false).expect("detection");

        match &detected.format {
            config::InputFormat::Cascade(members) => {
                assert_eq!(
                    members,
                    &vec![config::InputFormat::Json, config::InputFormat::Line]
                );
            }
            other => panic!("expected cascade(json,line), got {other:?}"),
        }
        assert!(detected.probe_lines > 0, "probes must have contributed");
    }

    #[test]
    fn probing_keeps_homogeneous_large_files_single_format() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = write_input(&dir, "big.json", &json_block(1200));

        let detected = detect_format_from_files(&[path], false).expect("detection");

        assert!(
            matches!(detected.format, config::InputFormat::Json),
            "homogeneous large file must stay json, got {:?}",
            detected.format
        );
        assert!(detected.probe_lines > 0, "file is large enough to probe");
    }

    #[test]
    fn small_files_are_not_probed() {
        // Below PROBE_MIN_FILE_BYTES the head sample is the whole story: a
        // format change past the 64-line head window is (documentedly) missed.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let contents = format!("{}{}", json_block(100), text_block(20));
        assert!((contents.len() as u64) < PROBE_MIN_FILE_BYTES);
        let path = write_input(&dir, "small.log", &contents);

        let detected = detect_format_from_files(&[path], false).expect("detection");

        assert_eq!(detected.probe_lines, 0);
        assert!(matches!(detected.format, config::InputFormat::Json));
    }

    #[test]
    fn probe_skips_compressed_files() {
        // Byte offsets in a compressed stream are meaningless for the
        // decompressed lines, so files with gzip magic are never probed —
        // regardless of size.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mut contents = vec![0x1F, 0x8B, 0x08];
        contents.extend(std::iter::repeat_n(
            b'a',
            (PROBE_MIN_FILE_BYTES * 2) as usize,
        ));
        let path = dir.path().join("fake.gz");
        std::fs::write(&path, contents).expect("write");

        assert!(probe_file_offsets(&path.to_string_lossy()).is_empty());
    }

    #[test]
    fn probe_window_skips_partial_and_drops_unterminated_lines() {
        // Seeking into the middle of a line must not sample the fragment, and
        // a final line without its newline (EOF or window cap) is dropped —
        // a truncated JSON line would misdetect as `line`.
        let mut file = tempfile::tempfile().expect("tempfile");
        {
            use std::io::Write;
            write!(file, "first line\nsecond line\nthird line\nunterminated").expect("write");
        }

        // Offset 3 lands inside "first line": skip it, keep the two complete
        // lines, drop the unterminated tail.
        let lines = probe_lines_at(&mut file, 3).expect("probe");
        assert_eq!(
            lines,
            vec!["second line\n".to_string(), "third line\n".to_string()]
        );

        // Offset 0 skips nothing but still drops the unterminated tail.
        let lines = probe_lines_at(&mut file, 0).expect("probe");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "first line\n");
    }

    #[test]
    fn file_detection_stays_single_format_for_homogeneous_files() {
        // Sampling must not change the result for files that detect cleanly.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = write_input(&dir, "clean.json", "{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n");

        let detected = detect_format_from_files(&[json], false).expect("detection");

        assert!(
            matches!(detected.format, config::InputFormat::Json),
            "homogeneous file must not become a cascade, got {:?}",
            detected.format
        );
    }

    #[test]
    fn detected_cascade_notice_mentions_mixed_formats() {
        let detected = DetectedFormat {
            format: config::InputFormat::Cascade(vec![
                config::InputFormat::Json,
                config::InputFormat::Line,
            ]),
            had_input: true,
            saw_content: true,
            sample_lines: 42,
            probe_lines: 0,
        };

        let mut verbose_cfg = base_config();
        verbose_cfg.processing.verbose = 1;
        let message =
            format_detected_format_notice(&verbose_cfg, &detected).expect("expected info notice");
        assert!(
            message.contains("cascade(json,line)") && message.contains("first 42 lines"),
            "message was {message}"
        );
    }

    #[test]
    fn mixed_format_suggestion_skips_auto_detected_cascades() {
        // A cascade is already the mixed-format remedy; suggesting
        // `-f cascade(json,line),syslog` would be invalid syntax.
        let stats = ProcessingStats {
            detected_format: Some("cascade(json,line)".to_string()),
            first_parse_error_sample: Some("<13>Apr 15 10:00:00 host app: hi".to_string()),
            ..Default::default()
        };
        assert!(mixed_format_suggestion(&stats).is_none());
    }

    #[test]
    fn multi_file_detection_falls_back_to_line_when_all_files_are_empty() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let a = write_input(&dir, "a.log", "");
        let b = write_input(&dir, "b.log", "");

        let detected = detect_format_from_files(&[a, b], false).expect("detection");

        assert!(matches!(detected.format, config::InputFormat::Line));
        assert!(!detected.saw_content);
        // No bytes at all, so the fell-back-to-line hint must stay quiet.
        assert!(!detected.had_input);
        assert!(!detected.fell_back_to_line());
    }

    #[test]
    fn multi_file_detection_keeps_had_input_from_skipped_blank_files() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        // First file has nothing at all, second holds only blank lines: the scan
        // skips both but must remember that input *was* read, so the
        // fell-back-to-line hint still fires.
        let empty = write_input(&dir, "empty.log", "");
        let blank = write_input(&dir, "blank.log", "\n\n");

        let detected = detect_format_from_files(&[empty, blank], false).expect("detection");

        assert!(matches!(detected.format, config::InputFormat::Line));
        assert!(detected.had_input);
        assert!(detected.fell_back_to_line());
    }

    #[test]
    fn multi_file_detection_errors_when_nothing_opens() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let missing = dir.path().join("nope.log").to_string_lossy().into_owned();

        let err = detect_format_from_files(&[missing], false).expect_err("expected failure");

        assert!(
            err.downcast_ref::<AllInputsUnopenable>().is_some(),
            "expected AllInputsUnopenable, got: {err}"
        );
    }

    #[test]
    fn detected_format_notice_is_verbose_only() {
        let detected = DetectedFormat {
            format: config::InputFormat::Json,
            had_input: true,
            saw_content: true,
            sample_lines: 1,
            probe_lines: 0,
        };

        // A confident auto-detection is silent on a normal run...
        let cfg = base_config();
        assert!(
            format_detected_format_notice(&cfg, &detected).is_none(),
            "confident auto-detect must stay silent without -v"
        );

        // ...and surfaces only under -v/--verbose.
        let mut verbose_cfg = base_config();
        verbose_cfg.processing.verbose = 1;
        let message =
            format_detected_format_notice(&verbose_cfg, &detected).expect("expected info notice");
        assert!(
            message.contains("Auto-detected format: json"),
            "message was {message}"
        );
    }

    #[test]
    fn parse_failure_warning_triggers_on_heavy_errors() {
        let cfg = base_config();
        let stats = ProcessingStats {
            lines_errors: 10,
            events_created: 0,
            ..Default::default()
        };

        let message = parse_failure_warning_message(&cfg, Some(&stats), true, false)
            .expect("expected warning");

        assert!(
            message.contains("Parsing mostly failed"),
            "message was {message}"
        );
        assert!(
            message.contains("--help-multiline"),
            "message should point to multiline help: {message}"
        );
    }

    #[test]
    fn parse_failure_warning_names_mixed_cascade_formats() {
        let cfg = base_config();
        let stats = ProcessingStats {
            lines_errors: 10,
            events_created: 5,
            detected_format: Some("json".to_string()),
            first_parse_error_sample: Some("just a plain text line".to_string()),
            ..Default::default()
        };

        let message = parse_failure_warning_message(&cfg, Some(&stats), true, false)
            .expect("expected warning");

        assert!(
            message.contains("Detected mixed formats (json + line)"),
            "message was {message}"
        );
        assert!(
            message.contains("-f json,line"),
            "should suggest the comma cascade: {message}"
        );
    }

    #[test]
    fn parse_failure_warning_suggests_repeated_flags_for_schema_formats() {
        let cfg = base_config();
        let stats = ProcessingStats {
            lines_errors: 10,
            events_created: 5,
            detected_format: Some("json".to_string()),
            // A CSV-looking line can't participate in a comma cascade.
            first_parse_error_sample: Some("name,age,city".to_string()),
            ..Default::default()
        };

        let message = parse_failure_warning_message(&cfg, Some(&stats), true, false)
            .expect("expected warning");

        assert!(
            message.contains("Detected mixed formats (json + csv)"),
            "message was {message}"
        );
        assert!(
            message.contains("-f json -f csv"),
            "should suggest repeated flags: {message}"
        );
    }

    #[test]
    fn parse_failure_warning_uses_generic_message_without_a_sample() {
        let cfg = base_config();
        // No sample line captured (e.g. stats collection produced none): the
        // warning must still fire, falling back to the generic guidance.
        let stats = ProcessingStats {
            lines_errors: 10,
            events_created: 0,
            detected_format: Some("json".to_string()),
            first_parse_error_sample: None,
            ..Default::default()
        };

        let message = parse_failure_warning_message(&cfg, Some(&stats), true, false)
            .expect("expected warning");

        assert!(
            message.contains("Parsing mostly failed"),
            "should fall back to generic message: {message}"
        );
    }

    #[test]
    fn mixed_format_suggestion_skips_same_format_secondary() {
        // Defensive: if a failing line re-detects as the already-active format,
        // we must not suggest a useless `json,json` cascade.
        let stats = ProcessingStats {
            detected_format: Some("line".to_string()),
            first_parse_error_sample: Some("just a plain text line".to_string()),
            ..Default::default()
        };
        assert!(
            mixed_format_suggestion(&stats).is_none(),
            "same primary/secondary format should yield no specific suggestion"
        );
    }

    #[test]
    fn parse_failure_warning_skips_light_error_rates() {
        let cfg = base_config();
        let stats = ProcessingStats {
            lines_errors: 2,
            events_created: 10,
            ..Default::default()
        };

        assert!(
            parse_failure_warning_message(&cfg, Some(&stats), true, false).is_none(),
            "should not warn on low error rate"
        );
    }
}
