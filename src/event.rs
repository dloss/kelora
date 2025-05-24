use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: Option<DateTime<Utc>>,
    pub level: Option<String>,
    pub message: Option<String>,
    pub fields: HashMap<String, FieldValue>,
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
    pub fn new() -> Self {
        Self {
            timestamp: None,
            level: None,
            message: None,
            fields: HashMap::new(),
        }
    }
    
    pub fn set_field(&mut self, key: String, value: FieldValue) {
        self.fields.insert(key, value);
    }
    
    /// Filter to only show specified keys, keeping only fields that actually exist
    pub fn filter_keys(&mut self, keys: &[String]) {
        let mut new_fields = HashMap::new();
        
        // First, handle core fields - only keep them if they exist and are requested
        let mut keep_timestamp = false;
        let mut keep_level = false;
        let mut keep_message = false;
        
        for key in keys {
            match key.as_str() {
                "timestamp" | "ts" | "time" | "at" | "_t" | "@t" | "t" => {
                    if self.timestamp.is_some() {
                        keep_timestamp = true;
                    }
                }
                "level" | "log_level" | "loglevel" | "lvl" | "severity" | "@l" => {
                    if self.level.is_some() {
                        keep_level = true;
                    }
                }
                "message" | "msg" | "@m" => {
                    if self.message.is_some() {
                        keep_message = true;
                    }
                }
                _ => {
                    // For other fields, only include if they exist
                    if let Some(value) = self.fields.get(key) {
                        new_fields.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        
        // Clear core fields if they weren't requested or don't exist
        if !keep_timestamp {
            self.timestamp = None;
        }
        if !keep_level {
            self.level = None;
        }
        if !keep_message {
            self.message = None;
        }
        
        self.fields = new_fields;
    }
    
    /// Try to parse and extract core fields from the fields map
    pub fn extract_core_fields(&mut self) {
        // Extract timestamp
        for ts_key in &["timestamp", "ts", "time", "at", "_t", "@t", "t"] {
            if let Some(FieldValue::String(ts_str)) = self.fields.get(*ts_key) {
                if let Ok(ts) = parse_timestamp(ts_str) {
                    self.timestamp = Some(ts);
                    break;
                }
            }
        }
        
        // Extract level
        for level_key in &["level", "log_level", "loglevel", "lvl", "severity", "@l"] {
            if let Some(value) = self.fields.get(*level_key) {
                if let Some(level_str) = value.as_string() {
                    self.level = Some(level_str.clone());
                    break;
                }
            }
        }
        
        // Extract message
        for msg_key in &["message", "msg", "@m"] {
            if let Some(value) = self.fields.get(*msg_key) {
                if let Some(msg_str) = value.as_string() {
                    self.message = Some(msg_str.clone());
                    break;
                }
            }
        }
    }
    
    /// Check if the event has any content to display
    pub fn has_displayable_content(&self) -> bool {
        self.timestamp.is_some() || 
        self.level.is_some() || 
        self.message.is_some() || 
        !self.fields.is_empty()
    }
}

impl FieldValue {
    pub fn as_string(&self) -> Option<&String> {
        match self {
            FieldValue::String(s) => Some(s),
            _ => None,
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

fn parse_timestamp(ts_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    // Try common timestamp formats in order of likelihood
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.fZ",           // ISO 8601 with subseconds
        "%Y-%m-%dT%H:%M:%SZ",              // ISO 8601 
        "%Y-%m-%dT%H:%M:%S%.f%:z",         // ISO 8601 with timezone
        "%Y-%m-%dT%H:%M:%S%:z",            // ISO 8601 with timezone
        "%Y-%m-%d %H:%M:%S%.f",            // Common log format with subseconds
        "%Y-%m-%d %H:%M:%S",               // Common log format
        "%b %d %H:%M:%S",                  // Syslog format
    ];
    
    for format in &formats {
        if let Ok(dt) = DateTime::parse_from_str(ts_str, format) {
            return Ok(dt.with_timezone(&Utc));
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts_str, format) {
            return Ok(dt.and_utc());
        }
    }
    
    // Return a proper chrono parse error
    chrono::NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%dT%H:%M:%SZ")
        .map(|dt| dt.and_utc())
}