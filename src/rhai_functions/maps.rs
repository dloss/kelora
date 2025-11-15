use crate::event::{flatten_dynamic, FlattenStyle};
use indexmap::IndexMap;
use rhai::{Array, Dynamic, Engine, Map};

/// Registers map merge/enrich operators to the Rhai engine.
pub fn register_functions(engine: &mut Engine) {
    // event.merge(map): overwrites existing keys
    engine.register_fn("merge", |lhs: &mut Map, rhs: Map| {
        for (k, v) in rhs {
            lhs.insert(k, v);
        }
    });

    // event.enrich(map): inserts only if key is missing
    engine.register_fn("enrich", |lhs: &mut Map, rhs: Map| {
        for (k, v) in rhs {
            lhs.entry(k).or_insert(v);
        }
    });

    // event += map: shorthand for merge()
    engine.register_fn("+=", |lhs: &mut Map, rhs: Map| {
        for (k, v) in rhs {
            lhs.insert(k, v);
        }
    });

    // Flattening functions

    // Default flattened() - uses bracket style, unlimited depth
    engine.register_fn("flattened", |map: Map| -> Map {
        let dynamic_map = Dynamic::from(map);
        let flattened = flatten_dynamic(&dynamic_map, FlattenStyle::default(), 0);
        convert_indexmap_to_rhai_map(flattened)
    });

    // flattened(style) - specify style, unlimited depth
    engine.register_fn("flattened", |map: Map, style: &str| -> Map {
        let flatten_style = match style {
            "dot" => FlattenStyle::Dot,
            "bracket" => FlattenStyle::Bracket,
            "underscore" => FlattenStyle::Underscore,
            _ => FlattenStyle::default(), // Default to bracket for unknown styles
        };
        let dynamic_map = Dynamic::from(map);
        let flattened = flatten_dynamic(&dynamic_map, flatten_style, 0);
        convert_indexmap_to_rhai_map(flattened)
    });

    // flattened(style, max_depth) - full control
    engine.register_fn(
        "flattened",
        |map: Map, style: &str, max_depth: i64| -> Map {
            let flatten_style = match style {
                "dot" => FlattenStyle::Dot,
                "bracket" => FlattenStyle::Bracket,
                "underscore" => FlattenStyle::Underscore,
                _ => FlattenStyle::default(),
            };
            let max_depth = if max_depth < 0 { 0 } else { max_depth as usize }; // negative = unlimited
            let dynamic_map = Dynamic::from(map);
            let flattened = flatten_dynamic(&dynamic_map, flatten_style, max_depth);
            convert_indexmap_to_rhai_map(flattened)
        },
    );

    // flatten_field(field_name) - flatten just one field from the map
    engine.register_fn("flatten_field", |map: &Map, field_name: &str| -> Map {
        let mut result = Map::new();

        if let Some(field_value) = map.get(field_name) {
            let flattened = flatten_dynamic(field_value, FlattenStyle::default(), 0);
            for (key, value) in flattened {
                let full_key = if key == "value" {
                    field_name.to_string()
                } else {
                    format!("{}.{}", field_name, key)
                };
                result.insert(full_key.into(), value);
            }
        }

        result
    });

    // Unflattening functions - reconstruct nested structures from flat keys

    // Default unflatten() - uses underscore separator with smart heuristics
    engine.register_fn("unflatten", |map: Map| -> Map { unflatten_map(map, "_") });

    // unflatten(separator) - specify separator with smart heuristics
    engine.register_fn("unflatten", |map: Map, separator: &str| -> Map {
        unflatten_map(map, separator)
    });

    // map.has(key) - check if map contains key AND value is not unit ()
    engine.register_fn("has", |map: Map, key: rhai::ImmutableString| -> bool {
        map.get(key.as_str()).is_some_and(|value| !value.is_unit())
    });

    // map.rename_field(old_name, new_name) - rename a field, returns true if successful
    engine.register_fn("rename_field", rename_field);
}

/// Rename a field in the map
/// Returns true if old_name existed and was renamed, false otherwise
/// If new_name already exists, it will be overwritten
pub fn rename_field(map: &mut Map, old_name: &str, new_name: &str) -> bool {
    if let Some(value) = map.remove(old_name) {
        map.insert(new_name.into(), value);
        true
    } else {
        false
    }
}

/// Unflatten a map by reconstructing nested structures from flat keys
/// Uses smart heuristics to determine when to create arrays vs objects
fn unflatten_map(flat_map: Map, separator: &str) -> Map {
    let mut result = Map::new();

    // First pass: analyze all keys to determine container types
    let mut key_analysis = std::collections::HashMap::new();
    for flat_key in flat_map.keys() {
        let parts: Vec<&str> = flat_key.split(separator).collect();
        analyze_key_path(&parts, &mut key_analysis, separator);
    }

    // Second pass: build the nested structure
    for (flat_key, value) in flat_map {
        let parts: Vec<&str> = flat_key.split(separator).collect();
        if !parts.is_empty() {
            set_nested_value(&mut result, &parts, value, &key_analysis, separator);
        }
    }

    result
}

/// Analyze a key path to determine what type of containers should be created
fn analyze_key_path(
    parts: &[&str],
    analysis: &mut std::collections::HashMap<String, ContainerType>,
    separator: &str,
) {
    let mut current_path = String::new();

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            current_path.push_str(separator);
        }
        current_path.push_str(part);

        // Look at the next part to determine what container type this should be
        if i + 1 < parts.len() {
            let next_part = parts[i + 1];
            let container_type = if is_array_index(next_part) {
                ContainerType::Array
            } else {
                ContainerType::Object
            };

            // If we've seen this path before, check for conflicts
            match analysis.get(&current_path) {
                Some(existing_type) => {
                    if *existing_type != container_type {
                        // Conflict: array index and non-array key for same parent
                        // Default to object in case of conflict
                        analysis.insert(current_path.clone(), ContainerType::Object);
                    }
                }
                None => {
                    analysis.insert(current_path.clone(), container_type);
                }
            }
        }
    }
}

/// Check if a string represents an array index (pure number)
fn is_array_index(s: &str) -> bool {
    s.parse::<usize>().is_ok()
}

/// Container type for reconstruction
#[derive(Debug, Clone, Copy, PartialEq)]
enum ContainerType {
    Array,
    Object,
}

/// Set a nested value in the result structure
fn set_nested_value(
    container: &mut Map,
    parts: &[&str],
    value: Dynamic,
    analysis: &std::collections::HashMap<String, ContainerType>,
    separator: &str,
) {
    set_nested_value_with_path(container, parts, value, analysis, separator, &[]);
}

/// Set a nested value in the result structure with full path context
fn set_nested_value_with_path(
    container: &mut Map,
    parts: &[&str],
    value: Dynamic,
    analysis: &std::collections::HashMap<String, ContainerType>,
    separator: &str,
    parent_path: &[&str],
) {
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        // Leaf value
        container.insert(parts[0].into(), value);
        return;
    }

    let current_key = parts[0];
    let remaining_parts = &parts[1..];

    // Determine what kind of container we need to create/access
    // Build the full path to the current container
    let mut full_path = parent_path.to_vec();
    full_path.push(current_key);
    let lookup_key = full_path.join(separator);

    let container_type = analysis
        .get(&lookup_key)
        .copied()
        .unwrap_or(ContainerType::Object);

    match container_type {
        ContainerType::Object => {
            // Ensure we have a Map for this key
            let nested_map = container
                .entry(current_key.into())
                .or_insert_with(|| Dynamic::from(Map::new()));

            if let Some(mut map) = nested_map.clone().try_cast::<Map>() {
                let mut new_path = parent_path.to_vec();
                new_path.push(current_key);
                set_nested_value_with_path(
                    &mut map,
                    remaining_parts,
                    value,
                    analysis,
                    separator,
                    &new_path,
                );
                *nested_map = Dynamic::from(map);
            }
        }
        ContainerType::Array => {
            // Ensure we have an Array for this key
            let nested_array = container
                .entry(current_key.into())
                .or_insert_with(|| Dynamic::from(Array::new()));

            if let Some(mut array) = nested_array.clone().try_cast::<Array>() {
                let mut new_path = parent_path.to_vec();
                new_path.push(current_key);
                set_array_value_with_path(
                    &mut array,
                    remaining_parts,
                    value,
                    analysis,
                    separator,
                    &new_path,
                );
                *nested_array = Dynamic::from(array);
            }
        }
    }
}

/// Set a value in an array structure with full path context
fn set_array_value_with_path(
    array: &mut Array,
    parts: &[&str],
    value: Dynamic,
    analysis: &std::collections::HashMap<String, ContainerType>,
    separator: &str,
    parent_path: &[&str],
) {
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        // Leaf value - parts[0] should be an index
        if let Ok(index) = parts[0].parse::<usize>() {
            // Extend array if necessary
            while array.len() <= index {
                array.push(Dynamic::UNIT);
            }
            array[index] = value;
        }
        return;
    }

    let current_index_str = parts[0];
    let remaining_parts = &parts[1..];

    if let Ok(index) = current_index_str.parse::<usize>() {
        // Extend array if necessary
        while array.len() <= index {
            array.push(Dynamic::UNIT);
        }

        // Determine what kind of container the next level needs
        let mut full_path = parent_path.to_vec();
        full_path.push(current_index_str);
        let lookup_key = full_path.join(separator);
        let container_type = analysis
            .get(&lookup_key)
            .copied()
            .unwrap_or(ContainerType::Object);

        match container_type {
            ContainerType::Object => {
                // Ensure we have a Map at this index
                if array[index].is_unit() {
                    array[index] = Dynamic::from(Map::new());
                }

                if let Some(mut map) = array[index].clone().try_cast::<Map>() {
                    let mut new_path = parent_path.to_vec();
                    new_path.push(current_index_str);
                    set_nested_value_with_path(
                        &mut map,
                        remaining_parts,
                        value,
                        analysis,
                        separator,
                        &new_path,
                    );
                    array[index] = Dynamic::from(map);
                }
            }
            ContainerType::Array => {
                // Ensure we have an Array at this index
                if array[index].is_unit() {
                    array[index] = Dynamic::from(Array::new());
                }

                if let Some(mut nested_array) = array[index].clone().try_cast::<Array>() {
                    let mut new_path = parent_path.to_vec();
                    new_path.push(current_index_str);
                    set_array_value_with_path(
                        &mut nested_array,
                        remaining_parts,
                        value,
                        analysis,
                        separator,
                        &new_path,
                    );
                    array[index] = Dynamic::from(nested_array);
                }
            }
        }
    }
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
    use rhai::Map;

    #[test]
    fn test_json_to_dynamic_conversion() {
        let json_val = serde_json::json!({
            "string": "hello",
            "number": 42,
            "boolean": true,
            "null": null,
            "array": [1, 2, 3],
            "object": {"nested": "value"}
        });

        let dynamic = crate::event::json_to_dynamic(&json_val);

        // Test that JSON arrays become proper Rhai arrays
        if let Some(map) = dynamic.clone().try_cast::<Map>() {
            assert_eq!(map.get("string").unwrap().clone().cast::<String>(), "hello");
            assert_eq!(map.get("number").unwrap().clone().cast::<i64>(), 42);
            assert!(map.get("boolean").unwrap().clone().cast::<bool>());
            assert!(map.get("null").unwrap().is_unit());

            // Test array conversion
            if let Some(array) = map.get("array").unwrap().clone().try_cast::<rhai::Array>() {
                assert_eq!(array.len(), 3);
                assert_eq!(array[0].clone().cast::<i64>(), 1);
            } else {
                panic!("Array field is not a proper Rhai array");
            }

            // Test nested object conversion
            if let Some(nested_map) = map.get("object").unwrap().clone().try_cast::<Map>() {
                assert_eq!(
                    nested_map.get("nested").unwrap().clone().cast::<String>(),
                    "value"
                );
            } else {
                panic!("Object field is not a proper Rhai map");
            }
        } else {
            panic!("Root object is not a proper Rhai map");
        }
    }

    #[test]
    fn test_rename_field_success() {
        use super::*;
        use rhai::Dynamic;

        let mut map = Map::new();
        map.insert("old_name".into(), Dynamic::from("value"));
        map.insert("other".into(), Dynamic::from(42i64));

        let result = rename_field(&mut map, "old_name", "new_name");

        assert!(result);
        assert!(!map.contains_key("old_name"));
        assert_eq!(
            map.get("new_name").unwrap().clone().cast::<String>(),
            "value"
        );
        assert_eq!(map.get("other").unwrap().as_int().unwrap(), 42i64);
    }

    #[test]
    fn test_rename_field_missing_source() {
        use super::*;
        use rhai::Dynamic;

        let mut map = Map::new();
        map.insert("existing".into(), Dynamic::from("value"));

        let result = rename_field(&mut map, "nonexistent", "new_name");

        assert!(!result);
        assert!(map.contains_key("existing"));
        assert!(!map.contains_key("new_name"));
    }

    #[test]
    fn test_rename_field_overwrite_target() {
        use super::*;
        use rhai::Dynamic;

        let mut map = Map::new();
        map.insert("old_name".into(), Dynamic::from("new_value"));
        map.insert("new_name".into(), Dynamic::from("old_value"));

        let result = rename_field(&mut map, "old_name", "new_name");

        assert!(result);
        assert!(!map.contains_key("old_name"));
        assert_eq!(
            map.get("new_name").unwrap().clone().cast::<String>(),
            "new_value"
        );
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_rename_field_same_name() {
        use super::*;
        use rhai::Dynamic;

        let mut map = Map::new();
        map.insert("field".into(), Dynamic::from("value"));

        let result = rename_field(&mut map, "field", "field");

        assert!(result);
        assert_eq!(map.get("field").unwrap().clone().cast::<String>(), "value");
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_rename_field_rhai_integration() {
        use super::*;
        use rhai::Engine;

        let mut engine = Engine::new();
        register_functions(&mut engine);

        let result = engine
            .eval::<bool>(
                r#"
            let e = #{timestamp: "2024-01-01", level: "info"};
            e.rename_field("timestamp", "ts")
        "#,
            )
            .unwrap();

        assert!(result);

        let map = engine
            .eval::<Map>(
                r#"
            let e = #{timestamp: "2024-01-01", level: "info"};
            e.rename_field("timestamp", "ts");
            e
        "#,
            )
            .unwrap();

        assert!(!map.contains_key("timestamp"));
        assert_eq!(
            map.get("ts").unwrap().clone().cast::<String>(),
            "2024-01-01"
        );
        assert_eq!(map.get("level").unwrap().clone().cast::<String>(), "info");
    }

    #[test]
    fn test_rename_field_chained() {
        use super::*;
        use rhai::Engine;

        let mut engine = Engine::new();
        register_functions(&mut engine);

        let map = engine
            .eval::<Map>(
                r#"
            let e = #{a: 1, b: 2, c: 3};
            e.rename_field("a", "x");
            e.rename_field("b", "y");
            e.rename_field("c", "z");
            e
        "#,
            )
            .unwrap();

        assert_eq!(map.get("x").unwrap().clone().cast::<i64>(), 1);
        assert_eq!(map.get("y").unwrap().clone().cast::<i64>(), 2);
        assert_eq!(map.get("z").unwrap().clone().cast::<i64>(), 3);
        assert!(!map.contains_key("a"));
        assert!(!map.contains_key("b"));
        assert!(!map.contains_key("c"));
    }
}
