use rhai::{Array, Dynamic, Engine, ImmutableString, Map};

/// Safe equality check for a path
/// Usage: path_equals(e, "user.role", "admin")
pub fn path_equals(event: Map, path: ImmutableString, expected: Dynamic) -> bool {
    let path_str = path.as_str();
    let parts: Vec<&str> = path_str.split('.').collect();

    let mut current_map = event;

    for (i, part) in parts.iter().enumerate() {
        if let Some(value) = current_map.get(*part).cloned() {
            if i == parts.len() - 1 {
                // Last part - compare with expected
                return value.type_name() == expected.type_name()
                    && value.to_string() == expected.to_string();
            } else {
                // Intermediate part - must be a map to continue
                if let Some(nested_map) = value.read_lock::<Map>() {
                    current_map = nested_map.clone();
                } else {
                    return false;
                }
            }
        } else {
            return false;
        }
    }

    false
}

/// Convert value to integer (strict - returns () on error)
/// Usage: to_int(e.amount)
pub fn to_int_strict(value: Dynamic) -> Dynamic {
    // Already an integer
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num);
    }

    // Try to parse string as integer
    if let Some(s) = value.read_lock::<ImmutableString>() {
        if let Ok(num) = s.parse::<i64>() {
            return Dynamic::from(num);
        }
    }

    // Return UNIT if conversion failed
    Dynamic::UNIT
}

/// Convert value to integer with default fallback
/// Usage: to_int_or(e.amount, 0)
pub fn to_int_or(value: Dynamic, default: Dynamic) -> Dynamic {
    // Already an integer
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num);
    }

    // Try to parse string as integer
    if let Some(s) = value.read_lock::<ImmutableString>() {
        if let Ok(num) = s.parse::<i64>() {
            return Dynamic::from(num);
        }
    }

    // Return default if conversion failed
    default
}

/// Convert value to float (strict - returns () on error)
/// Usage: to_float(e.amount)
pub fn to_float_strict(value: Dynamic) -> Dynamic {
    // Already a float
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num);
    }

    // Try to convert integer to float
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num as f64);
    }

    // Try to parse string as float
    if let Some(s) = value.read_lock::<ImmutableString>() {
        if let Ok(num) = s.parse::<f64>() {
            return Dynamic::from(num);
        }
    }

    // Return UNIT if conversion failed
    Dynamic::UNIT
}

/// Convert value to float with default fallback
/// Usage: to_float_or(e.amount, 0.0)
pub fn to_float_or(value: Dynamic, default: Dynamic) -> Dynamic {
    // Already a float
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num);
    }

    // Try to convert integer to float
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num as f64);
    }

    // Try to parse string as float
    if let Some(s) = value.read_lock::<ImmutableString>() {
        if let Ok(num) = s.parse::<f64>() {
            return Dynamic::from(num);
        }
    }

    // Return default if conversion failed
    default
}

/// Convert value to integer with explicit thousands separator
/// Usage: to_int(',') for US format with comma thousands separator
pub fn to_int_with_format(value: Dynamic, thousands_sep: ImmutableString) -> Dynamic {
    // Try existing conversion first (for already-numeric values)
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num);
    }

    // Clean and parse string
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let cleaned = clean_number_string_int(s.as_str(), thousands_sep.as_str());
        if let Ok(num) = cleaned.parse::<i64>() {
            return Dynamic::from(num);
        }
    }

    Dynamic::UNIT
}

/// Convert value to integer with explicit thousands separator and default fallback
/// Usage: to_int_or(',', 0) for US format with comma thousands separator
pub fn to_int_or_with_format(
    value: Dynamic,
    thousands_sep: ImmutableString,
    default: Dynamic,
) -> Dynamic {
    // Try existing conversion first (for already-numeric values)
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num);
    }

    // Clean and parse string
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let cleaned = clean_number_string_int(s.as_str(), thousands_sep.as_str());
        if let Ok(num) = cleaned.parse::<i64>() {
            return Dynamic::from(num);
        }
    }

    default
}

/// Convert value to float with explicit format
/// Usage: to_float(',', '.') for US format (comma thousands, dot decimal)
pub fn to_float_with_format(
    value: Dynamic,
    thousands_sep: ImmutableString,
    decimal_sep: ImmutableString,
) -> Dynamic {
    // Validate decimal_sep: must be single char or empty
    if decimal_sep.chars().count() > 1 {
        return Dynamic::UNIT;
    }

    // Try existing conversion first (for already-numeric values)
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num);
    }
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num as f64);
    }

    // Clean and parse string
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let cleaned =
            clean_number_string_float(s.as_str(), thousands_sep.as_str(), decimal_sep.as_str());
        if let Ok(num) = cleaned.parse::<f64>() {
            return Dynamic::from(num);
        }
    }

    Dynamic::UNIT
}

/// Convert value to float with explicit format and default fallback
/// Usage: to_float_or(',', '.', 0.0) for US format with default
pub fn to_float_or_with_format(
    value: Dynamic,
    thousands_sep: ImmutableString,
    decimal_sep: ImmutableString,
    default: Dynamic,
) -> Dynamic {
    // Validate decimal_sep: must be single char or empty
    if decimal_sep.chars().count() > 1 {
        return default;
    }

    // Try existing conversion first (for already-numeric values)
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num);
    }
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num as f64);
    }

    // Clean and parse string
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let cleaned =
            clean_number_string_float(s.as_str(), thousands_sep.as_str(), decimal_sep.as_str());
        if let Ok(num) = cleaned.parse::<f64>() {
            return Dynamic::from(num);
        }
    }

    default
}

/// Helper to clean number string for integer parsing
/// Removes any character that appears in thousands_sep
fn clean_number_string_int(s: &str, thousands_sep: &str) -> String {
    if thousands_sep.is_empty() {
        return s.to_string();
    }

    // Remove any character that appears in thousands_sep
    thousands_sep
        .chars()
        .fold(s.to_string(), |acc, c| acc.replace(c, ""))
}

/// Helper to clean number string for float parsing
/// Removes any character that appears in thousands_sep
/// Replaces decimal_sep (must be single char or empty) with '.'
/// Note: decimal replacement happens BEFORE thousands removal to avoid conflicts
fn clean_number_string_float(s: &str, thousands_sep: &str, decimal_sep: &str) -> String {
    let mut result = s.to_string();

    // Replace decimal separator with standard dot FIRST (if not empty and not already '.')
    // This must happen before thousands removal to avoid conflicts when the same
    // character appears in both (e.g., comma in "1,234 567,89" with thousands=", " and decimal=",")
    if !decimal_sep.is_empty() && decimal_sep != "." {
        result = result.replace(decimal_sep, ".");
    }

    // Remove any character that appears in thousands_sep (if not empty)
    if !thousands_sep.is_empty() {
        result = thousands_sep
            .chars()
            .fold(result, |acc, c| acc.replace(c, ""));
    }

    result
}

/// Extract value from a nested path with default fallback
/// Usage: get_path(e, "user.role", "guest")
/// Supports dot notation and bracket notation: "user.items[0].name"
pub fn get_path_with_default(event: Map, path: ImmutableString, default: Dynamic) -> Dynamic {
    extract_path_value(&Dynamic::from(event), path.as_str()).unwrap_or(default)
}

/// Extract value from a nested path (returns null if not found)
/// Usage: get_path(e, "user.role")
pub fn get_path(event: Map, path: ImmutableString) -> Dynamic {
    extract_path_value(&Dynamic::from(event), path.as_str()).unwrap_or(Dynamic::UNIT)
}

/// Extract value from a JSON string with default fallback
/// Usage: get_path(json_string, "user.role", "guest")
pub fn get_path_json_with_default(
    json_string: ImmutableString,
    path: ImmutableString,
    default: Dynamic,
) -> Dynamic {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_string.as_str()) {
        let dynamic = crate::event::json_to_dynamic(&parsed);
        extract_path_value(&dynamic, path.as_str()).unwrap_or(default)
    } else {
        default
    }
}

/// Extract value from a JSON string (returns null if not found)
/// Usage: get_path(json_string, "user.role")
pub fn get_path_json(json_string: ImmutableString, path: ImmutableString) -> Dynamic {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_string.as_str()) {
        let dynamic = crate::event::json_to_dynamic(&parsed);
        extract_path_value(&dynamic, path.as_str()).unwrap_or(Dynamic::UNIT)
    } else {
        Dynamic::UNIT
    }
}

/// Check if a nested path exists
/// Usage: has_path(e, "user.role")
pub fn has_path(event: Map, path: ImmutableString) -> bool {
    extract_path_value(&Dynamic::from(event), path.as_str()).is_some()
}

/// Internal function to extract value from a path string
/// Supports dot notation (user.name) and bracket notation (scores[0], scores[-1])
fn extract_path_value(value: &Dynamic, path: &str) -> Option<Dynamic> {
    let mut current = value.clone();

    // Parse the path into tokens
    let tokens = parse_path_tokens(path);

    for token in tokens {
        match token {
            PathToken::Field(field_name) => {
                let next_value = {
                    let map = current.read_lock::<Map>()?;
                    map.get(field_name.as_str())?.clone()
                };
                current = next_value;
            }
            PathToken::Index(index) => {
                let next_value = {
                    let array = current.read_lock::<Array>()?;
                    let array_len = array.len() as i64;
                    let actual_index = if index < 0 {
                        array_len + index // Negative indexing
                    } else {
                        index
                    };

                    if actual_index >= 0 && (actual_index as usize) < array.len() {
                        array[actual_index as usize].clone()
                    } else {
                        return None;
                    }
                };
                current = next_value;
            }
        }
    }

    Some(current)
}

/// Path token types for parsing
#[derive(Debug, Clone)]
enum PathToken {
    Field(String),
    Index(i64),
}

/// Parse a path string into tokens
/// Examples:
///   "user.name" -> [Field("user"), Field("name")]
///   "scores[0]" -> [Field("scores"), Index(0)]
///   "user.items[-1].name" -> [Field("user"), Field("items"), Index(-1), Field("name")]
fn parse_path_tokens(path: &str) -> Vec<PathToken> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !current_token.is_empty() {
                    tokens.push(PathToken::Field(current_token.clone()));
                    current_token.clear();
                }
            }
            '[' => {
                // Finish current field token if any
                if !current_token.is_empty() {
                    tokens.push(PathToken::Field(current_token.clone()));
                    current_token.clear();
                }

                // Parse the index
                let mut index_str = String::new();
                for inner_ch in chars.by_ref() {
                    if inner_ch == ']' {
                        break;
                    }
                    index_str.push(inner_ch);
                }

                if let Ok(index) = index_str.parse::<i64>() {
                    tokens.push(PathToken::Index(index));
                }
            }
            _ => {
                current_token.push(ch);
            }
        }
    }

    // Add final token if any
    if !current_token.is_empty() {
        tokens.push(PathToken::Field(current_token));
    }

    tokens
}

/// Convert value to boolean (strict - returns () on error)
/// Usage: to_bool(e.active)
pub fn to_bool_strict(value: Dynamic) -> Dynamic {
    // Already a boolean
    if let Ok(b) = value.as_bool() {
        return Dynamic::from(b);
    }

    // String conversion
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let s_lower = s.to_lowercase();
        match s_lower.as_str() {
            "true" | "yes" | "1" | "on" => return Dynamic::from(true),
            "false" | "no" | "0" | "off" => return Dynamic::from(false),
            _ => {}
        }
    }

    // Number conversion (0 = false, non-zero = true)
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num != 0);
    }
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num != 0.0);
    }

    // Return UNIT if conversion failed
    Dynamic::UNIT
}

/// Convert value to boolean with default fallback
/// Usage: to_bool_or(e.active, false)
pub fn to_bool_or(value: Dynamic, default: Dynamic) -> Dynamic {
    // Already a boolean
    if let Ok(b) = value.as_bool() {
        return Dynamic::from(b);
    }

    // String conversion
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let s_lower = s.to_lowercase();
        match s_lower.as_str() {
            "true" | "yes" | "1" | "on" => return Dynamic::from(true),
            "false" | "no" | "0" | "off" => return Dynamic::from(false),
            _ => {}
        }
    }

    // Number conversion (0 = false, non-zero = true)
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num != 0);
    }
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num != 0.0);
    }

    // Return default if conversion failed
    default
}

/// Register safety functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // Path access functions
    engine.register_fn("get_path", get_path);
    engine.register_fn("get_path", get_path_with_default);
    engine.register_fn("get_path", get_path_json);
    engine.register_fn("get_path", get_path_json_with_default);
    engine.register_fn("has_path", has_path);

    // Other safety functions
    engine.register_fn("path_equals", path_equals);

    // Conversion functions - strict variants (return () on error)
    engine.register_fn("to_int", to_int_strict);
    engine.register_fn("to_float", to_float_strict);
    engine.register_fn("to_bool", to_bool_strict);

    // Conversion functions - with defaults
    engine.register_fn("to_int_or", to_int_or);
    engine.register_fn("to_float_or", to_float_or);
    engine.register_fn("to_bool_or", to_bool_or);

    // Conversion functions - with explicit format (NEW overloads)
    engine.register_fn("to_int", to_int_with_format);
    engine.register_fn("to_int_or", to_int_or_with_format);
    engine.register_fn("to_float", to_float_with_format);
    engine.register_fn("to_float_or", to_float_or_with_format);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Map;

    fn create_test_event() -> Map {
        let mut event = Map::new();
        event.insert("name".into(), Dynamic::from("alice"));
        event.insert("score".into(), Dynamic::from(85));

        let mut user = Map::new();
        user.insert("role".into(), Dynamic::from("admin"));
        user.insert("active".into(), Dynamic::from(true));

        // Add arrays for testing
        let scores_array = vec![Dynamic::from(100), Dynamic::from(85), Dynamic::from(92)];
        event.insert("scores".into(), Dynamic::from(scores_array));

        // Add nested structure with arrays
        let mut items = Vec::new();
        let mut item1 = Map::new();
        item1.insert("name".into(), Dynamic::from("item1"));
        item1.insert("value".into(), Dynamic::from(42));
        items.push(Dynamic::from(item1));

        let mut item2 = Map::new();
        item2.insert("name".into(), Dynamic::from("item2"));
        item2.insert("value".into(), Dynamic::from(33));
        items.push(Dynamic::from(item2));

        let mut details = Map::new();
        details.insert("items".into(), Dynamic::from(items));
        user.insert("details".into(), Dynamic::from(details));

        event.insert("user".into(), Dynamic::from(user));

        event
    }

    #[test]
    fn test_path_equals() {
        let event = create_test_event();
        assert!(path_equals(
            event.clone(),
            "name".into(),
            Dynamic::from("alice")
        ));
        assert!(path_equals(
            event.clone(),
            "user.role".into(),
            Dynamic::from("admin")
        ));
        assert!(!path_equals(
            event.clone(),
            "name".into(),
            Dynamic::from("bob")
        ));
        assert!(!path_equals(
            event,
            "missing".into(),
            Dynamic::from("anything")
        ));
    }

    #[test]
    fn test_to_int_strict() {
        // Test integer input
        let result = to_int_strict(Dynamic::from(42i64));
        assert_eq!(result.as_int().unwrap(), 42i64);

        // Test string integer input
        let result = to_int_strict(Dynamic::from("123"));
        assert_eq!(result.as_int().unwrap(), 123i64);

        // Test invalid input returns UNIT
        let result = to_int_strict(Dynamic::from("invalid"));
        assert!(result.is_unit());

        // Test float input returns UNIT (strict int conversion)
        let result = to_int_strict(Dynamic::from(12.5));
        assert!(result.is_unit());
    }

    #[test]
    fn test_to_int_or() {
        // Test integer input
        let result = to_int_or(Dynamic::from(42i64), Dynamic::from(0i64));
        assert_eq!(result.as_int().unwrap(), 42i64);

        // Test string integer input
        let result = to_int_or(Dynamic::from("123"), Dynamic::from(0i64));
        assert_eq!(result.as_int().unwrap(), 123i64);

        // Test invalid input with default
        let result = to_int_or(Dynamic::from("invalid"), Dynamic::from(999i64));
        assert_eq!(result.as_int().unwrap(), 999i64);
    }

    #[test]
    fn test_to_float_strict() {
        // Test float input
        let result = to_float_strict(Dynamic::from(std::f64::consts::PI));
        assert_eq!(result.as_float().unwrap(), std::f64::consts::PI);

        // Test integer input (should convert to float)
        let result = to_float_strict(Dynamic::from(42i64));
        assert_eq!(result.as_float().unwrap(), 42.0);

        // Test string float input
        let result = to_float_strict(Dynamic::from("12.5"));
        assert_eq!(result.as_float().unwrap(), 12.5);

        // Test invalid input returns UNIT
        let result = to_float_strict(Dynamic::from("invalid"));
        assert!(result.is_unit());
    }

    #[test]
    fn test_to_float_or() {
        // Test float input
        let result = to_float_or(Dynamic::from(std::f64::consts::PI), Dynamic::from(0.0));
        assert_eq!(result.as_float().unwrap(), std::f64::consts::PI);

        // Test integer input (should convert to float)
        let result = to_float_or(Dynamic::from(42i64), Dynamic::from(0.0));
        assert_eq!(result.as_float().unwrap(), 42.0);

        // Test string float input
        let result = to_float_or(Dynamic::from("12.5"), Dynamic::from(0.0));
        assert_eq!(result.as_float().unwrap(), 12.5);

        // Test invalid input with default
        let result = to_float_or(Dynamic::from("invalid"), Dynamic::from(999.0));
        assert_eq!(result.as_float().unwrap(), 999.0);
    }

    #[test]
    fn test_to_bool_strict() {
        // Test boolean input
        assert!(to_bool_strict(Dynamic::from(true)).as_bool().unwrap());
        assert!(!to_bool_strict(Dynamic::from(false)).as_bool().unwrap());

        // Test string conversion
        assert!(to_bool_strict(Dynamic::from("yes")).as_bool().unwrap());
        assert!(to_bool_strict(Dynamic::from("1")).as_bool().unwrap());
        assert!(!to_bool_strict(Dynamic::from("false")).as_bool().unwrap());

        // Test number conversion
        assert!(to_bool_strict(Dynamic::from(1i64)).as_bool().unwrap());
        assert!(!to_bool_strict(Dynamic::from(0i64)).as_bool().unwrap());

        // Test invalid input returns UNIT
        let result = to_bool_strict(Dynamic::from("invalid"));
        assert!(result.is_unit());
    }

    #[test]
    fn test_to_bool_or() {
        assert!(to_bool_or(Dynamic::from(true), Dynamic::from(false))
            .as_bool()
            .unwrap());
        assert!(to_bool_or(Dynamic::from("yes"), Dynamic::from(false))
            .as_bool()
            .unwrap());
        assert!(to_bool_or(Dynamic::from("1"), Dynamic::from(false))
            .as_bool()
            .unwrap());
        assert!(!to_bool_or(Dynamic::from("false"), Dynamic::from(true))
            .as_bool()
            .unwrap());
        assert!(to_bool_or(Dynamic::from(1i64), Dynamic::from(false))
            .as_bool()
            .unwrap());
        assert!(!to_bool_or(Dynamic::from(0i64), Dynamic::from(true))
            .as_bool()
            .unwrap());
        assert!(to_bool_or(Dynamic::from("invalid"), Dynamic::from(true))
            .as_bool()
            .unwrap());
    }

    #[test]
    fn test_get_path() {
        let event = create_test_event();

        // Test simple field access
        let name = get_path(event.clone(), "name".into());
        assert_eq!(name.cast::<String>(), "alice");

        // Test nested field access
        let role = get_path(event.clone(), "user.role".into());
        assert_eq!(role.cast::<String>(), "admin");

        // Test missing field returns UNIT
        let missing = get_path(event.clone(), "missing".into());
        assert!(missing.is_unit());

        // Test missing nested field returns UNIT
        let missing_nested = get_path(event, "user.missing".into());
        assert!(missing_nested.is_unit());
    }

    #[test]
    fn test_get_path_with_default() {
        let event = create_test_event();

        // Test simple field access
        let name = get_path_with_default(event.clone(), "name".into(), Dynamic::from("default"));
        assert_eq!(name.cast::<String>(), "alice");

        // Test nested field access
        let role = get_path_with_default(event.clone(), "user.role".into(), Dynamic::from("guest"));
        assert_eq!(role.cast::<String>(), "admin");

        // Test missing field returns default
        let missing =
            get_path_with_default(event.clone(), "missing".into(), Dynamic::from("default"));
        assert_eq!(missing.cast::<String>(), "default");

        // Test missing nested field returns default
        let missing_nested =
            get_path_with_default(event, "user.missing".into(), Dynamic::from("guest"));
        assert_eq!(missing_nested.cast::<String>(), "guest");
    }

    #[test]
    fn test_get_path_array_access() {
        let event = create_test_event();

        // Test array index access
        let first_score = get_path(event.clone(), "scores[0]".into());
        // Handle both i32 and i64 integer types
        let first_val = if let Ok(val) = first_score.as_int() {
            val
        } else {
            first_score.cast::<i32>() as i64
        };
        assert_eq!(first_val, 100);

        let second_score = get_path(event.clone(), "scores[1]".into());
        let second_val = if let Ok(val) = second_score.as_int() {
            val
        } else {
            second_score.cast::<i32>() as i64
        };
        assert_eq!(second_val, 85);

        // Test negative indexing
        let last_score = get_path(event.clone(), "scores[-1]".into());
        let last_val = if let Ok(val) = last_score.as_int() {
            val
        } else {
            last_score.cast::<i32>() as i64
        };
        assert_eq!(last_val, 92);

        // Test out of bounds returns UNIT
        let out_of_bounds = get_path(event, "scores[10]".into());
        assert!(out_of_bounds.is_unit());
    }

    #[test]
    fn test_get_path_nested_array_access() {
        let event = create_test_event();

        // Test nested array access
        let item_name = get_path(event.clone(), "user.details.items[0].name".into());
        assert_eq!(item_name.cast::<String>(), "item1");

        let item_value = get_path(event.clone(), "user.details.items[1].value".into());
        let item_val = if let Ok(val) = item_value.as_int() {
            val
        } else {
            item_value.cast::<i32>() as i64
        };
        assert_eq!(item_val, 33);

        // Test with default for missing nested array item
        let missing_item = get_path_with_default(
            event,
            "user.details.items[10].name".into(),
            Dynamic::from("unknown"),
        );
        assert_eq!(missing_item.cast::<String>(), "unknown");
    }

    #[test]
    fn test_has_path() {
        let event = create_test_event();

        // Test existing paths
        assert!(has_path(event.clone(), "name".into()));
        assert!(has_path(event.clone(), "user.role".into()));
        assert!(has_path(event.clone(), "scores[0]".into()));
        assert!(has_path(event.clone(), "user.details.items[0].name".into()));

        // Test missing paths
        assert!(!has_path(event.clone(), "missing".into()));
        assert!(!has_path(event.clone(), "user.missing".into()));
        assert!(!has_path(event.clone(), "scores[10]".into()));
        assert!(!has_path(event, "user.details.items[10].name".into()));
    }

    #[test]
    fn test_get_path_json() {
        let json_string: ImmutableString =
            r#"{"user": {"role": "admin"}, "scores": [100, 85, 92]}"#.into();

        // Test JSON parsing with path access
        let role = get_path_json(json_string.clone(), "user.role".into());
        assert_eq!(role.cast::<String>(), "admin");

        // Test with default for missing path
        let missing =
            get_path_json_with_default(json_string, "missing".into(), Dynamic::from("default"));
        assert_eq!(missing.cast::<String>(), "default");
    }

    #[test]
    fn test_get_path_json_invalid() {
        let invalid_json: ImmutableString = "invalid json".into();

        // Test invalid JSON returns UNIT
        let result = get_path_json(invalid_json.clone(), "user.role".into());
        assert!(result.is_unit());

        // Test invalid JSON returns default
        let result_with_default =
            get_path_json_with_default(invalid_json, "user.role".into(), Dynamic::from("default"));
        assert_eq!(result_with_default.cast::<String>(), "default");
    }

    #[test]
    fn test_parse_path_tokens() {
        // Test simple field
        let tokens = parse_path_tokens("name");
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            PathToken::Field(field) => assert_eq!(field, "name"),
            _ => panic!("Expected field token"),
        }

        // Test nested fields
        let tokens = parse_path_tokens("user.role");
        assert_eq!(tokens.len(), 2);
        match (&tokens[0], &tokens[1]) {
            (PathToken::Field(f1), PathToken::Field(f2)) => {
                assert_eq!(f1, "user");
                assert_eq!(f2, "role");
            }
            _ => panic!("Expected field tokens"),
        }

        // Test array access
        let tokens = parse_path_tokens("scores[0]");
        assert_eq!(tokens.len(), 2);
        match (&tokens[0], &tokens[1]) {
            (PathToken::Field(field), PathToken::Index(index)) => {
                assert_eq!(field, "scores");
                assert_eq!(*index, 0);
            }
            _ => panic!("Expected field and index tokens"),
        }

        // Test negative array access
        let tokens = parse_path_tokens("scores[-1]");
        assert_eq!(tokens.len(), 2);
        match &tokens[1] {
            PathToken::Index(index) => assert_eq!(*index, -1),
            _ => panic!("Expected negative index token"),
        }

        // Test complex path
        let tokens = parse_path_tokens("user.details.items[1].name");
        assert_eq!(tokens.len(), 5);
        match (&tokens[0], &tokens[1], &tokens[2], &tokens[3], &tokens[4]) {
            (
                PathToken::Field(f1),
                PathToken::Field(f2),
                PathToken::Field(f3),
                PathToken::Index(index),
                PathToken::Field(f4),
            ) => {
                assert_eq!(f1, "user");
                assert_eq!(f2, "details");
                assert_eq!(f3, "items");
                assert_eq!(*index, 1);
                assert_eq!(f4, "name");
            }
            _ => panic!("Expected complex path tokens"),
        }
    }

    #[test]
    fn test_to_int_with_multi_char_thousands_sep() {
        // Test removing any character in thousands_sep string
        let result = to_int_with_format(Dynamic::from("1,234'567"), ImmutableString::from(",'"));
        assert_eq!(result.as_int().unwrap(), 1234567);

        // Test with multiple separator chars (space and comma)
        let result = to_int_with_format(Dynamic::from("1,234 567"), ImmutableString::from(", "));
        assert_eq!(result.as_int().unwrap(), 1234567);

        // Test with underscore and dash
        let result = to_int_with_format(Dynamic::from("1_234-567"), ImmutableString::from("_-"));
        assert_eq!(result.as_int().unwrap(), 1234567);
    }

    #[test]
    fn test_to_float_with_multi_char_thousands_sep() {
        // Test removing any character in thousands_sep string (comma and apostrophe)
        let result = to_float_with_format(
            Dynamic::from("1,234'567.89"),
            ImmutableString::from(",'"),
            ImmutableString::from("."),
        );
        assert!((result.as_float().unwrap() - 1234567.89).abs() < 0.001);

        // Test with space and apostrophe for thousands, comma for decimal
        // Note: Can't use dot in thousands_sep when comma is decimal (they'd conflict)
        let result = to_float_with_format(
            Dynamic::from("1'234 567,89"),
            ImmutableString::from("' "),
            ImmutableString::from(","),
        );
        assert!((result.as_float().unwrap() - 1234567.89).abs() < 0.001);

        // Test with underscore and dash for thousands
        let result = to_float_with_format(
            Dynamic::from("1_234-567.89"),
            ImmutableString::from("_-"),
            ImmutableString::from("."),
        );
        assert!((result.as_float().unwrap() - 1234567.89).abs() < 0.001);
    }

    #[test]
    fn test_to_float_rejects_multi_char_decimal_sep() {
        // Multi-char decimal_sep should return UNIT
        let result = to_float_with_format(
            Dynamic::from("1234.56"),
            ImmutableString::from(","),
            ImmutableString::from(".,"),
        );
        assert!(result.is_unit());

        // Multi-char decimal_sep with default should return default
        let result = to_float_or_with_format(
            Dynamic::from("1234.56"),
            ImmutableString::from(","),
            ImmutableString::from(".,"),
            Dynamic::from(999.0),
        );
        assert_eq!(result.as_float().unwrap(), 999.0);
    }

    #[test]
    fn test_to_int_or_with_multi_char_thousands_sep() {
        // Test with default fallback
        let result = to_int_or_with_format(
            Dynamic::from("1,234'567"),
            ImmutableString::from(",'"),
            Dynamic::from(0i64),
        );
        assert_eq!(result.as_int().unwrap(), 1234567);

        // Test invalid input with default
        let result = to_int_or_with_format(
            Dynamic::from("invalid"),
            ImmutableString::from(",'"),
            Dynamic::from(999i64),
        );
        assert_eq!(result.as_int().unwrap(), 999);
    }

    #[test]
    fn test_to_float_or_with_multi_char_thousands_sep() {
        // Test with default fallback
        let result = to_float_or_with_format(
            Dynamic::from("1,234'567.89"),
            ImmutableString::from(",'"),
            ImmutableString::from("."),
            Dynamic::from(0.0),
        );
        assert!((result.as_float().unwrap() - 1234567.89).abs() < 0.001);

        // Test invalid input with default
        let result = to_float_or_with_format(
            Dynamic::from("invalid"),
            ImmutableString::from(",'"),
            ImmutableString::from("."),
            Dynamic::from(999.0),
        );
        assert_eq!(result.as_float().unwrap(), 999.0);
    }
}
