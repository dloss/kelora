use anyhow::Result;
use crossbeam_channel::Receiver;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::thread;

use crate::decompression::DecompressionReader;

/// A reader that can peek at the first line without consuming it
/// Used for format auto-detection on streams
pub struct PeekableLineReader<R: BufRead> {
    inner: R,
    peeked_line: Option<String>,
    first_line_consumed: bool,
}

impl<R: BufRead> PeekableLineReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            peeked_line: None,
            first_line_consumed: false,
        }
    }

    /// Peek at the first line without consuming it
    pub fn peek_first_line(&mut self) -> io::Result<Option<String>> {
        if self.peeked_line.is_some() {
            return Ok(self.peeked_line.clone());
        }

        let mut line = String::new();
        match self.inner.read_line(&mut line) {
            Ok(0) => Ok(None), // EOF
            Ok(_) => {
                self.peeked_line = Some(line.clone());
                Ok(Some(line))
            }
            Err(e) => Err(e),
        }
    }
}

impl<R: BufRead> BufRead for PeekableLineReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt)
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        // If we have a peeked line and it hasn't been consumed yet, return it
        if let Some(peeked) = &self.peeked_line {
            if !self.first_line_consumed {
                buf.push_str(peeked);
                self.first_line_consumed = true;
                return Ok(peeked.len());
            }
        }

        // Otherwise, read from the inner reader
        self.inner.read_line(buf)
    }
}

impl<R: BufRead> std::io::Read for PeekableLineReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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

            if file_path == "-" {
                // Handle stdin with gzip detection
                match ChannelStdinReader::new() {
                    Ok(stdin_reader) => {
                        // Apply magic bytes detection to stdin reader
                        match crate::decompression::maybe_decompress(stdin_reader) {
                            Ok(processed_reader) => {
                                self.current_reader = Some(Box::new(BufReader::with_capacity(
                                    self.buffer_size,
                                    processed_reader,
                                )));
                                return Ok(true);
                            }
                            Err(e) => {
                                eprintln!(
                                    "{}",
                                    crate::config::format_error_message_auto(&format!(
                                        "Failed to setup stdin decompression: {}",
                                        e
                                    ))
                                );
                                crate::stats::stats_file_open_failed("-");
                                if self.strict {
                                    return Err(io::Error::other(e));
                                }
                                self.current_file_idx += 1;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "{}",
                            crate::config::format_error_message_auto(&format!(
                                "Failed to setup stdin reader: {}",
                                e
                            ))
                        );
                        crate::stats::stats_file_open_failed("-");
                        if self.strict {
                            return Err(io::Error::other(e));
                        }
                        self.current_file_idx += 1;
                        continue;
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
                        if self.strict {
                            return Err(io::Error::other(format!(
                                "Input path '{}' is a directory; only files are supported",
                                file_path
                            )));
                        }
                        self.current_file_idx += 1;
                        continue;
                    }
                }

                match DecompressionReader::new(file_path) {
                    Ok(decompressor) => {
                        self.current_reader = Some(Box::new(BufReader::with_capacity(
                            self.buffer_size,
                            decompressor,
                        )));
                        return Ok(true);
                    }
                    Err(e) => {
                        eprintln!(
                            "{}",
                            crate::config::format_error_message_auto(&format!(
                                "Failed to open file '{}': {}",
                                file_path, e
                            ))
                        );
                        crate::stats::stats_file_open_failed(file_path);
                        if self.strict {
                            return Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                format!("Failed to open file '{}': {}", file_path, e),
                            ));
                        }
                        self.current_file_idx += 1;
                        continue;
                    }
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
                match reader.read_line(buf) {
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
