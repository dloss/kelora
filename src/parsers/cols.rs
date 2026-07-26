use crate::event::Event;
use crate::parsers::type_conversion::{convert_value_to_type, FieldType, TypeMap};
use crate::pipeline::EventParser;
use crate::rhai_functions::columns::{
    parse_cols_whitespace, parse_cols_with_sep, set_parse_cols_strict,
};
use anyhow::Result;

/// Parser for column-based text input using parse_cols spec
pub struct ColsParser {
    spec: String,
    separator: Option<String>,
    type_map: TypeMap,
    strict: bool,
}

impl ColsParser {
    pub fn new(spec: String, separator: Option<String>) -> Self {
        Self {
            spec,
            separator,
            type_map: TypeMap::new(),
            strict: false,
        }
    }

    /// Parse the spec string to extract type annotations and build the clean spec
    /// Returns (clean_spec, type_map)
    pub(crate) fn extract_type_annotations(spec: &str) -> (String, TypeMap) {
        let mut clean_tokens = Vec::new();
        let mut type_map = TypeMap::new();

        for token in spec.split_whitespace() {
            if token.is_empty() {
                continue;
            }

            // Handle rest token (*field or *field:type)
            if let Some(rest) = token.strip_prefix('*') {
                let (field_name, field_type) = Self::parse_type_annotation(rest);
                clean_tokens.push(format!("*{}", field_name));
                if let Some(ftype) = field_type {
                    type_map.insert(field_name, ftype);
                }
                continue;
            }

            // Handle skip token (- or -(N))
            if token.starts_with('-') {
                clean_tokens.push(token.to_string());
                continue;
            }

            // Handle field token (field, field(N), field:type, field(N):type)
            let (field_token, field_type) = Self::parse_type_annotation(token);
            clean_tokens.push(field_token.clone());

            // Extract field name from token (might have count)
            let field_name = if let Some(open) = field_token.find('(') {
                field_token[..open].to_string()
            } else {
                field_token
            };

            if let Some(ftype) = field_type {
                type_map.insert(field_name, ftype);
            }
        }

        (clean_tokens.join(" "), type_map)
    }

    /// Parse a single token to extract type annotation
    /// Returns (clean_token, optional_type)
    fn parse_type_annotation(token: &str) -> (String, Option<FieldType>) {
        // Find the last colon that's not inside parentheses
        if let Some(colon_pos) = token.rfind(':') {
            let paren_pos = token.find('(');
            let close_paren_pos = token.rfind(')');

            // Check if colon is after closing paren or no parens exist
            if paren_pos.is_none() || colon_pos > close_paren_pos.unwrap_or(0) {
                let base = &token[..colon_pos];
                let type_str = &token[colon_pos + 1..];

                if let Some(field_type) = FieldType::from_str(type_str) {
                    return (base.to_string(), Some(field_type));
                }
            }
        }

        (token.to_string(), None)
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// Validate a `cols:` spec, including its optional `field:type` annotations.
///
/// Used by CLI and config validation so a malformed spec is reported once, as a
/// usage error, instead of raising the same runtime error on every input line.
/// Annotations are stripped with the parser's own routine so validation cannot
/// drift from what parsing actually accepts.
pub fn validate_spec(spec: &str) -> Result<(), String> {
    let (clean_spec, _types) = ColsParser::extract_type_annotations(spec);

    // A colon surviving annotation-stripping means the suffix was not a known
    // type name. Say which types exist rather than reporting the whole token as
    // an invalid field name.
    for token in clean_spec.split_whitespace() {
        if let Some((base, suffix)) = token.rsplit_once(':') {
            return Err(format!(
                "unknown type annotation ':{}' on '{}' — valid types are int, float, bool, string",
                suffix,
                base.trim_start_matches('*')
            ));
        }
    }

    crate::rhai_functions::columns::validate_cols_spec(&clean_spec)
}

impl EventParser for ColsParser {
    fn parse(&self, line: &str) -> Result<Event> {
        // Extract type annotations from spec and get clean spec
        let (clean_spec, extracted_types) = Self::extract_type_annotations(&self.spec);

        // Merge extracted types with explicitly set type_map (explicit takes precedence)
        let mut combined_types = extracted_types;
        for (k, v) in &self.type_map {
            combined_types.insert(k.clone(), v.clone());
        }

        // Set strict mode to false for resilient parsing
        set_parse_cols_strict(false);

        let result = if let Some(ref sep) = self.separator {
            parse_cols_with_sep(line, &clean_spec, sep)
        } else {
            parse_cols_whitespace(line, &clean_spec)
        };

        match result {
            Ok(map) => {
                let mut event = Event::default_with_line(line.to_string());
                for (key, value) in map {
                    // Apply type conversion if specified
                    let converted_value = if let Some(field_type) = combined_types.get(&*key) {
                        // Get string representation for conversion
                        if let Ok(str_value) = value.clone().into_string() {
                            // Propagate so --strict aborts and resilient mode
                            // yields () — same as regex/csv (the shared
                            // convert_value_to_type decides which).
                            convert_value_to_type(&str_value, field_type, self.strict).map_err(
                                |e| {
                                    anyhow::anyhow!(
                                        "Type conversion error for field '{}' (value: '{}'): {}",
                                        key,
                                        str_value,
                                        e
                                    )
                                },
                            )?
                        } else {
                            value
                        }
                    } else {
                        value
                    };

                    event.fields.insert(key.to_string(), converted_value);
                }
                Ok(event)
            }
            Err(err) => {
                // Return error for the parser to handle according to strict/resilient mode
                Err(anyhow::anyhow!("{}", err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cols_parser_whitespace() {
        let parser = ColsParser::new("ts(2) level *msg".to_string(), None);
        let result = parser
            .parse("2025-09-22 12:33:44 INFO hello world")
            .unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2025-09-22 12:33:44"
        );
        assert_eq!(
            result
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "INFO"
        );
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_cols_parser_with_separator() {
        let parser = ColsParser::new("ts(2) level *msg".to_string(), Some("|".to_string()));
        let result = parser
            .parse("2025-09-22|12:33:44|INFO|hello|world")
            .unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2025-09-22|12:33:44"
        );
        assert_eq!(
            result
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "INFO"
        );
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "hello|world"
        );
    }

    #[test]
    fn test_cols_parser_shortage() {
        let parser = ColsParser::new("ts level user action".to_string(), None);
        let result = parser.parse("2025-09-22 INFO alice").unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2025-09-22"
        );
        assert_eq!(
            result
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "INFO"
        );
        assert_eq!(
            result
                .fields
                .get("user")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice"
        );
        assert!(result.fields.get("action").unwrap().is_unit()); // Unit type for missing fields
    }

    #[test]
    fn test_cols_parser_with_type_annotations() {
        let parser = ColsParser::new("status:int bytes:int active:bool msg".to_string(), None);
        let result = parser.parse("200 1024 true hello").unwrap();

        assert_eq!(result.fields.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(result.fields.get("bytes").unwrap().as_int().unwrap(), 1024);
        assert!(result.fields.get("active").unwrap().as_bool().unwrap());
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_cols_parser_with_count_and_type() {
        let parser = ColsParser::new(
            "ts(2) level:int *msg:string".to_string(),
            Some("|".to_string()),
        );
        let result = parser.parse("2025-09-22|12:33:44|200|hello|world").unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2025-09-22|12:33:44"
        );
        assert_eq!(result.fields.get("level").unwrap().as_int().unwrap(), 200);
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "hello|world"
        );
    }
}
