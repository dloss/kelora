use anyhow::Result;
use crossbeam_channel::Receiver;
use std::borrow::Cow;
use std::cell::RefCell;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::decompression::DecompressionReader;

// When set, the byte->String boundary aborts on invalid UTF-8 (the historical
// behavior, restored via `--strict-utf8`). When unset (the default), input is
// decoded losslessly with `U+FFFD` substitution so a single bad byte no longer
// truncates the rest of the stream. See issue #239.
static STRICT_UTF8: AtomicBool = AtomicBool::new(false);

/// Select strict (abort-on-invalid) vs. lossy UTF-8 decoding for all line reads.
/// Set once during pipeline setup; read on every reader thread.
pub fn set_strict_utf8(enabled: bool) {
    STRICT_UTF8.store(enabled, Ordering::Relaxed);
}

fn strict_utf8() -> bool {
    STRICT_UTF8.load(Ordering::Relaxed)
}

/// Read a single line (through the next `\n`, inclusive) from `reader`, decoding
/// bytes as UTF-8 *lossily*: invalid sequences become `U+FFFD` (�) instead of
/// erroring out and tearing down the pipeline. Returns the number of bytes
/// consumed from the stream (0 at EOF), matching `BufRead::read_line` so callers
/// can keep using the count solely as an EOF signal. The decoded text is appended
/// to `buf`.
///
/// This is the shared, encoding-tolerant replacement for `BufRead::read_line`
/// (see issue #239). Splitting on `\n` (0x0A) before decoding is safe because
/// 0x0A never appears inside a multibyte UTF-8 sequence, so per-line lossy
/// decoding is equivalent to decoding the whole stream. A reused thread-local
/// scratch buffer keeps the clean-log path allocation-free, like `read_line`.
///
/// With `--strict-utf8` this restores the historical behavior: invalid UTF-8
/// yields `io::ErrorKind::InvalidData`.
pub(crate) fn read_line_lossy<R: BufRead + ?Sized>(
    reader: &mut R,
    buf: &mut String,
) -> io::Result<usize> {
    thread_local! {
        static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    SCRATCH.with(|cell| {
        let mut bytes = cell.borrow_mut();
        bytes.clear();
        let n = reader.read_until(b'\n', &mut bytes)?;
        if n == 0 {
            return Ok(0);
        }

        if strict_utf8() {
            match std::str::from_utf8(&bytes) {
                Ok(s) => buf.push_str(s),
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream did not contain valid UTF-8",
                    ));
                }
            }
        } else {
            match String::from_utf8_lossy(&bytes) {
                Cow::Borrowed(s) => buf.push_str(s),
                Cow::Owned(s) => {
                    // Only allocates when bytes were actually replaced (rare path).
                    crate::stats::stats_record_decode_warning(&s);
                    buf.push_str(&s);
                }
            }
        }

        Ok(n)
    })
}

/// A reader that can peek at the first line without consuming it
/// Used for format auto-detection on streams
///
/// Peeked bytes are buffered *raw* and replayed through `fill_buf`/`consume`/
/// `read`, so the wrapper is transparent to byte-level readers like
/// `read_until` (which `read_line_lossy` uses). Buffering the raw bytes — rather
/// than a decoded `String` only reachable via a custom `read_line` — is what
/// keeps the peeked first line from being skipped by the downstream lossy read.
pub struct PeekableLineReader<R: BufRead> {
    inner: R,
    /// Raw bytes read during peeking, awaiting replay to the consumer.
    buffered_prefix: Vec<u8>,
    /// Read cursor into `buffered_prefix`.
    prefix_pos: usize,
    detected_line: Option<Option<String>>,
    saw_any_input: bool,
}

impl<R: BufRead> PeekableLineReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            buffered_prefix: Vec::new(),
            prefix_pos: 0,
            detected_line: None,
            saw_any_input: false,
        }
    }

    /// Bytes still buffered from peeking that haven't been replayed yet.
    fn prefix_remaining(&self) -> &[u8] {
        &self.buffered_prefix[self.prefix_pos..]
    }

    /// Advance the prefix cursor, freeing the buffer once it's drained.
    fn advance_prefix(&mut self, amt: usize) {
        self.prefix_pos = (self.prefix_pos + amt).min(self.buffered_prefix.len());
        if self.prefix_pos >= self.buffered_prefix.len() {
            self.buffered_prefix.clear();
            self.prefix_pos = 0;
        }
    }

    /// Peek at the first non-empty line without consuming already-read lines.
    /// Blank lines encountered before detection are replayed later from the
    /// buffered prefix. The peeked bytes are decoded losslessly here purely for
    /// format detection; decode warnings are counted when the bytes are actually
    /// consumed downstream (via `read_line_lossy`), to avoid double counting.
    pub fn peek_first_non_empty_line(&mut self) -> io::Result<Option<String>> {
        if let Some(cached) = &self.detected_line {
            return Ok(cached.clone());
        }

        loop {
            let start = self.buffered_prefix.len();
            let n = self.inner.read_until(b'\n', &mut self.buffered_prefix)?;
            if n == 0 {
                self.detected_line = Some(None);
                return Ok(None);
            }
            self.saw_any_input = true;
            let line = String::from_utf8_lossy(&self.buffered_prefix[start..]).into_owned();
            if !line.trim().is_empty() {
                self.detected_line = Some(Some(line.clone()));
                return Ok(Some(line));
            }
        }
    }

    pub fn saw_any_input(&self) -> bool {
        self.saw_any_input
    }
}

impl<R: BufRead> BufRead for PeekableLineReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.prefix_pos < self.buffered_prefix.len() {
            // `consume` clears the buffer once drained, so a non-empty remainder
            // here means there are still buffered bytes to replay first.
            return Ok(&self.buffered_prefix[self.prefix_pos..]);
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        if self.prefix_pos < self.buffered_prefix.len() {
            self.advance_prefix(amt);
        } else {
            self.inner.consume(amt);
        }
    }
}

impl<R: BufRead> std::io::Read for PeekableLineReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.prefix_remaining();
        if !remaining.is_empty() {
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.advance_prefix(n);
            return Ok(n);
        }
        self.inner.read(buf)
    }
}

/// A channel-based stdin reader that is Send-compatible
pub struct ChannelStdinReader {
    receiver: Receiver<Vec<u8>>,
    current_buffer: Option<Vec<u8>>,
    current_pos: usize,
    eof: bool,
}

impl ChannelStdinReader {
    pub fn new() -> Result<Self> {
        let (sender, receiver) = crossbeam_channel::unbounded();

        // Spawn a thread to read from stdin using raw bytes
        thread::spawn(move || {
            let stdin = io::stdin();
            let mut lock = stdin.lock();
            let mut buffer = vec![0u8; 8192]; // 8KB buffer

            loop {
                match lock.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if sender.send(buffer[..n].to_vec()).is_err() {
                            break; // Receiver dropped
                        }
                    }
                    Err(_) => break, // Error reading
                }
            }
        });

        Ok(Self {
            receiver,
            current_buffer: None,
            current_pos: 0,
            eof: false,
        })
    }

    fn ensure_current_buffer(&mut self) -> io::Result<()> {
        if self.current_buffer.is_none() && !self.eof {
            match self.receiver.recv() {
                Ok(buffer) => {
                    self.current_buffer = Some(buffer);
                    self.current_pos = 0;
                }
                Err(_) => {
                    self.eof = true;
                }
            }
        }
        Ok(())
    }
}

impl io::Read for ChannelStdinReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.ensure_current_buffer()?;

        if let Some(ref buffer) = self.current_buffer {
            let remaining = &buffer[self.current_pos..];
            let to_copy = std::cmp::min(buf.len(), remaining.len());

            if to_copy > 0 {
                buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
                self.current_pos += to_copy;

                // If we've consumed the entire buffer, clear it
                if self.current_pos >= buffer.len() {
                    self.current_buffer = None;
                    self.current_pos = 0;
                }

                Ok(to_copy)
            } else {
                Ok(0)
            }
        } else {
            Ok(0) // EOF
        }
    }
}

impl io::BufRead for ChannelStdinReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.ensure_current_buffer()?;

        if let Some(ref buffer) = self.current_buffer {
            Ok(&buffer[self.current_pos..])
        } else {
            Ok(&[])
        }
    }

    fn consume(&mut self, amt: usize) {
        if let Some(ref buffer) = self.current_buffer {
            self.current_pos = std::cmp::min(self.current_pos + amt, buffer.len());

            // If we've consumed the entire buffer, clear it
            if self.current_pos >= buffer.len() {
                self.current_buffer = None;
                self.current_pos = 0;
            }
        }
    }
}

/// A multi-file reader that streams through files sequentially
pub struct MultiFileReader {
    files: Vec<String>,
    current_file_idx: usize,
    current_reader: Option<Box<dyn BufRead + Send>>,
    buffer_size: usize,
    strict: bool,
}

pub fn open_input_reader(
    file_path: &str,
    buffer_size: usize,
    strict: bool,
) -> io::Result<Option<Box<dyn BufRead + Send>>> {
    if file_path == "-" {
        match ChannelStdinReader::new() {
            Ok(stdin_reader) => match crate::decompression::maybe_decompress(stdin_reader) {
                Ok(processed_reader) => Ok(Some(Box::new(BufReader::with_capacity(
                    buffer_size,
                    processed_reader,
                )))),
                Err(e) => {
                    eprintln!(
                        "{}",
                        crate::config::format_error_message_auto(&format!(
                            "Failed to setup stdin decompression: {}",
                            e
                        ))
                    );
                    crate::stats::stats_file_open_failed("-");
                    if strict {
                        Err(io::Error::other(e))
                    } else {
                        Ok(None)
                    }
                }
            },
            Err(e) => {
                eprintln!(
                    "{}",
                    crate::config::format_error_message_auto(&format!(
                        "Failed to setup stdin reader: {}",
                        e
                    ))
                );
                crate::stats::stats_file_open_failed("-");
                if strict {
                    Err(io::Error::other(e))
                } else {
                    Ok(None)
                }
            }
        }
    } else {
        if let Ok(metadata) = fs::metadata(file_path) {
            if metadata.is_dir() {
                eprintln!(
                    "{}",
                    crate::config::format_error_message_auto(&format!(
                        "Input path '{}' is a directory; skipping (input files only)",
                        file_path
                    ))
                );
                crate::stats::stats_file_open_failed(file_path);
                if strict {
                    return Err(io::Error::other(format!(
                        "Input path '{}' is a directory; only files are supported",
                        file_path
                    )));
                }
                return Ok(None);
            }
        }

        match DecompressionReader::new(file_path) {
            Ok(decompressor) => Ok(Some(Box::new(BufReader::with_capacity(
                buffer_size,
                decompressor,
            )))),
            Err(e) => {
                eprintln!(
                    "{}",
                    crate::config::format_error_message_auto(
                        &crate::config::format_input_open_error(file_path, &e.to_string()),
                    )
                );
                crate::stats::stats_file_open_failed(file_path);
                if strict {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        crate::config::format_input_open_error(file_path, &e.to_string()),
                    ))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

/// A file-aware reader that can provide filename information
pub trait FileAwareRead: BufRead + Send {
    fn current_filename(&self) -> Option<&str>;
}

/// A multi-file reader that provides filename information
pub struct FileAwareMultiFileReader {
    inner: MultiFileReader,
}

impl FileAwareMultiFileReader {
    pub fn new(files: Vec<String>, strict: bool) -> Result<Self> {
        Ok(Self {
            inner: MultiFileReader::new(files, strict)?,
        })
    }
}

impl io::Read for FileAwareMultiFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl io::BufRead for FileAwareMultiFileReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt)
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_line(buf)
    }
}

impl FileAwareRead for FileAwareMultiFileReader {
    fn current_filename(&self) -> Option<&str> {
        self.inner.current_filename()
    }
}

impl MultiFileReader {
    /// Create a new MultiFileReader with default buffer size (256KB for better throughput)
    pub fn new(files: Vec<String>, strict: bool) -> Result<Self> {
        Self::with_buffer_size(files, 256 * 1024, strict)
    }

    /// Create a new MultiFileReader with custom buffer size
    pub fn with_buffer_size(files: Vec<String>, buffer_size: usize, strict: bool) -> Result<Self> {
        Ok(Self {
            files,
            current_file_idx: 0,
            current_reader: None,
            buffer_size,
            strict,
        })
    }

    fn ensure_current_reader(&mut self) -> io::Result<bool> {
        while self.current_reader.is_none() && self.current_file_idx < self.files.len() {
            let file_path = &self.files[self.current_file_idx];
            match open_input_reader(file_path, self.buffer_size, self.strict)? {
                Some(reader) => {
                    self.current_reader = Some(reader);
                    return Ok(true);
                }
                None => {
                    self.current_file_idx += 1;
                    continue;
                }
            }
        }

        Ok(self.current_reader.is_some())
    }

    fn advance_to_next_file(&mut self) {
        self.current_reader = None;
        self.current_file_idx += 1;
    }

    /// Get the current filename being read (if any)
    pub fn current_filename(&self) -> Option<&str> {
        if self.current_file_idx < self.files.len() {
            Some(&self.files[self.current_file_idx])
        } else {
            None
        }
    }
}

impl io::Read for MultiFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if !self.ensure_current_reader()? {
                return Ok(0); // No more files
            }

            if let Some(ref mut reader) = self.current_reader {
                match reader.read(buf) {
                    Ok(0) => {
                        // EOF on current file, advance to next
                        self.advance_to_next_file();
                        continue;
                    }
                    Ok(n) => return Ok(n),
                    Err(e) => return Err(e),
                }
            }
        }
    }
}

impl io::BufRead for MultiFileReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if !self.ensure_current_reader()? {
            return Ok(&[]); // No more files
        }

        if let Some(ref mut reader) = self.current_reader {
            reader.fill_buf()
        } else {
            Ok(&[])
        }
    }

    fn consume(&mut self, amt: usize) {
        if let Some(ref mut reader) = self.current_reader {
            reader.consume(amt);
        }
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        loop {
            if !self.ensure_current_reader()? {
                return Ok(0); // No more files
            }

            if let Some(ref mut reader) = self.current_reader {
                match read_line_lossy(reader, buf) {
                    Ok(0) => {
                        // EOF on current file, advance to next
                        self.advance_to_next_file();

                        // Add newline between files if the previous file didn't end with one
                        if !buf.is_empty() && !buf.ends_with('\n') {
                            buf.push('\n');
                            return Ok(1);
                        }
                        continue;
                    }
                    Ok(n) => return Ok(n),
                    Err(e) => return Err(e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use tempfile::NamedTempFile;

    #[test]
    fn test_multi_file_reader_single_file() -> Result<()> {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "line1")?;
        writeln!(temp_file, "line2")?;
        temp_file.flush()?;

        let files = vec![temp_file.path().to_string_lossy().to_string()];
        let mut reader = MultiFileReader::new(files, false)?;

        let mut line = String::new();

        // Read first line
        let n = reader.read_line(&mut line)?;
        assert_eq!(line, "line1\n");
        assert_eq!(n, 6);

        line.clear();

        // Read second line
        let n = reader.read_line(&mut line)?;
        assert_eq!(line, "line2\n");
        assert_eq!(n, 6);

        line.clear();

        // EOF
        let n = reader.read_line(&mut line)?;
        assert_eq!(n, 0);
        assert!(line.is_empty());

        Ok(())
    }

    #[test]
    fn test_multi_file_reader_multiple_files() -> Result<()> {
        // Create temporary files
        let mut temp_file1 = NamedTempFile::new()?;
        writeln!(temp_file1, "file1_line1")?;
        writeln!(temp_file1, "file1_line2")?;
        temp_file1.flush()?;

        let mut temp_file2 = NamedTempFile::new()?;
        writeln!(temp_file2, "file2_line1")?;
        temp_file2.flush()?;

        let files = vec![
            temp_file1.path().to_string_lossy().to_string(),
            temp_file2.path().to_string_lossy().to_string(),
        ];
        let mut reader = MultiFileReader::new(files, false)?;

        let mut all_content = String::new();
        reader.read_to_string(&mut all_content)?;

        assert_eq!(all_content, "file1_line1\nfile1_line2\nfile2_line1\n");

        Ok(())
    }
}
