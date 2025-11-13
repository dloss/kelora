use crate::event::Event;
use crate::parsers::type_conversion::{convert_value_to_type, FieldType, TypeMap};
use crate::pipeline::EventParser;
use anyhow::{Context, Result};
use regex::Regex;
use rhai::Dynamic;

/// Parser for regex-based input with named capture groups and optional type annotations
#[derive(Debug)]
pub struct RegexParser {
    regex: Regex,
    type_map: TypeMap,
    strict: bool,
}

impl RegexParser {
    /// Create a new RegexParser from a pattern string with optional type annotations
    ///
    /// Patterns use standard regex syntax with named capture groups.
    /// Type annotations are specified within the group name using the syntax: (?P<name:type>...)
    ///
    /// Supported types:
    /// - :int - Convert to i64
    /// - :float - Convert to f64
    /// - :bool - Convert to boolean
    /// - (no suffix) - Store as string
    ///
    /// # Examples
    /// ```ignore
    /// // Simple pattern without types
    /// RegexParser::new(r"(?P<ip>\S+) (?P<msg>.*)")?;
    ///
    /// // Pattern with type annotations
    /// RegexParser::new(r"(?P<code:int>\d+) (?P<duration:float>[\d.]+)")?;
    /// ```
    pub fn new(pattern: &str) -> Result<Self> {
        let (clean_pattern, type_map) = Self::extract_type_annotations(pattern)?;

        let regex = Regex::new(&clean_pattern)
            .with_context(|| format!("Failed to compile regex pattern: {}", pattern))?;

        Ok(Self {
            regex,
            type_map,
            strict: false,
        })
    }

    /// Set strict mode for parsing
    ///
    /// In strict mode:
    /// - Lines that don't match the pattern will cause an error
    /// - Type conversion failures will cause an error
    ///
    /// In lenient mode (default):
    /// - Lines that don't match will create an empty event
    /// - Type conversion failures will fall back to string
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Extract type annotations from a regex pattern
    ///
    /// Transforms patterns like:
    ///   (?P<status:int>\d+) -> (?P<status>\d+) with type_map["status"] = Int
    ///   (?P<msg>.*) -> (?P<msg>.*) with no type entry
    ///
    /// Returns (clean_pattern, type_map)
    fn extract_type_annotations(pattern: &str) -> Result<(String, TypeMap)> {
        let mut type_map = TypeMap::new();
        let mut clean_pattern = String::with_capacity(pattern.len());
        let mut chars = pattern.chars().peekable();
        let mut in_group_name = false;
        let mut current_group_name = String::new();
        let mut paren_depth = 0;
        let mut in_named_group = false;

        while let Some(ch) = chars.next() {
            if ch == '(' && chars.peek() == Some(&'?') {
                // Start of a group - look for (?P<
                clean_pattern.push(ch);
                clean_pattern.push(chars.next().unwrap()); // consume '?'

                if chars.peek() == Some(&'P') {
                    clean_pattern.push(chars.next().unwrap()); // consume 'P'
                    if chars.peek() == Some(&'<') {
                        // Check if we're already inside a named group
                        if in_named_group {
                            return Err(anyhow::anyhow!(
                                "Nested named capture groups are not supported"
                            ));
                        }
                        clean_pattern.push(chars.next().unwrap()); // consume '<'
                        in_group_name = true;
                        in_named_group = true;
                        current_group_name.clear();
                        paren_depth = 1;
                    }
                }
            } else if in_group_name && ch == '>' {
                // End of group name
                in_group_name = false;

                // Check if group name contains type annotation
                if let Some(colon_pos) = current_group_name.find(':') {
                    let field_name = &current_group_name[..colon_pos];
                    let type_str = &current_group_name[colon_pos + 1..];

                    // Validate field name is not empty
                    if field_name.is_empty() {
                        return Err(anyhow::anyhow!(
                            "Empty field name in capture group: (?P<:{}>)",
                            type_str
                        ));
                    }

                    // Validate field name contains only valid characters
                    if !field_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return Err(anyhow::anyhow!(
                            "Invalid field name '{}': must contain only alphanumeric characters and underscores",
                            field_name
                        ));
                    }

                    // Check for reserved field names
                    if field_name == "original_line"
                        || field_name == "parsed_ts"
                        || field_name == "fields"
                    {
                        return Err(anyhow::anyhow!(
                            "Field name '{}' is reserved and cannot be used",
                            field_name
                        ));
                    }

                    // Validate type annotation
                    if type_str.is_empty() {
                        return Err(anyhow::anyhow!(
                            "Empty type annotation in capture group: (?P<{}:>)",
                            field_name
                        ));
                    }

                    // Parse type annotation - only accept exact lowercase matches
                    let field_type = match type_str {
                        "int" => FieldType::Int,
                        "float" => FieldType::Float,
                        "bool" => FieldType::Bool,
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Unknown type annotation '{}' in field '{}'. Supported types: int, float, bool",
                                type_str,
                                field_name
                            ));
                        }
                    };

                    type_map.insert(field_name.to_string(), field_type);
                    clean_pattern.push_str(field_name);
                } else {
                    // No type annotation, use field name as-is
                    let field_name = &current_group_name;

                    // Validate field name is not empty
                    if field_name.is_empty() {
                        return Err(anyhow::anyhow!("Empty field name in capture group: (?P<>)"));
                    }

                    // Validate field name contains only valid characters
                    if !field_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return Err(anyhow::anyhow!(
                            "Invalid field name '{}': must contain only alphanumeric characters and underscores",
                            field_name
                        ));
                    }

                    // Check for reserved field names
                    if field_name == "original_line"
                        || field_name == "parsed_ts"
                        || field_name == "fields"
                    {
                        return Err(anyhow::anyhow!(
                            "Field name '{}' is reserved and cannot be used",
                            field_name
                        ));
                    }

                    clean_pattern.push_str(field_name);
                }

                clean_pattern.push(ch); // push '>'
                current_group_name.clear();
            } else if in_group_name {
                // Inside group name, accumulate characters
                current_group_name.push(ch);
            } else {
                // Regular pattern character
                if ch == '(' && in_named_group {
                    paren_depth += 1;
                } else if ch == ')' && in_named_group {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        in_named_group = false;
                    }
                }
                clean_pattern.push(ch);
            }
        }

        // Verify we're not still inside a group name
        if in_group_name {
            return Err(anyhow::anyhow!("Unclosed named capture group in pattern"));
        }

        Ok((clean_pattern, type_map))
    }
}

impl EventParser for RegexParser {
    fn parse(&self, line: &str) -> Result<Event> {
        // Try to match the full line with anchors
        let full_pattern = format!("^{}$", self.regex.as_str());
        let full_regex = Regex::new(&full_pattern)?;

        let captures = match full_regex.captures(line) {
            Some(caps) => caps,
            None => {
                if self.strict {
                    return Err(anyhow::anyhow!(
                        "Line does not match regex pattern: {}",
                        line
                    ));
                } else {
                    // Lenient mode: return event with empty fields
                    return Ok(Event::default_with_line(line.to_string()));
                }
            }
        };

        let mut event = Event::default_with_line(line.to_string());

        // Extract all named capture groups
        for name in self.regex.capture_names().flatten() {
            if let Some(matched) = captures.name(name) {
                let value_str = matched.as_str();

                // Skip empty captures
                if value_str.is_empty() {
                    continue;
                }

                // Apply type conversion if specified
                let converted_value = if let Some(field_type) = self.type_map.get(name) {
                    match convert_value_to_type(value_str, field_type, self.strict) {
                        Ok(val) => val,
                        Err(err) => {
                            if self.strict {
                                return Err(anyhow::anyhow!(
                                    "Type conversion error for field '{}': {}",
                                    name,
                                    err
                                ));
                            } else {
                                // Lenient mode already returns string on error
                                Dynamic::from(value_str.to_string())
                            }
                        }
                    }
                } else {
                    // No type annotation, store as string
                    Dynamic::from(value_str.to_string())
                };

                event.fields.insert(name.to_string(), converted_value);
            }
        }

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pattern_without_types() {
        let parser = RegexParser::new(r"(?P<ip>\S+) (?P<msg>.*)").unwrap();
        let event = parser.parse("192.168.1.1 Hello world").unwrap();

        assert_eq!(
            event
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            event
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Hello world"
        );
    }

    #[test]
    fn test_pattern_with_int_type() {
        let parser = RegexParser::new(r"(?P<code:int>\d+) (?P<msg>.*)").unwrap();
        let event = parser.parse("404 Not found").unwrap();

        assert_eq!(event.fields.get("code").unwrap().as_int().unwrap(), 404);
        assert_eq!(
            event
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Not found"
        );
    }

    #[test]
    fn test_pattern_with_float_type() {
        let parser = RegexParser::new(r"(?P<duration:float>[\d.]+)ms").unwrap();
        let event = parser.parse("123.45ms").unwrap();

        let duration = event.fields.get("duration").unwrap().as_float().unwrap();
        assert!((duration - 123.45).abs() < 0.001);
    }

    #[test]
    fn test_pattern_with_bool_type() {
        let parser = RegexParser::new(r"(?P<success:bool>true|false)").unwrap();
        let event = parser.parse("true").unwrap();

        assert!(event.fields.get("success").unwrap().as_bool().unwrap());
    }

    #[test]
    fn test_pattern_with_mixed_types() {
        let parser = RegexParser::new(
            r"^(?P<ts>\S+) \[(?P<level>\w+)\] (?P<code:int>\d+) (?P<duration:float>[\d.]+)ms (?P<msg>.+)$",
        )
        .unwrap();

        let event = parser
            .parse("2025-01-13T10:00:00Z [ERROR] 500 123.45ms Internal error")
            .unwrap();

        assert_eq!(
            event
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2025-01-13T10:00:00Z"
        );
        assert_eq!(
            event
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "ERROR"
        );
        assert_eq!(event.fields.get("code").unwrap().as_int().unwrap(), 500);

        let duration = event.fields.get("duration").unwrap().as_float().unwrap();
        assert!((duration - 123.45).abs() < 0.001);

        assert_eq!(
            event
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Internal error"
        );
    }

    #[test]
    fn test_non_matching_line_lenient() {
        let parser = RegexParser::new(r"(?P<code:int>\d+)").unwrap();
        let event = parser.parse("no numbers here").unwrap();

        // Lenient mode: empty fields
        assert!(event.fields.is_empty());
        assert_eq!(event.original_line, "no numbers here");
    }

    #[test]
    fn test_non_matching_line_strict() {
        let parser = RegexParser::new(r"(?P<code:int>\d+)")
            .unwrap()
            .with_strict(true);
        let result = parser.parse("no numbers here");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not match"));
    }

    #[test]
    fn test_type_conversion_failure_lenient() {
        // Pattern captures letters, but type expects int
        let parser = RegexParser::new(r"(?P<num:int>[a-z]+)").unwrap();
        let event = parser.parse("abc").unwrap();

        // Lenient mode: falls back to string
        assert_eq!(
            event
                .fields
                .get("num")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "abc"
        );
    }

    #[test]
    fn test_type_conversion_failure_strict() {
        // Pattern captures letters, but type expects int
        let parser = RegexParser::new(r"(?P<num:int>[a-z]+)")
            .unwrap()
            .with_strict(true);
        let result = parser.parse("abc");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Type conversion"));
    }

    #[test]
    fn test_empty_capture_skipped() {
        let parser = RegexParser::new(r"(?P<optional>x)?(?P<required>.+)").unwrap();
        let event = parser.parse("abc").unwrap();

        // optional matched but captured empty string - should be skipped
        assert!(event.fields.get("optional").is_none());
        assert_eq!(
            event
                .fields
                .get("required")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "abc"
        );
    }

    #[test]
    fn test_reserved_field_name_original_line() {
        let result = RegexParser::new(r"(?P<original_line>.+)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_reserved_field_name_parsed_ts() {
        let result = RegexParser::new(r"(?P<parsed_ts>.+)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_reserved_field_name_fields() {
        let result = RegexParser::new(r"(?P<fields>.+)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_empty_field_name() {
        let result = RegexParser::new(r"(?P<>.+)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty field name"));
    }

    #[test]
    fn test_empty_field_name_with_type() {
        let result = RegexParser::new(r"(?P<:int>.+)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty field name"));
    }

    #[test]
    fn test_empty_type_annotation() {
        let result = RegexParser::new(r"(?P<field:>.+)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty type annotation"));
    }

    #[test]
    fn test_unknown_type_annotation() {
        let result = RegexParser::new(r"(?P<field:string>.+)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown type annotation 'string'"));
    }

    #[test]
    fn test_unknown_type_annotation_integer() {
        let result = RegexParser::new(r"(?P<field:integer>.+)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown type annotation 'integer'"));
    }

    #[test]
    fn test_case_sensitive_type() {
        // Only lowercase :int is accepted
        let result = RegexParser::new(r"(?P<field:INT>.+)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown type annotation 'INT'"));
    }

    #[test]
    fn test_nested_groups_not_supported() {
        let result = RegexParser::new(r"(?P<outer>foo(?P<inner>bar))");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Nested"));
    }

    #[test]
    fn test_non_capturing_groups_allowed() {
        let parser = RegexParser::new(r"(?:prefix-)?(?P<value>.+)").unwrap();
        let event = parser.parse("prefix-test").unwrap();

        assert_eq!(
            event
                .fields
                .get("value")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "test"
        );
    }

    #[test]
    fn test_invalid_field_name_characters() {
        let result = RegexParser::new(r"(?P<field-name>.+)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid field name"));
    }

    #[test]
    fn test_pattern_with_colons_in_regex() {
        // Pattern has colons in the regex part, not in group name
        let parser = RegexParser::new(r"(?P<time>\d{2}:\d{2}:\d{2})").unwrap();
        let event = parser.parse("12:34:56").unwrap();

        assert_eq!(
            event
                .fields
                .get("time")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "12:34:56"
        );
    }

    #[test]
    fn test_complex_real_world_pattern() {
        let parser = RegexParser::new(
            r#"^(?P<ip>\S+) - - \[(?P<timestamp>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+) HTTP/[\d.]+" (?P<status:int>\d+) (?P<bytes:int>\d+)$"#,
        )
        .unwrap();

        let event = parser
            .parse(r#"192.168.1.1 - - [13/Jan/2025:10:00:00 +0000] "GET /api/users HTTP/1.1" 200 1234"#)
            .unwrap();

        assert_eq!(
            event
                .fields
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            event
                .fields
                .get("timestamp")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "13/Jan/2025:10:00:00 +0000"
        );
        assert_eq!(
            event
                .fields
                .get("method")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET"
        );
        assert_eq!(
            event
                .fields
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/api/users"
        );
        assert_eq!(event.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(event.fields.get("bytes").unwrap().as_int().unwrap(), 1234);
    }
}
