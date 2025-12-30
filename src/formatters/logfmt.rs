//! Logfmt output formatter
//!
//! Produces strict logfmt-compliant output with proper key sanitization
//! and value quoting.

use crate::event::{flatten_dynamic, Event, FlattenStyle};
use crate::pipeline;
use rhai::Dynamic;

use super::utils::format_quoted_logfmt_value;

/// Sanitize a field key to ensure logfmt compliance
///
/// The logfmt specification requires keys to:
/// - Not contain whitespace (spaces, tabs, newlines, carriage returns)
/// - Not contain equals signs (=) as they delimit key-value pairs
///
/// This function replaces problematic characters with underscores to maintain
/// field information while ensuring valid logfmt output. This approach:
/// - Preserves all field data (no information loss)
/// - Produces parseable logfmt output
/// - Maintains deterministic key mapping
/// - Handles edge cases gracefully
///
/// # Examples
/// - sanitize_logfmt_key("field with spaces") -> "field_with_spaces"
/// - sanitize_logfmt_key("field=with=equals") -> "field_with_equals"
/// - sanitize_logfmt_key("normal_field") -> "normal_field"
pub fn sanitize_logfmt_key(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            c => c,
        })
        .collect()
}

/// Logfmt formatter - strict logfmt output formatter (no colors, no brief mode)
///
/// This formatter produces logfmt-compliant output by:
/// 1. Sanitizing field keys to replace invalid characters with underscores
/// 2. Properly quoting string values that contain special characters
/// 3. Leaving numeric and boolean values unquoted
///
/// Key sanitization ensures that output can be parsed by standard logfmt parsers,
/// including Kelora's own logfmt input parser.
pub struct LogfmtFormatter;

impl LogfmtFormatter {
    pub fn new() -> Self {
        Self
    }

    /// Format a Dynamic value directly into buffer for performance
    fn format_dynamic_value_into(&self, value: &Dynamic, output: &mut String) {
        let string_val = self.format_logfmt_value(value);
        let is_string = value.is_string();

        if is_string {
            format_quoted_logfmt_value(&string_val, output);
        } else {
            output.push_str(&string_val);
        }
    }

    /// Format a Dynamic value for logfmt output, flattening nested structures
    fn format_logfmt_value(&self, value: &Dynamic) -> String {
        // Check if this is a complex nested structure
        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            // Flatten nested structures using underscore style for logfmt safety
            let flattened = flatten_dynamic(value, FlattenStyle::Underscore, 0);

            if flattened.len() == 1 {
                // Single flattened value - use it directly
                flattened.values().next().unwrap().to_string()
            } else if flattened.is_empty() {
                // Empty structure
                String::new()
            } else {
                // Multiple flattened values - create a compact representation
                // Format as "key1=val1,key2=val2" for logfmt-style readability
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        } else {
            // Simple scalar value
            value.to_string()
        }
    }
}

impl pipeline::Formatter for LogfmtFormatter {
    fn format(&self, event: &Event) -> String {
        if event.fields.is_empty() {
            return String::new();
        }

        // Pre-allocate buffer with estimated capacity
        let estimated_capacity = event.fields.len() * 32;
        let mut output = String::with_capacity(estimated_capacity);
        let mut first = true;

        for (key, value) in crate::event::ordered_fields(event) {
            if !first {
                output.push(' ');
            }
            first = false;

            // Strict logfmt: key=value pairs with sanitized keys
            // Keys are sanitized to ensure logfmt compliance (no spaces, equals signs, etc.)
            let sanitized_key = sanitize_logfmt_key(key);
            output.push_str(&sanitized_key);
            output.push('=');
            self.format_dynamic_value_into(value, &mut output);
        }

        output
    }
}
