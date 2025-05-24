use crate::event::{Event, FieldValue};

pub trait Formatter {
    fn format(&self, event: &Event) -> String;
}

// Default logfmt-style formatter
pub struct DefaultFormatter;

impl DefaultFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Formatter for DefaultFormatter {
    fn format(&self, event: &Event) -> String {
        let mut parts = Vec::new();
        
        // Add core fields first if they exist
        if let Some(timestamp) = &event.timestamp {
            parts.push(format!("timestamp=\"{}\"", timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ")));
        }
        
        if let Some(level) = &event.level {
            parts.push(format!("level=\"{}\"", level));
        }
        
        if let Some(message) = &event.message {
            parts.push(format!("message=\"{}\"", escape_quotes(message)));
        }
        
        // Add other fields
        let mut field_keys: Vec<_> = event.fields.keys().collect();
        field_keys.sort();
        
        for key in field_keys {
            if let Some(value) = event.fields.get(key) {
                // Skip if we already showed this as a core field
                if matches!(key.as_str(), "timestamp" | "ts" | "time" | "at" | "_t" | "@t" |
                                         "level" | "log_level" | "loglevel" | "lvl" | "severity" | "@l" |
                                         "message" | "msg" | "@m") {
                    continue;
                }
                
                let formatted_value = match value {
                    FieldValue::String(s) => format!("\"{}\"", escape_quotes(s)),
                    FieldValue::Number(n) => n.to_string(),
                    FieldValue::Boolean(b) => b.to_string(),
                    FieldValue::Null => "null".to_string(),
                };
                parts.push(format!("{}={}", key, formatted_value));
            }
        }
        
        parts.join(" ")
    }
}

// JSONL formatter
pub struct JsonlFormatter;

impl JsonlFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Formatter for JsonlFormatter {
    fn format(&self, event: &Event) -> String {
        let mut json_obj = serde_json::Map::new();
        
        // Add core fields
        if let Some(timestamp) = &event.timestamp {
            json_obj.insert("timestamp".to_string(), 
                           serde_json::Value::String(timestamp.to_rfc3339()));
        }
        
        if let Some(level) = &event.level {
            json_obj.insert("level".to_string(), 
                           serde_json::Value::String(level.clone()));
        }
        
        if let Some(message) = &event.message {
            json_obj.insert("message".to_string(), 
                           serde_json::Value::String(message.clone()));
        }
        
        // Add other fields
        for (key, value) in &event.fields {
            let json_value = match value {
                FieldValue::String(s) => serde_json::Value::String(s.clone()),
                FieldValue::Number(n) => serde_json::Value::Number(
                    serde_json::Number::from_f64(*n).unwrap_or_else(|| serde_json::Number::from(0))
                ),
                FieldValue::Boolean(b) => serde_json::Value::Bool(*b),
                FieldValue::Null => serde_json::Value::Null,
            };
            json_obj.insert(key.clone(), json_value);
        }
        
        serde_json::to_string(&serde_json::Value::Object(json_obj))
            .unwrap_or_else(|_| "{}".to_string())
    }
}

fn escape_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}