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
use crate::rhai_functions::tracking::TrackingSnapshot;
use crate::stats;

/// Result of format detection
#[derive(Debug, Clone)]
pub struct DetectedFormat {
    pub format: config::InputFormat,
    pub had_input: bool,
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
    match reader.peek_first_line()? {
        None => Ok(DetectedFormat {
            format: config::InputFormat::Line,
            had_input: false,
        }),
        Some(line) => {
            // Remove newline for detection
            let trimmed_line = line.trim_end_matches(&['\r', '\n'][..]);
            let detected = parsers::detect_format(trimmed_line)?;
            Ok(DetectedFormat {
                format: detected,
                had_input: true,
            })
        }
    }
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
        // For files, read first line from first file
        let sorted_files = pipeline::builders::sort_files(files, &config::FileOrder::Cli)?;

        let mut failed_opens: Vec<(String, String)> = Vec::new();
        let mut failed_dirs: Vec<String> = Vec::new();
        let mut detected: Option<DetectedFormat> = None;

        for file_path in &sorted_files {
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
                    detected = Some(detect_format_from_peekable_reader(&mut peekable_reader)?);
                    break;
                }
                Err(e) => {
                    if strict {
                        return Err(anyhow::anyhow!(
                            "Failed to open file '{}': {}",
                            file_path,
                            e
                        ));
                    }
                    failed_opens.push((file_path.clone(), e.to_string()));
                }
            }
        }

        let detected = match detected {
            Some(detected) => detected,
            None => {
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
                        config::format_error_message_auto(&format!(
                            "Failed to open file '{}': {}",
                            path, err
                        ))
                    );
                    stats::stats_file_open_failed(&path);
                }
                return Err(anyhow::anyhow!(
                    "Failed to open any input files for detection"
                ));
            }
        };

        // For files we can reopen them later, so we don't need to keep this reader
        Ok((detected, None))
    }
}

/// Check if detection notices are allowed based on config and terminal state
pub fn detection_notices_allowed(config: &KeloraConfig, terminal_output: bool) -> bool {
    if config.processing.silent
        || config.processing.suppress_diagnostics
        || config.processing.quiet_events
        || std::env::var("KELORA_NO_TIPS").is_ok()
    {
        return false;
    }

    terminal_output
}

/// Format a notice about detected format
pub fn format_detected_format_notice(
    config: &KeloraConfig,
    detected: &DetectedFormat,
    terminal_output: bool,
) -> Option<String> {
    if !detection_notices_allowed(config, terminal_output) {
        return None;
    }

    if detected.detected_non_line() {
        let format_name = detected.format.to_display_string();
        let message = config.format_info_message(&format!("Auto-detected format: {}", format_name));
        Some(message)
    } else if detected.fell_back_to_line() {
        let message = config
            .format_hint_message("No input format detected; using line. Override with -f <fmt>.");
        Some(message)
    } else {
        None
    }
}

/// Emit a notice about detected format to stderr
pub fn emit_detected_format_notice(
    config: &KeloraConfig,
    detected: &DetectedFormat,
    terminal_output: bool,
) {
    if let Some(message) = format_detected_format_notice(config, detected, terminal_output) {
        eprintln!("{}", message);
    }
}

/// Extract a counter value from tracking data
pub fn extract_counter_from_tracking(tracking: &TrackingSnapshot, key: &str) -> i64 {
    tracking
        .internal
        .get(key)
        .or_else(|| tracking.user.get(key))
        .and_then(|value| {
            if value.is_int() {
                value.as_int().ok()
            } else if value.is_float() {
                value.as_float().ok().map(|v| v as i64)
            } else {
                None
            }
        })
        .unwrap_or(0)
}

/// Format a warning message about parse failures
pub fn parse_failure_warning_message(
    config: &KeloraConfig,
    tracking: Option<&TrackingSnapshot>,
    auto_detected_non_line: bool,
    events_were_output: bool,
    terminal_output: bool,
) -> Option<String> {
    if !auto_detected_non_line || !detection_notices_allowed(config, terminal_output) {
        return None;
    }

    let tracking = tracking?;

    let parse_errors = extract_counter_from_tracking(tracking, "__kelora_error_count_parse");
    let events_created = extract_counter_from_tracking(tracking, "__kelora_stats_events_created");

    let seen = std::cmp::max(1, events_created + parse_errors);
    let should_warn = (parse_errors >= 10 && parse_errors * 3 >= seen)
        || (events_created == 0 && parse_errors >= 3);

    if should_warn {
        let mut message = config
            .format_error_message("Parsing mostly failed; rerun with -f line or specify -f <fmt>.");
        if !events_were_output {
            message = message.trim_start_matches('\n').to_string();
        }
        Some(message)
    } else {
        None
    }
}

/// Emit a warning about parse failures to stderr
pub fn emit_parse_failure_warning(
    config: &KeloraConfig,
    tracking: Option<&TrackingSnapshot>,
    auto_detected_non_line: bool,
    events_were_output: bool,
    terminal_output: bool,
) {
    if let Some(message) = parse_failure_warning_message(
        config,
        tracking,
        auto_detected_non_line,
        events_were_output,
        terminal_output,
    ) {
        eprintln!("{}", message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorMode, EmojiMode};
    use rhai::Dynamic;

    fn base_config() -> KeloraConfig {
        let mut cfg = KeloraConfig::default();
        cfg.output.emoji = EmojiMode::Never;
        cfg.output.color = ColorMode::Never;
        cfg.processing.quiet_events = false;
        cfg.processing.silent = false;
        cfg.processing.suppress_diagnostics = false;
        cfg
    }

    #[test]
    fn detected_format_notice_for_non_line_format() {
        let cfg = base_config();
        let detected = DetectedFormat {
            format: config::InputFormat::Json,
            had_input: true,
        };

        let message =
            format_detected_format_notice(&cfg, &detected, true).expect("expected info notice");

        assert!(
            message.contains("Auto-detected format: json"),
            "message was {message}"
        );
    }

    #[test]
    fn parse_failure_warning_triggers_on_heavy_errors() {
        let cfg = base_config();
        let mut tracking = TrackingSnapshot::default();
        tracking.internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(10_i64),
        );
        tracking.internal.insert(
            "__kelora_stats_events_created".to_string(),
            Dynamic::from(0_i64),
        );

        let message = parse_failure_warning_message(&cfg, Some(&tracking), true, false, true)
            .expect("expected warning");

        assert!(
            message.contains("Parsing mostly failed"),
            "message was {message}"
        );
    }

    #[test]
    fn parse_failure_warning_skips_light_error_rates() {
        let cfg = base_config();
        let mut tracking = TrackingSnapshot::default();
        tracking.internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(2_i64),
        );
        tracking.internal.insert(
            "__kelora_stats_events_created".to_string(),
            Dynamic::from(10_i64),
        );

        assert!(
            parse_failure_warning_message(&cfg, Some(&tracking), true, false, true).is_none(),
            "should not warn on low error rate"
        );
    }
}
