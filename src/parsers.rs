use crate::event::{Event, FieldValue};
use regex::Regex;

pub trait LogParser {
    fn parse(&self, line: &str) -> Result<Event, ParseError>;
}

#[derive(Debug)]
pub enum ParseError {
    InvalidFormat(String),
    JsonError(serde_json::Error),
    RegexError(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            ParseError::JsonError(e) => write!(f, "JSON error: {}", e),
            ParseError::RegexError(msg) => write!(f, "Regex error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

// Logfmt Parser
pub struct LogfmtParser {
    key_value_regex: Regex,
}

impl LogfmtParser {
    pub fn new() -> Self {
        Self {
            key_value_regex: Regex::new(r#"([a-zA-Z_][a-zA-Z0-9_]*)=(?:"([^"]*)"|([^\s]+))"#).unwrap(),
        }
    }
}

impl LogParser for LogfmtParser {
    fn parse(&self, line: &str) -> Result<Event, ParseError> {
        let mut event = Event::new();
        
        for cap in self.key_value_regex.captures_iter(line) {
            let key = cap.get(1).unwrap().as_str().to_string();
            let value = if let Some(quoted) = cap.get(2) {
                quoted.as_str().to_string()
            } else if let Some(unquoted) = cap.get(3) {
                unquoted.as_str().to_string()
            } else {
                continue;
            };
            
            // Try to parse as number or boolean
            let field_value = if let Ok(num) = value.parse::<f64>() {
                FieldValue::Number(num)
            } else if let Ok(bool_val) = value.parse::<bool>() {
                FieldValue::Boolean(bool_val)
            } else if value == "null" {
                FieldValue::Null
            } else {
                FieldValue::String(value)
            };
            
            event.set_field(key, field_value);
        }
        
        event.extract_core_fields();
        Ok(event)
    }
}

// JSONL Parser
pub struct JsonlParser;

impl JsonlParser {
    pub fn new() -> Self {
        Self
    }
}

impl LogParser for JsonlParser {
    fn parse(&self, line: &str) -> Result<Event, ParseError> {
        let json_value: serde_json::Value = serde_json::from_str(line)
            .map_err(ParseError::JsonError)?;
        
        let mut event = Event::new();
        
        if let serde_json::Value::Object(map) = json_value {
            for (key, value) in map {
                let field_value = match value {
                    serde_json::Value::String(s) => FieldValue::String(s),
                    serde_json::Value::Number(n) => FieldValue::Number(n.as_f64().unwrap_or(0.0)),
                    serde_json::Value::Bool(b) => FieldValue::Boolean(b),
                    serde_json::Value::Null => FieldValue::Null,
                    _ => FieldValue::String(value.to_string()),
                };
                event.set_field(key, field_value);
            }
        } else {
            return Err(ParseError::InvalidFormat("Expected JSON object".to_string()));
        }
        
        event.extract_core_fields();
        Ok(event)
    }
}

// Basic Syslog Parser (RFC3164-ish)
pub struct SyslogParser {
    syslog_regex: Regex,
}

impl SyslogParser {
    pub fn new() -> Self {
        Self {
            // Basic syslog pattern: <priority>timestamp hostname process[pid]: message
            syslog_regex: Regex::new(
                r"^(?:<(\d+)>)?(\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+(\S+)\s+([^:\[]+)(?:\[(\d+)\])?\s*:\s*(.*)$"
            ).unwrap(),
        }
    }
}

impl LogParser for SyslogParser {
    fn parse(&self, line: &str) -> Result<Event, ParseError> {
        let mut event = Event::new();
        
        if let Some(caps) = self.syslog_regex.captures(line) {
            // Priority (optional)
            if let Some(priority) = caps.get(1) {
                if let Ok(pri) = priority.as_str().parse::<u32>() {
                    let facility = pri >> 3;
                    let severity = pri & 7;
                    event.set_field("priority".to_string(), FieldValue::Number(pri as f64));
                    event.set_field("facility".to_string(), FieldValue::Number(facility as f64));
                    event.set_field("severity".to_string(), FieldValue::Number(severity as f64));
                    
                    // Map severity to log level
                    let level = match severity {
                        0 => "EMERGENCY",
                        1 => "ALERT", 
                        2 => "CRITICAL",
                        3 => "ERROR",
                        4 => "WARNING",
                        5 => "NOTICE",
                        6 => "INFO",
                        7 => "DEBUG",
                        _ => "UNKNOWN",
                    };
                    event.level = Some(level.to_string());
                }
            }
            
            // Timestamp
            if let Some(timestamp) = caps.get(2) {
                event.set_field("timestamp".to_string(), FieldValue::String(timestamp.as_str().to_string()));
            }
            
            // Hostname
            if let Some(hostname) = caps.get(3) {
                event.set_field("hostname".to_string(), FieldValue::String(hostname.as_str().to_string()));
            }
            
            // Process name
            if let Some(process) = caps.get(4) {
                event.set_field("process".to_string(), FieldValue::String(process.as_str().to_string()));
            }
            
            // PID (optional)
            if let Some(pid) = caps.get(5) {
                if let Ok(pid_num) = pid.as_str().parse::<f64>() {
                    event.set_field("pid".to_string(), FieldValue::Number(pid_num));
                }
            }
            
            // Message
            if let Some(message) = caps.get(6) {
                event.message = Some(message.as_str().to_string());
                event.set_field("message".to_string(), FieldValue::String(message.as_str().to_string()));
            }
        } else {
            // If regex doesn't match, treat whole line as message
            event.message = Some(line.to_string());
            event.set_field("message".to_string(), FieldValue::String(line.to_string()));
        }
        
        event.extract_core_fields();
        Ok(event)
    }
}