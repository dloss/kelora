use crate::config::InputFormat as ConfigInputFormat;
use crate::parsers::{CefParser, CombinedParser, LogfmtParser, SyslogParser};
use crate::pipeline::EventParser;
use anyhow::Result;

/// Auto-detect the input format based on the first line of input.
/// Tries formats in order of specificity/commonality with 'line' as fallback.
///
/// Format detection priority:
/// 1. JSON - starts with '{' and valid JSON
/// 2. CEF - starts with "CEF:"
/// 3. Syslog - matches RFC5424 or RFC3164 patterns
/// 4. Combined - contains common Apache/Nginx log patterns
/// 5. Logfmt - contains key=value pairs
/// 6. CSV/TSV - contains delimiters with reasonable structure
/// 7. Named application-log formats adapted from lnav (regex-based)
/// 8. Line - fallback for everything else
pub fn detect_format(sample_line: &str) -> Result<ConfigInputFormat> {
    let trimmed = sample_line.trim();

    // Empty line detection - default to line format
    if trimmed.is_empty() {
        return Ok(ConfigInputFormat::Line);
    }

    // 1. JSON detection - most specific
    if detect_json(trimmed) {
        return Ok(ConfigInputFormat::Json);
    }

    // 2. CEF detection - very specific prefix
    if detect_cef(trimmed) {
        return Ok(ConfigInputFormat::Cef);
    }

    // 3. Syslog detection - structured patterns
    if detect_syslog(trimmed) {
        return Ok(ConfigInputFormat::Syslog);
    }

    // 4. Combined log format detection (Apache/Nginx)
    if detect_combined_logs(trimmed) {
        return Ok(ConfigInputFormat::Combined);
    }

    // 6. Logfmt detection - key=value patterns
    if detect_logfmt(trimmed) {
        return Ok(ConfigInputFormat::Logfmt);
    }

    // 7. CSV/TSV detection
    if let Some(csv_format) = detect_csv_variants(trimmed) {
        return Ok(csv_format);
    }

    // 8. Built-in named application-log formats adapted from lnav.
    //    Tried last (just before the line fallback) so it can only reclassify
    //    input that would otherwise become `line` — never a format already
    //    detected above. Returns the named format (regex-backed) so the notice
    //    and stats show its name (e.g. "log4j") rather than a bare "regex".
    if let Some(fmt) = crate::parsers::lnav_formats::detect(trimmed) {
        return Ok(ConfigInputFormat::Named(fmt));
    }

    // 9. Fallback to line format
    Ok(ConfigInputFormat::Line)
}

/// Detect JSON format - starts with '{' and is valid JSON
fn detect_json(line: &str) -> bool {
    if !line.starts_with('{') {
        return false;
    }

    // Try to parse as JSON - if it succeeds, it's likely JSON
    serde_json::from_str::<serde_json::Value>(line).is_ok()
}

/// Detect CEF format using actual parser for 100% accuracy
fn detect_cef(line: &str) -> bool {
    // Use strict mode for detection - we only want true positives
    let parser = CefParser::new_without_auto_timestamp().with_strict(true);
    parser.parse(line).is_ok()
}

/// Detect Syslog format using actual parser for 100% accuracy
fn detect_syslog(line: &str) -> bool {
    // SyslogParser::new() compiles regexes, returns Result
    if let Ok(parser) = SyslogParser::new_without_auto_timestamp() {
        parser.parse(line).is_ok()
    } else {
        false // Regex compilation failed (shouldn't happen)
    }
}

/// Detect combined log formats (Apache/Nginx) using actual parser for 100% accuracy
fn detect_combined_logs(line: &str) -> bool {
    // CombinedParser::new() compiles regexes, returns Result
    if let Ok(parser) = CombinedParser::new_without_auto_timestamp() {
        parser.parse(line).is_ok()
    } else {
        false // Regex compilation failed (shouldn't happen)
    }
}

/// Detect logfmt format using actual parser for 100% accuracy
fn detect_logfmt(line: &str) -> bool {
    let parser = LogfmtParser::new_without_auto_timestamp();
    parser.parse(line).is_ok()
}

/// Detect CSV/TSV variants
fn detect_csv_variants(line: &str) -> Option<ConfigInputFormat> {
    let comma_count = line.matches(',').count();
    let tab_count = line.matches('\t').count();

    // Require multiple delimiters to distinguish from random commas/tabs in text
    if tab_count >= 2 {
        // Check if it could have headers vs no headers
        // If first field looks like a column name (letters), assume headers
        if let Some(first_field) = line.split('\t').next() {
            if first_field.chars().any(|c| c.is_ascii_alphabetic())
                && !first_field.chars().all(|c| c.is_ascii_digit())
            {
                return Some(ConfigInputFormat::Tsv(None));
            } else {
                return Some(ConfigInputFormat::Tsvnh);
            }
        }
    }

    if comma_count >= 2 {
        // Similar logic for CSV
        if let Some(first_field) = line.split(',').next() {
            let trimmed_field = first_field.trim_matches('"').trim();
            if trimmed_field.chars().any(|c| c.is_ascii_alphabetic())
                && !trimmed_field.chars().all(|c| c.is_ascii_digit())
            {
                return Some(ConfigInputFormat::Csv(None));
            } else {
                return Some(ConfigInputFormat::Csvnh);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::strategy::{BoxedStrategy, Strategy};

    #[test]
    fn test_detect_json() {
        assert_eq!(
            detect_format(r#"{"key": "value", "num": 42}"#).unwrap(),
            ConfigInputFormat::Json
        );
        assert_eq!(
            detect_format(r#"{"timestamp": "2023-04-15T10:00:00Z"}"#).unwrap(),
            ConfigInputFormat::Json
        );
    }

    #[test]
    fn test_detect_cef() {
        assert_eq!(
            detect_format("CEF:0|Vendor|Product|Version|EventID|Name|Severity|Extension").unwrap(),
            ConfigInputFormat::Cef
        );
    }

    #[test]
    fn test_detect_syslog() {
        assert_eq!(
            detect_format("<34>1 2023-04-15T10:00:00.000Z hostname app - - - message").unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("<13>Apr 15 10:00:00 hostname program: message").unwrap(),
            ConfigInputFormat::Syslog
        );
        // Test syslog format without priority field (common in processed logs)
        assert_eq!(
            detect_format("Jan 15 10:30:45 server1 sshd[1234]: Accepted publickey for user")
                .unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("Dec 25 23:59:59 hostname kernel: USB disconnect").unwrap(),
            ConfigInputFormat::Syslog
        );
    }

    #[test]
    fn test_detect_combined() {
        assert_eq!(
            detect_format(
                r#"192.168.1.1 - - [15/Apr/2023:10:00:00 +0000] "GET /path HTTP/1.1" 200 1234"#
            )
            .unwrap(),
            ConfigInputFormat::Combined
        );
    }

    #[test]
    fn test_detect_logfmt() {
        assert_eq!(
            detect_format("time=2023-04-15T10:00:00Z level=info msg=test").unwrap(),
            ConfigInputFormat::Logfmt
        );
        assert_eq!(
            detect_format("key1=value1 key2=value2 key3=value3").unwrap(),
            ConfigInputFormat::Logfmt
        );
    }

    #[test]
    fn test_detect_csv() {
        assert!(matches!(
            detect_format("name,age,city").unwrap(),
            ConfigInputFormat::Csv(_)
        ));
        assert!(matches!(
            detect_format("1,2,3").unwrap(),
            ConfigInputFormat::Csvnh
        ));
        assert!(matches!(
            detect_format("john\t25\tnyc").unwrap(),
            ConfigInputFormat::Tsv(_)
        )); // "john" has letters, so it's treated as header
        assert!(matches!(
            detect_format("name\tage\tcity").unwrap(),
            ConfigInputFormat::Tsv(_)
        ));
        assert!(matches!(
            detect_format("1\t2\t3").unwrap(),
            ConfigInputFormat::Tsvnh
        ));
        // All numeric, no headers
    }

    #[test]
    fn test_detect_lnav_named_formats() {
        // Application-log layouts that would previously fall through to `line`
        // are now detected as named, regex-backed formats (and the notice/stats
        // show the name rather than a bare "regex").
        for (line, expected) in [
            (
                "2024-01-02T15:04:05.123Z INFO Starting service on port 8080",
                "iso8601-level",
            ),
            (
                "2024-01-02 15:04:05,123 INFO [main] com.example.Service - up",
                "log4j",
            ),
            (
                "2024-01-02 15:04:05,123 - myapp.module - INFO - Service started",
                "python-logging",
            ),
            (
                "2024/01/02 15:04:05 [error] 29#29: *1 open() failed",
                "nginx-error",
            ),
            (
                "I0102 15:04:05.123456 1234 server.go:42] Starting controller",
                "glog",
            ),
        ] {
            match detect_format(line).unwrap() {
                ConfigInputFormat::Named(fmt) => {
                    assert_eq!(fmt.name, expected, "wrong named format for: {line}")
                }
                other => panic!("expected named format {expected} for {line}, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_lnav_detection_does_not_shadow_existing_formats() {
        // Formats detected before the lnav step must keep their classification.
        assert_eq!(
            detect_format(r#"{"a":1}"#).unwrap(),
            ConfigInputFormat::Json
        );
        assert_eq!(
            detect_format("Jan 15 10:30:45 server1 sshd[1234]: Accepted").unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("level=info msg=hi count=1").unwrap(),
            ConfigInputFormat::Logfmt
        );
    }

    #[test]
    fn test_detect_line_fallback() {
        assert_eq!(
            detect_format("just some random text").unwrap(),
            ConfigInputFormat::Line
        );
        assert_eq!(detect_format("").unwrap(), ConfigInputFormat::Line);
        assert_eq!(
            detect_format("a single word").unwrap(),
            ConfigInputFormat::Line
        );
    }

    fn lower_ascii(len: std::ops::RangeInclusive<usize>) -> BoxedStrategy<String> {
        prop::collection::vec(proptest::char::range('a', 'z'), len)
            .prop_map(|chars| chars.into_iter().collect())
            .boxed()
    }

    fn identifier() -> BoxedStrategy<String> {
        lower_ascii(1..=8)
    }

    fn json_value() -> BoxedStrategy<serde_json::Value> {
        let string_val = lower_ascii(0..=8)
            .prop_map(serde_json::Value::String)
            .boxed();

        let number_val = any::<i64>()
            .prop_map(|v| serde_json::Value::Number(serde_json::Number::from(v)))
            .boxed();

        let bool_val = any::<bool>().prop_map(serde_json::Value::Bool).boxed();

        prop_oneof![string_val, number_val, bool_val].boxed()
    }

    fn json_line() -> BoxedStrategy<String> {
        prop::collection::vec((identifier(), json_value()), 1..=4)
            .prop_map(|entries| {
                let mut map = serde_json::Map::new();
                for (k, v) in entries {
                    map.insert(k, v);
                }
                serde_json::Value::Object(map).to_string()
            })
            .boxed()
    }

    fn cef_line() -> BoxedStrategy<String> {
        (
            identifier(),
            identifier(),
            identifier(),
            identifier(),
            identifier(),
            0u8..=10,
            identifier(),
            identifier(),
        )
            .prop_map(|(vendor, product, version, signature, name, severity, ext_key, ext_value)| {
                format!(
                    "CEF:0|{vendor}|{product}|{version}|{signature}|{name}|{severity}|{ext_key}={ext_value}"
                )
            })
            .boxed()
    }

    fn csv_with_headers() -> BoxedStrategy<String> {
        prop::collection::vec(identifier(), 3..=5)
            .prop_map(|fields| fields.join(","))
            .boxed()
    }

    fn csv_without_headers() -> BoxedStrategy<String> {
        prop::collection::vec(0u16..=999, 3..=5)
            .prop_map(|nums| {
                nums.into_iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .boxed()
    }

    fn tsv_with_headers() -> BoxedStrategy<String> {
        prop::collection::vec(identifier(), 3..=5)
            .prop_map(|fields| fields.join("\t"))
            .boxed()
    }

    fn tsv_without_headers() -> BoxedStrategy<String> {
        prop::collection::vec(0u16..=999, 3..=5)
            .prop_map(|nums| {
                nums.into_iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .boxed()
    }

    fn plain_line() -> BoxedStrategy<String> {
        lower_ascii(5..=30)
    }

    proptest! {
        #[test]
        fn prop_detects_json(line in json_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Json);
        }

        #[test]
        fn prop_detects_cef(line in cef_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Cef);
        }

        #[test]
        fn prop_detects_csv_headers(line in csv_with_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Csv(None));
        }

        #[test]
        fn prop_detects_csv_no_headers(line in csv_without_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Csvnh);
        }

        #[test]
        fn prop_detects_tsv_headers(line in tsv_with_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Tsv(None));
        }

        #[test]
        fn prop_detects_tsv_no_headers(line in tsv_without_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Tsvnh);
        }

        #[test]
        fn prop_detects_line_fallback(line in plain_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Line);
        }
    }
}
