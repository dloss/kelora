use rhai::Dynamic;

/// Escape a string for single-line display: backslashes and common control
/// characters become visible escape sequences, other control characters are
/// rendered as `\xNN`. Double quotes are left untouched; callers wrap the
/// result in quotes themselves when needed.
pub(crate) fn escape_for_display(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                escaped.push_str(&format!("\\x{:02X}", c as u32));
            }
            c => escaped.push(c),
        }
    }
    escaped
}

/// Format a Dynamic value as a string representation suitable for output
/// Returns (string_value, needs_quotes) - the second bool indicates if it should be quoted
pub(crate) fn format_dynamic_value(value: &Dynamic) -> (String, bool) {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            (s, true) // Strings can potentially need quotes
        } else {
            (value.to_string(), false)
        }
    } else if let Some(s) = crate::rhai_functions::datetime::render_custom_scalar(value) {
        // Custom wrapper scalars (datetime, duration) render via their Display;
        // flag as quotable so formats that quote on spaces (e.g. "1m 30s") do so.
        (s, true)
    } else {
        // Numbers, booleans, etc. - never need quotes
        (value.to_string(), false)
    }
}

/// Indent each subsequent line of a multiline string for consistent display
pub(super) fn indent_multiline_value(value: &str, indent: &str) -> String {
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

/// Convert rhai::Dynamic to serde_json::Value recursively
pub(super) fn dynamic_to_json(value: &Dynamic) -> serde_json::Value {
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
    } else if let Some(s) = crate::rhai_functions::datetime::render_custom_scalar(value) {
        serde_json::Value::String(s)
    } else {
        // For any remaining types, convert to string
        serde_json::Value::String(value.to_string())
    }
}
