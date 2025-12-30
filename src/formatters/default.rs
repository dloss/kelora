//! Default output formatter
//!
//! The default human-readable output format with logfmt-style key=value pairs,
//! color coding, word wrapping, and timestamp formatting.

use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};
use rhai::Dynamic;

use crate::colors::ColorScheme;
use crate::event::{ContextType, Event};
use crate::pipeline;

use super::utils::{
    dynamic_to_json, escape_single_quote_string, format_dynamic_value, indent_multiline_value,
};

/// Default formatter (logfmt-style with colors and brief mode)
pub struct DefaultFormatter {
    pub colors: ColorScheme,
    pub level_keys: Vec<&'static str>,
    pub brief: bool,
    pub timestamp_formatting: crate::config::TimestampFormatConfig,
    pub enable_wrapping: bool,
    pub terminal_width: usize,
    pub pretty_nested: bool,
    pub use_emoji: bool,
    pub quiet_level: u8,
}

impl DefaultFormatter {
    pub fn new_with_wrapping(
        use_colors: bool,
        use_emoji: bool,
        brief: bool,
        timestamp_formatting: crate::config::TimestampFormatConfig,
        enable_wrapping: bool,
        pretty_nested: bool,
        quiet_level: u8,
    ) -> Self {
        let terminal_width = if enable_wrapping {
            crate::tty::get_terminal_width()
        } else {
            100 // Doesn't matter if wrapping is disabled
        };

        Self {
            colors: ColorScheme::new(use_colors),
            level_keys: vec![
                "level",
                "loglevel",
                "log_level",
                "lvl",
                "severity",
                "levelname",
                "@l",
            ],
            brief,
            timestamp_formatting,
            enable_wrapping,
            terminal_width,
            pretty_nested,
            use_emoji: use_emoji && use_colors,
            quiet_level,
        }
    }

    /// Format a Dynamic value directly into buffer for performance (zero-allocation when possible)
    fn format_dynamic_value_into(&self, key: &str, value: &Dynamic, output: &mut String) {
        // Check if this field should be formatted as a timestamp
        if self.should_format_as_timestamp(key) {
            if let Some(formatted_ts) = self.try_format_timestamp(value) {
                // Use timestamp formatting
                if !self.colors.string.is_empty() {
                    output.push_str(self.colors.string);
                }
                output.push('\'');
                output.push_str(&escape_single_quote_string(&formatted_ts));
                output.push('\'');
                if !self.colors.string.is_empty() {
                    output.push_str(self.colors.reset);
                }
                return;
            }
        }

        // Choose color based on field type and value content
        let color = if self.is_level_field(key) {
            if let Ok(level_str) = value.clone().into_string() {
                self.level_color(&level_str)
            } else {
                self.colors.string
            }
        } else {
            self.colors.string
        };

        // Format value based on type - always quote strings for default formatter
        let (string_val, is_string) = self.format_default_value(value);
        if is_string {
            // Add opening quote (uncolored)
            output.push('\'');
            // Apply color to content only
            if !color.is_empty() {
                output.push_str(color);
            }
            output.push_str(&escape_single_quote_string(&string_val));
            // Reset color before closing quote
            if !color.is_empty() {
                output.push_str(self.colors.reset);
            }
            // Add closing quote (uncolored)
            output.push('\'')
        } else {
            // For non-strings, color the entire value
            if !color.is_empty() {
                output.push_str(color);
            }
            output.push_str(&string_val);
            if !color.is_empty() {
                output.push_str(self.colors.reset);
            }
        }
    }

    /// Format a Dynamic value for brief mode (no quotes, just the value with colors)
    fn format_dynamic_value_brief_into(&self, key: &str, value: &Dynamic, output: &mut String) {
        // Check if this field should be formatted as a timestamp
        if self.should_format_as_timestamp(key) {
            if let Some(formatted_ts) = self.try_format_timestamp(value) {
                // Use timestamp formatting (no quotes in brief mode)
                if !self.colors.string.is_empty() {
                    output.push_str(self.colors.string);
                }
                output.push_str(&formatted_ts);
                if !self.colors.string.is_empty() {
                    output.push_str(self.colors.reset);
                }
                return;
            }
        }

        // Choose color based on field type and value content
        let color = if self.is_level_field(key) {
            if let Ok(level_str) = value.clone().into_string() {
                self.level_color(&level_str)
            } else {
                self.colors.string
            }
        } else {
            self.colors.string
        };

        // Apply color
        if !color.is_empty() {
            output.push_str(color);
        }

        // In brief mode, output raw value (no quotes even for strings)
        let (formatted_val, _is_string) = self.format_default_value(value);
        output.push_str(&formatted_val);

        // Reset color
        if !color.is_empty() {
            output.push_str(self.colors.reset);
        }
    }

    /// Get appropriate color for log level values
    fn level_color(&self, level: &str) -> &str {
        match level.to_lowercase().as_str() {
            // Bright red for error levels
            "error" | "err" | "fatal" | "panic" | "alert" | "crit" | "critical" | "emerg"
            | "emergency" | "severe" => self.colors.level_error,
            // Bright yellow for warning levels
            "warn" | "warning" => self.colors.level_warn,
            // Bright green for info levels
            "info" | "informational" | "notice" => self.colors.level_info,
            // Bright cyan for debug levels
            "debug" | "finer" | "config" => self.colors.level_debug,
            // Cyan for trace levels
            "trace" | "finest" => self.colors.level_trace,
            // Default to no color for unknown levels
            _ => "",
        }
    }

    /// Check if key is likely a log level field
    fn is_level_field(&self, key: &str) -> bool {
        self.level_keys.contains(&key)
    }

    /// Check if a field should be formatted as a timestamp
    fn should_format_as_timestamp(&self, key: &str) -> bool {
        // Check if this field is explicitly listed in format_fields
        if self
            .timestamp_formatting
            .format_fields
            .contains(&key.to_string())
        {
            return true;
        }

        // Check if auto-formatting is enabled and this is a known timestamp field
        if self.timestamp_formatting.auto_format_all {
            return crate::event::TIMESTAMP_FIELD_NAMES.contains(&key);
        }

        false
    }

    /// Convert a parsed UTC timestamp into the configured display timezone
    fn format_timestamp_output(&self, dt: chrono::DateTime<chrono::Utc>) -> String {
        if self.timestamp_formatting.format_as_utc {
            dt.to_rfc3339()
        } else {
            dt.with_timezone(&chrono::Local).to_rfc3339()
        }
    }

    /// Try to format a value as a timestamp, returning formatted string if successful
    fn try_format_timestamp(&self, value: &Dynamic) -> Option<String> {
        use chrono::{DateTime, Utc};

        let format_hint = self.timestamp_formatting.parse_format_hint.as_deref();
        let timezone_hint = self.timestamp_formatting.parse_timezone_hint.as_deref();

        // First, try if it's already a DateTime value
        if let Some(dt) = value.clone().try_cast::<DateTime<Utc>>() {
            return Some(self.format_timestamp_output(dt));
        }

        // Try to parse numeric timestamps (Unix timestamps)
        if let Ok(timestamp_num) = value.as_int() {
            let timestamp_str = timestamp_num.to_string();
            let mut parser = crate::timestamp::AdaptiveTsParser::new();
            if let Some(parsed_dt) =
                parser.parse_ts_with_config(&timestamp_str, format_hint, timezone_hint)
            {
                return Some(self.format_timestamp_output(parsed_dt));
            }
        }

        // Try to parse float timestamps (Unix timestamps with fractional seconds)
        if let Ok(timestamp_float) = value.as_float() {
            // Handle Unix timestamps in float format by parsing directly
            use chrono::DateTime;

            // Determine precision based on magnitude
            let parsed_dt = if timestamp_float >= 1e15 {
                // Microseconds (16+ digits)
                DateTime::from_timestamp(
                    (timestamp_float / 1_000_000.0).floor() as i64,
                    ((timestamp_float % 1_000_000.0) * 1000.0) as u32,
                )
            } else if timestamp_float >= 1e12 {
                // Milliseconds (13+ digits)
                DateTime::from_timestamp(
                    (timestamp_float / 1000.0).floor() as i64,
                    ((timestamp_float % 1000.0) * 1_000_000.0) as u32,
                )
            } else if timestamp_float >= 1e9 {
                // Seconds with fractional part (10+ digits)
                DateTime::from_timestamp(
                    timestamp_float.floor() as i64,
                    (timestamp_float.fract() * 1_000_000_000.0) as u32,
                )
            } else {
                // Too small to be a valid Unix timestamp
                None
            };

            if let Some(dt) = parsed_dt {
                let utc_dt = dt.with_timezone(&Utc);
                return Some(self.format_timestamp_output(utc_dt));
            }
        }

        // Otherwise, try to parse it as a string timestamp
        if let Ok(ts_str) = value.clone().into_string() {
            let mut parser = crate::timestamp::AdaptiveTsParser::new();
            if let Some(parsed_dt) =
                parser.parse_ts_with_config(&ts_str, format_hint, timezone_hint)
            {
                return Some(self.format_timestamp_output(parsed_dt));
            }
        }

        None
    }

    /// Format a Dynamic value for default output, preserving nested structures
    fn format_default_value(&self, value: &Dynamic) -> (String, bool) {
        // Check if this is a complex nested structure and render it as JSON so the type is explicit
        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            let json_value = dynamic_to_json(value);
            let serialized = if self.pretty_nested {
                serde_json::to_string_pretty(&json_value)
            } else {
                serde_json::to_string(&json_value)
            };

            match serialized {
                Ok(mut s) => {
                    if self.pretty_nested {
                        // Keep continuation lines aligned with formatter indentation
                        s = indent_multiline_value(&s, "  ");
                    }
                    (s, false)
                }
                Err(_) => (value.to_string(), false),
            }
        } else {
            // Use the original format_dynamic_value for scalar values
            format_dynamic_value(value)
        }
    }

    /// Get the context prefix for an event based on its context type
    fn get_context_prefix(&self, event: &Event) -> String {
        // Suppress context markers when events are disabled (-q/--quiet)
        if self.quiet_level > 0 {
            return String::new();
        }

        match event.context_type {
            ContextType::Match => self.render_context_marker(self.colors.context_match, "◉", "*"),
            ContextType::Before => self.render_context_marker(self.colors.context_before, "/", "/"),
            ContextType::After => self.render_context_marker(self.colors.context_after, "\\", "\\"),
            ContextType::Both => self.render_context_marker(self.colors.context_overlap, "|", "|"),
            ContextType::None => String::new(),
        }
    }

    fn render_context_marker(
        &self,
        color: &'static str,
        emoji_marker: &str,
        ascii_marker: &str,
    ) -> String {
        let marker = if self.use_emoji {
            emoji_marker
        } else {
            ascii_marker
        };

        if !color.is_empty() {
            format!("{}{}{}", color, marker, self.colors.reset)
        } else {
            marker.to_string()
        }
    }

    /// Format the main content of an event while keeping wrapped lines aligned with the context marker
    fn format_content_with_context(&self, event: &Event, context_prefix: &str) -> String {
        if !self.enable_wrapping {
            // Use original single-line formatting when wrapping is disabled
            let single_line = self.format_single_line(event);
            if context_prefix.is_empty() {
                return single_line;
            } else {
                return format!("{} {}", context_prefix, single_line);
            }
        }

        // Word-wrapping implementation
        let estimated_capacity = event.fields.len() * 32;
        let mut output = String::with_capacity(estimated_capacity);

        // Add context prefix to the first line
        let mut current_line_length = 0;
        let prefix_display_length = self.display_length(context_prefix);
        if !context_prefix.is_empty() {
            output.push_str(context_prefix);
            output.push(' ');
            current_line_length = prefix_display_length + 1;
        }

        let mut first_on_line = true;
        let mut first_overall = true;

        for (key, value) in crate::event::ordered_fields(event) {
            // Build the field string first to measure its length
            let mut field_output = String::new();

            if self.brief {
                // Brief mode: only values (no keys, no quotes)
                self.format_dynamic_value_brief_into(key, value, &mut field_output);
            } else {
                // Normal mode: key=value pairs
                // Format key with color
                if !self.colors.key.is_empty() {
                    field_output.push_str(self.colors.key);
                }
                field_output.push_str(key);
                if !self.colors.key.is_empty() {
                    field_output.push_str(self.colors.reset);
                }

                // Add equals sign
                if !self.colors.equals.is_empty() {
                    field_output.push_str(self.colors.equals);
                }
                field_output.push('=');
                if !self.colors.equals.is_empty() {
                    field_output.push_str(self.colors.reset);
                }

                // Add formatted value (with proper quoting and colors)
                self.format_dynamic_value_into(key, value, &mut field_output);
            }

            // Calculate display length (ignoring ANSI escape codes)
            let field_display_length = self.display_length(&field_output);
            let space_needed = if first_on_line { 0 } else { 1 }; // Space before field

            // Check if we need to wrap (but always fit first field on first line)
            if !first_overall
                && current_line_length + space_needed + field_display_length > self.terminal_width
            {
                // Wrap: add newline, context prefix, and indentation
                output.push('\n');
                if !context_prefix.is_empty() {
                    output.push_str(&" ".repeat(prefix_display_length + 1));
                    current_line_length = prefix_display_length + 1;
                } else {
                    current_line_length = 0;
                }
                output.push_str("  "); // 2-space indentation as requested
                current_line_length += 2; // Account for indentation
                first_on_line = true;
            }

            // Add space separator if not first field on this line
            if !first_on_line {
                output.push(' ');
                current_line_length += 1;
            }

            // Add the field
            output.push_str(&field_output);
            current_line_length += field_display_length;

            first_on_line = false;
            first_overall = false;
        }

        output
    }

    /// Original single-line formatting for when wrapping is disabled
    fn format_single_line(&self, event: &Event) -> String {
        let estimated_capacity = event.fields.len() * 32;
        let mut output = String::with_capacity(estimated_capacity);
        let mut first = true;

        for (key, value) in crate::event::ordered_fields(event) {
            if !first {
                output.push(' ');
            }
            first = false;

            if self.brief {
                // Brief mode: only values (no keys, no quotes)
                self.format_dynamic_value_brief_into(key, value, &mut output);
            } else {
                // Normal mode: key=value pairs
                // Format key with color
                if !self.colors.key.is_empty() {
                    output.push_str(self.colors.key);
                }
                output.push_str(key);
                if !self.colors.key.is_empty() {
                    output.push_str(self.colors.reset);
                }

                // Add equals sign
                if !self.colors.equals.is_empty() {
                    output.push_str(self.colors.equals);
                }
                output.push('=');
                if !self.colors.equals.is_empty() {
                    output.push_str(self.colors.reset);
                }

                // Add formatted value (with proper quoting and colors)
                self.format_dynamic_value_into(key, value, &mut output);
            }
        }

        output
    }

    /// Calculate display length of a string, ignoring ANSI escape codes
    pub fn display_length(&self, text: &str) -> usize {
        let mut length = 0;
        let mut in_escape = false;

        for ch in text.chars() {
            if ch == '\x1b' {
                in_escape = true;
            } else if in_escape && ch == 'm' {
                in_escape = false;
            } else if !in_escape {
                length += 1;
            }
        }

        length
    }
}

#[cfg(test)]
impl DefaultFormatter {
    /// Test-only ctor that defaults to wrapping enabled (historical behavior)
    pub fn new(
        use_colors: bool,
        use_emoji: bool,
        brief: bool,
        timestamp_formatting: crate::config::TimestampFormatConfig,
        pretty_nested: bool,
        quiet_level: u8,
    ) -> Self {
        Self::new_with_wrapping(
            use_colors,
            use_emoji,
            brief,
            timestamp_formatting,
            true,
            pretty_nested,
            quiet_level,
        )
    }
}

impl pipeline::Formatter for DefaultFormatter {
    fn format(&self, event: &Event) -> String {
        if event.fields.is_empty() {
            return String::new();
        }

        // Add context prefix based on event context type
        let context_prefix = self.get_context_prefix(event);

        self.format_content_with_context(event, &context_prefix)
    }
}

/// Helper that tracks time gaps between events and renders markers when needed
#[derive(Debug, Clone)]
pub struct GapTracker {
    threshold: chrono::Duration,
    last_timestamp: Option<DateTime<Utc>>,
    use_colors: bool,
}

impl GapTracker {
    pub fn new(threshold: chrono::Duration, use_colors: bool) -> Self {
        Self {
            threshold,
            last_timestamp: None,
            use_colors,
        }
    }

    /// Returns a marker string if the supplied timestamp is sufficiently far from the last one
    pub fn check(&mut self, timestamp: Option<DateTime<Utc>>) -> Option<String> {
        let current_ts = timestamp?;
        let marker = self.last_timestamp.and_then(|previous_ts| {
            let diff = current_ts.signed_duration_since(previous_ts);
            if diff >= self.threshold || diff <= -self.threshold {
                Some(self.render_marker(diff))
            } else {
                None
            }
        });

        self.last_timestamp = Some(current_ts);
        marker
    }

    fn render_marker(&self, diff: chrono::Duration) -> String {
        let diff = if diff >= chrono::Duration::zero() {
            diff
        } else {
            -diff
        };

        let std_duration = diff.to_std().unwrap_or_else(|_| StdDuration::from_secs(0));

        let total_seconds = std_duration.as_secs();
        let micros = std_duration.subsec_micros();

        // Calculate time units
        let years = total_seconds / (365 * 24 * 3600);
        let remaining_after_years = total_seconds % (365 * 24 * 3600);
        let days = remaining_after_years / (24 * 3600);
        let remaining_after_days = remaining_after_years % (24 * 3600);
        let hours = remaining_after_days / 3600;
        let minutes = (remaining_after_days % 3600) / 60;
        let seconds = remaining_after_days % 60;

        // Build humanized time string
        let mut parts = Vec::new();

        if years > 0 {
            parts.push(format!(
                "{} year{}",
                years,
                if years == 1 { "" } else { "s" }
            ));
        }
        if days > 0 {
            parts.push(format!("{} day{}", days, if days == 1 { "" } else { "s" }));
        }
        if hours > 0 {
            parts.push(format!(
                "{} hour{}",
                hours,
                if hours == 1 { "" } else { "s" }
            ));
        }
        if minutes > 0 {
            parts.push(format!(
                "{} minute{}",
                minutes,
                if minutes == 1 { "" } else { "s" }
            ));
        }
        if seconds > 0 || parts.is_empty() {
            if micros > 0 {
                let mut fractional = format!("{:06}", micros);
                while fractional.ends_with('0') {
                    fractional.pop();
                }
                parts.push(format!(
                    "{}.{} second{}",
                    seconds,
                    fractional,
                    if seconds == 1 && micros == 0 { "" } else { "s" }
                ));
            } else {
                parts.push(format!(
                    "{} second{}",
                    seconds,
                    if seconds == 1 { "" } else { "s" }
                ));
            }
        }

        let time_label = parts.join(" ");
        let label = format!(" time gap: {} ", time_label);

        let blue = "\x1b[34m";
        let reset = "\x1b[0m";

        let mut width = crate::tty::get_terminal_width();
        if width == 0 {
            width = 80;
        }

        if width <= label.len() {
            let marker = label.trim().to_string();
            if self.use_colors {
                return format!("{}{}{}", blue, marker, reset);
            }
            return marker;
        }

        let remaining = width - label.len();
        let left = remaining / 2;
        let right = remaining - left;

        let mut marker = String::with_capacity(width);
        marker.push_str(&"_".repeat(left));
        marker.push_str(&label);
        marker.push_str(&"_".repeat(right));
        if self.use_colors {
            return format!("{}{}{}", blue, marker, reset);
        }
        marker
    }
}
