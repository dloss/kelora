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
            Err(anyhow::anyhow!("Invalid combined log format"))
        }
    }
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
