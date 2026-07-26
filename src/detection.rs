//! Format auto-detection and detection notice handling
//!
//! This module handles detecting input format from file content
//! and displaying appropriate notices to users.

use anyhow::Result;
use std::fs;
use std::io::BufRead;

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
        }),
        Some(line) => {
            // Remove newline for detection
            let trimmed_line = line.trim_end_matches(&['\r', '\n'][..]);
            let detected = parsers::detect_format(trimmed_line)?;
            Ok(DetectedFormat {
                format: detected,
                had_input: true,
                saw_content: true,
            })
        }
    }
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
                let candidate = detect_format_from_peekable_reader(&mut peekable_reader)?;
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
        let message = config.format_info_message(&format!(
            "Auto-detected format: {} (from first line)",
            format_name
        ));
        Some(message)
    } else if detected.fell_back_to_line() {
        // Advisory hint (💡): obeys --no-hints / --silent like every other hint,
        // and surfaces even when stderr is redirected (no terminal gate) — the
        // "I fell back to whole-line parsing" notice is exactly what someone
        // exploring an unknown file in a pipe wants to see.
        if !config.hints_allowed() {
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

/// Emit a notice about detected format to stderr
pub fn emit_detected_format_notice(config: &KeloraConfig, detected: &DetectedFormat) {
    if let Some(message) = format_detected_format_notice(config, detected) {
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
