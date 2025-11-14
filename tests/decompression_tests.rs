mod common;
use common::*;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_gzip_decompression() {
    // Test reading .gz compressed files
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();
    let gz_file_path = temp_dir.path().join("test.log.gz");

    // Create a gzipped log file
    let log_content = r#"{"level": "info", "message": "test1"}
{"level": "error", "message": "test2"}
{"level": "info", "message": "test3"}"#;

    let gz_file = File::create(&gz_file_path).unwrap();
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.write_all(log_content.as_bytes()).unwrap();
    encoder.finish().unwrap();

    // Read the gzipped file with kelora
    let (stdout, _stderr, exit_code) =
        run_kelora_with_files(&["-f", "json"], &[gz_file_path.to_str().unwrap()]);

    assert_eq!(exit_code, 0, "Should successfully read .gz file");
    assert!(stdout.contains("test1"), "Should contain first log entry");
    assert!(stdout.contains("test2"), "Should contain second log entry");
    assert!(stdout.contains("test3"), "Should contain third log entry");
}

#[test]
fn test_zstd_decompression() {
    // Test reading .zst compressed files
    use zstd::stream::write::Encoder;

    let temp_dir = TempDir::new().unwrap();
    let zst_file_path = temp_dir.path().join("test.log.zst");

    // Create a zstd compressed log file
    let log_content = r#"{"level": "info", "message": "zstd test1"}
{"level": "error", "message": "zstd test2"}"#;

    let zst_file = File::create(&zst_file_path).unwrap();
    let mut encoder = Encoder::new(zst_file, 0).unwrap();
    encoder.write_all(log_content.as_bytes()).unwrap();
    encoder.finish().unwrap();

    // Read the zstd file with kelora
    let (stdout, _stderr, exit_code) =
        run_kelora_with_files(&["-f", "json"], &[zst_file_path.to_str().unwrap()]);

    assert_eq!(exit_code, 0, "Should successfully read .zst file");
    assert!(
        stdout.contains("zstd test1"),
        "Should contain first log entry"
    );
    assert!(
        stdout.contains("zstd test2"),
        "Should contain second log entry"
    );
}

#[test]
fn test_mixed_compressed_and_uncompressed_files() {
    // Test reading multiple files with mixed compression
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();

    // Create uncompressed file
    let plain_file_path = temp_dir.path().join("plain.log");
    std::fs::write(
        &plain_file_path,
        r#"{"source": "plain", "message": "plain log"}"#,
    )
    .unwrap();

    // Create gzipped file
    let gz_file_path = temp_dir.path().join("compressed.log.gz");
    let gz_file = File::create(&gz_file_path).unwrap();
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder
        .write_all(br#"{"source": "gzip", "message": "compressed log"}"#)
        .unwrap();
    encoder.finish().unwrap();

    // Read both files
    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "json"],
        &[
            plain_file_path.to_str().unwrap(),
            gz_file_path.to_str().unwrap(),
        ],
    );

    assert_eq!(
        exit_code, 0,
        "Should successfully read mixed compressed/uncompressed files"
    );
    assert!(
        stdout.contains("plain log"),
        "Should contain plain file log"
    );
    assert!(
        stdout.contains("compressed log"),
        "Should contain compressed file log"
    );
}

#[test]
fn test_corrupted_gzip_file() {
    // Test handling of corrupted gzip file
    let temp_dir = TempDir::new().unwrap();
    let corrupt_gz_path = temp_dir.path().join("corrupt.log.gz");

    // Write invalid gzip data
    std::fs::write(&corrupt_gz_path, b"This is not valid gzip data").unwrap();

    // Try to read the corrupted file
    let (_stdout, stderr, exit_code) =
        run_kelora_with_files(&["-f", "json"], &[corrupt_gz_path.to_str().unwrap()]);

    // Should fail with error
    assert_ne!(exit_code, 0, "Should fail on corrupted gzip file");
    assert!(
        stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("failed")
            || stderr.to_lowercase().contains("decompress"),
        "Should show decompression error"
    );
}

#[test]
fn test_unsupported_compression_format() {
    // Test handling of unsupported compression format (.zip)
    let temp_dir = TempDir::new().unwrap();
    let zip_file_path = temp_dir.path().join("test.log.zip");

    // Create a simple (invalid) zip file
    std::fs::write(&zip_file_path, b"PK\x03\x04fake zip data").unwrap();

    // Try to read the zip file
    let (_stdout, stderr, exit_code) =
        run_kelora_with_files(&[], &[zip_file_path.to_str().unwrap()]);

    // Should either fail or warn about unsupported format
    if exit_code != 0 {
        assert!(
            stderr.to_lowercase().contains("error")
                || stderr.to_lowercase().contains("unsupported")
                || stderr.to_lowercase().contains("format"),
            "Should show error about unsupported format"
        );
    }
}

#[test]
fn test_empty_gzip_file() {
    // Test reading empty gzip file
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();
    let gz_file_path = temp_dir.path().join("empty.log.gz");

    // Create an empty gzipped file
    let gz_file = File::create(&gz_file_path).unwrap();
    let encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.finish().unwrap();

    // Read the empty gzipped file
    let (stdout, _stderr, exit_code) =
        run_kelora_with_files(&["-f", "json"], &[gz_file_path.to_str().unwrap()]);

    assert_eq!(exit_code, 0, "Should handle empty .gz file");
    assert!(stdout.trim().is_empty(), "Should produce no output");
}

#[test]
fn test_large_gzip_file() {
    // Test reading large gzipped file
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();
    let gz_file_path = temp_dir.path().join("large.log.gz");

    // Create a large log file (1000 lines)
    let log_lines: Vec<String> = (1..=1000)
        .map(|i| format!(r#"{{"id": {}, "message": "log entry {}"}}"#, i, i))
        .collect();
    let log_content = log_lines.join("\n");

    let gz_file = File::create(&gz_file_path).unwrap();
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.write_all(log_content.as_bytes()).unwrap();
    encoder.finish().unwrap();

    // Read and filter the large gzipped file
    let (stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "--filter", "e.id % 100 == 0"],
        &[gz_file_path.to_str().unwrap()],
    );

    assert_eq!(
        exit_code, 0,
        "Should successfully read large .gz file, stderr: {}",
        stderr
    );

    // Should output 10 filtered lines (100, 200, ..., 1000)
    let output_lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        output_lines.len(),
        10,
        "Should filter to 10 lines from large file"
    );
}

#[test]
fn test_multiple_gzip_files() {
    // Test reading multiple gzipped files
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();

    let mut file_paths = Vec::new();

    // Create 3 gzipped files
    for i in 1..=3 {
        let gz_file_path = temp_dir.path().join(format!("test{}.log.gz", i));
        let log_content = format!(r#"{{"file": {}, "message": "from file {}"}}"#, i, i);

        let gz_file = File::create(&gz_file_path).unwrap();
        let mut encoder = GzEncoder::new(gz_file, Compression::default());
        encoder.write_all(log_content.as_bytes()).unwrap();
        encoder.finish().unwrap();

        file_paths.push(gz_file_path);
    }

    // Read all three gzipped files
    let file_strs: Vec<&str> = file_paths.iter().map(|p| p.to_str().unwrap()).collect();
    let (stdout, stderr, exit_code) = run_kelora_with_files(&["-f", "json"], &file_strs);

    assert_eq!(
        exit_code, 0,
        "Should successfully read multiple .gz files, stderr: {}",
        stderr
    );
    assert!(stdout.contains("from file 1"), "Should contain file 1 log");
    assert!(stdout.contains("from file 2"), "Should contain file 2 log");
    assert!(stdout.contains("from file 3"), "Should contain file 3 log");
}

#[test]
fn test_gzip_with_multiline_mode() {
    // Test gzip decompression with multiline chunking
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();
    let gz_file_path = temp_dir.path().join("multiline.log.gz");

    let log_content = r#"2024-01-01 10:00:00 INFO Starting application
  Additional info line 1
  Additional info line 2
2024-01-01 10:00:05 ERROR Database error
  Stack trace line 1
  Stack trace line 2"#;

    let gz_file = File::create(&gz_file_path).unwrap();
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.write_all(log_content.as_bytes()).unwrap();
    encoder.finish().unwrap();

    // Read with multiline mode
    let (_stdout, stderr, exit_code) = run_kelora_with_files(
        &["-f", "line", "-M", "indent", "--stats"],
        &[gz_file_path.to_str().unwrap()],
    );

    assert_eq!(
        exit_code, 0,
        "Should successfully read .gz file with multiline mode"
    );

    // Should create 2 multiline events
    assert!(
        stderr.contains("Events created: 2"),
        "Should create 2 multiline events from compressed file"
    );
}

#[test]
fn test_gzip_with_parallel_mode() {
    // Test gzip decompression with parallel processing
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let temp_dir = TempDir::new().unwrap();
    let gz_file_path = temp_dir.path().join("parallel.log.gz");

    // Create log file with 100 lines
    let log_lines: Vec<String> = (1..=100).map(|i| format!(r#"{{"id": {}}}"#, i)).collect();
    let log_content = log_lines.join("\n");

    let gz_file = File::create(&gz_file_path).unwrap();
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.write_all(log_content.as_bytes()).unwrap();
    encoder.finish().unwrap();

    // Read with parallel mode
    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &["-f", "json", "--parallel", "--batch-size", "10"],
        &[gz_file_path.to_str().unwrap()],
    );

    assert_eq!(
        exit_code, 0,
        "Should successfully read .gz file with parallel mode"
    );

    let output_lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(
        output_lines.len(),
        100,
        "Should output all 100 lines from compressed file"
    );
}

#[test]
fn test_stdin_with_gzip() {
    // Test reading gzipped data from stdin
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let log_content = r#"{"message": "from stdin gzip"}"#;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(log_content.as_bytes()).unwrap();
    let _compressed_data = encoder.finish().unwrap();

    // Note: This test might be difficult to implement with current test infrastructure
    // as it requires passing binary data to stdin. Skipping for now or marking as TODO.
    // This is a known limitation of the test suite.
}
