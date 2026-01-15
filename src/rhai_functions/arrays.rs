use crate::event::{flatten_dynamic, FlattenStyle};
use indexmap::IndexMap;
use rhai::{Array, Dynamic, Engine, Map};

/// Register array manipulation functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // Register sorted function - like Python's sorted(), takes any iterable and returns sorted items
    engine.register_fn("sorted", sorted_array);

    // Register reversed function - like Python's reversed(), reverses array order
    engine.register_fn("reversed", reversed_array);

    // Register slice function - Python-style slicing for arrays
    engine.register_fn("slice", slice_array);

    // Register unique function - remove all duplicates from array
    engine.register_fn("unique", unique_array);

    // Register sorted_by function - sort objects/maps by field name
    engine.register_fn("sorted_by", sorted_by_field);

    // Register contains_any function - check if array contains any of the specified values
    engine.register_fn("contains_any", contains_any_array);

    // Register starts_with_any function - check if array starts with any of the specified values
    engine.register_fn("starts_with_any", starts_with_any_array);

    // Register field extraction helpers for arrays of maps
    engine.register_fn("pluck", pluck);
    engine.register_fn("pluck_as_nums", pluck_as_nums);

    // Register min/max functions - find minimum/maximum values in arrays
    engine.register_fn("min", min_array);
    engine.register_fn("max", max_array);

    // Array flattening functions

    // Default flattened() for arrays - uses bracket style, unlimited depth
    engine.register_fn("flattened", |array: Array| -> Map {
        let dynamic_array = Dynamic::from(array);
        let flattened = flatten_dynamic(&dynamic_array, FlattenStyle::default(), 0);
        convert_indexmap_to_rhai_map(flattened)
    });

    // flattened(style) for arrays - specify style, unlimited depth
    engine.register_fn("flattened", |array: Array, style: &str| -> Map {
        let flatten_style = match style {
            "dot" => FlattenStyle::Dot,
            "bracket" => FlattenStyle::Bracket,
            "underscore" => FlattenStyle::Underscore,
            _ => FlattenStyle::default(), // Default to bracket for unknown styles
        };
        let dynamic_array = Dynamic::from(array);
        let flattened = flatten_dynamic(&dynamic_array, flatten_style, 0);
        convert_indexmap_to_rhai_map(flattened)
    });

    // flattened(style, max_depth) for arrays - full control
    engine.register_fn(
        "flattened",
        |array: Array, style: &str, max_depth: i64| -> Map {
            let flatten_style = match style {
                "dot" => FlattenStyle::Dot,
                "bracket" => FlattenStyle::Bracket,
                "underscore" => FlattenStyle::Underscore,
                _ => FlattenStyle::default(),
            };
            let max_depth = if max_depth < 0 { 0 } else { max_depth as usize }; // negative = unlimited
            let dynamic_array = Dynamic::from(array);
            let flattened = flatten_dynamic(&dynamic_array, flatten_style, max_depth);
            convert_indexmap_to_rhai_map(flattened)
        },
    );
}

/// Extract a field from an array of maps, preserving original value types
fn pluck(array: Array, field_name: String) -> Array {
    let mut results = Array::new();

    for item in array {
        if let Some(map) = item.try_cast::<Map>() {
            if let Some(value) = map.get(field_name.as_str()) {
                if value.is_unit() {
                    continue;
                }
                results.push(value.clone());
            }
        }
    }

    results
}

/// Extract a field from an array of maps and convert results to f64
fn pluck_as_nums(array: Array, field_name: String) -> Array {
    let mut results = Array::new();

    for item in array {
        if let Some(map) = item.try_cast::<Map>() {
            if let Some(value) = map.get(field_name.as_str()) {
                if value.is_unit() {
                    continue;
                }

                if let Some(number) = dynamic_to_f64(value) {
                    results.push(Dynamic::from(number));
                }
            }
        }
    }

    results
}

/// Convert a Dynamic value to f64 if possible
fn dynamic_to_f64(value: &Dynamic) -> Option<f64> {
    if value.is_int() {
        value.as_int().ok().map(|v| v as f64)
    } else if value.is_float() {
        value.as_float().ok()
    } else if value.is_bool() {
        value.as_bool().ok().map(|v| if v { 1.0 } else { 0.0 })
    } else if value.is_string() {
        value.clone().into_string().ok()?.parse::<f64>().ok()
    } else {
        value.to_string().parse::<f64>().ok()
    }
}

/// Sort an array and return a new sorted array (like Python's sorted())
///
/// Takes any array and returns a new sorted array without modifying the original.
/// The sorting is done by converting elements to strings and comparing them lexicographically.
/// Numbers are compared as numbers when both elements are numeric.
///
/// # Arguments
/// * `arr` - The array to sort
///
/// # Returns
/// A new array containing the same elements in sorted order
///
/// # Examples
/// ```rhai
/// let numbers = [3, 1, 4, 1, 5];
/// let sorted_nums = sorted(numbers);  // [1, 1, 3, 4, 5]
/// print(numbers);  // [3, 1, 4, 1, 5] - original unchanged
///
/// let strings = ["banana", "apple", "cherry"];
/// let sorted_strings = sorted(strings);  // ["apple", "banana", "cherry"]
///
/// let mixed = [3, "banana", 1, "apple"];
/// let sorted_mixed = sorted(mixed);  // [1, 3, "apple", "banana"]
/// ```
///
/// # Sorting Behavior
/// - Numbers are compared numerically when both elements are numbers
/// - Strings are compared lexicographically
/// - Mixed types: numbers come before strings
/// - Booleans are converted to strings ("false", "true")
/// - Null values are converted to empty strings and sort first
fn sorted_array(mut arr: Array) -> Array {
    // Sort using a custom comparator
    arr.sort_by(|a, b| {
        // Handle null values first
        if a.is_unit() && b.is_unit() {
            return std::cmp::Ordering::Equal;
        }
        if a.is_unit() {
            return std::cmp::Ordering::Less;
        }
        if b.is_unit() {
            return std::cmp::Ordering::Greater;
        }

        // If both are numbers, compare numerically
        if let (Ok(a_num), Ok(b_num)) = (get_number_value(a), get_number_value(b)) {
            return a_num
                .partial_cmp(&b_num)
                .unwrap_or(std::cmp::Ordering::Equal);
        }

        // If one is a number and the other isn't, numbers come first
        if get_number_value(a).is_ok() && get_number_value(b).is_err() {
            return std::cmp::Ordering::Less;
        }
        if get_number_value(a).is_err() && get_number_value(b).is_ok() {
            return std::cmp::Ordering::Greater;
        }

        // Otherwise, compare as strings
        let a_str = a.to_string();
        let b_str = b.to_string();
        a_str.cmp(&b_str)
    });

    arr
}

/// Helper function to extract numeric value from a Dynamic
fn get_number_value(value: &Dynamic) -> Result<f64, ()> {
    if value.is_int() {
        Ok(value.as_int().unwrap_or(0) as f64)
    } else if value.is_float() {
        Ok(value.as_float().unwrap_or(0.0))
    } else if value.is_string() {
        // Try to parse string as number
        let str_val = value.clone().into_string().unwrap_or_default();
        str_val.parse::<f64>().map_err(|_| ())
    } else {
        Err(())
    }
}

/// Reverse an array and return a new reversed array (like Python's reversed())
///
/// Takes any array and returns a new array with elements in reverse order
/// without modifying the original.
///
/// # Arguments
/// * `arr` - The array to reverse
///
/// # Returns
/// A new array containing the same elements in reverse order
///
/// # Examples
/// ```rhai
/// let numbers = [1, 2, 3, 4, 5];
/// let backwards = reversed(numbers);  // [5, 4, 3, 2, 1]
/// print(numbers);  // [1, 2, 3, 4, 5] - original unchanged
///
/// let words = ["first", "second", "third"];
/// let reversed_words = reversed(words);  // ["third", "second", "first"]
///
/// // Common pattern: sort then reverse for descending order
/// let scores = [85, 92, 78, 96, 88];
/// let highest_first = reversed(sorted(scores));  // [96, 92, 88, 85, 78]
/// ```
fn reversed_array(mut arr: Array) -> Array {
    arr.reverse();
    arr
}

/// Python-style slicing for arrays using "start:end:step" notation.
fn slice_array(arr: Array, spec: &str) -> Array {
    let len = arr.len() as i32;
    if len == 0 {
        return Array::new();
    }

    let parts: Vec<&str> = spec.split(':').collect();

    let step = if parts.len() > 2 && !parts[2].trim().is_empty() {
        parts[2].trim().parse::<i32>().unwrap_or(1)
    } else {
        1
    };

    if step == 0 {
        return Array::new();
    }

    let (default_start, default_end) = if step > 0 { (0, len) } else { (len - 1, -1) };

    let start = if !parts.is_empty() && !parts[0].trim().is_empty() {
        let mut value = parts[0].trim().parse::<i32>().unwrap_or(default_start);
        if value < 0 {
            value += len;
        }
        if step > 0 {
            value.clamp(0, len)
        } else {
            value.clamp(0, len - 1)
        }
    } else {
        default_start
    };

    let end = if parts.len() > 1 && !parts[1].trim().is_empty() {
        let mut value = parts[1].trim().parse::<i32>().unwrap_or(default_end);
        if value < 0 {
            value += len;
        }
        if step > 0 {
            value.clamp(0, len)
        } else {
            value.clamp(-1, len - 1)
        }
    } else {
        default_end
    };

    let mut result = Array::new();
    let mut i = start;

    if step > 0 {
        while i < end {
            if i >= 0 && i < len {
                if let Some(value) = arr.get(i as usize) {
                    result.push(value.clone());
                }
            }
            i += step;
        }
    } else {
        while i > end {
            if i >= 0 && i < len {
                if let Some(value) = arr.get(i as usize) {
                    result.push(value.clone());
                }
            }
            i += step;
        }
    }

    result
}

/// Remove all duplicate elements from an array, preserving first occurrence order
///
/// Takes an array and returns a new array with all duplicate elements removed.
/// Unlike Rhai's built-in `dedup()` which only removes consecutive duplicates,
/// `unique()` removes ALL duplicates from anywhere in the array.
///
/// The first occurrence of each element is preserved in the order it appears.
/// Comparison is done by converting elements to strings.
///
/// # Arguments
/// * `arr` - The array to deduplicate
///
/// # Returns
/// A new array containing only unique elements in first-occurrence order
///
/// # Examples
/// ```rhai
/// let numbers = [1, 2, 2, 3, 2, 1];
/// let unique_nums = unique(numbers);  // [1, 2, 3]
/// let deduped = dedup(numbers);       // [1, 2, 3, 2, 1] - only consecutive removed
///
/// let tags = ["bug", "urgent", "bug", "frontend", "urgent"];
/// let unique_tags = unique(tags);  // ["bug", "urgent", "frontend"]
///
/// // Useful for aggregating unique values
/// let events = [
///     {"user": "alice", "action": "login"},
///     {"user": "bob", "action": "login"},
///     {"user": "alice", "action": "logout"}
/// ];
/// let users = events.map(|e| e.user);  // ["alice", "bob", "alice"]
/// let unique_users = unique(users);     // ["alice", "bob"]
/// ```
///
/// # Comparison with dedup()
/// - `unique([1, 2, 2, 3, 2, 1])` → `[1, 2, 3]` (all duplicates removed)
/// - `dedup([1, 2, 2, 3, 2, 1])` → `[1, 2, 3, 2, 1]` (only consecutive removed)
fn unique_array(arr: Array) -> Array {
    let mut seen = std::collections::HashSet::new();
    let mut result = Array::new();

    for item in arr {
        let item_str = item.to_string();
        if seen.insert(item_str) {
            result.push(item);
        }
    }

    result
}

/// Sort an array of objects/maps by a specific field name
///
/// Takes an array of objects (maps) and sorts them by the specified field.
/// The field values are compared using the same logic as sorted():
/// numbers numerically, strings lexicographically, mixed types with numbers first.
///
/// # Arguments
/// * `arr` - The array of objects to sort
/// * `field_name` - The name of the field to sort by
///
/// # Returns
/// A new array containing the same objects sorted by the specified field
///
/// # Examples
/// ```rhai
/// let users = [
///     {"name": "alice", "age": 30, "score": 85},
///     {"name": "bob", "age": 25, "score": 92},
///     {"name": "charlie", "age": 35, "score": 78}
/// ];
///
/// let by_age = sorted_by(users, "age");      // Sorted by age: bob(25), alice(30), charlie(35)
/// let by_score = sorted_by(users, "score");  // Sorted by score: charlie(78), alice(85), bob(92)
/// let by_name = sorted_by(users, "name");    // Sorted by name: alice, bob, charlie
///
/// // Use with log entries
/// let events = parse_json(line).events;
/// let by_timestamp = sorted_by(events, "timestamp");  // Chronological order
/// let by_severity = sorted_by(events, "level");       // By log level
/// ```
///
/// # Behavior with Missing Fields
/// - Objects without the specified field are placed at the beginning
/// - Objects with null/empty field values sort before non-empty values
/// - Field values are compared using the same rules as sorted()
fn sorted_by_field(mut arr: Array, field_name: String) -> Array {
    // Sort using a custom comparator that looks at the specified field
    arr.sort_by(|a, b| {
        // Extract field values from both objects
        let a_field = extract_field_value(a, &field_name);
        let b_field = extract_field_value(b, &field_name);

        // Handle missing fields - objects without the field come first
        match (a_field, b_field) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a_val), Some(b_val)) => {
                // Use the same comparison logic as sorted()
                compare_dynamic_values(&a_val, &b_val)
            }
        }
    });

    arr
}

/// Helper function to extract a field value from a Dynamic object
fn extract_field_value(obj: &Dynamic, field_name: &str) -> Option<Dynamic> {
    if let Some(map) = obj.clone().try_cast::<rhai::Map>() {
        map.get(field_name).cloned()
    } else {
        None
    }
}

/// Helper function to compare two Dynamic values using the same logic as sorted()
fn compare_dynamic_values(a: &Dynamic, b: &Dynamic) -> std::cmp::Ordering {
    // Handle null values first
    if a.is_unit() && b.is_unit() {
        return std::cmp::Ordering::Equal;
    }
    if a.is_unit() {
        return std::cmp::Ordering::Less;
    }
    if b.is_unit() {
        return std::cmp::Ordering::Greater;
    }

    // If both are numbers, compare numerically
    if let (Ok(a_num), Ok(b_num)) = (get_number_value(a), get_number_value(b)) {
        return a_num
            .partial_cmp(&b_num)
            .unwrap_or(std::cmp::Ordering::Equal);
    }

    // If one is a number and the other isn't, numbers come first
    if get_number_value(a).is_ok() && get_number_value(b).is_err() {
        return std::cmp::Ordering::Less;
    }
    if get_number_value(a).is_err() && get_number_value(b).is_ok() {
        return std::cmp::Ordering::Greater;
    }

    // Otherwise, compare as strings
    let a_str = a.to_string();
    let b_str = b.to_string();
    a_str.cmp(&b_str)
}

/// Check if an array contains any of the specified values
///
/// Takes an array and a search array, returns true if the array contains
/// any of the values from the search array.
///
/// # Arguments
/// * `arr` - The array to search in
/// * `search_values` - Array of values to search for
///
/// # Returns
/// true if any search value is found in the array, false otherwise
///
/// # Examples
/// ```rhai
/// let tags = ["urgent", "bug", "frontend"];
/// let critical_tags = ["urgent", "critical", "blocker"];
/// let has_critical = contains_any(tags, critical_tags);  // true (contains "urgent")
///
/// let numbers = [1, 2, 3, 4, 5];
/// let target_numbers = [6, 7, 8];
/// let has_target = contains_any(numbers, target_numbers);  // false
///
/// // Mixed types work too
/// let mixed = [1, "hello", true, 3.14];
/// let search = ["hello", 99];
/// let found = contains_any(mixed, search);  // true (contains "hello")
/// ```
///
/// # Comparison Behavior
/// - Uses string comparison for all values (converts to string first)
/// - Numbers are compared as their string representation
/// - Booleans are compared as "true"/"false" strings
/// - Null values are compared as empty strings
fn contains_any_array(arr: Array, search_values: Array) -> bool {
    // Convert search values to strings for comparison
    let search_strings: Vec<String> = search_values.iter().map(|v| v.to_string()).collect();

    // Check if any element in arr matches any search value
    arr.iter().any(|item| {
        let item_str = item.to_string();
        search_strings.contains(&item_str)
    })
}

/// Check if an array starts with any of the specified values
///
/// Takes an array and a search array, returns true if the array starts
/// with any of the values from the search array. Only checks the first element.
///
/// # Arguments
/// * `arr` - The array to check
/// * `search_values` - Array of values to check against the first element
///
/// # Returns
/// true if the first element matches any search value, false otherwise
///
/// # Examples
/// ```rhai
/// let log_levels = ["ERROR", "Database connection failed"];
/// let error_levels = ["ERROR", "FATAL", "CRITICAL"];
/// let is_error = starts_with_any(log_levels, error_levels);  // true
///
/// let commands = ["GET", "/api/users", "200"];
/// let read_methods = ["GET", "HEAD", "OPTIONS"];
/// let is_read = starts_with_any(commands, read_methods);  // true
///
/// let empty_array = [];
/// let search = ["any", "value"];
/// let starts = starts_with_any(empty_array, search);  // false (empty array)
/// ```
///
/// # Edge Cases
/// - Returns false for empty arrays
/// - Returns false if search_values is empty
/// - Uses string comparison (same as contains_any)
fn starts_with_any_array(arr: Array, search_values: Array) -> bool {
    // Return false if either array is empty
    if arr.is_empty() || search_values.is_empty() {
        return false;
    }

    // Convert search values to strings for comparison
    let search_strings: Vec<String> = search_values.iter().map(|v| v.to_string()).collect();

    // Check if the first element matches any search value
    let first_element_str = arr[0].to_string();
    search_strings.contains(&first_element_str)
}

/// Find the minimum value in an array
///
/// Returns the smallest value in the array. All elements must be of compatible types.
/// Actual numbers (integers, floats) are compared numerically.
/// Strings are compared lexicographically. Mixed incompatible types return an error.
///
/// # Arguments
/// * `arr` - The array to find the minimum value in
///
/// # Returns
/// The minimum value, or `()` if the array is empty or contains incompatible types
///
/// # Examples
/// ```rhai
/// let numbers = [3, 1, 4, 1, 5];
/// let min_val = min(numbers);  // 1
///
/// let strings = ["banana", "apple", "cherry"];
/// let min_str = min(strings);  // "apple"
///
/// let floats = [3.14, 2.71, 1.41];
/// let min_float = min(floats);  // 1.41
///
/// let numeric_strings = ["10", "2", "100"];
/// let min_num_str = min(numeric_strings);  // "10" (lexicographic comparison)
///
/// let mixed = [1, "apple"];  // Returns () - incompatible types
/// let min_mixed = min(mixed);
///
/// let mixed_num_str = [1, "2"];  // Returns () - numbers and strings are incompatible
/// let min_mixed_2 = min(mixed_num_str);
///
/// let empty = [];
/// let min_empty = min(empty);  // ()
/// ```
///
/// # Type Compatibility
/// - All actual numbers (i64, f64) are compatible with each other
/// - All strings (including numeric strings) are compatible with each other
/// - Actual numbers and strings are incompatible (no automatic coercion)
/// - Booleans are treated as strings ("false", "true")
/// - Empty arrays return `()`
/// - Arrays with incompatible types return `()`
fn min_array(arr: Array) -> Dynamic {
    if arr.is_empty() {
        return Dynamic::UNIT;
    }

    // Determine the type category of the array
    let array_type = determine_array_type(&arr);

    match array_type {
        ArrayType::Empty => Dynamic::UNIT,
        ArrayType::Mixed => Dynamic::UNIT, // Reject mixed types
        ArrayType::Numeric => find_numeric_min(&arr),
        ArrayType::String => find_string_min(&arr),
    }
}

/// Find the maximum value in an array
///
/// Returns the largest value in the array. All elements must be of compatible types.
/// Actual numbers (integers, floats) are compared numerically.
/// Strings are compared lexicographically. Mixed incompatible types return an error.
///
/// # Arguments
/// * `arr` - The array to find the maximum value in
///
/// # Returns
/// The maximum value, or `()` if the array is empty or contains incompatible types
///
/// # Examples
/// ```rhai
/// let numbers = [3, 1, 4, 1, 5];
/// let max_val = max(numbers);  // 5
///
/// let strings = ["banana", "apple", "cherry"];
/// let max_str = max(strings);  // "cherry"
///
/// let floats = [3.14, 2.71, 1.41];
/// let max_float = max(floats);  // 3.14
///
/// let numeric_strings = ["10", "2", "100"];
/// let max_num_str = max(numeric_strings);  // "2" (lexicographic comparison)
///
/// let mixed = [1, "apple"];  // Returns () - incompatible types
/// let max_mixed = max(mixed);
///
/// let mixed_num_str = [1, "2"];  // Returns () - numbers and strings are incompatible
/// let max_mixed_2 = max(mixed_num_str);
///
/// let empty = [];
/// let max_empty = max(empty);  // ()
/// ```
///
/// # Type Compatibility
/// - All actual numbers (i64, f64) are compatible with each other
/// - All strings (including numeric strings) are compatible with each other
/// - Actual numbers and strings are incompatible (no automatic coercion)
/// - Booleans are treated as strings ("false", "true")
/// - Empty arrays return `()`
/// - Arrays with incompatible types return `()`
fn max_array(arr: Array) -> Dynamic {
    if arr.is_empty() {
        return Dynamic::UNIT;
    }

    // Determine the type category of the array
    let array_type = determine_array_type(&arr);

    match array_type {
        ArrayType::Empty => Dynamic::UNIT,
        ArrayType::Mixed => Dynamic::UNIT, // Reject mixed types
        ArrayType::Numeric => find_numeric_max(&arr),
        ArrayType::String => find_string_max(&arr),
    }
}

/// Enum to categorize array element types
#[derive(Debug, PartialEq)]
pub(crate) enum ArrayType {
    Empty,
    Numeric, // All elements are actual numbers (i64, f64)
    String,  // All elements are non-numeric (strings, booleans, etc.)
    Mixed,   // Contains incompatible types (actual numbers + non-numeric)
}

/// Determine the type category of an array
pub(crate) fn determine_array_type(arr: &Array) -> ArrayType {
    if arr.is_empty() {
        return ArrayType::Empty;
    }

    let mut has_numeric = false;
    let mut has_string = false;

    for item in arr {
        if is_actual_number(item) {
            has_numeric = true;
        } else {
            has_string = true;
        }

        // If we have both types, it's mixed
        if has_numeric && has_string {
            return ArrayType::Mixed;
        }
    }

    if has_numeric {
        ArrayType::Numeric
    } else {
        ArrayType::String
    }
}

/// Check if a value is an actual numeric type (not a string that could be parsed as a number)
pub(crate) fn is_actual_number(value: &Dynamic) -> bool {
    value.is_int() || value.is_float()
}

/// Find minimum value in a numeric array
fn find_numeric_min(arr: &Array) -> Dynamic {
    let mut min_val = f64::INFINITY;
    let mut min_item = &arr[0];

    for item in arr {
        let num = if item.is_int() {
            item.as_int().unwrap_or(0) as f64
        } else if item.is_float() {
            item.as_float().unwrap_or(0.0)
        } else {
            continue; // Skip non-numeric items (shouldn't happen in a numeric array)
        };

        if num < min_val {
            min_val = num;
            min_item = item;
        }
    }

    min_item.clone()
}

/// Find maximum value in a numeric array
fn find_numeric_max(arr: &Array) -> Dynamic {
    let mut max_val = f64::NEG_INFINITY;
    let mut max_item = &arr[0];

    for item in arr {
        let num = if item.is_int() {
            item.as_int().unwrap_or(0) as f64
        } else if item.is_float() {
            item.as_float().unwrap_or(0.0)
        } else {
            continue; // Skip non-numeric items (shouldn't happen in a numeric array)
        };

        if num > max_val {
            max_val = num;
            max_item = item;
        }
    }

    max_item.clone()
}

/// Find minimum value in a string array
fn find_string_min(arr: &Array) -> Dynamic {
    let mut min_str = arr[0].to_string();
    let mut min_item = &arr[0];

    for item in arr {
        let item_str = item.to_string();
        if item_str < min_str {
            min_str = item_str;
            min_item = item;
        }
    }

    min_item.clone()
}

/// Find maximum value in a string array
fn find_string_max(arr: &Array) -> Dynamic {
    let mut max_str = arr[0].to_string();
    let mut max_item = &arr[0];

    for item in arr {
        let item_str = item.to_string();
        if item_str > max_str {
            max_str = item_str;
            max_item = item;
        }
    }

    max_item.clone()
}

/// Convert IndexMap<String, Dynamic> to rhai::Map
fn convert_indexmap_to_rhai_map(indexmap: IndexMap<String, Dynamic>) -> Map {
    let mut map = Map::new();
    for (key, value) in indexmap {
        map.insert(key.into(), value);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Array, Dynamic, Map};

    fn make_row(fields: Vec<(&str, Dynamic)>) -> Dynamic {
        let mut map = Map::new();
        for (key, value) in fields {
            map.insert(key.into(), value);
        }
        Dynamic::from(map)
    }

    #[test]
    fn test_pluck_preserves_types() {
        let rows = vec![
            make_row(vec![
                ("status", Dynamic::from(200i64)),
                ("message", Dynamic::from("ok")),
            ]),
            make_row(vec![
                ("status", Dynamic::from(404i64)),
                ("message", Dynamic::from("missing")),
            ]),
            make_row(vec![("status", Dynamic::from(true))]),
        ];

        let result = pluck(rows, "status".to_string());

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].as_int().unwrap(), 200);
        assert_eq!(result[1].as_int().unwrap(), 404);
        assert!(result[2].as_bool().unwrap());
    }

    #[test]
    fn test_pluck_skips_missing_and_unit() {
        let rows = vec![
            make_row(vec![("status", Dynamic::from(200i64))]),
            make_row(vec![("status", Dynamic::from(()))]),
            make_row(vec![("other", Dynamic::from("skip"))]),
            Dynamic::from("not a map"),
        ];

        let result = pluck(rows, "status".to_string());

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].as_int().unwrap(), 200);
    }

    #[test]
    fn test_pluck_as_nums_converts_mixed_types() {
        let rows = vec![
            make_row(vec![("value", Dynamic::from(42i64))]),
            make_row(vec![("value", Dynamic::from(2.5))]),
            make_row(vec![("value", Dynamic::from("3.5"))]),
            make_row(vec![("value", Dynamic::from(true))]),
        ];

        let result = pluck_as_nums(rows, "value".to_string());

        assert_eq!(result.len(), 4);
        assert_eq!(result[0].as_float().unwrap(), 42.0);
        assert!((result[1].as_float().unwrap() - 2.5).abs() < f64::EPSILON);
        assert!((result[2].as_float().unwrap() - 3.5).abs() < f64::EPSILON);
        assert_eq!(result[3].as_float().unwrap(), 1.0);
    }

    #[test]
    fn test_pluck_as_nums_skips_invalid_values() {
        let rows = vec![
            make_row(vec![("value", Dynamic::from("100"))]),
            make_row(vec![("value", Dynamic::from("invalid"))]),
            make_row(vec![("value", Dynamic::from(()))]),
            Dynamic::from("not a map"),
        ];

        let result = pluck_as_nums(rows, "value".to_string());

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].as_float().unwrap(), 100.0);
    }

    #[test]
    fn test_sorted_numbers() {
        let arr = vec![
            Dynamic::from(3i64),
            Dynamic::from(1i64),
            Dynamic::from(4i64),
            Dynamic::from(1i64),
            Dynamic::from(5i64),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 5);
        assert_eq!(sorted[0].as_int().unwrap(), 1i64);
        assert_eq!(sorted[1].as_int().unwrap(), 1i64);
        assert_eq!(sorted[2].as_int().unwrap(), 3i64);
        assert_eq!(sorted[3].as_int().unwrap(), 4i64);
        assert_eq!(sorted[4].as_int().unwrap(), 5i64);
    }

    #[test]
    fn test_sorted_strings() {
        let arr = vec![
            Dynamic::from("banana"),
            Dynamic::from("apple"),
            Dynamic::from("cherry"),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].clone().into_string().unwrap(), "apple");
        assert_eq!(sorted[1].clone().into_string().unwrap(), "banana");
        assert_eq!(sorted[2].clone().into_string().unwrap(), "cherry");
    }

    #[test]
    fn test_sorted_mixed_types() {
        let arr = vec![
            Dynamic::from(3i64),
            Dynamic::from("banana"),
            Dynamic::from(1i64),
            Dynamic::from("apple"),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 4);
        // Numbers should come first
        assert_eq!(sorted[0].as_int().unwrap(), 1i64);
        assert_eq!(sorted[1].as_int().unwrap(), 3i64);
        // Then strings
        assert_eq!(sorted[2].to_string(), "apple");
        assert_eq!(sorted[3].to_string(), "banana");
    }

    #[test]
    fn test_sorted_with_floats() {
        let arr = vec![
            Dynamic::from(std::f64::consts::PI),
            Dynamic::from(1.5),
            Dynamic::from(std::f64::consts::E),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].as_float().unwrap(), 1.5);
        assert_eq!(sorted[1].as_float().unwrap(), std::f64::consts::E);
        assert_eq!(sorted[2].as_float().unwrap(), std::f64::consts::PI);
    }

    #[test]
    fn test_sorted_with_string_numbers() {
        let arr = vec![
            Dynamic::from("10"),
            Dynamic::from("2"),
            Dynamic::from("100"),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 3);
        // Should be sorted numerically, not lexicographically
        assert_eq!(sorted[0].to_string(), "2");
        assert_eq!(sorted[1].to_string(), "10");
        assert_eq!(sorted[2].to_string(), "100");
    }

    #[test]
    fn test_sorted_empty_array() {
        let arr = Array::new();
        let sorted = sorted_array(arr);
        assert_eq!(sorted.len(), 0);
    }

    #[test]
    fn test_sorted_single_element() {
        let arr = vec![Dynamic::from("single")];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].to_string(), "single");
    }

    #[test]
    fn test_sorted_with_booleans() {
        let arr = vec![
            Dynamic::from(true),
            Dynamic::from(false),
            Dynamic::from("apple"),
        ];

        let sorted = sorted_array(arr);

        assert_eq!(sorted.len(), 3);
        // Booleans are converted to strings and sorted lexicographically
        assert_eq!(sorted[0].to_string(), "apple");
        assert_eq!(sorted[1].to_string(), "false");
        assert_eq!(sorted[2].to_string(), "true");
    }

    #[test]
    fn test_reversed_basic() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
            Dynamic::from(5i64),
        ];

        let reversed = reversed_array(arr);

        assert_eq!(reversed.len(), 5);
        assert_eq!(reversed[0].as_int().unwrap(), 5i64);
        assert_eq!(reversed[1].as_int().unwrap(), 4i64);
        assert_eq!(reversed[2].as_int().unwrap(), 3i64);
        assert_eq!(reversed[3].as_int().unwrap(), 2i64);
        assert_eq!(reversed[4].as_int().unwrap(), 1i64);
    }

    #[test]
    fn test_reversed_strings() {
        let arr = vec![
            Dynamic::from("first"),
            Dynamic::from("second"),
            Dynamic::from("third"),
        ];

        let reversed = reversed_array(arr);

        assert_eq!(reversed.len(), 3);
        assert_eq!(reversed[0].to_string(), "third");
        assert_eq!(reversed[1].to_string(), "second");
        assert_eq!(reversed[2].to_string(), "first");
    }

    #[test]
    fn test_reversed_empty_array() {
        let arr = Array::new();
        let reversed = reversed_array(arr);
        assert_eq!(reversed.len(), 0);
    }

    #[test]
    fn test_unique_numbers() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(2i64),
            Dynamic::from(1i64),
        ];

        let unique = unique_array(arr);

        assert_eq!(unique.len(), 3);
        assert_eq!(unique[0].as_int().unwrap(), 1i64);
        assert_eq!(unique[1].as_int().unwrap(), 2i64);
        assert_eq!(unique[2].as_int().unwrap(), 3i64);
    }

    #[test]
    fn test_unique_strings() {
        let arr = vec![
            Dynamic::from("bug"),
            Dynamic::from("urgent"),
            Dynamic::from("bug"),
            Dynamic::from("frontend"),
            Dynamic::from("urgent"),
        ];

        let unique = unique_array(arr);

        assert_eq!(unique.len(), 3);
        assert_eq!(unique[0].to_string(), "bug");
        assert_eq!(unique[1].to_string(), "urgent");
        assert_eq!(unique[2].to_string(), "frontend");
    }

    #[test]
    fn test_unique_empty_array() {
        let arr = Array::new();
        let unique = unique_array(arr);
        assert_eq!(unique.len(), 0);
    }

    #[test]
    fn test_unique_no_duplicates() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
        ];

        let unique = unique_array(arr.clone());

        assert_eq!(unique.len(), 3);
        assert_eq!(unique[0].as_int().unwrap(), 1i64);
        assert_eq!(unique[1].as_int().unwrap(), 2i64);
        assert_eq!(unique[2].as_int().unwrap(), 3i64);
    }

    #[test]
    fn test_unique_all_duplicates() {
        let arr = vec![
            Dynamic::from(42i64),
            Dynamic::from(42i64),
            Dynamic::from(42i64),
        ];

        let unique = unique_array(arr);

        assert_eq!(unique.len(), 1);
        assert_eq!(unique[0].as_int().unwrap(), 42i64);
    }

    #[test]
    fn test_unique_mixed_types() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from("hello"),
            Dynamic::from(1i64),
            Dynamic::from("hello"),
            Dynamic::from(true),
        ];

        let unique = unique_array(arr);

        assert_eq!(unique.len(), 3);
        assert_eq!(unique[0].as_int().unwrap(), 1i64);
        assert_eq!(unique[1].to_string(), "hello");
        assert!(unique[2].as_bool().unwrap());
    }

    #[test]
    fn test_unique_preserves_order() {
        let arr = vec![
            Dynamic::from("z"),
            Dynamic::from("a"),
            Dynamic::from("m"),
            Dynamic::from("a"),
            Dynamic::from("z"),
        ];

        let unique = unique_array(arr);

        // Should preserve first occurrence order: z, a, m
        assert_eq!(unique.len(), 3);
        assert_eq!(unique[0].to_string(), "z");
        assert_eq!(unique[1].to_string(), "a");
        assert_eq!(unique[2].to_string(), "m");
    }

    #[test]
    fn test_sorted_by_numeric_field() {
        let mut arr = Array::new();

        // Create objects with age field
        let mut obj1 = rhai::Map::new();
        obj1.insert("name".into(), Dynamic::from("alice"));
        obj1.insert("age".into(), Dynamic::from(30i64));
        arr.push(Dynamic::from(obj1));

        let mut obj2 = rhai::Map::new();
        obj2.insert("name".into(), Dynamic::from("bob"));
        obj2.insert("age".into(), Dynamic::from(25i64));
        arr.push(Dynamic::from(obj2));

        let mut obj3 = rhai::Map::new();
        obj3.insert("name".into(), Dynamic::from("charlie"));
        obj3.insert("age".into(), Dynamic::from(35i64));
        arr.push(Dynamic::from(obj3));

        let sorted = sorted_by_field(arr, "age".to_string());

        assert_eq!(sorted.len(), 3);

        // Should be sorted by age: bob(25), alice(30), charlie(35)
        if let Some(obj) = sorted[0].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "bob");
            assert_eq!(obj.get("age").unwrap().as_int().unwrap(), 25i64);
        }
        if let Some(obj) = sorted[1].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "alice");
            assert_eq!(obj.get("age").unwrap().as_int().unwrap(), 30i64);
        }
        if let Some(obj) = sorted[2].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "charlie");
            assert_eq!(obj.get("age").unwrap().as_int().unwrap(), 35i64);
        }
    }

    #[test]
    fn test_sorted_by_string_field() {
        let mut arr = Array::new();

        // Create objects with name field
        let mut obj1 = rhai::Map::new();
        obj1.insert("name".into(), Dynamic::from("charlie"));
        obj1.insert("score".into(), Dynamic::from(78i64));
        arr.push(Dynamic::from(obj1));

        let mut obj2 = rhai::Map::new();
        obj2.insert("name".into(), Dynamic::from("alice"));
        obj2.insert("score".into(), Dynamic::from(85i64));
        arr.push(Dynamic::from(obj2));

        let mut obj3 = rhai::Map::new();
        obj3.insert("name".into(), Dynamic::from("bob"));
        obj3.insert("score".into(), Dynamic::from(92i64));
        arr.push(Dynamic::from(obj3));

        let sorted = sorted_by_field(arr, "name".to_string());

        assert_eq!(sorted.len(), 3);

        // Should be sorted by name: alice, bob, charlie
        if let Some(obj) = sorted[0].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "alice");
        }
        if let Some(obj) = sorted[1].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "bob");
        }
        if let Some(obj) = sorted[2].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "charlie");
        }
    }

    #[test]
    fn test_sorted_by_missing_field() {
        let mut arr = Array::new();

        // Create objects, some with the field, some without
        let mut obj1 = rhai::Map::new();
        obj1.insert("name".into(), Dynamic::from("alice"));
        obj1.insert("age".into(), Dynamic::from(30i64));
        arr.push(Dynamic::from(obj1));

        let mut obj2 = rhai::Map::new();
        obj2.insert("name".into(), Dynamic::from("bob"));
        // No age field
        arr.push(Dynamic::from(obj2));

        let mut obj3 = rhai::Map::new();
        obj3.insert("name".into(), Dynamic::from("charlie"));
        obj3.insert("age".into(), Dynamic::from(25i64));
        arr.push(Dynamic::from(obj3));

        let sorted = sorted_by_field(arr, "age".to_string());

        assert_eq!(sorted.len(), 3);

        // Objects without the field should come first, then sorted by field value
        if let Some(obj) = sorted[0].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "bob"); // no age field
        }
        if let Some(obj) = sorted[1].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "charlie"); // age 25
        }
        if let Some(obj) = sorted[2].clone().try_cast::<rhai::Map>() {
            assert_eq!(obj.get("name").unwrap().to_string(), "alice"); // age 30
        }
    }

    #[test]
    fn test_array_flatten_simple() {
        let arr = vec![
            Dynamic::from("item1"),
            Dynamic::from("item2"),
            Dynamic::from(42i64),
        ];

        let flattened = {
            let dynamic_array = Dynamic::from(arr);
            let result = flatten_dynamic(&dynamic_array, FlattenStyle::Bracket, 10);
            convert_indexmap_to_rhai_map(result)
        };

        assert_eq!(flattened.get("[0]").unwrap().to_string(), "item1");
        assert_eq!(flattened.get("[1]").unwrap().to_string(), "item2");
        assert_eq!(flattened.get("[2]").unwrap().to_string(), "42");
    }

    #[test]
    fn test_array_flatten_nested() {
        let mut inner1 = Map::new();
        inner1.insert("id".into(), Dynamic::from(1i64));
        inner1.insert("name".into(), Dynamic::from("first"));

        let mut inner2 = Map::new();
        inner2.insert("id".into(), Dynamic::from(2i64));
        inner2.insert("name".into(), Dynamic::from("second"));

        let arr = vec![Dynamic::from(inner1), Dynamic::from(inner2)];

        let flattened = {
            let dynamic_array = Dynamic::from(arr);
            let result = flatten_dynamic(&dynamic_array, FlattenStyle::Bracket, 10);
            convert_indexmap_to_rhai_map(result)
        };

        assert_eq!(flattened.get("[0].id").unwrap().to_string(), "1");
        assert_eq!(flattened.get("[0].name").unwrap().to_string(), "first");
        assert_eq!(flattened.get("[1].id").unwrap().to_string(), "2");
        assert_eq!(flattened.get("[1].name").unwrap().to_string(), "second");
    }

    #[test]
    fn test_array_flatten_styles() {
        let mut inner = Map::new();
        inner.insert("value".into(), Dynamic::from(42i64));

        let arr = vec![Dynamic::from(inner)];

        let dynamic_array = Dynamic::from(arr);

        // Test bracket style
        let bracket = flatten_dynamic(&dynamic_array, FlattenStyle::Bracket, 10);
        assert!(bracket.contains_key("[0].value"));

        // Test dot style
        let dot = flatten_dynamic(&dynamic_array, FlattenStyle::Dot, 10);
        assert!(dot.contains_key("0.value"));

        // Test underscore style
        let underscore = flatten_dynamic(&dynamic_array, FlattenStyle::Underscore, 10);
        assert!(underscore.contains_key("0_value"));
    }

    #[test]
    fn test_contains_any_basic() {
        let arr = vec![
            Dynamic::from("urgent"),
            Dynamic::from("bug"),
            Dynamic::from("frontend"),
        ];

        let search = vec![Dynamic::from("urgent"), Dynamic::from("critical")];

        assert!(contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_no_match() {
        let arr = vec![Dynamic::from("info"), Dynamic::from("debug")];

        let search = vec![Dynamic::from("error"), Dynamic::from("warning")];

        assert!(!contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_numbers() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
        ];

        let search = vec![Dynamic::from(2i64), Dynamic::from(5i64)];

        assert!(contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_mixed_types() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from("hello"),
            Dynamic::from(true),
        ];

        let search = vec![Dynamic::from("hello"), Dynamic::from(99i64)];

        assert!(contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_string_number_conversion() {
        let arr = vec![Dynamic::from(42i64)];

        let search = vec![Dynamic::from("42")];

        assert!(contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_boolean_conversion() {
        let arr = vec![Dynamic::from(true), Dynamic::from(false)];

        let search = vec![Dynamic::from("true")];

        assert!(contains_any_array(arr, search));
    }

    #[test]
    fn test_contains_any_empty_arrays() {
        let arr = Array::new();
        let search = Array::new();
        assert!(!contains_any_array(arr, search));

        let arr = vec![Dynamic::from("test")];
        let search = Array::new();
        assert!(!contains_any_array(arr, search));

        let arr = Array::new();
        let search = vec![Dynamic::from("test")];
        assert!(!contains_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_basic() {
        let arr = vec![
            Dynamic::from("ERROR"),
            Dynamic::from("Database connection failed"),
        ];

        let search = vec![Dynamic::from("ERROR"), Dynamic::from("FATAL")];

        assert!(starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_no_match() {
        let arr = vec![Dynamic::from("INFO"), Dynamic::from("System started")];

        let search = vec![Dynamic::from("ERROR"), Dynamic::from("WARNING")];

        assert!(!starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_numbers() {
        let arr = vec![Dynamic::from(200i64), Dynamic::from("OK")];

        let search = vec![Dynamic::from(200i64), Dynamic::from(404i64)];

        assert!(starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_string_number_conversion() {
        let arr = vec![Dynamic::from(500i64)];

        let search = vec![Dynamic::from("500")];

        assert!(starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_empty_array() {
        let arr = Array::new();
        let search = vec![Dynamic::from("test")];

        assert!(!starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_empty_search() {
        let arr = vec![Dynamic::from("test")];
        let search = Array::new();

        assert!(!starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_only_first_element() {
        let arr = vec![
            Dynamic::from("INFO"),
            Dynamic::from("ERROR"), // This should be ignored
        ];

        let search = vec![Dynamic::from("ERROR")];

        // Should return false because only first element is checked
        assert!(!starts_with_any_array(arr, search));
    }

    #[test]
    fn test_starts_with_any_mixed_types() {
        let arr = vec![Dynamic::from(true), Dynamic::from("second")];

        let search = vec![Dynamic::from("true"), Dynamic::from("false")];

        assert!(starts_with_any_array(arr, search));
    }

    #[test]
    fn test_min_array_numbers() {
        let arr = vec![
            Dynamic::from(3i64),
            Dynamic::from(1i64),
            Dynamic::from(4i64),
            Dynamic::from(1i64),
            Dynamic::from(5i64),
        ];

        let result = min_array(arr);
        assert_eq!(result.as_int().unwrap(), 1i64);
    }

    #[test]
    fn test_max_array_numbers() {
        let arr = vec![
            Dynamic::from(3i64),
            Dynamic::from(1i64),
            Dynamic::from(4i64),
            Dynamic::from(1i64),
            Dynamic::from(5i64),
        ];

        let result = max_array(arr);
        assert_eq!(result.as_int().unwrap(), 5i64);
    }

    #[test]
    fn test_min_array_floats() {
        let arr = vec![
            Dynamic::from(std::f64::consts::PI),
            Dynamic::from(2.71),
            Dynamic::from(1.41),
        ];

        let result = min_array(arr);
        assert_eq!(result.as_float().unwrap(), 1.41);
    }

    #[test]
    fn test_max_array_floats() {
        let arr = vec![
            Dynamic::from(std::f64::consts::PI),
            Dynamic::from(2.71),
            Dynamic::from(1.41),
        ];

        let result = max_array(arr);
        assert!((result.as_float().unwrap() - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_min_array_strings() {
        let arr = vec![
            Dynamic::from("banana"),
            Dynamic::from("apple"),
            Dynamic::from("cherry"),
        ];

        let result = min_array(arr);
        assert_eq!(result.to_string(), "apple");
    }

    #[test]
    fn test_max_array_strings() {
        let arr = vec![
            Dynamic::from("banana"),
            Dynamic::from("apple"),
            Dynamic::from("cherry"),
        ];

        let result = max_array(arr);
        assert_eq!(result.to_string(), "cherry");
    }

    #[test]
    fn test_min_array_numeric_strings() {
        let arr = vec![
            Dynamic::from("10"),
            Dynamic::from("2"),
            Dynamic::from("100"),
        ];

        let result = min_array(arr);
        // Should use lexicographic comparison, not numeric
        assert_eq!(result.to_string(), "10");
    }

    #[test]
    fn test_max_array_numeric_strings() {
        let arr = vec![
            Dynamic::from("10"),
            Dynamic::from("2"),
            Dynamic::from("100"),
        ];

        let result = max_array(arr);
        // Should use lexicographic comparison, not numeric
        assert_eq!(result.to_string(), "2");
    }

    #[test]
    fn test_min_array_booleans() {
        let arr = vec![Dynamic::from(true), Dynamic::from(false)];

        let result = min_array(arr);
        assert_eq!(result.to_string(), "false");
    }

    #[test]
    fn test_max_array_booleans() {
        let arr = vec![Dynamic::from(true), Dynamic::from(false)];

        let result = max_array(arr);
        assert_eq!(result.to_string(), "true");
    }

    #[test]
    fn test_min_array_mixed_types() {
        let arr = vec![Dynamic::from(1i64), Dynamic::from("apple")];

        let result = min_array(arr);
        assert!(result.is_unit()); // Should return () for mixed types
    }

    #[test]
    fn test_max_array_mixed_types() {
        let arr = vec![Dynamic::from(1i64), Dynamic::from("apple")];

        let result = max_array(arr);
        assert!(result.is_unit()); // Should return () for mixed types
    }

    #[test]
    fn test_min_array_empty() {
        let arr = Array::new();

        let result = min_array(arr);
        assert!(result.is_unit()); // Should return () for empty array
    }

    #[test]
    fn test_max_array_empty() {
        let arr = Array::new();

        let result = max_array(arr);
        assert!(result.is_unit()); // Should return () for empty array
    }

    #[test]
    fn test_min_array_single_element() {
        let arr = vec![Dynamic::from(42i64)];

        let result = min_array(arr);
        assert_eq!(result.as_int().unwrap(), 42i64);
    }

    #[test]
    fn test_max_array_single_element() {
        let arr = vec![Dynamic::from(42i64)];

        let result = max_array(arr);
        assert_eq!(result.as_int().unwrap(), 42i64);
    }

    #[test]
    fn test_determine_array_type() {
        // Numeric array (actual numbers only)
        let numeric_arr = vec![Dynamic::from(1i64), Dynamic::from(2.5)];
        assert_eq!(determine_array_type(&numeric_arr), ArrayType::Numeric);

        // String array
        let string_arr = vec![Dynamic::from("hello"), Dynamic::from("world")];
        assert_eq!(determine_array_type(&string_arr), ArrayType::String);

        // Mixed array (number + string)
        let mixed_arr = vec![Dynamic::from(1i64), Dynamic::from("hello")];
        assert_eq!(determine_array_type(&mixed_arr), ArrayType::Mixed);

        // Empty array
        let empty_arr = Array::new();
        assert_eq!(determine_array_type(&empty_arr), ArrayType::Empty);

        // Numeric strings (treated as strings now, not numbers)
        let numeric_str_arr = vec![Dynamic::from("10"), Dynamic::from("20")];
        assert_eq!(determine_array_type(&numeric_str_arr), ArrayType::String);

        // Boolean array (treated as strings)
        let bool_arr = vec![Dynamic::from(true), Dynamic::from(false)];
        assert_eq!(determine_array_type(&bool_arr), ArrayType::String);

        // Mixed number + numeric string (should be Mixed now)
        let mixed_num_str = vec![Dynamic::from(1i64), Dynamic::from("10")];
        assert_eq!(determine_array_type(&mixed_num_str), ArrayType::Mixed);
    }

    #[test]
    fn test_min_max_with_mixed_numeric_types() {
        // Mix of integers and floats only (no strings)
        let arr = vec![Dynamic::from(3i64), Dynamic::from(1.5), Dynamic::from(4i64)];

        let min_result = min_array(arr.clone());
        let max_result = max_array(arr);

        // Min should be 1.5
        assert_eq!(min_result.as_float().unwrap(), 1.5);
        // Max should be 4
        assert_eq!(max_result.as_int().unwrap(), 4i64);
    }

    #[test]
    fn test_min_max_mixed_numbers_and_strings_rejected() {
        // Mix of actual numbers and numeric strings should be rejected
        let arr = vec![Dynamic::from(3i64), Dynamic::from("2")];

        let min_result = min_array(arr.clone());
        let max_result = max_array(arr);

        // Should return () for mixed types
        assert!(min_result.is_unit());
        assert!(max_result.is_unit());
    }

    fn array_to_ints(arr: Array) -> Vec<i64> {
        arr.into_iter().map(|d| d.as_int().unwrap()).collect()
    }

    #[test]
    fn test_slice_array_basic_range() {
        let arr = vec![
            Dynamic::from(0i64),
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
        ];

        let result = slice_array(arr, "1:4");
        assert_eq!(array_to_ints(result), vec![1, 2, 3]);
    }

    #[test]
    fn test_slice_array_negative_indices_and_step() {
        let arr = vec![
            Dynamic::from(0i64),
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
        ];

        let result = slice_array(arr.clone(), "-4:-1");
        assert_eq!(array_to_ints(result), vec![1, 2, 3]);

        let reversed = slice_array(arr, "3:0:-1");
        assert_eq!(array_to_ints(reversed), vec![3, 2, 1]);
    }

    #[test]
    fn test_slice_array_defaults_and_zero_step() {
        let arr = vec![
            Dynamic::from(10i64),
            Dynamic::from(20i64),
            Dynamic::from(30i64),
        ];

        let no_spec = slice_array(arr.clone(), "");
        assert_eq!(array_to_ints(no_spec), vec![10, 20, 30]);

        let zero_step = slice_array(arr, "0:2:0");
        assert!(zero_step.is_empty());
    }
}
