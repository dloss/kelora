use crate::event::{flatten_dynamic, Event, FlattenStyle};
use crate::pipeline;

use rhai::Dynamic;

/// Utility function for logfmt-compliant string escaping
/// Escapes quotes, backslashes, newlines, tabs, and carriage returns
pub(crate) fn escape_logfmt_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10); // Some extra space for escapes

    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            _ => output.push(ch),
        }
    }

    output
}

/// Check if a string value needs to be quoted per logfmt rules
pub(crate) fn needs_logfmt_quoting(value: &str) -> bool {
    // Quote values that contain spaces, tabs, newlines, quotes, equals, or are empty
    value.is_empty()
        || value.contains(' ')
        || value.contains('\t')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('"')
        || value.contains('=')
}

/// Utility to format a quoted logfmt value into a buffer
fn format_quoted_logfmt_value(value: &str, output: &mut String) {
    if needs_logfmt_quoting(value) {
        output.push('"');
        output.push_str(&escape_logfmt_string(value));
        output.push('"');
    } else {
        output.push_str(value);
    }
}

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
/// - sanitize_logfmt_key("") -> "_"
pub(crate) fn sanitize_logfmt_key(key: &str) -> String {
    // An empty key would render as "=value", which standard logfmt parsers
    // (including Kelora's own) reject as "Empty key found". Empty keys arise
    // naturally from blank CSV header columns, so map them to a placeholder to
    // keep the output parseable. See sanitize_logfmt_key tests.
    if key.is_empty() {
        return "_".to_string();
    }

    key.chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            c => c,
        })
        .collect()
}

// Logfmt formatter - strict logfmt output formatter (no colors, no brief mode)
//
// This formatter produces logfmt-compliant output by:
// 1. Sanitizing field keys to replace invalid characters with underscores
// 2. Properly quoting string values that contain special characters
// 3. Leaving numeric and boolean values unquoted
//
// Key sanitization ensures that output can be parsed by standard logfmt parsers,
// including Kelora's own logfmt input parser.
pub struct LogfmtFormatter;

impl LogfmtFormatter {
    pub fn new() -> Self {
        Self
    }

    /// Format a Dynamic value directly into buffer for performance
    fn format_dynamic_value_into(&self, value: &Dynamic, output: &mut String) {
        let string_val = self.format_logfmt_value(value);

        // Strings need quoting when they contain logfmt-special characters.
        // Nested maps/arrays are flattened into a compact "k=v,k=v" string that
        // likewise contains '=' and ',', so they must go through the same
        // quoting path to stay parseable (mirrors how the CSV formatter escapes
        // its compact cells). Numeric and boolean scalars never need quoting.
        if value.is_string()
            || value.is::<rhai::Map>()
            || value.is::<rhai::Array>()
            || crate::rhai_functions::datetime::render_custom_scalar(value).is_some()
        {
            format_quoted_logfmt_value(&string_val, output);
        } else {
            output.push_str(&string_val);
        }
    }

    /// Format a Dynamic value for logfmt output, flattening nested structures
    fn format_logfmt_value(&self, value: &Dynamic) -> String {
        // Check if this is a complex nested structure. Use is::<T>() rather than
        // clone().try_cast() to avoid cloning every scalar value just to probe its type.
        if value.is::<rhai::Map>() || value.is::<rhai::Array>() {
            // Flatten nested structures using underscore style for logfmt safety
            let flattened = flatten_dynamic(value, FlattenStyle::Underscore, 0);

            // flatten_dynamic always yields at least one entry; an empty
            // map/array collapses to a single UNIT placeholder, which we render
            // as an empty value. Every other case - including a single real
            // key - must keep its "key=value" shape so nested keys/indices are
            // preserved and the output round-trips back through the parser.
            if flattened.is_empty()
                || (flattened.len() == 1 && flattened.values().next().unwrap().is_unit())
            {
                String::new()
            } else {
                // Compact representation: "key1=val1,key2=val2".
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        } else if let Some(s) = crate::rhai_functions::datetime::render_custom_scalar(value) {
            s
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
