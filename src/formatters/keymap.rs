//! Keymap output formatter
//!
//! Displays a configurable field as single characters per event for a dense visual summary.
//! Similar to levelmap but works with any field.

use std::sync::Mutex;

use crate::event::Event;
use crate::pipeline;

use super::compact_map::{
    dynamic_to_trimmed_string, extract_timestamp, format_line, CompactMapState,
};

/// Keymap formatter - visualizes a configurable field as single characters
pub struct KeymapFormatter {
    state: Mutex<CompactMapState>,
    terminal_width: usize,
    buffer_width_override: Option<usize>,
    field_name: String,
}

impl KeymapFormatter {
    const FALLBACK_TERMINAL_WIDTH: usize = 80;

    pub fn new(field_name: Option<String>) -> Self {
        let detected_width = crate::tty::get_terminal_width();
        let terminal_width = if detected_width == 0 {
            Self::FALLBACK_TERMINAL_WIDTH
        } else {
            detected_width
        };

        Self {
            state: Mutex::new(CompactMapState::new(terminal_width)),
            terminal_width,
            buffer_width_override: None,
            field_name: field_name.unwrap_or_else(|| "level".to_string()),
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize, field_name: Option<String>) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(CompactMapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            field_name: field_name.unwrap_or_else(|| "level".to_string()),
        }
    }

    fn available_width(&self, timestamp: Option<&String>) -> usize {
        if let Some(override_width) = self.buffer_width_override {
            return override_width.max(1);
        }

        let terminal_width = self.terminal_width.max(1);
        let reserved = timestamp
            .filter(|ts| !ts.is_empty())
            .map(|ts| ts.len().saturating_add(1))
            .unwrap_or(0);

        terminal_width.saturating_sub(reserved).max(1)
    }

    fn extract_field_string(&self, event: &Event) -> Option<String> {
        if let Some(value) = event.fields.get(&self.field_name) {
            dynamic_to_trimmed_string(value)
        } else {
            None
        }
    }
}

impl pipeline::Formatter for KeymapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self.state.lock().expect("keymap formatter mutex poisoned");

        if state.current_timestamp.is_none() {
            state.current_timestamp = Some(extract_timestamp(event));
        }

        let available_width = self.available_width(state.current_timestamp.as_ref());

        let field_string = self.extract_field_string(event);
        let display_char = field_string
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('.');
        state.push_rendered(&display_char.to_string());

        if state.visible_len >= available_width {
            let line = format_line(state.current_timestamp.as_ref(), &state.buffer);
            state.reset();
            line
        } else {
            String::new()
        }
    }

    fn finish(&self) -> Option<String> {
        let mut state = self.state.lock().expect("keymap formatter mutex poisoned");
        if state.visible_len == 0 {
            return None;
        }

        let line = format_line(state.current_timestamp.as_ref(), &state.buffer);
        state.reset();

        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
}
