use crate::event::Event;
use crate::pipeline::EventParser;
use anyhow::{Context, Result};
use regex::Regex;
use rhai::Dynamic;

pub struct CombinedParser {
    combined_regex: Regex,
    combined_with_request_time_regex: Regex,
    common_regex: Regex,
    auto_timestamp: bool,
}

impl CombinedParser {
    fn build(auto_timestamp: bool) -> Result<Self> {
        // Combined Log Format pattern (Apache/NGINX with referer and user agent)
        // Example: 192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08"
        let combined_regex = Regex::new(
            r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "([^"]*)" (\d+) (\S+)(?: "([^"]*)" "([^"]*)")?(?:\r?\n)?$"#,
        )
        .context("Failed to compile Combined Log Format regex")?;

        // Combined Log Format with optional request time (NGINX-specific)
        // Example: 192.168.1.1 - - [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08" "0.123"
        // nginx configs log $request_time quoted or bare (`... "Mozilla/4.08" 0.123`); group 10
        // captures the quoted form, group 11 the bare number.
        let combined_with_request_time_regex = Regex::new(
            r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "([^"]*)" (\d+) (\S+)(?: "([^"]*)" "([^"]*)"(?: (?:"([^"]*)"|([0-9]+(?:\.[0-9]+)?)))?)?(?:\r?\n)?$"#
        ).context("Failed to compile Combined Log Format with request time regex")?;

        // Common Log Format pattern (Apache/NGINX basic format)
        // Example: 192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234
        let common_regex =
            Regex::new(r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "([^"]*)" (\d+) (\S+)(?:\r?\n)?$"#)
                .context("Failed to compile Common Log Format regex")?;

        Ok(Self {
            combined_regex,
            combined_with_request_time_regex,
            common_regex,
            auto_timestamp,
        })
    }

    pub fn new() -> Result<Self> {
        Self::build(true)
    }

    pub fn new_without_auto_timestamp() -> Result<Self> {
        Self::build(false)
    }

    /// Parse HTTP request string into method, path, and protocol
    fn parse_request(request: &str, event: &mut Event) {
        let parts: Vec<&str> = request.splitn(3, ' ').collect();
        if let Some(method) = parts.first() {
            event.set_field("method".to_string(), Dynamic::from(method.to_string()));
        }
        if let Some(path) = parts.get(1) {
            event.set_field("path".to_string(), Dynamic::from(path.to_string()));
        }
        if let Some(protocol) = parts.get(2) {
            event.set_field("protocol".to_string(), Dynamic::from(protocol.to_string()));
        }
    }

    /// Parse request time to float if possible
    fn parse_request_time(time_str: &str) -> Option<f64> {
        time_str.parse::<f64>().ok()
    }

    /// Set field if value is not "-"
    fn set_field_if_not_dash(event: &mut Event, field_name: &str, value: &str) {
        if value != "-" {
            event.set_field(field_name.to_string(), Dynamic::from(value.to_string()));
        }
    }

    /// Set numeric field if value is not "-" and can be parsed
    fn set_numeric_field_if_valid(event: &mut Event, field_name: &str, value: &str) {
        if value != "-" {
            if let Ok(num) = value.parse::<i64>() {
                event.set_field(field_name.to_string(), Dynamic::from(num));
            }
        }
    }

    /// Try to parse as Combined Log Format with optional request time (NGINX-style)
    fn try_parse_combined_with_request_time(&self, line: &str) -> Option<Event> {
        if let Some(captures) = self.combined_with_request_time_regex.captures(line) {
            let mut event = Event::with_capacity(line.to_string(), 13);

            // IP address
            if let Some(ip) = captures.get(1) {
                event.set_field("ip".to_string(), Dynamic::from(ip.as_str().to_string()));
            }

            // Identity (usually -)
            if let Some(identity) = captures.get(2) {
                Self::set_field_if_not_dash(&mut event, "identity", identity.as_str());
            }

            // User (usually -)
            if let Some(user) = captures.get(3) {
                Self::set_field_if_not_dash(&mut event, "user", user.as_str());
            }

            // Timestamp
            if let Some(timestamp) = captures.get(4) {
                event.set_field(
                    "ts".to_string(),
                    Dynamic::from(timestamp.as_str().to_string()),
                );
            }

            // Request
            if let Some(request) = captures.get(5) {
                let request_str = request.as_str();
                event.set_field(
                    "request".to_string(),
                    Dynamic::from(request_str.to_string()),
                );
                Self::parse_request(request_str, &mut event);
            }

            // Status code
            if let Some(status) = captures.get(6) {
                if let Ok(status_code) = status.as_str().parse::<i64>() {
                    event.set_field("status".to_string(), Dynamic::from(status_code));
                }
            }

            // Bytes
            if let Some(bytes) = captures.get(7) {
                Self::set_numeric_field_if_valid(&mut event, "bytes", bytes.as_str());
            }

            // Referer (Combined format only)
            if let Some(referer) = captures.get(8) {
                Self::set_field_if_not_dash(&mut event, "referer", referer.as_str());
            }

            // User agent (Combined format only)
            if let Some(user_agent) = captures.get(9) {
                Self::set_field_if_not_dash(&mut event, "user_agent", user_agent.as_str());
            }

            // Request time (NGINX-specific, optional)
            if let Some(request_time) = captures.get(10).or_else(|| captures.get(11)) {
                let time_str = request_time.as_str();
                if time_str != "-" {
                    if let Some(time_float) = Self::parse_request_time(time_str) {
                        event.set_field("request_time".to_string(), Dynamic::from(time_float));
                    }
                }
            }

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Some(event)
        } else {
            None
        }
    }

    /// Try to parse as Combined Log Format (Apache-style)
    fn try_parse_combined(&self, line: &str) -> Option<Event> {
        if let Some(captures) = self.combined_regex.captures(line) {
            let mut event = Event::with_capacity(line.to_string(), 12);

            // IP address
            if let Some(ip) = captures.get(1) {
                event.set_field("ip".to_string(), Dynamic::from(ip.as_str().to_string()));
            }

            // Identity (usually -)
            if let Some(identity) = captures.get(2) {
                Self::set_field_if_not_dash(&mut event, "identity", identity.as_str());
            }

            // User (usually -)
            if let Some(user) = captures.get(3) {
                Self::set_field_if_not_dash(&mut event, "user", user.as_str());
            }

            // Timestamp
            if let Some(timestamp) = captures.get(4) {
                event.set_field(
                    "ts".to_string(),
                    Dynamic::from(timestamp.as_str().to_string()),
                );
            }

            // Request
            if let Some(request) = captures.get(5) {
                let request_str = request.as_str();
                event.set_field(
                    "request".to_string(),
                    Dynamic::from(request_str.to_string()),
                );
                Self::parse_request(request_str, &mut event);
            }

            // Status code
            if let Some(status) = captures.get(6) {
                if let Ok(status_code) = status.as_str().parse::<i64>() {
                    event.set_field("status".to_string(), Dynamic::from(status_code));
                }
            }

            // Bytes
            if let Some(bytes) = captures.get(7) {
                Self::set_numeric_field_if_valid(&mut event, "bytes", bytes.as_str());
            }

            // Referer (Combined format only)
            if let Some(referer) = captures.get(8) {
                Self::set_field_if_not_dash(&mut event, "referer", referer.as_str());
            }

            // User agent (Combined format only)
            if let Some(user_agent) = captures.get(9) {
                Self::set_field_if_not_dash(&mut event, "user_agent", user_agent.as_str());
            }

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Some(event)
        } else {
            None
        }
    }

    /// Try to parse as Common Log Format
    fn try_parse_common(&self, line: &str) -> Option<Event> {
        if let Some(captures) = self.common_regex.captures(line) {
            let mut event = Event::with_capacity(line.to_string(), 10);

            // IP address
            if let Some(ip) = captures.get(1) {
                event.set_field("ip".to_string(), Dynamic::from(ip.as_str().to_string()));
            }

            // Identity (usually -)
            if let Some(identity) = captures.get(2) {
                Self::set_field_if_not_dash(&mut event, "identity", identity.as_str());
            }

            // User (usually -)
            if let Some(user) = captures.get(3) {
                Self::set_field_if_not_dash(&mut event, "user", user.as_str());
            }

            // Timestamp
            if let Some(timestamp) = captures.get(4) {
                event.set_field(
                    "ts".to_string(),
                    Dynamic::from(timestamp.as_str().to_string()),
                );
            }

            // Request
            if let Some(request) = captures.get(5) {
                let request_str = request.as_str();
                event.set_field(
                    "request".to_string(),
                    Dynamic::from(request_str.to_string()),
                );
                Self::parse_request(request_str, &mut event);
            }

            // Status code
            if let Some(status) = captures.get(6) {
                if let Ok(status_code) = status.as_str().parse::<i64>() {
                    event.set_field("status".to_string(), Dynamic::from(status_code));
                }
            }

            // Bytes
            if let Some(bytes) = captures.get(7) {
                Self::set_numeric_field_if_valid(&mut event, "bytes", bytes.as_str());
            }

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Some(event)
        } else {
            None
        }
    }
}

impl EventParser for CombinedParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        // Try Combined format with request time first (NGINX-style)
        if let Some(event) = self.try_parse_combined_with_request_time(line) {
            Ok(event)
        }
        // Then try Combined format without request time (Apache-style)
        else if let Some(event) = self.try_parse_combined(line) {
            Ok(event)
        }
        // Finally try Common format
        else if let Some(event) = self.try_parse_common(line) {
            Ok(event)
        } else {
            Err(anyhow::anyhow!(
                "Invalid combined log format: {}",
                diagnose_combined_failure(line)
            ))
        }
    }
}

/// The parts of a combined/common log line in order, each with the pattern for
/// the part *including* its leading separator. Mirrors the parse regexes above;
/// used only on the failure path to say where a line stopped matching (#362).
static COMBINED_PARTS: std::sync::LazyLock<Vec<(&'static str, &'static str, Regex)>> =
    std::sync::LazyLock::new(|| {
        [
            ("ip", "a client address", r"^\S+"),
            ("identity", "an identity field ('-' if unused)", r"^ \S+"),
            ("user", "a user field ('-' if unused)", r"^ \S+"),
            ("ts", "a timestamp in [brackets]", r"^ \[[^\]]+\]"),
            (
                "request",
                "a quoted request (\"GET /path HTTP/1.1\")",
                r#"^ "[^"]*""#,
            ),
            ("status", "a numeric status code", r"^ \d+"),
            ("bytes", "a byte count ('-' if none)", r"^ \S+"),
        ]
        .into_iter()
        .map(|(name, what, re)| {
            (
                name,
                what,
                Regex::new(re).expect("valid combined part regex"),
            )
        })
        .collect()
    });

static COMBINED_REFERER_AGENT: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r#"^ "[^"]*" "[^"]*""#).expect("valid referer/user_agent regex")
});

static COMBINED_REQUEST_TIME: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r#"^ (?:"[^"]*"|[0-9]+(?:\.[0-9]+)?)"#).expect("valid request_time regex")
});

/// Explain where `line` stops being a combined/common log line: either which
/// leading part did not match, or what unexpected text follows the last part
/// that did. The parser stays strict — this only names the reason.
fn diagnose_combined_failure(line: &str) -> String {
    fn column(line: &str, byte_pos: usize) -> usize {
        line[..byte_pos].chars().count() + 1
    }
    fn snippet(text: &str) -> String {
        const MAX_CHARS: usize = 40;
        if text.chars().count() > MAX_CHARS {
            let cut: String = text.chars().take(MAX_CHARS - 3).collect();
            format!("{cut}...")
        } else {
            text.to_string()
        }
    }

    let mut pos = 0;
    for (i, (_, what, re)) in COMBINED_PARTS.iter().enumerate() {
        match re.find(&line[pos..]) {
            Some(m) => pos += m.end(),
            None => {
                // Every part after the first is preceded by exactly one space.
                let sep = if i == 0 { 0 } else { 1 };
                let rest = &line[pos..];
                let found = rest.get(sep..).unwrap_or("");
                let col = column(line, (pos + sep).min(line.len()));
                return if rest.trim().is_empty() {
                    format!("line ends at column {col} where {what} was expected")
                } else if found.starts_with(char::is_whitespace) {
                    format!("extra whitespace at column {col} where {what} was expected")
                } else {
                    format!(
                        "expected {what} at column {col}, found '{}'",
                        snippet(found)
                    )
                };
            }
        }
    }

    let mut last = "bytes";
    if let Some(m) = COMBINED_REFERER_AGENT.find(&line[pos..]) {
        pos += m.end();
        last = "user_agent";
        if let Some(m) = COMBINED_REQUEST_TIME.find(&line[pos..]) {
            pos += m.end();
            last = "request_time";
        }
    }

    let rest = &line[pos..];
    let trimmed = rest.trim_start();
    let col = column(line, pos + (rest.len() - trimmed.len()));
    if trimmed.is_empty() {
        return format!(
            "unexpected trailing whitespace after {last} at column {}",
            column(line, pos)
        );
    }
    let expected = match last {
        "bytes" => "end of line, or \"referer\" \"user_agent\"",
        "user_agent" => "end of line, or a request_time",
        _ => "end of line",
    };
    format!(
        "unexpected trailing text after {last} at column {col}: '{}' (expected {expected})",
        snippet(trimmed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::EventParser;

    #[test]
    fn test_apache_combined_format() {
        let parser = CombinedParser::new().unwrap();
        let line = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08""#;
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            result
                .fields
                .get("user")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "user"
        );
        assert_eq!(
            result
                .fields
                .get("method")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET"
        );
        assert_eq!(
            result
                .fields
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/index.html"
        );
        assert_eq!(
            result
                .fields
                .get("protocol")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "HTTP/1.0"
        );
        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(result.fields.get("bytes").unwrap().as_int().unwrap(), 1234);
        assert_eq!(
            result
                .fields
                .get("referer")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "http://www.example.com/"
        );
        assert_eq!(
            result
                .fields
                .get("user_agent")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Mozilla/4.08"
        );
        // Should not have request_time for Apache format
        assert!(result.fields.get("request_time").is_none());
    }

    #[test]
    fn test_nginx_combined_with_request_time() {
        let parser = CombinedParser::new().unwrap();
        let line = r#"192.168.1.1 - - [25/Dec/1995:10:00:00 +0000] "GET /api/test HTTP/1.1" 200 1234 "-" "curl/7.68.0" "0.123""#;
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            result
                .fields
                .get("method")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET"
        );
        assert_eq!(
            result
                .fields
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/api/test"
        );
        assert_eq!(
            result
                .fields
                .get("protocol")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "HTTP/1.1"
        );
        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(result.fields.get("bytes").unwrap().as_int().unwrap(), 1234);
        assert_eq!(
            result
                .fields
                .get("user_agent")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "curl/7.68.0"
        );
        assert!(
            (result
                .fields
                .get("request_time")
                .unwrap()
                .as_float()
                .unwrap()
                - 0.123)
                .abs()
                < f64::EPSILON
        );
        // Referer should not be set for "-"
        assert!(result.fields.get("referer").is_none());
    }

    #[test]
    fn test_common_format() {
        let parser = CombinedParser::new().unwrap();
        let line = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234"#;
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            result
                .fields
                .get("user")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "user"
        );
        assert_eq!(
            result
                .fields
                .get("method")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET"
        );
        assert_eq!(
            result
                .fields
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/index.html"
        );
        assert_eq!(
            result
                .fields
                .get("protocol")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "HTTP/1.0"
        );
        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(result.fields.get("bytes").unwrap().as_int().unwrap(), 1234);
        assert!(result.fields.get("referer").is_none());
        assert!(result.fields.get("user_agent").is_none());
        assert!(result.fields.get("request_time").is_none());
    }

    #[test]
    fn test_with_dashes() {
        let parser = CombinedParser::new().unwrap();
        let line = r#"127.0.0.1 - - [25/Dec/1995:10:00:00 +0000] "GET / HTTP/1.0" 200 -"#;
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "127.0.0.1"
        );
        assert!(result.fields.get("identity").is_none());
        assert!(result.fields.get("user").is_none());
        assert_eq!(
            result
                .fields
                .get("method")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET"
        );
        assert_eq!(
            result
                .fields
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/"
        );
        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert!(result.fields.get("bytes").is_none());
    }

    #[test]
    fn test_invalid_format() {
        let parser = CombinedParser::new().unwrap();
        let line = "This is not a log line";
        assert!(EventParser::parse(&parser, line).is_err());
    }

    fn parse_error(line: &str) -> String {
        let parser = CombinedParser::new().unwrap();
        EventParser::parse(&parser, line).unwrap_err().to_string()
    }

    #[test]
    fn test_error_names_trailing_text_after_user_agent() {
        // #362: one extra field after user_agent must be named, not just rejected.
        let err = parse_error(
            r#"10.0.4.77 - - [14/Jul/2024:00:01:02 +0000] "PUT /v1/users/me HTTP/1.1" 200 2314 "https://acme.example/cart" "curl/8.4.0" trace=abc123"#,
        );
        assert_eq!(
            err,
            "Invalid combined log format: unexpected trailing text after user_agent at column 122: 'trace=abc123' (expected end of line, or a request_time)"
        );
    }

    #[test]
    fn test_error_names_trailing_text_after_request_time() {
        let err = parse_error(
            r#"10.0.0.1 - - [26/Jul/2026:13:40:00 +0000] "GET /x HTTP/1.1" 200 123 "-" "curl/8" 0.010 upstream=api"#,
        );
        assert!(
            err.contains("unexpected trailing text after request_time")
                && err.contains("'upstream=api'"),
            "{err}"
        );
    }

    #[test]
    fn test_error_names_trailing_text_after_bytes() {
        let err = parse_error(
            r#"10.0.0.1 - - [26/Jul/2026:13:40:00 +0000] "GET /x HTTP/1.1" 200 123 rt=0.1"#,
        );
        assert!(
            err.contains("unexpected trailing text after bytes") && err.contains("'rt=0.1'"),
            "{err}"
        );
    }

    #[test]
    fn test_error_names_the_leading_part_that_did_not_match() {
        let err = parse_error(r#"10.0.0.1 - - 26/Jul/2026:13:40:00 "GET / HTTP/1.1" 200 1"#);
        assert!(
            err.contains("expected a timestamp in [brackets] at column 14, found '26/Jul"),
            "{err}"
        );

        let err = parse_error(r#"10.0.0.1 - - [26/Jul/2026:13:40:00 +0000] "GET / HTTP/1.1" OK 1"#);
        assert!(err.contains("expected a numeric status code"), "{err}");

        let err = parse_error("garbage");
        assert!(
            err.contains("line ends at column 8 where an identity field"),
            "{err}"
        );

        let err = parse_error(
            r#"10.0.0.1 - - [26/Jul/2026:13:40:00 +0000] "GET / HTTP/1.1" 200 1 "-" "ua" "#,
        );
        assert!(
            err.contains("unexpected trailing whitespace after user_agent"),
            "{err}"
        );
    }

    #[test]
    fn test_nginx_vs_apache_compatibility() {
        let parser = CombinedParser::new().unwrap();

        // Test that both Apache and NGINX style logs work
        let apache_line = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08""#;
        let nginx_line = r#"192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://www.example.com/" "Mozilla/4.08" "0.050""#;

        let apache_result = EventParser::parse(&parser, apache_line).unwrap();
        let nginx_result = EventParser::parse(&parser, nginx_line).unwrap();

        // Both should parse successfully
        assert_eq!(
            apache_result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            nginx_result
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );

        // Apache result should not have request_time
        assert!(apache_result.fields.get("request_time").is_none());

        // NGINX result should have request_time
        assert!(nginx_result.fields.get("request_time").is_some());
        assert!(
            (nginx_result
                .fields
                .get("request_time")
                .unwrap()
                .as_float()
                .unwrap()
                - 0.050)
                .abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn test_nginx_combined_with_unquoted_request_time() {
        let parser = CombinedParser::new().unwrap();
        let line = r#"10.1.3.55 - - [06/Sep/2024:10:00:03 +0000] "GET /api/cart HTTP/1.1" 200 8437 "-" "curl/8.0" 0.02"#;
        let result = parser.parse(line).unwrap();
        assert_eq!(result.fields.get("path").unwrap().to_string(), "/api/cart");
        assert_eq!(
            result
                .fields
                .get("request_time")
                .unwrap()
                .as_float()
                .unwrap(),
            0.02
        );
    }
}
