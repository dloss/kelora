//! JSON extraction functions for Rhai scripts.
//!
//! Provides functions for extracting JSON objects and arrays from text.

use crate::event::json_to_dynamic;
use rhai::{Array, Dynamic, Engine};

/// Extract JSON objects or arrays from text
///
/// Searches through the text for valid JSON objects `{...}` or arrays `[...]`
/// and returns the nth occurrence (0-indexed).
///
/// # Arguments
/// * `text` - The input string to search for JSON
/// * `nth` - Which occurrence to extract (0 = first, 1 = second, etc.)
///
/// # Returns
/// A Rhai Dynamic containing the parsed JSON (Map for objects, Array for arrays),
/// or an empty string if not found or parsing fails
fn extract_json_impl(text: &str, nth: i64) -> Dynamic {
    if text.is_empty() || nth < 0 {
        return Dynamic::from(String::new());
    }

    let nth_usize = nth as usize;
    let mut found_count = 0;
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Iterate through each character looking for JSON start markers
    let mut i = 0;
    while i < len {
        let ch = bytes[i] as char;

        // Check if this is a potential JSON start
        if ch != '{' && ch != '[' {
            i += 1;
            continue;
        }

        let start_char = ch;
        let end_char = if ch == '{' { '}' } else { ']' };

        // Try to find matching closing bracket/brace
        // We need to handle nested structures
        let mut depth = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_pos = None;

        #[allow(clippy::needless_range_loop)]
        for j in i..len {
            let current = bytes[j] as char;

            if escape_next {
                escape_next = false;
                continue;
            }

            if current == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if current == '"' {
                in_string = !in_string;
                continue;
            }

            if in_string {
                continue;
            }

            if current == start_char {
                depth += 1;
            } else if current == end_char {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(j + 1);
                    break;
                }
            }
        }

        // If we found a complete structure, try to parse it
        if let Some(end) = end_pos {
            let candidate = &text[i..end];

            // Try to parse as JSON
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(candidate) {
                // Check if it's an object or array
                if json_value.is_object() || json_value.is_array() {
                    if found_count == nth_usize {
                        // This is the one we want!
                        return json_to_dynamic(&json_value);
                    }
                    found_count += 1;
                }
            }

            // Move past this structure
            i = end;
        } else {
            // No matching closing bracket found, move to next character
            i += 1;
        }
    }

    // Not found
    Dynamic::from(String::new())
}

/// Extract all JSON objects or arrays from text as strings
///
/// Searches through the text for all valid JSON objects `{...}` or arrays `[...]`
/// and returns them as an array of strings.
///
/// # Arguments
/// * `text` - The input string to search for JSON
///
/// # Returns
/// A Rhai Array containing all found JSON strings (raw JSON text)
fn extract_jsons_impl(text: &str) -> Array {
    let mut results = Array::new();

    if text.is_empty() {
        return results;
    }

    let bytes = text.as_bytes();
    let len = bytes.len();

    // Iterate through each character looking for JSON start markers
    let mut i = 0;
    while i < len {
        let ch = bytes[i] as char;

        // Check if this is a potential JSON start
        if ch != '{' && ch != '[' {
            i += 1;
            continue;
        }

        let start_char = ch;
        let end_char = if ch == '{' { '}' } else { ']' };

        // Try to find matching closing bracket/brace
        // We need to handle nested structures
        let mut depth = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_pos = None;

        #[allow(clippy::needless_range_loop)]
        for j in i..len {
            let current = bytes[j] as char;

            if escape_next {
                escape_next = false;
                continue;
            }

            if current == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if current == '"' {
                in_string = !in_string;
                continue;
            }

            if in_string {
                continue;
            }

            if current == start_char {
                depth += 1;
            } else if current == end_char {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(j + 1);
                    break;
                }
            }
        }

        // If we found a complete structure, try to parse it
        if let Some(end) = end_pos {
            let candidate = &text[i..end];

            // Try to parse as JSON to validate
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(candidate) {
                // Check if it's an object or array
                if json_value.is_object() || json_value.is_array() {
                    // Add the raw JSON string to results
                    results.push(Dynamic::from(candidate.to_string()));
                }
            }

            // Move past this structure
            i = end;
        } else {
            // No matching closing bracket found, move to next character
            i += 1;
        }
    }

    results
}

/// Register JSON extraction functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("extract_json", |text: &str| -> Dynamic {
        extract_json_impl(text, 0)
    });

    engine.register_fn("extract_json", |text: &str, nth: i64| -> Dynamic {
        extract_json_impl(text, nth)
    });

    engine.register_fn("extract_jsons", |text: &str| -> Array {
        extract_jsons_impl(text)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    #[test]
    fn test_extract_json_basic() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            r#"Log message: {"level": "error", "msg": "failed"} end"#,
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text)"#)
            .unwrap();

        assert_eq!(
            result.get("level").unwrap().clone().into_string().unwrap(),
            "error"
        );
        assert_eq!(
            result.get("msg").unwrap().clone().into_string().unwrap(),
            "failed"
        );
    }

    #[test]
    fn test_extract_json_nth() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            r#"First: {"a": 1} Second: {"b": 2} Third: {"c": 3}"#,
        );

        // Get second JSON (index 1)
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, 1)"#)
            .unwrap();

        assert_eq!(result.get("b").unwrap().clone().as_int().unwrap(), 2);
    }

    #[test]
    fn test_extract_jsons() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", r#"{"a": 1} text {"b": 2} more [1, 2, 3]"#);

        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"extract_jsons(text)"#)
            .unwrap();

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_extract_json_not_found() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "no json here");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_json(text)"#)
            .unwrap();

        assert!(result.is_empty());
    }
}
