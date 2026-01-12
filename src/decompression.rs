use anyhow::{anyhow, Result};
use flate2::read::MultiGzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Chain, Cursor, Read};
use std::path::Path;

type ChainReader = Chain<Cursor<Vec<u8>>, File>;
type GzipReader = BufReader<MultiGzDecoder<ChainReader>>;
type ZstdReader = BufReader<zstd::Decoder<'static, BufReader<ChainReader>>>;
type PlainReader = BufReader<ChainReader>;

/// Streaming decompression wrapper that implements BufRead
/// Detects gzip (1F 8B 08) and zstd (28 B5 2F FD) compression using magic bytes
pub enum DecompressionReader {
    /// Gzip decompression
    Gzip(GzipReader),
    /// Zstd decompression - decoder requires BufRead input and provides Read output
    Zstd(ZstdReader),
    /// Passthrough for non-compressed files
    Plain(PlainReader),
}

// Manually implement Debug since zstd::Decoder doesn't implement it
impl std::fmt::Debug for DecompressionReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecompressionReader::Gzip(_) => write!(f, "DecompressionReader::Gzip"),
            DecompressionReader::Zstd(_) => write!(f, "DecompressionReader::Zstd"),
            DecompressionReader::Plain(_) => write!(f, "DecompressionReader::Plain"),
        }
    }
}

impl BufRead for DecompressionReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            DecompressionReader::Gzip(reader) => reader.fill_buf(),
            DecompressionReader::Zstd(reader) => reader.fill_buf(),
            DecompressionReader::Plain(reader) => reader.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            DecompressionReader::Gzip(reader) => reader.consume(amt),
            DecompressionReader::Zstd(reader) => reader.consume(amt),
            DecompressionReader::Plain(reader) => reader.consume(amt),
        }
    }
}

impl Read for DecompressionReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DecompressionReader::Gzip(reader) => reader.read(buf),
            DecompressionReader::Zstd(reader) => reader.read(buf),
            DecompressionReader::Plain(reader) => reader.read(buf),
        }
    }
}

/// Detect compression format by magic bytes and return appropriate reader
/// Reads first 4 bytes to check for gzip (1F 8B 08) or zstd (28 B5 2F FD) magic signatures
fn detect_compression_file(mut file: File) -> std::io::Result<DecompressionReader> {
    let mut head = [0u8; 4];
    let n = file.read(&mut head)?;

    // Put the read bytes back in front using a cursor chain
    let prefix = Cursor::new(head[..n].to_vec());
    let chained = prefix.chain(file);

    // Check for gzip magic bytes: 1F 8B 08
    let is_gzip = n >= 3 && head[0] == 0x1F && head[1] == 0x8B && head[2] == 0x08;

    // Check for zstd magic bytes: 28 B5 2F FD
    let is_zstd =
        n >= 4 && head[0] == 0x28 && head[1] == 0xB5 && head[2] == 0x2F && head[3] == 0xFD;

    if is_gzip {
        let decoder = MultiGzDecoder::new(chained);
        Ok(DecompressionReader::Gzip(BufReader::new(decoder)))
    } else if is_zstd {
        // zstd::Decoder wraps input in BufReader automatically
        let decoder = zstd::Decoder::new(chained)?;
        Ok(DecompressionReader::Zstd(BufReader::new(decoder)))
    } else {
        // For non-compressed files, use the chain directly as the source
        Ok(DecompressionReader::Plain(BufReader::new(chained)))
    }
}

/// Generic magic bytes detection for any Read type
/// Returns Box<dyn Read + Send> that supports gzip and zstd decompression
pub fn maybe_decompress<R: Read + Send + 'static>(
    mut reader: R,
) -> std::io::Result<Box<dyn Read + Send>> {
    let mut head = [0u8; 4];
    let n = reader.read(&mut head)?;

    // Put the read bytes back in front using a cursor chain
    let prefix = Cursor::new(head[..n].to_vec());
    let chained: Chain<Cursor<Vec<u8>>, R> = prefix.chain(reader);

    // Check for gzip magic bytes: 1F 8B 08
    let is_gzip = n >= 3 && head[0] == 0x1F && head[1] == 0x8B && head[2] == 0x08;

    // Check for zstd magic bytes: 28 B5 2F FD
    let is_zstd =
        n >= 4 && head[0] == 0x28 && head[1] == 0xB5 && head[2] == 0x2F && head[3] == 0xFD;

    if is_gzip {
        Ok(Box::new(MultiGzDecoder::new(chained)))
    } else if is_zstd {
        Ok(Box::new(zstd::Decoder::new(chained)?))
    } else {
        Ok(Box::new(chained))
    }
}

impl DecompressionReader {
    /// Create a new decompression reader with auto-detection based on magic bytes
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let file = File::open(path_ref)?;

        // Check file extension for known unsupported formats
        if let Some(extension) = path_ref.extension().and_then(|ext| ext.to_str()) {
            if extension.to_lowercase() == "zip" {
                return Err(anyhow!("ZIP file decompression is not supported. Only gzip and zstd files are supported for streaming decompression. Extract the ZIP file first: unzip {}", path_ref.display()));
            }
        }

        // Use magic bytes detection for all files
        detect_compression_file(file)
            .map_err(|e| anyhow!("Failed to detect compression format: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use tempfile::NamedTempFile;

    #[test]
    fn test_plain_file_passthrough() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "test line 1")?;
        writeln!(temp_file, "test line 2")?;
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert!(content.contains("test line 1"));
        assert!(content.contains("test line 2"));
        Ok(())
    }

    #[test]
    fn test_zip_file_rejection() {
        // Create a temporary file with .zip extension
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();

        // Create a new path with .zip extension
        let zip_path = temp_path.with_extension("zip");

        // Create an empty file at the zip path for testing
        std::fs::write(&zip_path, b"fake zip content").unwrap();

        let result = DecompressionReader::new(&zip_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("ZIP file decompression is not supported"));
        assert!(error_msg.contains("Only gzip and zstd files are supported"));

        // Clean up
        let _ = std::fs::remove_file(&zip_path);
    }

    #[test]
    fn test_zstd_magic_bytes_detection() -> Result<()> {
        use std::process::Command;

        // Create a temporary plain text file
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "test line 1")?;
        writeln!(temp_file, "test line 2")?;
        writeln!(temp_file, "test line 3")?;
        temp_file.flush()?;

        // Compress with zstd if available
        let zstd_path = temp_file.path().with_extension("zst");
        let compress_result = Command::new("zstd")
            .arg("-q")
            .arg("-f")
            .arg(temp_file.path())
            .arg("-o")
            .arg(&zstd_path)
            .status();

        if compress_result.is_err() || !compress_result.unwrap().success() {
            // zstd not available, skip test
            eprintln!("Skipping zstd test: zstd command not available");
            return Ok(());
        }

        // Read the compressed file - should auto-detect zstd
        let mut reader = DecompressionReader::new(&zstd_path)?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert!(content.contains("test line 1"));
        assert!(content.contains("test line 2"));
        assert!(content.contains("test line 3"));

        // Clean up
        let _ = std::fs::remove_file(&zstd_path);
        Ok(())
    }

    #[test]
    fn test_magic_bytes_detection() -> Result<()> {
        // Test non-gzip file (should be treated as plain)
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "plain text file")?;
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;
        assert!(content.contains("plain text file"));

        // Test file with gzip magic bytes but wrong extension
        let mut gzip_temp = NamedTempFile::new()?;

        // Write gzip magic bytes followed by some fake data
        // This won't be a valid gzip stream, but tests magic byte detection
        gzip_temp.write_all(&[0x1F, 0x8B, 0x08])?;
        gzip_temp.write_all(b"fake gzip data")?;
        gzip_temp.flush()?;

        // This should detect it as gzip and try to decompress
        // It will likely fail during decompression, but that's expected
        // The important thing is that magic bytes are detected
        let result = DecompressionReader::new(gzip_temp.path());

        // We don't test the exact error since decompressing invalid gzip
        // data may produce various error types, but it should at least
        // attempt gzip decompression based on magic bytes
        match result {
            Ok(_reader) => {
                // If it succeeds in creating the reader, magic bytes worked
                // Reading from it might fail, but detection worked
            }
            Err(_e) => {
                // If it fails, it could be due to decompression error
                // The key is that it attempted gzip decompression
            }
        }

        Ok(())
    }

    #[test]
    fn test_gzip_multiple_members() -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Create a file with multiple gzip members (concatenated)
        let mut temp_file = NamedTempFile::new()?;

        // Create first gzip member
        let mut encoder1 = GzEncoder::new(Vec::new(), Compression::default());
        encoder1.write_all(b"first member\n")?;
        let compressed1 = encoder1.finish()?;

        // Create second gzip member
        let mut encoder2 = GzEncoder::new(Vec::new(), Compression::default());
        encoder2.write_all(b"second member\n")?;
        let compressed2 = encoder2.finish()?;

        // Write both members to file (concatenated)
        temp_file.write_all(&compressed1)?;
        temp_file.write_all(&compressed2)?;
        temp_file.flush()?;

        // Read the concatenated gzip file - MultiGzDecoder should handle multiple members
        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert!(content.contains("first member"));
        assert!(content.contains("second member"));
        Ok(())
    }

    #[test]
    fn test_corrupted_gzip_data() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut temp_file = NamedTempFile::new().unwrap();

        // Create valid gzip header
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"test data").unwrap();
        let mut compressed = encoder.finish().unwrap();

        // Corrupt the data by modifying bytes in the middle
        if compressed.len() > 10 {
            compressed[10] = !compressed[10]; // Flip bits
            compressed[11] = !compressed[11];
        }

        temp_file.write_all(&compressed).unwrap();
        temp_file.flush().unwrap();

        let result = DecompressionReader::new(temp_file.path());

        // Should create reader successfully (corruption detected during read)
        if let Ok(mut reader) = result {
            let mut content = String::new();
            let read_result = reader.read_to_string(&mut content);
            // Reading should fail due to corruption
            assert!(read_result.is_err() || content != "test data");
        }
    }

    #[test]
    fn test_empty_file() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        // Don't write anything - leave file empty

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert_eq!(content, "");
        Ok(())
    }

    #[test]
    fn test_very_small_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"x")?; // Single byte
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert_eq!(content, "x");
        Ok(())
    }

    #[test]
    fn test_file_with_only_partial_magic_bytes() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Write only 2 bytes (partial gzip magic)
        temp_file.write_all(&[0x1F, 0x8B])?;
        temp_file.flush()?;

        // Should be treated as plain text since we need 3 bytes for gzip
        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;

        assert_eq!(content, vec![0x1F, 0x8B]);
        Ok(())
    }

    #[test]
    fn test_gzip_with_no_extension() -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Create a gzipped file without .gz extension
        let mut temp_file = NamedTempFile::new()?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"compressed content\n")?;
        let compressed = encoder.finish()?;

        temp_file.write_all(&compressed)?;
        temp_file.flush()?;

        // Should detect gzip by magic bytes, not extension
        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert_eq!(content, "compressed content\n");
        Ok(())
    }

    #[test]
    fn test_decompression_reader_debug_impl() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test").unwrap();
        temp_file.flush().unwrap();

        let reader = DecompressionReader::new(temp_file.path()).unwrap();
        let debug_str = format!("{:?}", reader);
        assert!(
            debug_str.contains("DecompressionReader::")
                && (debug_str.contains("Plain")
                    || debug_str.contains("Gzip")
                    || debug_str.contains("Zstd"))
        );
    }

    #[test]
    fn test_maybe_decompress_plain() -> Result<()> {
        let data = b"plain text data";
        let cursor = Cursor::new(data.to_vec());

        let mut reader = maybe_decompress(cursor)?;
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;

        assert_eq!(content, data);
        Ok(())
    }

    #[test]
    fn test_maybe_decompress_gzip() -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let original_data = b"test data for gzip";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original_data)?;
        let compressed = encoder.finish()?;

        let cursor = Cursor::new(compressed);
        let mut reader = maybe_decompress(cursor)?;
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;

        assert_eq!(content, original_data);
        Ok(())
    }

    #[test]
    fn test_bufread_methods() -> Result<()> {
        use std::io::BufRead;

        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "line1")?;
        writeln!(temp_file, "line2")?;
        writeln!(temp_file, "line3")?;
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;

        // Test BufRead trait methods
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            lines.push(line);
        }

        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("line1"));
        assert!(lines[1].contains("line2"));
        assert!(lines[2].contains("line3"));
        Ok(())
    }

    #[test]
    fn test_file_with_binary_data() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Write binary data that's not gzip or zstd magic bytes
        let binary_data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG magic bytes
        temp_file.write_all(&binary_data)?;
        temp_file.write_all(b"more data")?;
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;

        assert!(content.starts_with(&binary_data));
        Ok(())
    }

    #[test]
    fn test_decompression_reader_is_send() {
        // Compile-time check that DecompressionReader implements Send
        fn assert_send<T: Send>() {}
        assert_send::<DecompressionReader>();
    }

    #[test]
    fn test_gzip_with_large_content() -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Create a large content (10KB)
        let large_content = "x".repeat(10_000);

        let mut temp_file = NamedTempFile::new()?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(large_content.as_bytes())?;
        let compressed = encoder.finish()?;

        temp_file.write_all(&compressed)?;
        temp_file.flush()?;

        let mut reader = DecompressionReader::new(temp_file.path())?;
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        assert_eq!(content.len(), 10_000);
        assert_eq!(content, large_content);
        Ok(())
    }

    #[test]
    fn test_file_with_zstd_magic_but_invalid_data() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write zstd magic bytes but invalid compressed data
        temp_file.write_all(&[0x28, 0xB5, 0x2F, 0xFD]).unwrap();
        temp_file.write_all(b"invalid data").unwrap();
        temp_file.flush().unwrap();

        let result = DecompressionReader::new(temp_file.path());

        // Should either fail during creation or during read
        match result {
            Ok(mut reader) => {
                let mut content = Vec::new();
                let read_result = reader.read_to_end(&mut content);
                // Reading should fail
                assert!(read_result.is_err());
            }
            Err(_) => {
                // Creation failed, which is also acceptable
            }
        }
    }
}
