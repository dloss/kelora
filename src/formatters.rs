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
        
        // Add other fields in sorted order
        let mut field_keys: Vec<_> = event.fields.keys().collect();
        field_keys.sort();
        
        for key in field_keys {
            if let Some(value) = event.fields.get(key) {
                let formatted_value = match value {
                    FieldValue::String(s) => format!("\"{}\"", escape_quotes(s)),
                    FieldValue::Number(n) => {
                        // Format numbers nicely - avoid unnecessary decimal places for integers
                        if n.fract() == 0.0 {
                            format!("{}", *n as i64)
                        } else {
                            format!("{}", n)
                        }
                    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_default_formatter_empty_event() {
        let event = Event::new();
        let formatter = DefaultFormatter::new();
        assert_eq!(formatter.format(&event), "");
    }

    #[test]
    fn test_default_formatter_with_fields() {
        let mut event = Event::new();
        event.level = Some("INFO".to_string());
        event.message = Some("Test message".to_string());
        event.set_field("key1".to_string(), FieldValue::String("value1".to_string()));
        event.set_field("key2".to_string(), FieldValue::Number(42.0));
        
        let formatter = DefaultFormatter::new();
        let result = formatter.format(&event);
        
        assert!(result.contains("level=\"INFO\""));
        assert!(result.contains("message=\"Test message\""));
        assert!(result.contains("key1=\"value1\""));
        assert!(result.contains("key2=42"));
    }

    #[test]
    fn test_number_formatting() {
        let mut event = Event::new();
        event.set_field("int".to_string(), FieldValue::Number(42.0));
        event.set_field("float".to_string(), FieldValue::Number(42.5));
        
        let formatter = DefaultFormatter::new();
        let result = formatter.format(&event);
        
        assert!(result.contains("int=42"));
        assert!(result.contains("float=42.5"));
    }

    #[test]
    fn test_escape_quotes() {
        assert_eq!(escape_quotes("hello"), "hello");
        assert_eq!(escape_quotes("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(escape_quotes("path\\to\\file"), "path\\\\to\\\\file");
    }
}