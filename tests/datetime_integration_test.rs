use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_datetime_parsing_integration() {
    // Create a temporary file with some test data
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(
        temp_file,
        r#"{{"timestamp": "2023-07-04T12:34:56Z", "message": "Test message"}}"#
    )
    .unwrap();
    temp_file.flush().unwrap();

    // Run kelora with datetime parsing
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("-f")
        .arg("json")
        .arg(temp_file.path())
        .arg("--exec")
        .arg("let dt = to_datetime(e.timestamp); let year = dt.year(); if year == 2023 { print(\"YEAR_MATCH\") }")
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("YEAR_MATCH"),
        "Expected YEAR_MATCH in output. stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}

#[test]
fn test_duration_parsing_integration() {
    // Create a temporary file with some test data
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(
        temp_file,
        r#"{{"duration": "1h 30m", "operation": "backup"}}"#
    )
    .unwrap();
    temp_file.flush().unwrap();

    // Run kelora with duration parsing
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("-f")
        .arg("json")
        .arg(temp_file.path())
        .arg("--exec")
        .arg("let dur = to_duration(e.duration); let minutes = dur.as_minutes(); if minutes == 90 { print(\"DURATION_MATCH\") }")
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DURATION_MATCH"),
        "Expected DURATION_MATCH in output: {}",
        stdout
    );
}

#[test]
fn test_datetime_arithmetic_integration() {
    // Create a temporary file with some test data
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(
        temp_file,
        r#"{{"start": "2023-07-04T12:00:00Z", "end": "2023-07-04T13:30:00Z"}}"#
    )
    .unwrap();
    temp_file.flush().unwrap();

    // Run kelora with datetime arithmetic
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("-f")
        .arg("json")
        .arg(temp_file.path())
        .arg("--exec")
        .arg("let start_dt = to_datetime(e.start); let end_dt = to_datetime(e.end); let diff = end_dt - start_dt; let minutes = diff.as_minutes(); if minutes == 90 { print(\"ARITHMETIC_MATCH\") }")
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ARITHMETIC_MATCH"),
        "Expected ARITHMETIC_MATCH in output: {}",
        stdout
    );
}

#[test]
fn test_current_time_functions_integration() {
    // Create a temporary file with some test data
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, r#"{{"message": "Current time test"}}"#).unwrap();
    temp_file.flush().unwrap();

    // Run kelora with current time functions
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("-f")
        .arg("json")
        .arg(temp_file.path())
        .arg("--exec")
        .arg("let now = now(); let year = now.year(); if year >= 2023 { print(\"NOW_MATCH\") }")
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("NOW_MATCH"),
        "Expected NOW_MATCH in output. stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}

#[test]
fn test_datetime_formatting_integration() {
    // Create a temporary file with some test data
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(
        temp_file,
        r#"{{"timestamp": "2023-07-04T12:34:56Z", "message": "Test message"}}"#
    )
    .unwrap();
    temp_file.flush().unwrap();

    // Run kelora with datetime formatting
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("-f")
        .arg("json")
        .arg(temp_file.path())
        .arg("--exec")
        .arg("let dt = to_datetime(e.timestamp); let formatted = dt.format(\"%Y-%m-%d\"); if formatted == \"2023-07-04\" { print(\"FORMAT_MATCH\") }")
        .output()
        .expect("Failed to run kelora");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FORMAT_MATCH"),
        "Expected FORMAT_MATCH in output: {}",
        stdout
    );
}

// Regression: ts_nanos() must surface an error for datetimes outside the
// i64-nanosecond range (~1677-09-21..2262-04-11) instead of silently
// returning 0, which mapped such timestamps to the Unix epoch and corrupted
// downstream analysis. In-range timestamps must still convert correctly.
#[test]
fn test_ts_nanos_out_of_range_errors_not_zero() {
    let binary = env!("CARGO_BIN_EXE_kelora");

    // Out-of-range (year 9999): must NOT yield 0; must report an error.
    let out = Command::new(binary)
        .args([
            "-e",
            "e.n = to_datetime(\"9999-12-31T23:59:59Z\").ts_nanos()",
        ])
        .arg("--strict")
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.as_mut().unwrap().write_all(b"x\n").unwrap();
            c.wait_with_output()
        })
        .expect("run kelora");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stdout.contains("n=0"),
        "out-of-range ts_nanos must not silently be 0; stdout: {stdout}"
    );
    assert!(
        stderr.contains("out of range"),
        "expected out-of-range error; stderr: {stderr}"
    );

    // In-range timestamp must still produce the correct nanosecond value.
    let out = Command::new(binary)
        .args([
            "-e",
            "e.n = to_datetime(\"2024-01-15T12:00:00Z\").ts_nanos()",
        ])
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.as_mut().unwrap().write_all(b"x\n").unwrap();
            c.wait_with_output()
        })
        .expect("run kelora");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("n=1705320000000000000"),
        "in-range ts_nanos must convert correctly; stdout: {stdout}"
    );
}
