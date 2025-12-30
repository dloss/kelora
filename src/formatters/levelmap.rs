//! Levelmap output formatter
//!
//! Displays log levels as single characters per event for a dense visual summary.
//! Color-coded by severity level when colors are enabled.

use std::sync::Mutex;

use crate::colors::ColorScheme;
use crate::event::Event;
use crate::pipeline;

use super::compact_map::{
    dynamic_to_trimmed_string, extract_timestamp, format_line, CompactMapState,
};

/// Levelmap formatter - visualizes log levels as single characters
pub struct LevelmapFormatter {
    state: Mutex<CompactMapState>,
    terminal_width: usize,
    buffer_width_override: Option<usize>,
    colors: ColorScheme,
}

impl LevelmapFormatter {
    const FALLBACK_TERMINAL_WIDTH: usize = 80;

    pub fn new(use_colors: bool) -> Self {
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
            colors: ColorScheme::new(use_colors),
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(CompactMapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            colors: ColorScheme::new(false),
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

    fn extract_level_string(event: &Event) -> Option<String> {
        for key in crate::event::LEVEL_FIELD_NAMES {
            if let Some(value) = event.fields.get(*key) {
                if let Some(level) = dynamic_to_trimmed_string(value) {
                    return Some(level);
                }
            }
        }
        None
    }

    fn render_level_char(&self, level: Option<&str>, ch: char) -> String {
        if let Some(level_str) = level {
            let color = self.level_color(level_str);
            if !color.is_empty() {
                let mut rendered = String::with_capacity(color.len() + self.colors.reset.len() + 1);
                rendered.push_str(color);
                rendered.push(ch);
                rendered.push_str(self.colors.reset);
                return rendered;
            }
        }

        ch.to_string()
    }

    fn level_color<'a>(&'a self, level: &str) -> &'a str {
        match level.to_lowercase().as_str() {
            "error" | "err" | "fatal" | "panic" | "alert" | "crit" | "critical" | "emerg"
            | "emergency" | "severe" => self.colors.level_error,
            "warn" | "warning" => self.colors.level_warn,
            "info" | "informational" | "notice" => self.colors.level_info,
            "debug" | "finer" | "config" => self.colors.level_debug,
            "trace" | "finest" => self.colors.level_trace,
            _ => "",
        }
    }
}

impl pipeline::Formatter for LevelmapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self
            .state
            .lock()
            .expect("levelmap formatter mutex poisoned");

        if state.current_timestamp.is_none() {
            state.current_timestamp = Some(extract_timestamp(event));
        }

        let available_width = self.available_width(state.current_timestamp.as_ref());

        let level_string = Self::extract_level_string(event);
        let display_char = level_string
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('?');
        let rendered = self.render_level_char(level_string.as_deref(), display_char);
        state.push_rendered(&rendered);

        if state.visible_len >= available_width {
            let line = format_line(state.current_timestamp.as_ref(), &state.buffer);
            state.reset();
            line
        } else {
            String::new()
        }
    }

    fn finish(&self) -> Option<String> {
        let mut state = self
            .state
            .lock()
            .expect("levelmap formatter mutex poisoned");
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
