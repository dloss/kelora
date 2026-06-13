use super::{Chunker, EventLimiter, OutputWriter, WindowManager};
use crate::event::Event;
use std::collections::VecDeque;

/// Default flush timeout for multiline chunkers when input is idle (milliseconds)
pub const DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS: u64 = 400;

/// Default implementations for pipeline stages
///
/// Simple pass-through chunker (no multi-line support)
pub struct SimpleChunker;

impl Chunker for SimpleChunker {
    fn feed_line(&mut self, line: String) -> Option<String> {
        Some(line)
    }

    fn flush(&mut self) -> Option<String> {
        None
    }

    fn has_pending(&self) -> bool {
        false
    }
}

/// Chunker for the CSV/TSV family that reassembles RFC 4180 records whose quoted
/// fields contain embedded newlines.
///
/// The reader splits input on physical newlines, but a CSV value like
/// `"line1\nline2"` legitimately spans several of them. This chunker tracks
/// double-quote parity across lines: while a quoted field is open it buffers
/// continuation lines (re-joining them with the newline the reader stripped) and
/// only emits once the field closes, so the parser receives one complete record.
/// The overwhelmingly common single-line record (balanced quotes) passes straight
/// through with no buffering.
#[derive(Default)]
pub struct CsvChunker {
    /// Partially-accumulated record; empty unless a quoted field is currently open.
    buffer: String,
    /// True while inside a quoted field whose closing quote hasn't been seen yet.
    in_quoted_field: bool,
}

impl CsvChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-append a line to the buffer, preserving the newline the reader stripped
    /// so the embedded newline survives into the field value.
    fn push_line(&mut self, line: &str) {
        if !self.buffer.is_empty() {
            self.buffer.push('\n');
        }
        self.buffer.push_str(line);
    }
}

impl Chunker for CsvChunker {
    fn feed_line(&mut self, line: String) -> Option<String> {
        let odd_quotes = line.bytes().filter(|&b| b == b'"').count() % 2 == 1;

        // Fast path: a self-contained record (no open field carried over and an
        // even number of quotes on this line) needs no buffering.
        if self.buffer.is_empty() && !self.in_quoted_field && !odd_quotes {
            return Some(line);
        }

        self.push_line(&line);
        if odd_quotes {
            // An odd number of quotes flips whether we're inside a quoted field.
            self.in_quoted_field = !self.in_quoted_field;
        }

        if self.in_quoted_field {
            None // still mid-field: wait for the line that closes the quote
        } else {
            Some(std::mem::take(&mut self.buffer))
        }
    }

    fn flush(&mut self) -> Option<String> {
        // At end of input, surface whatever was buffered. If a quote was still
        // open the record is malformed; the parser's completeness guard reports it
        // rather than silently corrupting the columns.
        if self.buffer.is_empty() {
            None
        } else {
            self.in_quoted_field = false;
            Some(std::mem::take(&mut self.buffer))
        }
    }

    fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }
}

/// Simple window manager (no windowing support)
pub struct SimpleWindowManager {
    current: Option<Event>,
}

impl Default for SimpleWindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleWindowManager {
    pub fn new() -> Self {
        Self { current: None }
    }
}

impl WindowManager for SimpleWindowManager {
    fn get_window(&self) -> Vec<Event> {
        if let Some(ref event) = self.current {
            vec![event.clone()]
        } else {
            Vec::new()
        }
    }

    fn update(&mut self, current: &Event) {
        self.current = Some(current.clone());
    }
}

/// Standard output writer
pub struct StdoutWriter;

impl OutputWriter for StdoutWriter {
    fn write(&mut self, line: &str) -> std::io::Result<()> {
        println!("{}", line);
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        use std::io::Write;
        std::io::stdout().flush()
    }
}

/// Simple event limiter for --take N
pub struct TakeNLimiter {
    remaining: usize,
}

impl TakeNLimiter {
    pub fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }
}

impl EventLimiter for TakeNLimiter {
    fn allow(&mut self) -> bool {
        if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }

    fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }
}

/// Sliding window manager that maintains a configurable window of recent events
///
/// The window maintains events in order: [current, previous, older...]
/// - window[0] = current event  
/// - window[1] = previous event
/// - window[2] = event before that, etc.
///
/// When window_size=N, we keep N+1 events total (current + N previous).
/// For example, --window 2 gives access to window[0], window[1], window[2].
pub struct SlidingWindowManager {
    window_size: usize,
    buffer: VecDeque<Event>,
}

impl SlidingWindowManager {
    /// Create new sliding window manager with specified window size
    ///
    /// # Arguments
    /// * `window_size` - Number of previous events to keep (0 = only current event)
    ///
    /// # Examples
    /// ```
    /// use kelora::pipeline::defaults::SlidingWindowManager;
    /// // Keep current + 2 previous events (window[0], window[1], window[2])
    /// let manager = SlidingWindowManager::new(2);
    /// ```
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            buffer: VecDeque::with_capacity(window_size + 1),
        }
    }
}

impl WindowManager for SlidingWindowManager {
    /// Get current window of events
    ///
    /// Returns events in order: [current, previous, older...]
    /// The returned vector always has the current event at index 0.
    fn get_window(&self) -> Vec<Event> {
        self.buffer.iter().cloned().collect()
    }

    /// Update window with new current event
    ///
    /// The new event becomes window[0], previous events shift:
    /// - Old window[0] becomes window[1]  
    /// - Old window[1] becomes window[2]
    /// - etc.
    ///
    /// If buffer exceeds window_size+1, oldest events are discarded.
    fn update(&mut self, current: &Event) {
        // Add new event to front (becomes window[0])
        self.buffer.push_front(current.clone());

        // Remove excess events beyond window_size + 1 (current + N previous)
        while self.buffer.len() > self.window_size + 1 {
            self.buffer.pop_back();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed each physical line through the chunker and collect the records it
    /// emits, including the final flush. Lines are fed *without* a trailing
    /// newline, exactly as the readers hand them to the pipeline; the chunker
    /// re-inserts the newline between buffered continuation lines.
    fn chunk_all(input: &str) -> Vec<String> {
        let mut chunker = CsvChunker::new();
        let mut out = Vec::new();
        for line in input.lines() {
            if let Some(record) = chunker.feed_line(line.to_string()) {
                out.push(record);
            }
        }
        if let Some(record) = chunker.flush() {
            out.push(record);
        }
        out
    }

    #[test]
    fn single_line_records_pass_through_unbuffered() {
        let records = chunk_all("a,b,c\nd,e,f\n");
        assert_eq!(records, vec!["a,b,c", "d,e,f"]);
    }

    #[test]
    fn quoted_field_with_embedded_newline_is_reassembled() {
        // RFC 4180: the newline inside "hello\nworld" is part of the value.
        let records = chunk_all("name,note\n\"alice\",\"hello\nworld\"\n\"bob\",\"ok\"\n");
        assert_eq!(
            records,
            vec!["name,note", "\"alice\",\"hello\nworld\"", "\"bob\",\"ok\""]
        );
        // Every emitted record is complete (even quote parity).
        assert!(records
            .iter()
            .all(|r| crate::parsers::csv::csv_record_complete(r)));
    }

    #[test]
    fn field_spanning_several_lines_is_reassembled() {
        let records = chunk_all("\"a\",\"one\ntwo\nthree\"\nx,y\n");
        assert_eq!(records, vec!["\"a\",\"one\ntwo\nthree\"", "x,y"]);
    }

    #[test]
    fn escaped_quotes_inside_a_field_do_not_close_it() {
        // The "" is an escaped quote; the field stays open across the newline.
        let records = chunk_all("\"a\",\"he said \"\"hi\"\"\nbye\"\nz\n");
        assert_eq!(records, vec!["\"a\",\"he said \"\"hi\"\"\nbye\"", "z"]);
    }

    #[test]
    fn unterminated_quote_at_eof_is_flushed_for_the_parser_to_reject() {
        let mut chunker = CsvChunker::new();
        assert!(chunker.feed_line("\"oops,unclosed".to_string()).is_none());
        assert!(chunker.has_pending());
        let flushed = chunker.flush().expect("buffered partial record");
        assert_eq!(flushed, "\"oops,unclosed");
        assert!(!crate::parsers::csv::csv_record_complete(&flushed));
        assert!(!chunker.has_pending());
    }
}
