use super::{Chunk, ChunkLine, Chunker, EventLimiter, OutputWriter, WindowManager};
use crate::event::Event;
use std::collections::VecDeque;

/// Default idle-flush timeout for multiline chunkers reading from a stream
/// (stdin, FIFO). Regular-file input defaults to no timeout — see
/// `resolve_multiline_idle_timeout` in main.rs. Milliseconds.
pub const DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS: u64 = 400;

/// Default line cap for multiline events (`--multiline-max-lines`); 0 = off.
pub const DEFAULT_MULTILINE_MAX_LINES: usize = 10_000;

/// Default implementations for pipeline stages
///
/// Simple pass-through chunker (no multi-line support)
pub struct SimpleChunker;

impl Chunker for SimpleChunker {
    fn feed_line(&mut self, line: ChunkLine, out: &mut Vec<Chunk>) {
        out.push(Chunk {
            text: line.text,
            first_line_num: line.line_num,
            filename: line.filename,
            line_count: 1,
        });
    }

    fn flush(&mut self, _out: &mut Vec<Chunk>) {}

    fn has_pending(&self) -> bool {
        false
    }

    fn is_passthrough(&self) -> bool {
        true
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
    /// Provenance of the buffered record's first physical line.
    first_line_num: usize,
    filename: Option<String>,
    line_count: usize,
}

impl CsvChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-append a line to the buffer, preserving the newline the reader stripped
    /// so the embedded newline survives into the field value.
    fn push_line(&mut self, line: ChunkLine) {
        if self.buffer.is_empty() {
            self.first_line_num = line.line_num;
            self.filename = line.filename;
            self.line_count = 0;
        }
        if !self.buffer.is_empty() {
            self.buffer.push('\n');
        }
        self.buffer.push_str(&line.text);
        self.line_count += 1;
    }

    fn take_buffered(&mut self) -> Chunk {
        Chunk {
            text: std::mem::take(&mut self.buffer),
            first_line_num: self.first_line_num,
            filename: self.filename.take(),
            line_count: std::mem::take(&mut self.line_count),
        }
    }
}

impl Chunker for CsvChunker {
    fn feed_line(&mut self, line: ChunkLine, out: &mut Vec<Chunk>) {
        let odd_quotes = line.text.bytes().filter(|&b| b == b'"').count() % 2 == 1;

        // Fast path: a self-contained record (no open field carried over and an
        // even number of quotes on this line) needs no buffering.
        if self.buffer.is_empty() && !self.in_quoted_field && !odd_quotes {
            out.push(Chunk {
                text: line.text,
                first_line_num: line.line_num,
                filename: line.filename,
                line_count: 1,
            });
            return;
        }

        self.push_line(line);
        if odd_quotes {
            // An odd number of quotes flips whether we're inside a quoted field.
            self.in_quoted_field = !self.in_quoted_field;
        }

        if !self.in_quoted_field {
            let chunk = self.take_buffered();
            out.push(chunk);
        }
        // else: still mid-field, wait for the line that closes the quote
    }

    fn flush(&mut self, out: &mut Vec<Chunk>) {
        // At end of input, surface whatever was buffered. If a quote was still
        // open the record is malformed; the parser's completeness guard reports it
        // rather than silently corrupting the columns.
        if !self.buffer.is_empty() {
            self.in_quoted_field = false;
            let chunk = self.take_buffered();
            out.push(chunk);
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
        let mut chunks = Vec::new();
        for (idx, line) in input.lines().enumerate() {
            chunker.feed_line(
                ChunkLine {
                    text: line.to_string(),
                    line_num: idx + 1,
                    filename: None,
                },
                &mut chunks,
            );
        }
        chunker.flush(&mut chunks);
        chunks.into_iter().map(|c| c.text).collect()
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
        let mut chunks = Vec::new();
        chunker.feed_line(
            ChunkLine {
                text: "\"oops,unclosed".to_string(),
                line_num: 7,
                filename: Some("x.csv".to_string()),
            },
            &mut chunks,
        );
        assert!(chunks.is_empty());
        assert!(chunker.has_pending());
        chunker.flush(&mut chunks);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "\"oops,unclosed");
        assert_eq!(chunks[0].first_line_num, 7);
        assert_eq!(chunks[0].filename.as_deref(), Some("x.csv"));
        assert!(!crate::parsers::csv::csv_record_complete(&chunks[0].text));
        assert!(!chunker.has_pending());
    }
}
