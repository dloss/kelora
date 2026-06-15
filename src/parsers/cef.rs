use crate::event::Event;
use crate::parsers::type_conversion::looks_like_json_number;
use crate::pipeline::EventParser;
use anyhow::Result;
use nom::Parser;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_until},
    character::complete::char,
    multi::many0,
    sequence::preceded,
    IResult,
};
use rhai::Dynamic;
use std::collections::HashMap;

pub struct CefParser {
    auto_timestamp: bool,
    strict: bool,
}

impl CefParser {
    pub fn new() -> Self {
        Self {
            auto_timestamp: true,
            strict: false,
        }
    }

    pub fn new_without_auto_timestamp() -> Self {
        Self {
            auto_timestamp: false,
            strict: false,
        }
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Parse syslog prefix before CEF: (optional timestamp and hostname)
    fn parse_syslog_prefix(input: &str) -> IResult<&str, (Option<&str>, Option<&str>)> {
        let (input, prefix) = take_until("CEF:")(input)?;
        let prefix = prefix.trim();

        if prefix.is_empty() {
            return Ok((input, (None, None)));
        }

        let tokens: Vec<&str> = prefix.split_whitespace().collect();
        let result = match tokens.len() {
            0 => (None, None),
            1 => (None, Some(tokens[0])), // Just hostname
            _ => {
                // Multiple tokens: last is hostname, rest is timestamp
                let hostname = tokens[tokens.len() - 1];
                let timestamp = prefix[..prefix.len() - hostname.len()].trim_end();
                (Some(timestamp), Some(hostname))
            }
        };

        Ok((input, result))
    }

    /// Parse escaped character in CEF header (handles \| \\ etc.)
    fn parse_escaped_char(input: &str) -> IResult<&str, char> {
        preceded(char('\\'), nom::character::complete::anychar).parse(input)
    }

    /// Parse unescaped character (not a pipe or backslash)
    fn parse_unescaped_char(input: &str) -> IResult<&str, char> {
        nom::character::complete::none_of("\\|").parse(input)
    }

    /// Parse a CEF header field (handles escaping)
    fn parse_cef_header_field(input: &str) -> IResult<&str, String> {
        let (input, chars) =
            many0(alt((Self::parse_escaped_char, Self::parse_unescaped_char))).parse(input)?;
        Ok((input, chars.into_iter().collect()))
    }

    /// Parse the 7 CEF header fields separated by pipes
    fn parse_cef_header(input: &str) -> IResult<&str, Vec<String>> {
        let (input, _) = tag("CEF:")(input)?;
        let (input, version) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, vendor) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, product) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, device_version) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, signature_id) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, name) = Self::parse_cef_header_field(input)?;
        let (input, _) = char('|')(input)?;
        let (input, severity) = Self::parse_cef_header_field(input)?;

        Ok((
            input,
            vec![
                version,
                vendor,
                product,
                device_version,
                signature_id,
                name,
                severity,
            ],
        ))
    }

    /// Parse all extension key=value pairs using simple approach
    fn parse_cef_extension(input: &str) -> IResult<&str, HashMap<String, String>> {
        let mut pairs = HashMap::new();
        let input = input.trim();

        if input.is_empty() {
            return Ok(("", pairs));
        }

        // Split by spaces first, then handle key=value pairs
        let parts: Vec<&str> = input.split_whitespace().collect();

        for part in parts {
            if let Some(eq_pos) = part.find('=') {
                let key = part[..eq_pos].to_string();
                let value = part[eq_pos + 1..].to_string();
                if !key.is_empty() {
                    // Handle escape sequences in value
                    let unescaped_value = value
                        .replace("\\=", "=")
                        .replace("\\|", "|")
                        .replace("\\\\", "\\")
                        .replace("\\n", "\n")
                        .replace("\\r", "\r")
                        .replace("\\t", "\t");
                    pairs.insert(key, unescaped_value);
                }
            }
        }

        Ok(("", pairs))
    }

    /// Convert string to appropriate Dynamic type
    fn parse_value_to_dynamic(&self, value: String) -> Dynamic {
        // Only coerce values that are syntactically valid JSON numbers, so
        // zero-padded IDs, signed values, and inf/nan stay strings.
        if looks_like_json_number(&value) {
            // Try integer
            if let Ok(i) = value.parse::<i64>() {
                return Dynamic::from(i);
            }

            // Try float
            if let Ok(f) = value.parse::<f64>() {
                return Dynamic::from(f);
            }
        }

        // Try boolean
        match value.to_lowercase().as_str() {
            "true" => Dynamic::from(true),
            "false" => Dynamic::from(false),
            _ => Dynamic::from(value),
        }
    }
}

impl EventParser for CefParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        let line = line.trim();

        // Parse syslog prefix if present
        let (remaining, (timestamp, hostname)) = Self::parse_syslog_prefix(line)
            .map_err(|e| anyhow::anyhow!("Invalid syslog prefix: {}", e))?;

        // Parse CEF header
        let (remaining, header_fields) = Self::parse_cef_header(remaining)
            .map_err(|e| anyhow::anyhow!("Invalid CEF header: {}", e))?;

        if header_fields.len() != 7 {
            return Err(anyhow::anyhow!("CEF header must have exactly 7 fields"));
        }

        // Parse extension if present
        let extension_pairs = if remaining.trim().is_empty() {
            HashMap::new()
        } else {
            // Skip the leading pipe if present (from header parsing)
            let extension_text = remaining.trim_start_matches('|').trim();
            if extension_text.is_empty() {
                HashMap::new()
            } else {
                match Self::parse_cef_extension(extension_text) {
                    Ok((_, pairs)) => pairs,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Invalid CEF extension: {}", e));
                    }
                }
            }
        };

        // Create event with appropriate capacity
        let capacity = 7
            + extension_pairs.len()
            + if timestamp.is_some() { 1 } else { 0 }
            + if hostname.is_some() { 1 } else { 0 };
        let mut event = Event::with_capacity(line.to_string(), capacity);

        // Set syslog fields
        if let Some(ts) = timestamp {
            event.set_field("ts".to_string(), Dynamic::from(ts.to_string()));
        }
        if let Some(host) = hostname {
            event.set_field("host".to_string(), Dynamic::from(host.to_string()));
        }

        // Set CEF header fields
        event.set_field(
            "cefver".to_string(),
            Dynamic::from(header_fields[0].clone()),
        );
        event.set_field(
            "vendor".to_string(),
            Dynamic::from(header_fields[1].clone()),
        );
        event.set_field(
            "product".to_string(),
            Dynamic::from(header_fields[2].clone()),
        );
        event.set_field(
            "version".to_string(),
            Dynamic::from(header_fields[3].clone()),
        );
        event.set_field(
            "eventid".to_string(),
            Dynamic::from(header_fields[4].clone()),
        );
        event.set_field("event".to_string(), Dynamic::from(header_fields[5].clone()));
        event.set_field(
            "severity".to_string(),
            Dynamic::from(header_fields[6].clone()),
        );

        // Set extension fields with type conversion
        for (key, value) in extension_pairs {
            let dynamic_value = self.parse_value_to_dynamic(value);
            event.set_field(key, dynamic_value);
        }

        // Extract core fields
        if self.auto_timestamp {
            event.extract_timestamp();
        }

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::EventParser;

    #[test]
    fn test_cef_basic() {
        let parser = CefParser::new();
        let line = "CEF:0|Security|threatmanager|1.0|100|worm successfully stopped|10|src=10.0.0.1 dst=2.1.2.2 spt=1232";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("cefver")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "0"
        );
        assert_eq!(
            result
                .fields
                .get("vendor")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Security"
        );
        assert_eq!(
            result
                .fields
                .get("product")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "threatmanager"
        );
        assert_eq!(
            result
                .fields
                .get("src")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "10.0.0.1"
        );
        assert_eq!(result.fields.get("spt").unwrap().as_int().unwrap(), 1232);
    }

    #[test]
    fn test_cef_with_syslog_prefix() {
        let parser = CefParser::new();
        let line = "Sep 19 08:26:10 host CEF:0|Security|threatmanager|1.0|100|worm successfully stopped|10|src=10.0.0.1";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Sep 19 08:26:10"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "host"
        );
        assert_eq!(
            result
                .fields
                .get("vendor")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Security"
        );
    }

    #[test]
    fn test_cef_escaped_pipe() {
        let parser = CefParser::new();
        let line = r"CEF:0|security|threatmanager|1.0|100|detected a \| in message|10|src=10.0.0.1";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("event")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "detected a | in message"
        );
    }

    #[test]
    fn test_cef_escaped_extension() {
        let parser = CefParser::new();
        let line =
            r"CEF:0|vendor|product|1.0|100|event|10|key=value\=with\=equals msg=test\nmultiline";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("key")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "value=with=equals"
        );
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "test\nmultiline"
        );
    }

    #[test]
    fn test_cef_no_extension() {
        let parser = CefParser::new();
        let line = "CEF:0|vendor|product|1.0|100|event|10";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("cefver")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "0"
        );
        assert_eq!(
            result
                .fields
                .get("severity")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "10"
        );
        assert!(result.fields.get("src").is_none());
    }

    #[test]
    fn test_cef_type_conversion() {
        let parser = CefParser::new();
        let line =
            "CEF:0|vendor|product|1.0|100|event|10|count=42 rate=2.5 enabled=true disabled=false";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(result.fields.get("count").unwrap().as_int().unwrap(), 42);
        assert_eq!(result.fields.get("rate").unwrap().as_float().unwrap(), 2.5);
        assert!(result.fields.get("enabled").unwrap().as_bool().unwrap());
        assert!(!result.fields.get("disabled").unwrap().as_bool().unwrap());
    }
}
