#![allow(dead_code)]
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use rhai::Dynamic;
use serde::{Deserialize, Serialize};

/// Context type for lines around matches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextType {
    #[default]
    None, // Regular event, no context
    Match,  // Event that matched filters
    Before, // Before-context for a match
    After,  // After-context for a match
    Both,   // Overlapping context (before + after)
}

/// Flattening style for nested data structures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlattenStyle {
    /// Use dots for objects and brackets for arrays: "user.name", "items[0].value"
    #[default]
    Bracket,
    /// Use dots everywhere: "user.name", "items.0.value"
    Dot,
    /// Use underscores everywhere: "user_name", "items_0_value"
    Underscore,
}

impl FlattenStyle {
    /// Format an object field key
    fn format_object_key(&self, parent: &str, key: &str) -> String {
        match self {
            FlattenStyle::Bracket | FlattenStyle::Dot => {
                if parent.is_empty() {
                    key.to_string()
                } else {
                    format!("{}.{}", parent, key)
                }
            }
            FlattenStyle::Underscore => {
                if parent.is_empty() {
                    key.to_string()
                } else {
                    format!("{}_{}", parent, key)
                }
            }
        }
    }

    /// Format an array index key
    fn format_array_key(&self, parent: &str, index: usize) -> String {
        match self {
            FlattenStyle::Bracket => {
                if parent.is_empty() {
                    format!("[{}]", index)
                } else {
                    format!("{}[{}]", parent, index)
                }
            }
            FlattenStyle::Dot => {
                if parent.is_empty() {
                    index.to_string()
                } else {
                    format!("{}.{}", parent, index)
                }
            }
            FlattenStyle::Underscore => {
                if parent.is_empty() {
                    index.to_string()
                } else {
                    format!("{}_{}", parent, index)
                }
            }
        }
    }
}

/// Flatten a Dynamic value into a flat map of key-value pairs
///
/// This function recursively traverses nested maps and arrays, generating
/// flat keys according to the specified style. It respects max_depth to
/// prevent infinite recursion and memory issues.
///
/// # Arguments
/// * `value` - The Dynamic value to flatten
/// * `style` - The flattening style (Bracket, Dot, Underscore)
/// * `max_depth` - Maximum recursion depth (0 = unlimited)
pub fn flatten_dynamic(
    value: &Dynamic,
    style: FlattenStyle,
    max_depth: usize,
) -> IndexMap<String, Dynamic> {
    let mut result = IndexMap::new();
    // Convert max_depth=0 to unlimited
    let effective_max_depth = if max_depth == 0 {
        usize::MAX
    } else {
        max_depth
    };
    flatten_dynamic_recursive(value, "", style, 0, effective_max_depth, &mut result);
    result
}

/// Recursive helper for flatten_dynamic
fn flatten_dynamic_recursive(
    value: &Dynamic,
    prefix: &str,
    style: FlattenStyle,
    current_depth: usize,
    max_depth: usize,
    result: &mut IndexMap<String, Dynamic>,
) {
    // If we've reached max depth, store as-is
    if current_depth >= max_depth {
        let key = if prefix.is_empty() {
            "value".to_string()
        } else {
            prefix.to_string()
        };
        result.insert(key, value.clone());
        return;
    }

    if let Some(map) = value.clone().try_cast::<rhai::Map>() {
        // Handle Rhai Map (object)
        if map.is_empty() {
            // Empty objects become null values
            let key = if prefix.is_empty() {
                "value".to_string()
            } else {
                prefix.to_string()
            };
            result.insert(key, Dynamic::UNIT);
        } else {
            for (key, val) in map {
                let new_key = style.format_object_key(prefix, key.as_ref());
                flatten_dynamic_recursive(
                    &val,
                    &new_key,
                    style,
                    current_depth + 1,
                    max_depth,
                    result,
                );
            }
        }
    } else if let Some(array) = value.clone().try_cast::<rhai::Array>() {
        // Handle Rhai Array
        if array.is_empty() {
            // Empty arrays become null values
            let key = if prefix.is_empty() {
                "value".to_string()
            } else {
                prefix.to_string()
            };
            result.insert(key, Dynamic::UNIT);
        } else {
            for (index, val) in array.iter().enumerate() {
                let new_key = style.format_array_key(prefix, index);
                flatten_dynamic_recursive(
                    val,
                    &new_key,
                    style,
                    current_depth + 1,
                    max_depth,
                    result,
                );
            }
        }
    } else {
        // Scalar value - store it
        let key = if prefix.is_empty() {
            "value".to_string()
        } else {
            prefix.to_string()
        };
        result.insert(key, value.clone());
    }
}

/// Flatten an entire Event's fields
pub fn flatten_event_fields(
    event: &Event,
    style: FlattenStyle,
    max_depth: usize,
) -> IndexMap<String, Dynamic> {
    let mut result = IndexMap::new();

    for (key, value) in &event.fields {
        if value.clone().try_cast::<rhai::Map>().is_some() {
            // Flatten nested objects
            let flattened = flatten_dynamic(value, style, max_depth);
            for (flat_key, flat_value) in flattened {
                let full_key = style.format_object_key(key, &flat_key);
                result.insert(full_key, flat_value);
            }
        } else if value.clone().try_cast::<rhai::Array>().is_some() {
            // Flatten arrays
            let flattened = flatten_dynamic(value, style, max_depth);
            for (flat_key, flat_value) in flattened {
                let full_key = if flat_key == "value" {
                    key.clone()
                } else {
                    style.format_object_key(key, &flat_key)
                };
                result.insert(full_key, flat_value);
            }
        } else {
            // Scalar values - keep as-is
            result.insert(key.clone(), value.clone());
        }
    }

    result
}

/// Convert serde_json::Value to rhai::Dynamic recursively
/// This is the single source of truth for JSON to Rhai conversion
pub fn json_to_dynamic(value: &serde_json::Value) -> Dynamic {
    match value {
        serde_json::Value::String(s) => Dynamic::from(s.clone()),
        serde_json::Value::Number(n) => {
            // Try i64 first for signed integers
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            // Try u64 for large unsigned integers (prevents precision loss)
            } else if let Some(u) = n.as_u64() {
                Dynamic::from(u)
            // Fall back to f64 for floating-point numbers
            } else if let Some(f) = n.as_f64() {
                Dynamic::from(f)
            // Final fallback: preserve as string
            } else {
                Dynamic::from(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => Dynamic::from(*b),
        serde_json::Value::Null => Dynamic::UNIT,
        serde_json::Value::Array(arr) => {
            // Convert JSON array to Rhai array recursively
            let mut rhai_array = rhai::Array::new();
            for item in arr {
                rhai_array.push(json_to_dynamic(item));
            }
            Dynamic::from(rhai_array)
        }
        serde_json::Value::Object(obj) => {
            // Convert JSON object to Rhai map recursively
            let mut rhai_map = rhai::Map::new();
            for (key, val) in obj {
                rhai_map.insert(key.clone().into(), json_to_dynamic(val));
            }
            Dynamic::from(rhai_map)
        }
    }
}

/// Core field name constants to ensure consistency across the codebase
pub const TIMESTAMP_FIELD_NAMES: &[&str] = &[
    "ts",
    "_ts",
    "timestamp",
    "at",
    "time",
    "@timestamp",
    "log_timestamp",
    "event_time",
    "datetime",
    "date_time",
    "created_at",
    "logged_at",
    "_t",
    "@t",
    "t",
];

pub const LEVEL_FIELD_NAMES: &[&str] = &[
    "level",
    "lvl",
    "severity",
    "log_level",
    "loglevel",
    "priority",
    "sev",
    "@level",
    "log_severity",
    "error_level",
    "event_level",
    "_level",
    "@l",
];

pub const MESSAGE_FIELD_NAMES: &[&str] = &[
    "msg",
    "message",
    "content",
    "data",
    "log",
    "text",
    "description",
    "details",
    "body",
    "payload",
    "event_message",
    "log_message",
    "_message",
    "@message",
    "@m",
    "event", // CEF event name field (lowest priority)
];

/// Create an ordered iterator over event fields, prioritizing timestamps, log levels, and messages
///
/// If the event has been processed by key filtering (--keys/--exclude-keys),
/// the existing order is preserved. Otherwise, returns field pairs in this order:
/// 1. Timestamp fields (in order of TIMESTAMP_FIELD_NAMES)
/// 2. Log level fields (in order of LEVEL_FIELD_NAMES)
/// 3. Message fields (in order of MESSAGE_FIELD_NAMES)
/// 4. All other fields (sorted alphabetically)
pub fn ordered_fields(event: &Event) -> Vec<(&String, &rhai::Dynamic)> {
    // If the event has been processed by key filtering, preserve the existing order
    if event.key_filtered {
        return event.fields.iter().collect();
    }

    let mut ordered = Vec::with_capacity(event.fields.len());

    // 1. Add timestamp fields in priority order
    for &ts_name in TIMESTAMP_FIELD_NAMES {
        if let Some((key, value)) = event.fields.get_key_value(ts_name) {
            ordered.push((key, value));
        }
    }

    // 2. Add log level fields in priority order
    for &level_name in LEVEL_FIELD_NAMES {
        if let Some((key, value)) = event.fields.get_key_value(level_name) {
            ordered.push((key, value));
        }
    }

    // 3. Add message fields in priority order
    for &msg_name in MESSAGE_FIELD_NAMES {
        if let Some((key, value)) = event.fields.get_key_value(msg_name) {
            ordered.push((key, value));
        }
    }

    // 4. Add all remaining fields in IndexMap order (no additional sorting)
    let remaining: Vec<_> = event
        .fields
        .iter()
        .filter(|(key, _)| {
            !TIMESTAMP_FIELD_NAMES.contains(&key.as_str())
                && !LEVEL_FIELD_NAMES.contains(&key.as_str())
                && !MESSAGE_FIELD_NAMES.contains(&key.as_str())
        })
        .collect();

    ordered.extend(remaining);

    ordered
}

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub fields: IndexMap<String, Dynamic>,
    /// Flag indicating whether this event has been processed by key filtering (--keys/--exclude-keys)
    pub key_filtered: bool,
    pub original_line: String,
    pub line_num: Option<usize>,
    pub filename: Option<String>,
    pub span: SpanInfo,
    /// Parsed timestamp field for efficient timestamp operations
    /// This is populated automatically when timestamps are extracted from fields
    pub parsed_ts: Option<DateTime<Utc>>,
    /// Context type for this event (match, before, after, none)
    pub context_type: ContextType,
    /// Ensures timestamp parsing statistics are recorded at most once per event
    pub(crate) timestamp_stats_recorded: bool,
}

/// Span assignment status for events when --span is active
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    Included,
    Late,
    Unassigned,
    Filtered,
}

impl SpanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanStatus::Included => "included",
            SpanStatus::Late => "late",
            SpanStatus::Unassigned => "unassigned",
            SpanStatus::Filtered => "filtered",
        }
    }
}

/// Span metadata stored on each event for Rhai exposure
#[derive(Debug, Clone, Default)]
pub struct SpanInfo {
    pub status: Option<SpanStatus>,
    pub span_id: Option<String>,
    pub span_start: Option<DateTime<Utc>>,
    pub span_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

impl Event {
    pub fn with_capacity(original_line: String, capacity: usize) -> Self {
        Self {
            fields: IndexMap::with_capacity(capacity),
            key_filtered: false,
            original_line,
            line_num: None,
            filename: None,
            span: SpanInfo::default(),
            parsed_ts: None,
            context_type: ContextType::None,
            timestamp_stats_recorded: false,
        }
    }

    pub fn default_with_line(line: String) -> Self {
        Self {
            original_line: line,
            ..Default::default()
        }
    }

    pub fn set_field(&mut self, key: String, value: Dynamic) {
        self.fields.insert(key, value);
    }

    pub fn set_metadata(&mut self, line_num: usize, filename: Option<String>) {
        self.line_num = Some(line_num);
        self.filename = filename;
    }

    pub fn set_span_info(&mut self, info: SpanInfo) {
        self.span = info;
    }

    /// Filter to only show specified keys, keeping only fields that actually exist
    pub fn filter_keys(&mut self, keys: &[String]) {
        let mut new_fields = IndexMap::with_capacity(keys.len());

        // Only include fields that are both requested and exist
        for key in keys {
            if let Some(value) = self.fields.get(key) {
                new_fields.insert(key.clone(), value.clone());
            }
        }

        self.fields = new_fields;
    }

    /// Try to parse and extract timestamp from the fields map
    pub fn extract_timestamp(&mut self) {
        self.extract_timestamp_with_parser(None);
    }

    /// Try to parse and extract timestamp from the fields map with optional adaptive parser
    pub fn extract_timestamp_with_parser(
        &mut self,
        parser: Option<&mut crate::timestamp::AdaptiveTsParser>,
    ) {
        self.extract_timestamp_with_config(parser, &crate::timestamp::TsConfig::default());
    }

    /// Extract timestamp with configuration
    pub fn extract_timestamp_with_config(
        &mut self,
        parser: Option<&mut crate::timestamp::AdaptiveTsParser>,
        ts_config: &crate::timestamp::TsConfig,
    ) {
        // Extract and parse timestamp with comprehensive field recognition
        if self.parsed_ts.is_none() {
            if let Some((field_name, ts_str)) =
                crate::timestamp::identify_timestamp_field(&self.fields, ts_config)
            {
                let mut parsed = false;
                let parsed_ts = if let Some(parser) = parser {
                    parser.parse_ts_with_config(
                        &ts_str,
                        ts_config.custom_format.as_deref(),
                        ts_config.default_timezone.as_deref(),
                    )
                } else {
                    // Use the enhanced adaptive parser as default
                    let mut default_parser = crate::timestamp::AdaptiveTsParser::new();
                    default_parser.parse_ts_with_config(
                        &ts_str,
                        ts_config.custom_format.as_deref(),
                        ts_config.default_timezone.as_deref(),
                    )
                };

                if let Some(ts) = parsed_ts {
                    self.parsed_ts = Some(ts);
                    parsed = true;
                }

                if !self.timestamp_stats_recorded {
                    crate::stats::stats_record_timestamp_detection(&field_name, &ts_str, parsed);
                    self.timestamp_stats_recorded = true;
                }
            } else if !self.timestamp_stats_recorded {
                crate::stats::stats_record_timestamp_absent();
                self.timestamp_stats_recorded = true;
            }
        }
    }
}

impl std::fmt::Display for FieldValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldValue::String(s) => write!(f, "{}", s),
            FieldValue::Number(n) => write!(f, "{}", n),
            FieldValue::Boolean(b) => write!(f, "{}", b),
            FieldValue::Null => write!(f, "null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::strategy::{BoxedStrategy, Strategy};

    fn arb_dynamic(depth: u32) -> BoxedStrategy<Dynamic> {
        use proptest::collection::vec;

        let leaf = prop_oneof![
            any::<i64>().prop_map(Dynamic::from),
            prop::num::f64::NORMAL.prop_map(Dynamic::from),
            any::<bool>().prop_map(Dynamic::from),
            "[ -~]{0,16}".prop_map(Dynamic::from),
            Just(Dynamic::UNIT),
        ]
        .boxed();

        leaf.prop_recursive(depth, 64, 8, |inner| {
            let map = vec(("[a-z]{1,8}".prop_map(|s: String| s), inner.clone()), 0..4).prop_map(
                |entries| {
                    let mut map = rhai::Map::new();
                    for (k, v) in entries {
                        map.insert(k.into(), v);
                    }
                    Dynamic::from(map)
                },
            );

            let array = vec(inner, 0..4).prop_map(|items| {
                let mut arr = rhai::Array::new();
                for item in items {
                    arr.push(item);
                }
                Dynamic::from(arr)
            });

            prop_oneof![map, array]
        })
        .boxed()
    }
    use chrono::{TimeZone, Utc};
    use rhai::{Array, Map};

    #[test]
    fn test_flatten_dynamic_simple_object() {
        let mut map = Map::new();
        map.insert("name".into(), Dynamic::from("alice"));
        map.insert("age".into(), Dynamic::from(25i64));

        let dynamic_map = Dynamic::from(map);
        let flattened = flatten_dynamic(&dynamic_map, FlattenStyle::Bracket, 10);

        assert_eq!(flattened.get("name").unwrap().to_string(), "alice");
        assert_eq!(flattened.get("age").unwrap().to_string(), "25");
    }

    #[test]
    fn test_flatten_dynamic_nested_object() {
        let mut inner_map = Map::new();
        inner_map.insert("street".into(), Dynamic::from("123 Main St"));
        inner_map.insert("city".into(), Dynamic::from("Boston"));

        let mut outer_map = Map::new();
        outer_map.insert("name".into(), Dynamic::from("alice"));
        outer_map.insert("address".into(), Dynamic::from(inner_map));

        let dynamic_map = Dynamic::from(outer_map);
        let flattened = flatten_dynamic(&dynamic_map, FlattenStyle::Bracket, 10);

        assert_eq!(flattened.get("name").unwrap().to_string(), "alice");
        assert_eq!(
            flattened.get("address.street").unwrap().to_string(),
            "123 Main St"
        );
        assert_eq!(flattened.get("address.city").unwrap().to_string(), "Boston");
    }

    #[test]
    fn test_flatten_dynamic_array() {
        let array = vec![
            Dynamic::from("item1"),
            Dynamic::from("item2"),
            Dynamic::from(42i64),
        ];

        let dynamic_array = Dynamic::from(array);
        let flattened = flatten_dynamic(&dynamic_array, FlattenStyle::Bracket, 10);

        assert_eq!(flattened.get("[0]").unwrap().to_string(), "item1");
        assert_eq!(flattened.get("[1]").unwrap().to_string(), "item2");
        assert_eq!(flattened.get("[2]").unwrap().to_string(), "42");
    }

    #[test]
    fn test_flatten_dynamic_mixed_structure() {
        let mut item1 = Map::new();
        item1.insert("id".into(), Dynamic::from(1i64));
        item1.insert("name".into(), Dynamic::from("first"));

        let mut item2 = Map::new();
        item2.insert("id".into(), Dynamic::from(2i64));
        item2.insert("name".into(), Dynamic::from("second"));

        let array = vec![Dynamic::from(item1), Dynamic::from(item2)];

        let mut root = Map::new();
        root.insert("user".into(), Dynamic::from("alice"));
        root.insert("items".into(), Dynamic::from(array));

        let dynamic_root = Dynamic::from(root);
        let flattened = flatten_dynamic(&dynamic_root, FlattenStyle::Bracket, 10);

        assert_eq!(flattened.get("user").unwrap().to_string(), "alice");
        assert_eq!(flattened.get("items[0].id").unwrap().to_string(), "1");
        assert_eq!(flattened.get("items[0].name").unwrap().to_string(), "first");
        assert_eq!(flattened.get("items[1].id").unwrap().to_string(), "2");
        assert_eq!(
            flattened.get("items[1].name").unwrap().to_string(),
            "second"
        );
    }

    #[test]
    fn test_flatten_styles() {
        let mut inner = Map::new();
        inner.insert("value".into(), Dynamic::from(42i64));

        let array = vec![Dynamic::from(inner)];

        let mut root = Map::new();
        root.insert("data".into(), Dynamic::from(array));

        let dynamic_root = Dynamic::from(root);

        // Test bracket style
        let bracket = flatten_dynamic(&dynamic_root, FlattenStyle::Bracket, 10);
        assert!(bracket.contains_key("data[0].value"));

        // Test dot style
        let dot = flatten_dynamic(&dynamic_root, FlattenStyle::Dot, 10);
        assert!(dot.contains_key("data.0.value"));

        // Test underscore style
        let underscore = flatten_dynamic(&dynamic_root, FlattenStyle::Underscore, 10);
        assert!(underscore.contains_key("data_0_value"));
    }

    #[test]
    fn extract_timestamp_uses_custom_field_when_present() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field(
            "custom_ts".to_string(),
            Dynamic::from("2024-05-19T12:34:56Z"),
        );
        event.set_field("ts".to_string(), Dynamic::from("2020-01-01T00:00:00Z"));

        let config = crate::timestamp::TsConfig {
            custom_field: Some("custom_ts".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        event.extract_timestamp_with_config(None, &config);

        let parsed = event
            .parsed_ts
            .expect("expected timestamp parsed from custom field");
        assert_eq!(
            parsed,
            Utc.with_ymd_and_hms(2024, 5, 19, 12, 34, 56).unwrap()
        );
    }

    #[test]
    fn extract_timestamp_missing_custom_field_does_not_fallback() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("ts".to_string(), Dynamic::from("2024-05-19T12:34:56Z"));

        let config = crate::timestamp::TsConfig {
            custom_field: Some("custom_ts".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        event.extract_timestamp_with_config(None, &config);

        assert!(
            event.parsed_ts.is_none(),
            "timestamp should remain unset when custom field is missing"
        );
        assert!(
            event.timestamp_stats_recorded,
            "absence should be recorded even when timestamp not parsed"
        );
    }

    #[test]
    fn extract_timestamp_non_string_custom_field_does_not_fallback() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("custom_ts".to_string(), Dynamic::from(123_i64));
        event.set_field("ts".to_string(), Dynamic::from("2024-05-19T12:34:56Z"));

        let config = crate::timestamp::TsConfig {
            custom_field: Some("custom_ts".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        event.extract_timestamp_with_config(None, &config);

        assert!(
            event.parsed_ts.is_none(),
            "non-string custom timestamp should not be parsed"
        );
        assert!(
            event.timestamp_stats_recorded,
            "invalid custom timestamp should count as absent"
        );
    }

    #[test]
    fn test_flatten_max_depth() {
        let mut deep = Map::new();
        deep.insert("level4".into(), Dynamic::from("deep"));

        let mut level3 = Map::new();
        level3.insert("level3".into(), Dynamic::from(deep));

        let mut level2 = Map::new();
        level2.insert("level2".into(), Dynamic::from(level3));

        let mut level1 = Map::new();
        level1.insert("level1".into(), Dynamic::from(level2));

        let dynamic_root = Dynamic::from(level1);

        // With max_depth=2, should stop at level2
        let flattened = flatten_dynamic(&dynamic_root, FlattenStyle::Bracket, 2);

        // Should have flattened up to level2 but not deeper
        assert!(flattened.contains_key("level1.level2"));
        assert!(!flattened.contains_key("level1.level2.level3.level4"));
    }

    #[test]
    fn test_flatten_empty_structures() {
        let empty_map = Map::new();
        let empty_array = Array::new();

        let flattened_map = flatten_dynamic(&Dynamic::from(empty_map), FlattenStyle::Bracket, 10);
        let flattened_array =
            flatten_dynamic(&Dynamic::from(empty_array), FlattenStyle::Bracket, 10);

        // Empty structures should produce a single null value
        assert_eq!(flattened_map.len(), 1);
        assert!(flattened_map.get("value").unwrap().is_unit());

        assert_eq!(flattened_array.len(), 1);
        assert!(flattened_array.get("value").unwrap().is_unit());
    }

    #[test]
    fn test_flatten_unlimited_depth() {
        // Create a very deeply nested structure
        let mut deep = Map::new();
        deep.insert("level8".into(), Dynamic::from("deepest"));

        let mut level7 = Map::new();
        level7.insert("level7".into(), Dynamic::from(deep));

        let mut level6 = Map::new();
        level6.insert("level6".into(), Dynamic::from(level7));

        let mut level5 = Map::new();
        level5.insert("level5".into(), Dynamic::from(level6));

        let mut level4 = Map::new();
        level4.insert("level4".into(), Dynamic::from(level5));

        let mut level3 = Map::new();
        level3.insert("level3".into(), Dynamic::from(level4));

        let mut level2 = Map::new();
        level2.insert("level2".into(), Dynamic::from(level3));

        let mut level1 = Map::new();
        level1.insert("level1".into(), Dynamic::from(level2));

        let dynamic_root = Dynamic::from(level1);

        // With max_depth=0 (unlimited), should flatten completely
        let unlimited = flatten_dynamic(&dynamic_root, FlattenStyle::Bracket, 0);

        // Should have fully flattened the deep structure
        assert!(unlimited.contains_key("level1.level2.level3.level4.level5.level6.level7.level8"));
        assert_eq!(
            unlimited
                .get("level1.level2.level3.level4.level5.level6.level7.level8")
                .unwrap()
                .to_string(),
            "deepest"
        );

        // Compare with limited depth
        let limited = flatten_dynamic(&dynamic_root, FlattenStyle::Bracket, 3);

        // Should stop at level 3 and contain the remaining structure as a string
        assert!(limited.contains_key("level1.level2.level3"));
        assert!(!limited.contains_key("level1.level2.level3.level4.level5.level6.level7.level8"));
    }
    proptest! {
        #[test]
        fn prop_flatten_preserves_scalars(value in arb_dynamic(3), style in prop_oneof![Just(FlattenStyle::Bracket), Just(FlattenStyle::Dot), Just(FlattenStyle::Underscore)]) {
            let flattened = flatten_dynamic(&value, style, 0);
            prop_assert!(!flattened.is_empty());

            if value.clone().try_cast::<rhai::Map>().is_none() && value.clone().try_cast::<rhai::Array>().is_none() {
                prop_assert_eq!(flattened.len(), 1);
                let (_, v) = flattened.iter().next().unwrap();
                prop_assert_eq!(v.clone().type_name(), value.type_name());
            }
        }

        #[test]
        fn prop_flatten_depth_limit_dot(depth in 1usize..5, value in arb_dynamic(3)) {
            let flattened = flatten_dynamic(&value, FlattenStyle::Dot, depth);
            for key in flattened.keys() {
                if key != "value" {
                    let segments = key.split('.').count();
                    prop_assert!(segments <= depth);
                }
            }
        }
    }

    // Tests for json_to_dynamic
    #[test]
    fn test_json_to_dynamic_string() {
        let json = serde_json::json!("hello");
        let dynamic = json_to_dynamic(&json);
        assert_eq!(dynamic.to_string(), "hello");
    }

    #[test]
    fn test_json_to_dynamic_integer() {
        let json = serde_json::json!(42);
        let dynamic = json_to_dynamic(&json);
        assert_eq!(dynamic.as_int().unwrap(), 42);
    }

    #[test]
    fn test_json_to_dynamic_float() {
        let json = serde_json::json!(42.5);
        let dynamic = json_to_dynamic(&json);
        assert!((dynamic.as_float().unwrap() - 42.5).abs() < 0.001);
    }

    #[test]
    fn test_json_to_dynamic_bool() {
        let json_true = serde_json::json!(true);
        let json_false = serde_json::json!(false);
        assert!(json_to_dynamic(&json_true).as_bool().unwrap());
        assert!(!json_to_dynamic(&json_false).as_bool().unwrap());
    }

    #[test]
    fn test_json_to_dynamic_null() {
        let json = serde_json::json!(null);
        let dynamic = json_to_dynamic(&json);
        assert!(dynamic.is_unit());
    }

    #[test]
    fn test_json_to_dynamic_array() {
        let json = serde_json::json!([1, "two", 3.0, true, null]);
        let dynamic = json_to_dynamic(&json);
        let array = dynamic.clone().try_cast::<Array>().unwrap();
        assert_eq!(array.len(), 5);
        assert_eq!(array[0].as_int().unwrap(), 1);
        assert_eq!(array[1].to_string(), "two");
        assert!((array[2].as_float().unwrap() - 3.0).abs() < 0.001);
        assert!(array[3].as_bool().unwrap());
        assert!(array[4].is_unit());
    }

    #[test]
    fn test_json_to_dynamic_object() {
        let json = serde_json::json!({
            "name": "alice",
            "age": 30,
            "active": true
        });
        let dynamic = json_to_dynamic(&json);
        let map = dynamic.clone().try_cast::<Map>().unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("name").unwrap().to_string(), "alice");
        assert_eq!(map.get("age").unwrap().as_int().unwrap(), 30);
        assert!(map.get("active").unwrap().as_bool().unwrap());
    }

    #[test]
    fn test_json_to_dynamic_nested_structure() {
        let json = serde_json::json!({
            "user": {
                "name": "bob",
                "scores": [95, 87, 92]
            },
            "metadata": {
                "created": "2024-01-01",
                "tags": ["urgent", "review"]
            }
        });
        let dynamic = json_to_dynamic(&json);
        let map = dynamic.clone().try_cast::<Map>().unwrap();

        let user = map.get("user").unwrap().clone().try_cast::<Map>().unwrap();
        assert_eq!(user.get("name").unwrap().to_string(), "bob");

        let scores = user
            .get("scores")
            .unwrap()
            .clone()
            .try_cast::<Array>()
            .unwrap();
        assert_eq!(scores.len(), 3);
        assert_eq!(scores[0].as_int().unwrap(), 95);
    }

    #[test]
    fn test_json_to_dynamic_empty_array() {
        let json = serde_json::json!([]);
        let dynamic = json_to_dynamic(&json);
        let array = dynamic.clone().try_cast::<Array>().unwrap();
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_json_to_dynamic_empty_object() {
        let json = serde_json::json!({});
        let dynamic = json_to_dynamic(&json);
        let map = dynamic.clone().try_cast::<Map>().unwrap();
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_json_to_dynamic_very_large_number() {
        let json = serde_json::json!(9007199254740992i64); // 2^53, max safe integer in f64
        let dynamic = json_to_dynamic(&json);
        assert_eq!(dynamic.as_int().unwrap(), 9007199254740992i64);
    }

    #[test]
    fn test_json_to_dynamic_negative_number() {
        let json = serde_json::json!(-42);
        let dynamic = json_to_dynamic(&json);
        assert_eq!(dynamic.as_int().unwrap(), -42);
    }

    #[test]
    fn test_json_to_dynamic_large_u64() {
        // Test a u64 value larger than i64::MAX (9,223,372,036,854,775,807)
        // but still precisely representable
        let large_u64 = 18_446_744_073_709_551_615u64; // u64::MAX

        // Parse from JSON string to preserve exact value
        let json_str = format!("{}", large_u64);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let dynamic = json_to_dynamic(&json);

        // Should preserve the exact u64 value, not convert to lossy f64
        // Rhai supports u64, so we should be able to extract it
        assert!(
            dynamic.is::<u64>(),
            "Expected u64 type, got: {:?}",
            dynamic.type_name()
        );
        let extracted: u64 = dynamic.clone().try_cast().unwrap();
        assert_eq!(extracted, large_u64);
    }

    #[test]
    fn test_json_to_dynamic_u64_above_i64_max() {
        // Test a u64 value just above i64::MAX
        let value = 9_223_372_036_854_775_808u64; // i64::MAX + 1

        let json_str = format!("{}", value);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let dynamic = json_to_dynamic(&json);

        // Should be stored as u64, not f64 (which would lose precision)
        assert!(
            dynamic.is::<u64>(),
            "Expected u64 type for value > i64::MAX"
        );
        let extracted: u64 = dynamic.clone().try_cast().unwrap();
        assert_eq!(extracted, value);
    }

    // Tests for Event field operations
    #[test]
    fn test_event_default() {
        let event = Event::default();
        assert!(event.fields.is_empty());
        assert!(!event.key_filtered);
        assert!(event.original_line.is_empty());
        assert!(event.line_num.is_none());
        assert!(event.filename.is_none());
        assert!(event.parsed_ts.is_none());
    }

    #[test]
    fn test_event_with_capacity() {
        let event = Event::with_capacity("test line".to_string(), 10);
        assert_eq!(event.original_line, "test line");
        assert_eq!(event.fields.capacity(), 10);
    }

    #[test]
    fn test_event_set_field() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("key1".to_string(), Dynamic::from("value1"));
        event.set_field("key2".to_string(), Dynamic::from(42i64));

        assert_eq!(event.fields.len(), 2);
        assert_eq!(event.fields.get("key1").unwrap().to_string(), "value1");
        assert_eq!(event.fields.get("key2").unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_event_set_metadata() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_metadata(42, Some("test.log".to_string()));

        assert_eq!(event.line_num, Some(42));
        assert_eq!(event.filename, Some("test.log".to_string()));
    }

    #[test]
    fn test_event_set_span_info() {
        let mut event = Event::default_with_line("line".to_string());
        let span_info = SpanInfo {
            status: Some(SpanStatus::Included),
            span_id: Some("span123".to_string()),
            span_start: Some(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()),
            span_end: Some(Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap()),
        };
        event.set_span_info(span_info.clone());

        assert_eq!(event.span.status, Some(SpanStatus::Included));
        assert_eq!(event.span.span_id, Some("span123".to_string()));
    }

    #[test]
    fn test_event_filter_keys() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("name".to_string(), Dynamic::from("alice"));
        event.set_field("age".to_string(), Dynamic::from(30i64));
        event.set_field("city".to_string(), Dynamic::from("Boston"));

        let keys = vec!["name".to_string(), "city".to_string()];
        event.filter_keys(&keys);

        assert_eq!(event.fields.len(), 2);
        assert!(event.fields.contains_key("name"));
        assert!(event.fields.contains_key("city"));
        assert!(!event.fields.contains_key("age"));
    }

    #[test]
    fn test_event_filter_keys_non_existent() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("name".to_string(), Dynamic::from("alice"));
        event.set_field("age".to_string(), Dynamic::from(30i64));

        let keys = vec!["name".to_string(), "nonexistent".to_string()];
        event.filter_keys(&keys);

        assert_eq!(event.fields.len(), 1);
        assert!(event.fields.contains_key("name"));
        assert!(!event.fields.contains_key("nonexistent"));
    }

    #[test]
    fn test_event_filter_keys_preserves_order() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("z".to_string(), Dynamic::from(1i64));
        event.set_field("a".to_string(), Dynamic::from(2i64));
        event.set_field("m".to_string(), Dynamic::from(3i64));

        // Filter with specific order
        let keys = vec!["a".to_string(), "z".to_string()];
        event.filter_keys(&keys);

        let collected_keys: Vec<&String> = event.fields.keys().collect();
        assert_eq!(collected_keys, vec![&"a".to_string(), &"z".to_string()]);
    }

    #[test]
    fn test_event_empty() {
        let event = Event::default_with_line("".to_string());
        assert!(event.fields.is_empty());
        assert_eq!(event.original_line, "");
    }

    #[test]
    fn test_event_very_large() {
        let mut event = Event::with_capacity("line".to_string(), 1000);

        // Add 1000 fields
        for i in 0..1000 {
            event.set_field(format!("field_{}", i), Dynamic::from(i as i64));
        }

        assert_eq!(event.fields.len(), 1000);
        assert_eq!(
            event.fields.get("field_500").unwrap().as_int().unwrap(),
            500
        );
    }

    // Tests for ordered_fields
    #[test]
    fn test_ordered_fields_basic() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("ts".to_string(), Dynamic::from("2024-01-01"));
        event.set_field("level".to_string(), Dynamic::from("ERROR"));
        event.set_field("msg".to_string(), Dynamic::from("test message"));
        event.set_field("user".to_string(), Dynamic::from("alice"));

        let ordered = ordered_fields(&event);
        let keys: Vec<&str> = ordered.iter().map(|(k, _)| k.as_str()).collect();

        // Should be: ts, level, msg, then others
        assert_eq!(keys[0], "ts");
        assert_eq!(keys[1], "level");
        assert_eq!(keys[2], "msg");
        assert_eq!(keys[3], "user");
    }

    #[test]
    fn test_ordered_fields_multiple_timestamp_fields() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("time".to_string(), Dynamic::from("2024-01-01"));
        event.set_field("ts".to_string(), Dynamic::from("2024-01-02"));
        event.set_field("msg".to_string(), Dynamic::from("test"));

        let ordered = ordered_fields(&event);
        let keys: Vec<&str> = ordered.iter().map(|(k, _)| k.as_str()).collect();

        // ts has higher priority than time
        assert_eq!(keys[0], "ts");
        assert_eq!(keys[1], "time");
    }

    #[test]
    fn test_ordered_fields_key_filtered_preserves_order() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("z".to_string(), Dynamic::from(1i64));
        event.set_field("a".to_string(), Dynamic::from(2i64));
        event.key_filtered = true;

        let ordered = ordered_fields(&event);
        let keys: Vec<&str> = ordered.iter().map(|(k, _)| k.as_str()).collect();

        // When key_filtered is true, preserve insertion order
        assert_eq!(keys[0], "z");
        assert_eq!(keys[1], "a");
    }

    #[test]
    fn test_ordered_fields_empty_event() {
        let event = Event::default_with_line("line".to_string());
        let ordered = ordered_fields(&event);
        assert_eq!(ordered.len(), 0);
    }

    #[test]
    fn test_ordered_fields_no_special_fields() {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field("user".to_string(), Dynamic::from("alice"));
        event.set_field("action".to_string(), Dynamic::from("login"));

        let ordered = ordered_fields(&event);
        assert_eq!(ordered.len(), 2);
    }

    // Tests for flatten_event_fields
    #[test]
    fn test_flatten_event_fields_basic() {
        let mut event = Event::default_with_line("line".to_string());

        let mut nested = Map::new();
        nested.insert("city".into(), Dynamic::from("Boston"));
        nested.insert("state".into(), Dynamic::from("MA"));

        event.set_field("name".to_string(), Dynamic::from("alice"));
        event.set_field("address".to_string(), Dynamic::from(nested));

        let flattened = flatten_event_fields(&event, FlattenStyle::Bracket, 10);

        assert_eq!(flattened.get("name").unwrap().to_string(), "alice");
        assert_eq!(flattened.get("address.city").unwrap().to_string(), "Boston");
        assert_eq!(flattened.get("address.state").unwrap().to_string(), "MA");
    }

    #[test]
    fn test_flatten_event_fields_with_arrays() {
        let mut event = Event::default_with_line("line".to_string());

        let array = vec![Dynamic::from("tag1"), Dynamic::from("tag2")];
        event.set_field("tags".to_string(), Dynamic::from(array));

        let flattened = flatten_event_fields(&event, FlattenStyle::Bracket, 10);

        // Array fields are flattened with brackets
        // The key format should be "tags.[0]" based on how flatten_event_fields works
        let keys: Vec<String> = flattened.keys().cloned().collect();
        // Check if the expected keys exist (could be "tags.[0]" or "tags[0]" depending on implementation)
        if flattened.contains_key("tags[0]") {
            assert_eq!(flattened.get("tags[0]").unwrap().to_string(), "tag1");
            assert_eq!(flattened.get("tags[1]").unwrap().to_string(), "tag2");
        } else if flattened.contains_key("tags.[0]") {
            assert_eq!(flattened.get("tags.[0]").unwrap().to_string(), "tag1");
            assert_eq!(flattened.get("tags.[1]").unwrap().to_string(), "tag2");
        } else {
            panic!("Expected array keys not found. Available keys: {:?}", keys);
        }
    }

    #[test]
    fn test_flatten_event_fields_empty_event() {
        let event = Event::default_with_line("line".to_string());
        let flattened = flatten_event_fields(&event, FlattenStyle::Bracket, 10);
        assert_eq!(flattened.len(), 0);
    }

    // Tests for context type
    #[test]
    fn test_context_type_default() {
        let event = Event::default();
        assert_eq!(event.context_type, ContextType::None);
    }

    // Tests for span status
    #[test]
    fn test_span_status_as_str() {
        assert_eq!(SpanStatus::Included.as_str(), "included");
        assert_eq!(SpanStatus::Late.as_str(), "late");
        assert_eq!(SpanStatus::Unassigned.as_str(), "unassigned");
        assert_eq!(SpanStatus::Filtered.as_str(), "filtered");
    }

    // Tests for FlattenStyle formatting
    #[test]
    fn test_flatten_style_format_object_key() {
        let style = FlattenStyle::Bracket;
        assert_eq!(style.format_object_key("", "key"), "key");
        assert_eq!(style.format_object_key("parent", "key"), "parent.key");

        let style = FlattenStyle::Underscore;
        assert_eq!(style.format_object_key("", "key"), "key");
        assert_eq!(style.format_object_key("parent", "key"), "parent_key");
    }

    #[test]
    fn test_flatten_style_format_array_key() {
        let style = FlattenStyle::Bracket;
        assert_eq!(style.format_array_key("", 0), "[0]");
        assert_eq!(style.format_array_key("arr", 5), "arr[5]");

        let style = FlattenStyle::Dot;
        assert_eq!(style.format_array_key("", 0), "0");
        assert_eq!(style.format_array_key("arr", 5), "arr.5");

        let style = FlattenStyle::Underscore;
        assert_eq!(style.format_array_key("", 0), "0");
        assert_eq!(style.format_array_key("arr", 5), "arr_5");
    }
}
