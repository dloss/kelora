mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_syslog_rfc5424_parsing() {
    let input = r#"<165>1 2023-10-11T22:14:15.003Z server01 sshd 1234 ID47 - Failed password for user alice
<33>1 2023-10-11T22:14:16.123Z server01 nginx 5678 - - Request processed successfully"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "syslog", "-F", "json"], input);
    assert_eq!(exit_code, 0, "syslog parsing should succeed");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should parse 2 syslog lines");

    // Check first line (SSH failure)
    let first_line: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first_line["pri"].as_i64().unwrap(), 165);
    assert_eq!(first_line["facility"].as_i64().unwrap(), 20); // 165 >> 3
    assert_eq!(first_line["severity"].as_i64().unwrap(), 5); // 165 & 7
    assert_eq!(first_line["host"].as_str().unwrap(), "server01");
    assert_eq!(first_line["prog"].as_str().unwrap(), "sshd");
    assert_eq!(first_line["pid"].as_i64().unwrap(), 1234);
    assert_eq!(
        first_line["msg"].as_str().unwrap(),
        "Failed password for user alice"
    );

    // Check second line (nginx success)
    let second_line: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second_line["pri"].as_i64().unwrap(), 33);
    assert_eq!(second_line["facility"].as_i64().unwrap(), 4); // 33 >> 3
    assert_eq!(second_line["severity"].as_i64().unwrap(), 1); // 33 & 7
    assert_eq!(second_line["prog"].as_str().unwrap(), "nginx");
    assert_eq!(second_line["pid"].as_i64().unwrap(), 5678);
}

#[test]
fn test_syslog_rfc3164_parsing() {
    let input = r#"Oct 11 22:14:15 server01 sshd[1234]: Failed password for user bob
Oct 11 22:14:16 server01 kernel: CPU0: Core temperature above threshold"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "syslog", "-F", "json"], input);
    assert_eq!(exit_code, 0, "syslog parsing should succeed");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should parse 2 syslog lines");

    // Check first line (with PID)
    let first_line: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first_line["ts"].as_str().unwrap(), "Oct 11 22:14:15");
    assert_eq!(first_line["host"].as_str().unwrap(), "server01");
    assert_eq!(first_line["prog"].as_str().unwrap(), "sshd");
    assert_eq!(first_line["pid"].as_i64().unwrap(), 1234);
    assert_eq!(
        first_line["msg"].as_str().unwrap(),
        "Failed password for user bob"
    );

    // Check second line (no PID)
    let second_line: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second_line["ts"].as_str().unwrap(), "Oct 11 22:14:16");
    assert_eq!(second_line["host"].as_str().unwrap(), "server01");
    assert_eq!(second_line["prog"].as_str().unwrap(), "kernel");
    assert_eq!(second_line["pid"], serde_json::Value::Null); // No PID for kernel messages
    assert_eq!(
        second_line["msg"].as_str().unwrap(),
        "CPU0: Core temperature above threshold"
    );
}

#[test]
fn test_syslog_filtering_and_analysis() {
    let input = r#"<165>1 2023-10-11T22:14:15.003Z server01 sshd 1234 ID47 - Failed password for user alice
<86>1 2023-10-11T22:14:16.456Z server01 postfix 9012 - - NOQUEUE: reject: RCPT from unknown
<33>1 2023-10-11T22:14:17.123Z server01 nginx 5678 - - Request processed successfully
Oct 11 22:14:18 server01 sshd[1234]: Failed password for user bob
Oct 11 22:14:19 server01 kernel: CPU0: Core temperature above threshold"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "syslog",
        "--filter", "e.msg.matches(\"Failed|reject\")",
        "--exec", "track_sum(\"errors\", 1); track_unique(\"programs\", e.prog);",
        "--end", "print(`Total errors: ${metrics[\"errors\"]}, Programs: ${metrics[\"programs\"].len()}`);"
    ], input);
    assert_eq!(exit_code, 0, "syslog filtering should succeed");

    // Should find 3 error messages (2 failed passwords, 1 postfix reject)
    assert!(
        stdout.contains("Total errors: 3"),
        "Should count 3 error messages"
    );
    assert!(
        stdout.contains("Programs: 2"),
        "Should identify 2 different programs (sshd, postfix)"
    );
}

#[test]
fn test_syslog_severity_analysis() {
    let input = r#"<165>1 2023-10-11T22:14:15.003Z server01 sshd 1234 ID47 - Failed password for user alice
<30>1 2023-10-11T22:14:16.123Z server01 nginx 5678 - - Request processed successfully
<11>1 2023-10-11T22:14:17.456Z server01 postgres 2345 - - Database connection established"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "syslog",
        "--exec", "e.sev_name = if e.severity == 5 { \"notice\" } else if e.severity == 6 { \"info\" } else if e.severity == 3 { \"error\" } else { \"other\" }; track_count(\"severities\", e.sev_name);",
        "--end", "let counts = metrics[\"severities\"]; print(`notice: ${counts.get(\"notice\") ?? 0}, info: ${counts.get(\"info\") ?? 0}, error: ${counts.get(\"error\") ?? 0}`);"
    ], input);
    assert_eq!(exit_code, 0, "syslog severity analysis should succeed");

    // Verify severity distribution
    // 165 & 7 = 5 (notice), 30 & 7 = 6 (info), 11 & 7 = 3 (error)
    assert!(stdout.contains("notice: 1"), "Should have 1 notice message");
    assert!(stdout.contains("info: 1"), "Should have 1 info message");
    assert!(stdout.contains("error: 1"), "Should have 1 error message");
}

#[test]
fn test_syslog_with_file() {
    let syslog_content = r#"<34>Jan 15 10:00:00 webserver nginx: 192.168.1.10 - - [15/Jan/2024:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 612
<27>Jan 15 10:00:30 batch01 cron: nightly job started
<27>Jan 15 10:00:45 webserver nginx: 192.168.1.30 - - [15/Jan/2024:10:00:45 +0000] "GET /api/data HTTP/1.1" 500 0
<27>Jan 15 10:00:55 db01 postgres: connection accepted
"#;

    let (stdout, _stderr, exit_code) = run_kelora_with_file(
        &[
            "-f",
            "syslog",
            "--filter",
            "e.host == \"webserver\"",
            "-F",
            "json",
        ],
        syslog_content,
    );
    assert_eq!(exit_code, 0, "syslog file processing should succeed");

    // Should only show entries from webserver host
    let lines: Vec<&str> = stdout.trim().lines().collect();
    for line in lines {
        if line.starts_with('{') {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("Should be valid JSON");
            assert_eq!(parsed["host"].as_str().unwrap(), "webserver");
        }
    }
}

#[test]
fn test_apache_combined_format_parsing() {
    let input = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08"
127.0.0.1 - - [25/Dec/1995:10:00:01 +0000] "POST /api/data HTTP/1.1" 201 456 "-" "curl/7.68.0"
10.0.0.1 - admin [25/Dec/1995:10:00:02 +0000] "GET /admin/dashboard HTTP/1.1" 403 - "https://admin.example.com/" "Mozilla/5.0""#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "combined", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Apache parsing should succeed");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "Should parse 3 Apache log lines");

    // Check first line (Combined format with all fields)
    let first_line: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(first_line["ip"].as_str().unwrap(), "192.168.1.1");
    assert_eq!(first_line["user"].as_str().unwrap(), "user");
    assert_eq!(first_line["method"].as_str().unwrap(), "GET");
    assert_eq!(first_line["path"].as_str().unwrap(), "/index.html");
    assert_eq!(first_line["protocol"].as_str().unwrap(), "HTTP/1.0");
    assert_eq!(first_line["status"].as_i64().unwrap(), 200);
    assert_eq!(first_line["bytes"].as_i64().unwrap(), 1234);
    assert_eq!(
        first_line["referer"].as_str().unwrap(),
        "http://www.example.com/"
    );
    assert_eq!(first_line["user_agent"].as_str().unwrap(), "Mozilla/4.08");

    // Check second line (POST with dashes for user)
    let second_line: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(second_line["ip"].as_str().unwrap(), "127.0.0.1");
    assert!(second_line.get("user").is_none()); // Should be null for "-"
    assert_eq!(second_line["method"].as_str().unwrap(), "POST");
    assert_eq!(second_line["path"].as_str().unwrap(), "/api/data");
    assert_eq!(second_line["status"].as_i64().unwrap(), 201);
    assert_eq!(second_line["user_agent"].as_str().unwrap(), "curl/7.68.0");

    // Check third line (403 error with no bytes)
    let third_line: serde_json::Value =
        serde_json::from_str(lines[2]).expect("Third line should be valid JSON");
    assert_eq!(third_line["status"].as_i64().unwrap(), 403);
    assert!(third_line.get("bytes").is_none()); // Should be null for "-"
}

#[test]
fn test_apache_common_format_parsing() {
    let input = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234
127.0.0.1 - - [25/Dec/1995:10:00:01 +0000] "POST /api/data HTTP/1.1" 201 456"#;

    let (stdout, _stderr, exit_code) =
        run_kelora_with_input(&["-f", "combined", "-F", "json"], input);
    assert_eq!(exit_code, 0, "Apache common format parsing should succeed");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "Should parse 2 Apache common log lines");

    // Check that referer and user_agent fields are not present (common format)
    for line in lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("Should be valid JSON");
        assert!(
            parsed.get("referer").is_none(),
            "Common format should not have referer"
        );
        assert!(
            parsed.get("user_agent").is_none(),
            "Common format should not have user_agent"
        );
        assert!(parsed.get("ip").is_some(), "Should have IP address");
        assert!(parsed.get("method").is_some(), "Should have HTTP method");
        assert!(parsed.get("status").is_some(), "Should have status code");
    }
}

#[test]
fn test_apache_filtering_and_analysis() {
    let input = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08"
127.0.0.1 - - [25/Dec/1995:10:00:01 +0000] "POST /api/data HTTP/1.1" 404 0 "-" "curl/7.68.0"
10.0.0.1 - admin [25/Dec/1995:10:00:02 +0000] "GET /admin/dashboard HTTP/1.1" 403 - "https://admin.example.com/" "Mozilla/5.0"
192.168.1.50 - - [25/Dec/1995:10:00:03 +0000] "GET /favicon.ico HTTP/1.1" 500 1024 "http://www.site.com/" "Safari/537.36""#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "combined",
        "--filter", "e.status >= 400",
        "--exec", "track_sum(\"errors\", 1); track_count(\"methods\", e.method);",
        "--end", "let methods = metrics[\"methods\"]; print(`Total errors: ${metrics[\"errors\"]}, GET: ${methods.get(\"GET\") ?? 0}, POST: ${methods.get(\"POST\") ?? 0}`);"
    ], input);
    assert_eq!(exit_code, 0, "Apache filtering should succeed");

    // Should find 3 error responses (404, 403, 500)
    assert!(
        stdout.contains("Total errors: 3"),
        "Should count 3 error responses"
    );
    assert!(stdout.contains("GET: 2"), "Should have 2 GET errors");
    assert!(stdout.contains("POST: 1"), "Should have 1 POST error");
}

#[test]
fn test_apache_status_code_analysis() {
    let input = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08"
127.0.0.1 - - [25/Dec/1995:10:00:01 +0000] "POST /api/data HTTP/1.1" 201 456 "-" "curl/7.68.0"
10.0.0.1 - admin [25/Dec/1995:10:00:02 +0000] "GET /admin/dashboard HTTP/1.1" 403 - "https://admin.example.com/" "Mozilla/5.0"
192.168.1.50 - - [25/Dec/1995:10:00:03 +0000] "GET /favicon.ico HTTP/1.1" 500 1024 "http://www.site.com/" "Safari/537.36""#;

    let (stdout, _stderr, exit_code) = run_kelora_with_input(&[
        "-f", "combined",
        "--exec", "e.class = if e.status < 300 { \"2xx\" } else if e.status < 400 { \"3xx\" } else if e.status < 500 { \"4xx\" } else { \"5xx\" }; track_count(\"status_classes\", e.class);",
        "--end", "let classes = metrics[\"status_classes\"]; print(`2xx: ${classes.get(\"2xx\") ?? 0}, 4xx: ${classes.get(\"4xx\") ?? 0}, 5xx: ${classes.get(\"5xx\") ?? 0}`);"
    ], input);
    assert_eq!(exit_code, 0, "Apache status code analysis should succeed");

    // Verify status code distribution: 200, 201 (2xx), 403 (4xx), 500 (5xx)
    assert!(stdout.contains("2xx: 2"), "Should have 2 success responses");
    assert!(stdout.contains("4xx: 1"), "Should have 1 client error");
    assert!(stdout.contains("5xx: 1"), "Should have 1 server error");
}

#[test]
fn test_per_file_csv_schema_detection_sequential() {
    // Test per-file CSV schema detection in sequential mode
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"name,age\nAlice,30\nBob,25\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"user,score,level\nCharlie,95,A\nDave,88,B\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f", "csv",
            "--exec", "let fields = e.keys(); print(\"File: \" + meta.filename + \", Fields: \" + fields.join(\",\"))"
        ],
        &[temp_file1.path().to_str().unwrap(), temp_file2.path().to_str().unwrap()],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Fields: name,age") || stdout.contains("Fields: age,name"),
        "Should detect schema for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("Fields: user,score,level")
            || stdout.contains("Fields: level,score,user")
            || stdout.contains("Fields: score,user,level"),
        "Should detect schema for file2: {}",
        stdout
    );
}

#[test]
fn test_per_file_csv_schema_detection_parallel() {
    // Test per-file CSV schema detection in parallel mode
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"name,age\nAlice,30\nBob,25\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"user,score,level\nCharlie,95,A\nDave,88,B\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f", "csv",
            "--parallel",
            "--exec", "let fields = e.keys(); print(\"File: \" + meta.filename + \", Fields: \" + fields.join(\",\"))"
        ],
        &[temp_file1.path().to_str().unwrap(), temp_file2.path().to_str().unwrap()],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Fields: name,age") || stdout.contains("Fields: age,name"),
        "Should detect schema for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("Fields: user,score,level")
            || stdout.contains("Fields: level,score,user")
            || stdout.contains("Fields: score,user,level"),
        "Should detect schema for file2: {}",
        stdout
    );
}

#[test]
fn test_csv_with_different_column_counts() {
    // Test CSV files with different numbers of columns
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"a,b\n1,2\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"x,y,z,w\n10,20,30,40\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f", "csv",
            "--exec", "let count = e.keys().len(); print(\"File: \" + meta.filename + \", Columns: \" + count)"
        ],
        &[temp_file1.path().to_str().unwrap(), temp_file2.path().to_str().unwrap()],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("Columns: 2"),
        "Should detect 2 columns in file1: {}",
        stdout
    );
    assert!(
        stdout.contains("Columns: 4"),
        "Should detect 4 columns in file2: {}",
        stdout
    );
}

#[test]
fn test_csv_ragged_rows_preserved_with_hint() {
    let input = "name,age\nalice,30\nbob,25,engineer,boston\ncarol\n";

    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "csv", "-F", "json"], input);
    assert_eq!(exit_code, 0, "ragged rows are not errors in resilient mode");

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "no row may be dropped: {}", stdout);

    // Overflow columns keep positional names
    let bob: serde_json::Value = serde_json::from_str(lines[1]).expect("valid JSON");
    assert_eq!(bob["name"].as_str().unwrap(), "bob");
    assert_eq!(bob["c3"].as_str().unwrap(), "engineer");
    assert_eq!(bob["c4"].as_str().unwrap(), "boston");

    // Short rows leave fields absent, not empty
    let carol: serde_json::Value = serde_json::from_str(lines[2]).expect("valid JSON");
    assert_eq!(carol["name"].as_str().unwrap(), "carol");
    assert!(carol.get("age").is_none(), "missing field must stay absent");

    // Counted diagnostic on stderr, with a concrete way to find the rows
    assert!(
        stderr.contains("Ragged rows")
            && stderr.contains("1 row with more columns")
            && stderr.contains("1 row with fewer columns"),
        "expected ragged-row hint, got: {}",
        stderr
    );
    assert!(
        stderr.contains("may be misaligned") && stderr.contains("\"c3\" in e"),
        "hint should warn about misalignment and suggest an inspection filter: {}",
        stderr
    );
}

#[test]
fn test_csv_ragged_rows_rejected_under_strict() {
    let input = "name,age\nalice,30\nbob,25,engineer\n";

    let (_stdout, stderr, exit_code) =
        run_kelora_with_input(&["-f", "csv", "--strict", "-F", "json"], input);
    assert_ne!(exit_code, 0, "--strict must reject ragged rows");
    assert!(
        stderr.contains("expected 2"),
        "error should report expected column count: {}",
        stderr
    );
}

#[test]
fn test_csv_no_headers_with_filename_tracking() {
    // Test CSV without headers but with filename tracking
    let mut temp_file1 = NamedTempFile::new().expect("Failed to create temp file");
    let mut temp_file2 = NamedTempFile::new().expect("Failed to create temp file");

    temp_file1
        .write_all(b"alice,30\nbob,25\n")
        .expect("Failed to write to temp file");
    temp_file2
        .write_all(b"charlie,95,A\ndave,88,B\n")
        .expect("Failed to write to temp file");

    let (stdout, _stderr, exit_code) = run_kelora_with_files(
        &[
            "-f",
            "csvnh",
            "--exec",
            "print(\"File: \" + meta.filename + \", Col1: \" + e.c1 + \", Col2: \" + e.c2)",
        ],
        &[
            temp_file1.path().to_str().unwrap(),
            temp_file2.path().to_str().unwrap(),
        ],
    );

    assert_eq!(exit_code, 0, "Should exit successfully");
    assert!(
        stdout.contains("File: ") && stdout.contains("Col1: alice"),
        "Should show filename and data for file1: {}",
        stdout
    );
    assert!(
        stdout.contains("File: ") && stdout.contains("Col1: charlie"),
        "Should show filename and data for file2: {}",
        stdout
    );
}
