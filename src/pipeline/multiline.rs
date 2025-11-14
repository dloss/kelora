use super::Chunker;
use crate::config::{InputFormat, MultilineConfig, MultilineStrategy};
use crate::timestamp::AdaptiveTsParser;
use regex::Regex;

const MAX_TIMESTAMP_PREFIX_CHARS: usize = 64;
const MAX_TIMESTAMP_TOKENS: usize = 6;

/// Multi-line chunker that implements the reduced set of strategies for detecting event boundaries
pub struct MultilineChunker {
    config: MultilineConfig,
    buffer: Vec<String>,
    start_regex: Option<Regex>,
    end_regex: Option<Regex>,
    input_format: InputFormat,
    timestamp_detector: Option<TimestampDetector>,
    pending_output: Option<String>,
}

impl MultilineChunker {
    pub fn new(config: MultilineConfig, input_format: InputFormat) -> Result<Self, String> {
        let mut start_regex = None;
        let mut end_regex = None;
        let mut timestamp_detector = None;

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
            MultilineStrategy::Timestamp { chrono_format } => {
                timestamp_detector = Some(TimestampDetector::new(chrono_format.clone()));
            }
            MultilineStrategy::Indent | MultilineStrategy::All => {}
        }

        Ok(Self {
            config,
            buffer: Vec::new(),
            start_regex,
            end_regex,
            input_format,
            timestamp_detector,
            pending_output: None,
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
            MultilineStrategy::Indent => !is_line_indented(line),
            MultilineStrategy::Regex { .. } => {
                if let Some(regex) = &self.start_regex {
                    regex.is_match(line)
                } else {
                    false
                }
            }
            MultilineStrategy::All => false,
        }
    }

    /// Check if this line ends the current event (only relevant for regex strategies with end=...)
    fn ends_current_event(&self, line: &str) -> bool {
        match (&self.config.strategy, &self.end_regex) {
            (MultilineStrategy::Regex { end: Some(_), .. }, Some(regex)) => regex.is_match(line),
            _ => false,
        }
    }

    /// Flush the current buffer and return the event content
    fn flush_buffer(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }

        let joined = match self.config.strategy {
            MultilineStrategy::All => self.buffer.join("\n"),
            _ => self.buffer.join(""),
        };

        let content = match self.config.strategy {
            MultilineStrategy::All => joined,
            _ => match self.input_format {
                InputFormat::Raw => joined,
                _ => joined.replace('\n', " ").replace('\r', ""),
            },
        };

        self.buffer.clear();
        Some(content)
    }
}

struct TimestampDetector {
    parser: AdaptiveTsParser,
    chrono_format: Option<String>,
}

impl TimestampDetector {
    fn new(chrono_format: Option<String>) -> Self {
        Self {
            parser: AdaptiveTsParser::new(),
            chrono_format,
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

        let custom_format = self.chrono_format.clone();
        if let Some(format) = custom_format.as_deref() {
            if self.try_candidates(&candidates, Some(format)) {
                return true;
            }
        }

        self.try_candidates(&candidates, None)
    }

    fn try_candidates(&mut self, candidates: &[String], custom_format: Option<&str>) -> bool {
        for candidate in candidates {
            if self
                .parser
                .parse_ts_with_config(candidate, custom_format, None)
                .is_some()
            {
                return true;
            }
        }

        false
    }
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
    fn feed_line(&mut self, line: String) -> Option<String> {
        let mut produced = Vec::new();

        if let Some(pending) = self.pending_output.take() {
            produced.push(pending);
        }

        let flush_on_start = matches!(
            self.config.strategy,
            MultilineStrategy::Timestamp { .. }
                | MultilineStrategy::Indent
                | MultilineStrategy::Regex { .. }
        );

        if flush_on_start && !self.buffer.is_empty() && self.starts_new_event(&line) {
            if let Some(event) = self.flush_buffer() {
                produced.push(event);
            }
        }

        match &self.config.strategy {
            MultilineStrategy::All => {
                self.buffer.push(line);
            }
            MultilineStrategy::Regex { .. } => {
                self.buffer.push(line);

                if let Some(last_line) = self.buffer.last() {
                    if self.ends_current_event(last_line) {
                        if let Some(event) = self.flush_buffer() {
                            produced.push(event);
                        }
                    }
                }
            }
            MultilineStrategy::Timestamp { .. } | MultilineStrategy::Indent => {
                self.buffer.push(line);
            }
        }

        if produced.is_empty() {
            None
        } else {
            let first = produced.remove(0);
            if let Some(next) = produced.into_iter().next() {
                self.pending_output = Some(next);
            }
            Some(first)
        }
    }

    fn flush(&mut self) -> Option<String> {
        if let Some(pending) = self.pending_output.take() {
            return Some(pending);
        }

        self.flush_buffer()
    }

    fn has_pending(&self) -> bool {
        self.pending_output.is_some() || !self.buffer.is_empty()
    }
}

/// Create a chunker based on multiline configuration
pub fn create_multiline_chunker(
    config: &MultilineConfig,
    input_format: InputFormat,
) -> Result<Box<dyn Chunker>, String> {
    let chunker = MultilineChunker::new(config.clone(), input_format)?;
    Ok(Box::new(chunker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_detection_with_format_hint() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Timestamp {
                chrono_format: Some("%b %e %H:%M:%S".to_string()),
            },
        };

        let mut chunker =
            MultilineChunker::new(config, InputFormat::Syslog).expect("chunker should build");

        assert!(chunker
            .feed_line("Jan  2 03:04:05 host app: one\n".to_string())
            .is_none());
        assert!(chunker
            .feed_line("  stack frame line\n".to_string())
            .is_none());

        let flushed = chunker.feed_line("Jan  3 03:04:05 host app: two\n".to_string());
        assert!(flushed.is_some());

        assert!(chunker.flush().is_some());
    }

    #[test]
    fn test_indent_strategy_basic() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        // First line starts event
        assert!(chunker.feed_line("Header line\n".to_string()).is_none());

        // Indented line continues
        assert!(chunker
            .feed_line("  continued line\n".to_string())
            .is_none());
        assert!(chunker
            .feed_line("\tmore continuation\n".to_string())
            .is_none());

        // Non-indented starts new event
        let event = chunker.feed_line("New header\n".to_string());
        assert!(event.is_some());
        assert!(event.unwrap().contains("Header"));

        let final_event = chunker.flush();
        assert!(final_event.is_some());
        assert!(final_event.unwrap().contains("New header"));
    }

    #[test]
    fn test_regex_strategy_start_only() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Regex {
                start: r"^\d{4}-\d{2}-\d{2}".to_string(),
                end: None,
            },
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        assert!(chunker
            .feed_line("2024-01-01 First event\n".to_string())
            .is_none());
        assert!(chunker.feed_line("continuation\n".to_string()).is_none());

        let event = chunker.feed_line("2024-01-02 Second event\n".to_string());
        assert!(event.is_some());
        assert!(event.unwrap().contains("2024-01-01"));

        let final_event = chunker.flush();
        assert!(final_event.is_some());
        assert!(final_event.unwrap().contains("2024-01-02"));
    }

    #[test]
    fn test_regex_strategy_with_end() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Regex {
                start: r"^START".to_string(),
                end: Some(r"^END".to_string()),
            },
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        assert!(chunker.feed_line("START event 1\n".to_string()).is_none());
        assert!(chunker.feed_line("middle line\n".to_string()).is_none());

        // End marker should flush
        let event = chunker.feed_line("END\n".to_string());
        assert!(event.is_some());
        assert!(event.unwrap().contains("START"));
    }

    #[test]
    fn test_all_strategy_joins_with_newlines() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::All,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        assert!(chunker.feed_line("line1\n".to_string()).is_none());
        assert!(chunker.feed_line("line2\n".to_string()).is_none());
        assert!(chunker.feed_line("line3\n".to_string()).is_none());

        let event = chunker.flush();
        assert!(event.is_some());
        let content = event.unwrap();
        assert!(content.contains("line1\n"));
        assert!(content.contains("line2\n"));
        assert!(content.contains("line3\n"));
    }

    #[test]
    fn test_flush_empty_buffer() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();
        assert!(chunker.flush().is_none());
    }

    #[test]
    fn test_has_pending() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();
        assert!(!chunker.has_pending());

        chunker.feed_line("test\n".to_string());
        assert!(chunker.has_pending());

        chunker.flush();
        assert!(!chunker.has_pending());
    }

    #[test]
    fn test_empty_line_handling_indent() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        chunker.feed_line("Header\n".to_string());
        chunker.feed_line("  continuation\n".to_string());
        // Empty line (not indented) starts new event
        let event = chunker.feed_line("\n".to_string());
        assert!(event.is_some());
    }

    #[test]
    fn test_very_large_multiline_event() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        chunker.feed_line("Header\n".to_string());

        // Add 1000 continuation lines
        for i in 0..1000 {
            chunker.feed_line(format!("  line {}\n", i));
        }

        let event = chunker.flush();
        assert!(event.is_some());
        let content = event.unwrap();
        assert!(content.contains("Header"));
        assert!(content.contains("line 999"));
    }

    #[test]
    fn test_timestamp_strategy_without_format_hint() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Timestamp {
                chrono_format: None,
            },
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        assert!(chunker
            .feed_line("2024-01-01T10:00:00 First\n".to_string())
            .is_none());
        assert!(chunker.feed_line("continuation\n".to_string()).is_none());

        let event = chunker.feed_line("2024-01-01T10:00:01 Second\n".to_string());
        assert!(event.is_some());
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
        let config = MultilineConfig {
            strategy: MultilineStrategy::Regex {
                start: r"[invalid(".to_string(),
                end: None,
            },
        };

        let result = MultilineChunker::new(config, InputFormat::Raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_end_regex_pattern() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Regex {
                start: r"^START".to_string(),
                end: Some(r"[invalid(".to_string()),
            },
        };

        let result = MultilineChunker::new(config, InputFormat::Raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_timestamp_detector_empty_line() {
        let mut detector = TimestampDetector::new(None);
        assert!(!detector.is_header(""));
        assert!(!detector.is_header("   \n"));
    }

    #[test]
    fn test_timestamp_detector_indented_line() {
        let mut detector = TimestampDetector::new(None);
        assert!(!detector.is_header("  2024-01-01 test"));
    }

    #[test]
    fn test_timestamp_detector_valid_timestamp() {
        let mut detector = TimestampDetector::new(None);
        assert!(detector.is_header("2024-01-01T10:00:00 message"));
    }

    #[test]
    fn test_pending_output_handling() {
        // Test that pending_output is used correctly when multiple events are produced
        // This happens with the timestamp/indent strategies when a new header arrives
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        // First event
        chunker.feed_line("Header 1\n".to_string());
        chunker.feed_line("  continuation\n".to_string());

        // Second event starts - should flush first event
        let first = chunker.feed_line("Header 2\n".to_string());
        assert!(first.is_some());
        assert!(first.unwrap().contains("Header 1"));

        // Flush should return second event
        let second = chunker.flush();
        assert!(second.is_some());
        assert!(second.unwrap().contains("Header 2"));
    }

    #[test]
    fn test_multiline_with_raw_format_preserves_newlines_in_all_strategy() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::All,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        chunker.feed_line("line1\n".to_string());
        chunker.feed_line("line2\n".to_string());

        let event = chunker.flush();
        assert!(event.is_some());
        let content = event.unwrap();
        // All strategy joins with \n, so it becomes "line1\n\nline2\n"
        assert_eq!(content, "line1\n\nline2\n");
    }

    #[test]
    fn test_multiline_with_json_format_removes_newlines() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Json).unwrap();

        chunker.feed_line("Header\n".to_string());
        chunker.feed_line("  continuation\n".to_string());

        let event = chunker.flush();
        assert!(event.is_some());
        let content = event.unwrap();
        // Should replace newlines with spaces for non-Raw formats
        assert!(!content.contains('\n'));
        assert!(content.contains(' '));
    }

    #[test]
    fn test_create_multiline_chunker_function() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let result = create_multiline_chunker(&config, InputFormat::Raw);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_flush_on_empty_buffer() {
        let config = MultilineConfig {
            strategy: MultilineStrategy::Indent,
        };

        let mut chunker = MultilineChunker::new(config, InputFormat::Raw).unwrap();

        assert!(chunker.flush().is_none());
        assert!(chunker.flush().is_none());
        assert!(chunker.flush().is_none());
    }
}
