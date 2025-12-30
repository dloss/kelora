//! JSON output formatter
//!
//! Formats events as JSON objects, one per line.

use crate::event::Event;
use crate::pipeline;

use super::utils::dynamic_to_json;

/// JSON formatter - outputs events as JSON objects
pub struct JsonFormatter;

impl JsonFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl pipeline::Formatter for JsonFormatter {
    fn format(&self, event: &Event) -> String {
        // Convert Dynamic values to JSON manually
        let mut json_obj = serde_json::Map::new();

        for (key, value) in crate::event::ordered_fields(event) {
            let json_value = dynamic_to_json(value);
            json_obj.insert(key.clone(), json_value);
        }

        serde_json::to_string(&serde_json::Value::Object(json_obj))
            .unwrap_or_else(|_| "{}".to_string())
    }
}
