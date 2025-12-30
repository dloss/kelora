//! Utility functions for formatters
//!
//! This module contains shared utility functions used by multiple formatters,
//! including string escaping, value formatting, and type conversions.

use rhai::Dynamic;

/// Utility function for single-quote string escaping for default formatter
/// Escapes single quotes, backslashes, newlines, tabs, and carriage returns
pub fn escape_single_quote_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10); // Some extra space for escapes

    for ch in input.chars() {
        match ch {
            '\'' => output.push_str("\\'"),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            _ => output.push(ch),
        }
    }

    output
}

/// Utility function for logfmt-compliant string escaping
/// Escapes quotes, backslashes, newlines, tabs, and carriage returns
pub fn escape_logfmt_string(input: &str) -> String {
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
pub fn needs_logfmt_quoting(value: &str) -> bool {
    // Quote values that contain spaces, tabs, newlines, quotes, equals, or are empty
    value.is_empty()
        || value.contains(' ')
        || value.contains('\t')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('"')
        || value.contains('=')
}

/// Format a Dynamic value as a string representation suitable for output
/// Returns (string_value, needs_quotes) - the second bool indicates if it should be quoted
pub fn format_dynamic_value(value: &Dynamic) -> (String, bool) {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            (s, true) // Strings can potentially need quotes
        } else {
            (value.to_string(), false)
        }
    } else {
        // Numbers, booleans, etc. - never need quotes
        (value.to_string(), false)
    }
}

/// Indent each subsequent line of a multiline string for consistent display
pub fn indent_multiline_value(value: &str, indent: &str) -> String {
    let mut lines = value.lines();
    match lines.next() {
        Some(first_line) => {
            let mut output = String::from(first_line);
            for line in lines {
                output.push('\n');
                output.push_str(indent);
                output.push_str(line);
            }
            output
        }
        None => String::new(),
    }
}

/// Utility to format a quoted logfmt value into a buffer
pub fn format_quoted_logfmt_value(value: &str, output: &mut String) {
    if needs_logfmt_quoting(value) {
        output.push('"');
        output.push_str(&escape_logfmt_string(value));
        output.push('"');
    } else {
        output.push_str(value);
    }
}

/// Convert rhai::Dynamic to serde_json::Value recursively
pub fn dynamic_to_json(value: &Dynamic) -> serde_json::Value {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            serde_json::Value::String(s)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_int() {
        if let Ok(i) = value.as_int() {
            serde_json::Value::Number(serde_json::Number::from(i))
        } else {
            serde_json::Value::Null
        }
    } else if value.is_float() {
        if let Ok(f) = value.as_float() {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_bool() {
        if let Ok(b) = value.as_bool() {
            serde_json::Value::Bool(b)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_unit() {
        serde_json::Value::Null
    } else if let Some(arr) = value.clone().try_cast::<rhai::Array>() {
        // Convert Rhai array to JSON array recursively
        let json_array: Vec<serde_json::Value> = arr.iter().map(dynamic_to_json).collect();
        serde_json::Value::Array(json_array)
    } else if let Some(map) = value.clone().try_cast::<rhai::Map>() {
        // Convert Rhai map to JSON object recursively
        let mut json_obj = serde_json::Map::new();
        for (key, val) in map {
            json_obj.insert(key.to_string(), dynamic_to_json(&val));
        }
        serde_json::Value::Object(json_obj)
    } else {
        // For any remaining types, convert to string
        serde_json::Value::String(value.to_string())
    }
}

/// Check if a CSV value needs quoting
pub fn needs_csv_quoting(value: &str, delimiter: char) -> bool {
    value.is_empty()
        || value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
        || value.starts_with(' ')
        || value.ends_with(' ')
}

/// Escape CSV value with proper quoting
pub fn escape_csv_value(value: &str, delimiter: char) -> String {
    if needs_csv_quoting(value, delimiter) {
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}
