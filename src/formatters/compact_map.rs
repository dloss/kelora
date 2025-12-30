//! Compact map formatter utilities
//!
//! Shared state and utilities for compact map formatters (levelmap, keymap, tailmap).
//! These formatters display one character per event for dense visual summaries.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use rhai::Dynamic;

use crate::event::Event;

/// Shared state for compact map formatters
pub struct CompactMapState {
    pub current_timestamp: Option<String>,
    pub buffer: String,
    pub visible_len: usize,
}

impl CompactMapState {
    pub fn new(initial_capacity: usize) -> Self {
        let base_capacity = initial_capacity.max(1) * 4;
        Self {
            current_timestamp: None,
            buffer: String::with_capacity(base_capacity),
            visible_len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_timestamp = None;
        self.buffer.clear();
        self.visible_len = 0;
    }

    pub fn push_rendered(&mut self, rendered: &str) {
        self.buffer.push_str(rendered);
        self.visible_len += 1;
    }
}

/// Convert a Dynamic value to a trimmed string, returning None if empty
pub fn dynamic_to_trimmed_string(value: &Dynamic) -> Option<String> {
    if let Ok(s) = value.clone().into_string() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        let fallback = value.to_string();
        let trimmed = fallback.trim();
        if trimmed.is_empty() || trimmed == "()" {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

/// Format a line with optional timestamp prefix
pub fn format_line(timestamp: Option<&String>, buffer: &str) -> String {
    match timestamp {
        Some(ts) if !ts.is_empty() => format!("{} {}", ts, buffer),
        _ => buffer.to_string(),
    }
}

/// Extract timestamp from an event, trying various sources
pub fn extract_timestamp(event: &Event) -> String {
    if let Some(ts) = event.parsed_ts {
        return format_timestamp(ts);
    }

    for key in crate::event::TIMESTAMP_FIELD_NAMES {
        if let Some(value) = event.fields.get(*key) {
            if let Some(ts) = value.clone().try_cast::<DateTime<Utc>>() {
                return format_timestamp(ts);
            }

            if let Some(ts) = value.clone().try_cast::<DateTime<FixedOffset>>() {
                return format_timestamp(ts.with_timezone(&Utc));
            }

            if let Ok(string_value) = value.clone().into_string() {
                let trimmed = string_value.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            } else {
                let fallback = value.to_string();
                let trimmed = fallback.trim();
                if !trimmed.is_empty() && trimmed != "()" {
                    return trimmed.to_string();
                }
            }
        }
    }

    if let Some(line_num) = event.line_num {
        format!("line {}", line_num)
    } else {
        "unknown".to_string()
    }
}

/// Format a DateTime as RFC3339 with millisecond precision
pub fn format_timestamp(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}
