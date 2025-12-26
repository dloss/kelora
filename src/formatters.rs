use crate::colors::ColorScheme;
use crate::event::{flatten_dynamic, Event, FlattenStyle};
use crate::pipeline;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use rhai::Dynamic;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration as StdDuration;

use once_cell::sync::Lazy;

/// Global header tracking registry for CSV formatters in parallel mode
/// Key format: "{delimiter}_{keys_hash}" for uniqueness across different CSV configurations
static CSV_FORMATTER_HEADER_REGISTRY: Lazy<Mutex<HashMap<String, bool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
use crate::pipeline::Formatter;

/// Utility function for single-quote string escaping for default formatter
/// Escapes single quotes, backslashes, newlines, tabs, and carriage returns
fn escape_single_quote_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10); // Some extra space for escapes

    for ch in input.chars() {
        match ch {
            '\'' => output.push_str("\\'"),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            _ => output.push(ch),
        }
    }

    output
}

/// Utility function for logfmt-compliant string escaping
/// Escapes quotes, backslashes, newlines, tabs, and carriage returns
fn escape_logfmt_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10); // Some extra space for escapes

    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            _ => output.push(ch),
        }
    }

    output
}

/// Check if a string value needs to be quoted per logfmt rules
fn needs_logfmt_quoting(value: &str) -> bool {
    // Quote values that contain spaces, tabs, newlines, quotes, equals, or are empty
    value.is_empty()
        || value.contains(' ')
        || value.contains('\t')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('"')
        || value.contains('=')
}

/// Format a Dynamic value as a string representation suitable for output
/// Returns (string_value, needs_quotes) - the second bool indicates if it should be quoted
fn format_dynamic_value(value: &Dynamic) -> (String, bool) {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            (s, true) // Strings can potentially need quotes
        } else {
            (value.to_string(), false)
        }
    } else {
        // Numbers, booleans, etc. - never need quotes
        (value.to_string(), false)
    }
}

/// Indent each subsequent line of a multiline string for consistent display
fn indent_multiline_value(value: &str, indent: &str) -> String {
    let mut lines = value.lines();
    match lines.next() {
        Some(first_line) => {
            let mut output = String::from(first_line);
            for line in lines {
                output.push('\n');
                output.push_str(indent);
                output.push_str(line);
            }
            output
        }
        None => String::new(),
    }
}

/// Utility to format a quoted logfmt value into a buffer
fn format_quoted_logfmt_value(value: &str, output: &mut String) {
    if needs_logfmt_quoting(value) {
        output.push('"');
        output.push_str(&escape_logfmt_string(value));
        output.push('"');
    } else {
        output.push_str(value);
    }
}

/// Convert rhai::Dynamic to serde_json::Value recursively
fn dynamic_to_json(value: &Dynamic) -> serde_json::Value {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            serde_json::Value::String(s)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_int() {
        if let Ok(i) = value.as_int() {
            serde_json::Value::Number(serde_json::Number::from(i))
        } else {
            serde_json::Value::Null
        }
    } else if value.is_float() {
        if let Ok(f) = value.as_float() {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_bool() {
        if let Ok(b) = value.as_bool() {
            serde_json::Value::Bool(b)
        } else {
            serde_json::Value::Null
        }
    } else if value.is_unit() {
        serde_json::Value::Null
    } else if let Some(arr) = value.clone().try_cast::<rhai::Array>() {
        // Convert Rhai array to JSON array recursively
        let json_array: Vec<serde_json::Value> = arr.iter().map(dynamic_to_json).collect();
        serde_json::Value::Array(json_array)
    } else if let Some(map) = value.clone().try_cast::<rhai::Map>() {
        // Convert Rhai map to JSON object recursively
        let mut json_obj = serde_json::Map::new();
        for (key, val) in map {
            json_obj.insert(key.to_string(), dynamic_to_json(&val));
        }
        serde_json::Value::Object(json_obj)
    } else {
        // For any remaining types, convert to string
        serde_json::Value::String(value.to_string())
    }
}

/// Check if a CSV value needs quoting
fn needs_csv_quoting(value: &str, delimiter: char) -> bool {
    value.is_empty()
        || value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
        || value.starts_with(' ')
        || value.ends_with(' ')
}

/// Escape CSV value with proper quoting
fn escape_csv_value(value: &str, delimiter: char) -> String {
    if needs_csv_quoting(value, delimiter) {
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

// JSON formatter
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

// Default formatter (logfmt-style with colors and brief mode)
pub struct DefaultFormatter {
    colors: ColorScheme,
    level_keys: Vec<&'static str>,
    brief: bool,
    timestamp_formatting: crate::config::TimestampFormatConfig,
    enable_wrapping: bool,
    terminal_width: usize,
    pretty_nested: bool,
    use_emoji: bool,
    quiet_level: u8,
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

impl DefaultFormatter {
    /// Get the context prefix for an event based on its context type
    fn get_context_prefix(&self, event: &Event) -> String {
        use crate::event::ContextType;

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

impl DefaultFormatter {
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
    fn display_length(&self, text: &str) -> usize {
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

// Hide formatter - suppresses all event output
pub struct HideFormatter;

impl HideFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl pipeline::Formatter for HideFormatter {
    fn format(&self, _event: &Event) -> String {
        String::new()
    }
}

// Inspect formatter - detailed, type-aware introspection output
pub struct InspectFormatter {
    max_inline_chars: usize,
}

struct LineSpec<'a> {
    indent: usize,
    name: &'a str,
    name_width: usize,
    type_width: usize,
    type_label: &'a str,
    value_repr: &'a str,
}

impl InspectFormatter {
    const KEY_WIDTH_CAP: usize = 40;

    pub fn new(verbosity: u8) -> Self {
        // Gradually relax truncation with higher verbosity levels
        let max_inline_chars = match verbosity {
            0 => 80,
            1 => 160,
            _ => usize::MAX,
        };

        Self { max_inline_chars }
    }

    fn format_entries<'a, I>(&self, lines: &mut Vec<String>, entries: I, indent: usize)
    where
        I: IntoIterator<Item = (&'a str, &'a Dynamic)>,
    {
        // Collect to compute alignment without re-iterating source data
        let collected: Vec<(&str, &Dynamic)> = entries.into_iter().collect();
        if collected.is_empty() {
            return;
        }

        let name_width = self.compute_key_width(collected.iter().map(|(k, _)| *k));
        let type_width = self.compute_type_width(collected.iter().map(|(_, v)| *v));

        for (key, value) in collected {
            self.format_entry_with_width(lines, key, value, indent, name_width, type_width);
        }
    }

    fn format_entry_with_width(
        &self,
        lines: &mut Vec<String>,
        name: &str,
        value: &Dynamic,
        indent: usize,
        name_width: usize,
        type_width: usize,
    ) {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            let entries: Vec<(String, Dynamic)> =
                map.into_iter().map(|(k, v)| (k.into(), v)).collect();
            let type_label = format!("map({})", entries.len());
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: "{",
                },
            );

            if !entries.is_empty() {
                let child_width = self.compute_key_width(entries.iter().map(|(k, _)| k.as_str()));
                let child_type_width = self.compute_type_width(entries.iter().map(|(_, v)| v));
                for (child_key, child_value) in &entries {
                    self.format_entry_with_width(
                        lines,
                        child_key,
                        child_value,
                        indent + 1,
                        child_width,
                        child_type_width,
                    );
                }
            }

            lines.push(format!("{}{}", "  ".repeat(indent), "}"));
        } else if let Some(array) = value.clone().try_cast::<rhai::Array>() {
            let elements: Vec<Dynamic> = array.into_iter().collect();
            let type_label = format!("array({})", elements.len());
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: "[",
                },
            );

            if !elements.is_empty() {
                let index_labels: Vec<String> =
                    (0..elements.len()).map(|i| format!("[{}]", i)).collect();
                let child_width = self.compute_key_width(index_labels.iter().map(|s| s.as_str()));
                let child_type_width = self.compute_type_width(elements.iter());

                for (idx, element) in elements.iter().enumerate() {
                    let child_name = &index_labels[idx];
                    self.format_entry_with_width(
                        lines,
                        child_name,
                        element,
                        indent + 1,
                        child_width,
                        child_type_width,
                    );
                }
            }

            lines.push(format!("{}{}", "  ".repeat(indent), "]"));
        } else {
            let (type_label, value_repr) = self.describe_scalar(value);
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: &value_repr,
                },
            );
        }
    }

    fn push_line(&self, lines: &mut Vec<String>, spec: LineSpec<'_>) {
        let indent_str = "  ".repeat(spec.indent);
        let name_cell = if spec.name_width > 0 {
            format!("{name:<width$}", name = spec.name, width = spec.name_width)
        } else {
            spec.name.to_string()
        };
        let effective_type_width = spec.type_width.max(spec.type_label.len());
        let type_cell = format!(
            "{type_label:<width$}",
            type_label = spec.type_label,
            width = effective_type_width
        );
        lines.push(format!(
            "{indent}{name_cell} | {type_cell} | {value}",
            indent = indent_str,
            name_cell = name_cell,
            type_cell = type_cell,
            value = spec.value_repr
        ));
    }

    fn compute_key_width<'a, I>(&self, keys: I) -> usize
    where
        I: Iterator<Item = &'a str>,
    {
        keys.map(|k| k.len())
            .max()
            .unwrap_or(0)
            .min(Self::KEY_WIDTH_CAP)
    }

    fn compute_type_width<'a, I>(&self, values: I) -> usize
    where
        I: Iterator<Item = &'a Dynamic>,
    {
        values
            .map(|value| self.type_label_for(value).len())
            .max()
            .unwrap_or(0)
    }

    fn type_label_for(&self, value: &Dynamic) -> String {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            return format!("map({})", map.len());
        }
        if let Some(array) = value.clone().try_cast::<rhai::Array>() {
            return format!("array({})", array.len());
        }
        if value.is_string() {
            return "string".to_string();
        }
        if value.is_bool() {
            return "bool".to_string();
        }
        if value.is_int() {
            return "int".to_string();
        }
        if value.is_float() {
            return "float".to_string();
        }
        if value.is_char() {
            return "char".to_string();
        }
        if value.is_unit() {
            return "null".to_string();
        }
        value.type_name().to_string()
    }

    fn describe_scalar(&self, value: &Dynamic) -> (String, String) {
        if value.is_string() {
            if let Ok(inner) = value.clone().into_string() {
                let escaped = self.escape_for_display(&inner);
                let (truncated, was_truncated) = self.truncate_value(&escaped);
                let mut rendered = format!("\"{}\"", truncated);
                if was_truncated {
                    rendered.push_str("...");
                }
                return ("string".to_string(), rendered);
            }
        }

        if value.is_bool() {
            if let Ok(b) = value.as_bool() {
                return ("bool".to_string(), b.to_string());
            }
        }

        if value.is_int() {
            if let Ok(i) = value.as_int() {
                return ("int".to_string(), i.to_string());
            }
        }

        if value.is_float() {
            if let Ok(f) = value.as_float() {
                return ("float".to_string(), format!("{f}"));
            }
        }

        if value.is_char() {
            if let Ok(c) = value.as_char() {
                return (
                    "char".to_string(),
                    format!("'{}'", self.escape_for_display(&c.to_string())),
                );
            }
        }

        if value.is_unit() {
            return ("null".to_string(), "null".to_string());
        }

        // Fallback for other scalar types
        let type_label = value.type_name().to_string();
        let rendered = self.escape_for_display(&value.to_string());
        let (truncated, was_truncated) = self.truncate_value(&rendered);
        let mut repr = truncated;
        if was_truncated {
            repr.push_str("...");
        }
        (type_label, repr)
    }

    fn escape_for_display(&self, input: &str) -> String {
        let mut escaped = String::with_capacity(input.len());
        for ch in input.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                c if c.is_control() => {
                    escaped.push_str(&format!("\\x{:02X}", c as u32));
                }
                c => escaped.push(c),
            }
        }
        escaped
    }

    fn truncate_value(&self, value: &str) -> (String, bool) {
        if self.max_inline_chars == usize::MAX || value.chars().count() <= self.max_inline_chars {
            return (value.to_string(), false);
        }

        let truncated: String = value.chars().take(self.max_inline_chars).collect();

        (truncated, true)
    }
}

impl pipeline::Formatter for InspectFormatter {
    fn format(&self, event: &Event) -> String {
        if event.fields.is_empty() {
            return "---".to_string();
        }

        let mut lines = Vec::new();
        self.format_entries(
            &mut lines,
            crate::event::ordered_fields(event)
                .into_iter()
                .map(|(k, v)| (k.as_str(), v)),
            0,
        );
        lines.insert(0, "---".to_string());
        lines.join("\n")
    }
}

/// Sanitize a field key to ensure logfmt compliance
///
/// The logfmt specification requires keys to:
/// - Not contain whitespace (spaces, tabs, newlines, carriage returns)
/// - Not contain equals signs (=) as they delimit key-value pairs
///
/// This function replaces problematic characters with underscores to maintain
/// field information while ensuring valid logfmt output. This approach:
/// - Preserves all field data (no information loss)
/// - Produces parseable logfmt output
/// - Maintains deterministic key mapping
/// - Handles edge cases gracefully
///
/// # Examples
/// - sanitize_logfmt_key("field with spaces") -> "field_with_spaces"
/// - sanitize_logfmt_key("field=with=equals") -> "field_with_equals"  
/// - sanitize_logfmt_key("normal_field") -> "normal_field"
fn sanitize_logfmt_key(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            c => c,
        })
        .collect()
}

// Logfmt formatter - strict logfmt output formatter (no colors, no brief mode)
//
// This formatter produces logfmt-compliant output by:
// 1. Sanitizing field keys to replace invalid characters with underscores
// 2. Properly quoting string values that contain special characters
// 3. Leaving numeric and boolean values unquoted
//
// Key sanitization ensures that output can be parsed by standard logfmt parsers,
// including Kelora's own logfmt input parser.
pub struct LogfmtFormatter;

impl LogfmtFormatter {
    pub fn new() -> Self {
        Self
    }

    /// Format a Dynamic value directly into buffer for performance
    fn format_dynamic_value_into(&self, value: &Dynamic, output: &mut String) {
        let string_val = self.format_logfmt_value(value);
        let is_string = value.is_string();

        if is_string {
            format_quoted_logfmt_value(&string_val, output);
        } else {
            output.push_str(&string_val);
        }
    }

    /// Format a Dynamic value for logfmt output, flattening nested structures
    fn format_logfmt_value(&self, value: &Dynamic) -> String {
        // Check if this is a complex nested structure
        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            // Flatten nested structures using underscore style for logfmt safety
            let flattened = flatten_dynamic(value, FlattenStyle::Underscore, 0);

            if flattened.len() == 1 {
                // Single flattened value - use it directly
                flattened.values().next().unwrap().to_string()
            } else if flattened.is_empty() {
                // Empty structure
                String::new()
            } else {
                // Multiple flattened values - create a compact representation
                // Format as "key1=val1,key2=val2" for logfmt-style readability
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        } else {
            // Simple scalar value
            value.to_string()
        }
    }
}

impl pipeline::Formatter for LogfmtFormatter {
    fn format(&self, event: &Event) -> String {
        if event.fields.is_empty() {
            return String::new();
        }

        // Pre-allocate buffer with estimated capacity
        let estimated_capacity = event.fields.len() * 32;
        let mut output = String::with_capacity(estimated_capacity);
        let mut first = true;

        for (key, value) in crate::event::ordered_fields(event) {
            if !first {
                output.push(' ');
            }
            first = false;

            // Strict logfmt: key=value pairs with sanitized keys
            // Keys are sanitized to ensure logfmt compliance (no spaces, equals signs, etc.)
            let sanitized_key = sanitize_logfmt_key(key);
            output.push_str(&sanitized_key);
            output.push('=');
            self.format_dynamic_value_into(value, &mut output);
        }

        output
    }
}

struct LevelmapState {
    current_timestamp: Option<String>,
    buffer: String,
    visible_len: usize,
}

impl LevelmapState {
    fn new(initial_capacity: usize) -> Self {
        let base_capacity = initial_capacity.max(1) * 4;
        Self {
            current_timestamp: None,
            buffer: String::with_capacity(base_capacity),
            visible_len: 0,
        }
    }

    fn reset(&mut self) {
        self.current_timestamp = None;
        self.buffer.clear();
        self.visible_len = 0;
    }

    fn push_rendered(&mut self, rendered: &str) {
        self.buffer.push_str(rendered);
        self.visible_len += 1;
    }
}

pub struct LevelmapFormatter {
    state: Mutex<LevelmapState>,
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
            state: Mutex::new(LevelmapState::new(terminal_width)),
            terminal_width,
            buffer_width_override: None,
            colors: ColorScheme::new(use_colors),
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(LevelmapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            colors: ColorScheme::new(false),
        }
    }

    fn format_line(timestamp: Option<&String>, buffer: &str) -> String {
        match timestamp {
            Some(ts) if !ts.is_empty() => format!("{} {}", ts, buffer),
            _ => buffer.to_string(),
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
                if let Some(level) = Self::dynamic_to_trimmed_string(value) {
                    return Some(level);
                }
            }
        }
        None
    }

    fn dynamic_to_trimmed_string(value: &Dynamic) -> Option<String> {
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

    fn extract_timestamp(event: &Event) -> String {
        if let Some(ts) = event.parsed_ts {
            return Self::format_timestamp(ts);
        }

        for key in crate::event::TIMESTAMP_FIELD_NAMES {
            if let Some(value) = event.fields.get(*key) {
                if let Some(ts) = value.clone().try_cast::<DateTime<Utc>>() {
                    return Self::format_timestamp(ts);
                }

                if let Some(ts) = value.clone().try_cast::<DateTime<FixedOffset>>() {
                    return Self::format_timestamp(ts.with_timezone(&Utc));
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

    fn format_timestamp(ts: DateTime<Utc>) -> String {
        ts.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl pipeline::Formatter for LevelmapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self
            .state
            .lock()
            .expect("levelmap formatter mutex poisoned");

        if state.current_timestamp.is_none() {
            state.current_timestamp = Some(Self::extract_timestamp(event));
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
            let line = Self::format_line(state.current_timestamp.as_ref(), &state.buffer);
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

        let line = Self::format_line(state.current_timestamp.as_ref(), &state.buffer);
        state.reset();

        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
}

struct KeymapState {
    current_timestamp: Option<String>,
    buffer: String,
    visible_len: usize,
}

impl KeymapState {
    fn new(initial_capacity: usize) -> Self {
        let base_capacity = initial_capacity.max(1) * 4;
        Self {
            current_timestamp: None,
            buffer: String::with_capacity(base_capacity),
            visible_len: 0,
        }
    }

    fn reset(&mut self) {
        self.current_timestamp = None;
        self.buffer.clear();
        self.visible_len = 0;
    }

    fn push_rendered(&mut self, rendered: &str) {
        self.buffer.push_str(rendered);
        self.visible_len += 1;
    }
}

pub struct KeymapFormatter {
    state: Mutex<KeymapState>,
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
            state: Mutex::new(KeymapState::new(terminal_width)),
            terminal_width,
            buffer_width_override: None,
            field_name: field_name.unwrap_or_else(|| "level".to_string()),
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize, field_name: Option<String>) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(KeymapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            field_name: field_name.unwrap_or_else(|| "level".to_string()),
        }
    }

    fn format_line(timestamp: Option<&String>, buffer: &str) -> String {
        match timestamp {
            Some(ts) if !ts.is_empty() => format!("{} {}", ts, buffer),
            _ => buffer.to_string(),
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
            Self::dynamic_to_trimmed_string(value)
        } else {
            None
        }
    }

    fn dynamic_to_trimmed_string(value: &Dynamic) -> Option<String> {
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

    fn extract_timestamp(event: &Event) -> String {
        if let Some(ts) = event.parsed_ts {
            return Self::format_timestamp(ts);
        }

        for key in crate::event::TIMESTAMP_FIELD_NAMES {
            if let Some(value) = event.fields.get(*key) {
                if let Some(ts) = value.clone().try_cast::<DateTime<Utc>>() {
                    return Self::format_timestamp(ts);
                }

                if let Some(ts) = value.clone().try_cast::<DateTime<FixedOffset>>() {
                    return Self::format_timestamp(ts.with_timezone(&Utc));
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

    fn format_timestamp(ts: DateTime<Utc>) -> String {
        ts.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl pipeline::Formatter for KeymapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self.state.lock().expect("keymap formatter mutex poisoned");

        if state.current_timestamp.is_none() {
            state.current_timestamp = Some(Self::extract_timestamp(event));
        }

        let available_width = self.available_width(state.current_timestamp.as_ref());

        let field_string = self.extract_field_string(event);
        let display_char = field_string
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('.');
        state.push_rendered(&display_char.to_string());

        if state.visible_len >= available_width {
            let line = Self::format_line(state.current_timestamp.as_ref(), &state.buffer);
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

        let line = Self::format_line(state.current_timestamp.as_ref(), &state.buffer);
        state.reset();

        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
}

// CSV formatter - outputs CSV format with required field order
pub struct CsvFormatter {
    delimiter: char,
    keys: Vec<String>,
    include_header: bool,
    formatter_key: String,
    worker_mode: bool, // If true, never write headers (for parallel workers)
}

impl CsvFormatter {
    pub fn new(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: true,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_tsv(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: true,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_csv_no_header(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_noheader_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_tsv_no_header(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_noheader_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: false,
        }
    }

    /// Create worker-mode variants that never write headers
    pub fn new_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false, // Workers never write headers
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_tsv_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false, // Workers never write headers
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_csv_no_header_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_noheader_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_tsv_no_header_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_noheader_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: true,
        }
    }

    /// Create a simple hash of the keys for uniqueness
    fn keys_hash(keys: &[String]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        keys.hash(&mut hasher);
        hasher.finish()
    }

    /// Mark header as written globally for this formatter configuration
    /// Returns true if this call was the first to mark it (header should be written)
    fn mark_header_written_globally(&self) -> bool {
        let mut registry = CSV_FORMATTER_HEADER_REGISTRY.lock().unwrap();
        if registry.get(&self.formatter_key).copied().unwrap_or(false) {
            // Already marked by another thread
            false
        } else {
            // This is the first thread to mark it
            registry.insert(self.formatter_key.clone(), true);
            true
        }
    }

    /// Format the header row
    pub fn format_header(&self) -> String {
        self.keys
            .iter()
            .map(|key| escape_csv_value(key, self.delimiter))
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    /// Format a data row
    fn format_data_row(&self, event: &Event) -> String {
        self.keys
            .iter()
            .map(|key| {
                if let Some(value) = event.fields.get(key) {
                    let string_value = self.format_csv_value(value);
                    escape_csv_value(&string_value, self.delimiter)
                } else {
                    String::new() // Empty field for missing values
                }
            })
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    /// Format a Dynamic value for CSV output, flattening nested structures
    fn format_csv_value(&self, value: &Dynamic) -> String {
        // Check if this is a complex nested structure
        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            // Flatten nested structures using underscore style for CSV safety
            let flattened = flatten_dynamic(value, FlattenStyle::Underscore, 0);

            if flattened.len() == 1 {
                // Single flattened value - use it directly
                flattened.values().next().unwrap().to_string()
            } else if flattened.is_empty() {
                // Empty structure
                String::new()
            } else {
                // Multiple flattened values - create a compact representation
                // Format as "key1:val1,key2:val2" for readability in CSV cells
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        } else {
            // Simple scalar value
            value.to_string()
        }
    }
}

impl pipeline::Formatter for CsvFormatter {
    fn format(&self, event: &Event) -> String {
        let mut output = String::new();

        // Write header row if needed (thread-safe, once only across all workers)
        // Workers in parallel mode never write headers
        if !self.worker_mode && self.include_header && self.mark_header_written_globally() {
            output.push_str(&self.format_header());
            output.push('\n');
        }

        // Write data row
        output.push_str(&self.format_data_row(event));
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ContextType;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use rhai::{Array, Map};

    fn parts(line: &str) -> Vec<String> {
        line.split('|')
            .map(|segment| segment.trim().to_string())
            .collect()
    }

    #[test]
    fn test_json_formatter_empty_event() {
        let event = Event::default();
        let formatter = JsonFormatter::new();
        let result = formatter.format(&event);
        assert!(result.starts_with('{') && result.ends_with('}'));
    }

    #[test]
    fn test_json_formatter_with_fields() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
        event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
        event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
        event.set_field("status".to_string(), Dynamic::from(200i64));

        let formatter = JsonFormatter::new();
        let result = formatter.format(&event);

        assert!(result.contains("\"level\":\"INFO\""));
        assert!(result.contains("\"msg\":\"Test message\""));
        assert!(result.contains("\"user\":\"alice\""));
        assert!(result.contains("\"status\":200"));
    }

    #[test]
    fn test_inspect_formatter_basic() {
        let mut event = Event::default();
        event.set_field("message".to_string(), Dynamic::from("hello"));
        event.set_field("code".to_string(), Dynamic::from(42_i64));
        event.set_field("active".to_string(), Dynamic::from(true));

        let formatter = InspectFormatter::new(0);
        let output = formatter.format(&event);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "---");
        assert_eq!(lines.len(), 4);
        assert_eq!(
            parts(lines[1]),
            vec![
                "message".to_string(),
                "string".to_string(),
                "\"hello\"".to_string()
            ]
        );
        assert_eq!(
            parts(lines[2]),
            vec!["code".to_string(), "int".to_string(), "42".to_string()]
        );
        assert_eq!(
            parts(lines[3]),
            vec!["active".to_string(), "bool".to_string(), "true".to_string()]
        );
    }

    #[test]
    fn test_inspect_formatter_nested_structure() {
        let mut inner = Map::new();
        inner.insert("id".into(), Dynamic::from(7_i64));
        inner.insert("name".into(), Dynamic::from("alpha"));

        let mut event = Event::default();
        event.set_field("meta".to_string(), Dynamic::from(inner));

        let formatter = InspectFormatter::new(0);
        let output = formatter.format(&event);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "---");
        assert_eq!(lines.len(), 5);
        assert_eq!(
            parts(lines[1]),
            vec!["meta".to_string(), "map(2)".to_string(), "{".to_string()]
        );
        assert_eq!(
            parts(lines[2]),
            vec!["id".to_string(), "int".to_string(), "7".to_string()]
        );
        assert_eq!(
            parts(lines[3]),
            vec![
                "name".to_string(),
                "string".to_string(),
                "\"alpha\"".to_string()
            ]
        );
        assert_eq!(lines[4], "}");
    }

    #[test]
    fn test_inspect_formatter_truncates_long_values() {
        let long_value = "a".repeat(120);
        let mut event = Event::default();
        event.set_field("payload".to_string(), Dynamic::from(long_value.clone()));

        let formatter = InspectFormatter::new(0);
        let output = formatter.format(&event);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "---");
        assert_eq!(lines.len(), 2);
        let expected_truncated = format!("\"{}\"...", "a".repeat(80));
        assert_eq!(
            parts(lines[1]),
            vec![
                "payload".to_string(),
                "string".to_string(),
                expected_truncated.clone()
            ]
        );

        let verbose_formatter = InspectFormatter::new(2);
        let verbose_output = verbose_formatter.format(&event);
        let verbose_lines: Vec<&str> = verbose_output.lines().collect();
        assert_eq!(verbose_lines[0], "---");
        assert_eq!(verbose_lines.len(), 2);
        let expected_full = format!("\"{}\"", long_value);
        assert_eq!(
            parts(verbose_lines[1]),
            vec!["payload".to_string(), "string".to_string(), expected_full]
        );
        assert!(verbose_output.len() > output.len());
    }

    #[test]
    fn test_default_formatter() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
        event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
        event.set_field("count".to_string(), Dynamic::from(42i64));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            false, // Disable wrapping for this test
            false,
            0, // No quiet mode
        ); // No colors, no brief mode, no wrapping
        let result = formatter.format(&event);

        // Check that all fields are present with proper formatting
        // Strings should be quoted with single quotes, numbers should not be
        assert!(result.contains("level='INFO'"));
        assert!(result.contains("user='alice'"));
        assert!(result.contains("count=42"));
        // Fields should be space-separated
        assert!(result.contains(" "));
    }

    #[test]
    fn test_default_formatter_uses_ts_format_hint() {
        let mut event = Event::default();
        event.set_field("ts".to_string(), Dynamic::from("2000/01/01 17.59.55,210"));
        event.set_field("msg".to_string(), Dynamic::from("hello"));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig {
                format_fields: Vec::new(),
                auto_format_all: true,
                format_as_utc: true,
                parse_format_hint: Some("%Y/%m/%d %H.%M.%S,%f".to_string()),
                parse_timezone_hint: Some("UTC".to_string()),
            },
            false,
            false,
            0,
        );

        let result = formatter.format(&event);
        assert!(result.contains("ts='2000-01-01T17:59:55.210+00:00'"));
    }

    #[test]
    fn test_default_formatter_nested_values_render_as_json() {
        let mut meta = Map::new();
        meta.insert("id".into(), Dynamic::from(7_i64));
        meta.insert("name".into(), Dynamic::from("alpha"));

        let tags: Array = vec![Dynamic::from("blue"), Dynamic::from("green")];

        let mut event = Event::default();
        event.set_field("meta".to_string(), Dynamic::from(meta));
        event.set_field("tags".to_string(), Dynamic::from(tags));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            false,
            false,
            0, // No quiet mode
        );
        let result = formatter.format(&event);

        assert!(result.contains("meta={\"id\":7,\"name\":\"alpha\"}"));
        assert!(result.contains("tags=[\"blue\",\"green\"]"));
    }

    #[test]
    fn test_default_formatter_pretty_nested_output() {
        let mut meta = Map::new();
        meta.insert("id".into(), Dynamic::from(7_i64));
        meta.insert("name".into(), Dynamic::from("alpha"));

        let tags: Array = vec![Dynamic::from("blue"), Dynamic::from("green")];

        let mut event = Event::default();
        event.set_field("meta".to_string(), Dynamic::from(meta));
        event.set_field("tags".to_string(), Dynamic::from(tags));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            false,
            true,
            0, // No quiet mode
        );
        let result = formatter.format(&event);

        assert!(result.contains("meta={\n    \"id\": 7,\n    \"name\": \"alpha\"\n  }"));
        assert!(result.contains("tags=[\n    \"blue\",\n    \"green\"\n  ]"));
    }

    #[test]
    fn test_default_formatter_brief_mode() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("info".to_string()));
        event.set_field("msg".to_string(), Dynamic::from("test message".to_string()));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            true,
            crate::config::TimestampFormatConfig::default(),
            false, // Disable wrapping for this test
            false,
            0, // No quiet mode
        ); // No colors, brief mode, no wrapping
        let result = formatter.format(&event);

        // Brief mode should output only values, space-separated
        assert_eq!(result, "info test message");
    }

    #[test]
    fn test_context_markers_use_emoji_when_enabled() {
        let formatter = DefaultFormatter::new_with_wrapping(
            true,
            true,
            false,
            crate::config::TimestampFormatConfig::default(),
            false,
            false,
            0, // No quiet mode
        );

        let mut before_event = Event {
            context_type: ContextType::Before,
            ..Default::default()
        };
        before_event.set_field("msg".to_string(), Dynamic::from("before".to_string()));
        let before_line = formatter.format(&before_event);
        assert!(before_line.starts_with("\x1b[34m/\x1b[0m "));

        let mut match_event = Event {
            context_type: ContextType::Match,
            ..Default::default()
        };
        match_event.set_field("msg".to_string(), Dynamic::from("match".to_string()));
        let match_line = formatter.format(&match_event);
        assert!(match_line.starts_with("\x1b[95m◉\x1b[0m "));

        let mut after_event = Event {
            context_type: ContextType::After,
            ..Default::default()
        };
        after_event.set_field("msg".to_string(), Dynamic::from("after".to_string()));
        let after_line = formatter.format(&after_event);
        assert!(after_line.starts_with("\x1b[34m\\\x1b[0m "));

        let mut overlap_event = Event {
            context_type: ContextType::Both,
            ..Default::default()
        };
        overlap_event.set_field("msg".to_string(), Dynamic::from("overlap".to_string()));
        let overlap_line = formatter.format(&overlap_event);
        assert!(overlap_line.starts_with("\x1b[36m|\x1b[0m "));
    }

    #[test]
    fn test_logfmt_formatter_basic() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
        event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
        event.set_field("user".to_string(), Dynamic::from("alice".to_string()));
        event.set_field("status".to_string(), Dynamic::from(200i64));

        let formatter = LogfmtFormatter::new();
        let result = formatter.format(&event);

        // Should properly quote strings with spaces, leave numbers unquoted
        assert!(result.contains("level=INFO"));
        assert!(result.contains("msg=\"Test message\""));
        assert!(result.contains("user=alice"));
        assert!(result.contains("status=200"));
        // Fields should be space-separated
        assert!(result.contains(" "));
    }

    #[test]
    fn test_logfmt_formatter_quoting() {
        let mut event = Event::default();
        event.set_field("simple".to_string(), Dynamic::from("value".to_string()));
        event.set_field(
            "spaced".to_string(),
            Dynamic::from("has spaces".to_string()),
        );
        event.set_field("empty".to_string(), Dynamic::from("".to_string()));
        event.set_field(
            "quoted".to_string(),
            Dynamic::from("has\"quotes".to_string()),
        );
        event.set_field("equals".to_string(), Dynamic::from("has=sign".to_string()));

        let formatter = LogfmtFormatter::new();
        let result = formatter.format(&event);

        assert!(result.contains("simple=value")); // No quotes needed
        assert!(result.contains("spaced=\"has spaces\"")); // Quotes due to space
        assert!(result.contains("empty=\"\"")); // Quotes due to empty
        assert!(result.contains("quoted=\"has\\\"quotes\"")); // Escaped quotes
        assert!(result.contains("equals=\"has=sign\"")); // Quotes due to equals sign
    }

    #[test]
    fn test_logfmt_formatter_types() {
        let mut event = Event::default();
        event.set_field("string".to_string(), Dynamic::from("hello".to_string()));
        event.set_field("integer".to_string(), Dynamic::from(42i64));
        event.set_field("float".to_string(), Dynamic::from(2.5f64));
        event.set_field("bool_true".to_string(), Dynamic::from(true));
        event.set_field("bool_false".to_string(), Dynamic::from(false));

        let formatter = LogfmtFormatter::new();
        let result = formatter.format(&event);

        // Numbers and booleans should not be quoted
        assert!(result.contains("string=hello"));
        assert!(result.contains("integer=42"));
        assert!(result.contains("float=2.5"));
        assert!(result.contains("bool_true=true"));
        assert!(result.contains("bool_false=false"));
    }

    #[test]
    fn test_logfmt_formatter_empty_event() {
        let event = Event::default();
        let formatter = LogfmtFormatter::new();
        let result = formatter.format(&event);
        assert_eq!(result, "");
    }

    #[test]
    fn test_logfmt_formatter_key_sanitization() {
        let mut event = Event::default();
        // Test various problematic key characters
        event.set_field(
            "field with spaces".to_string(),
            Dynamic::from("value1".to_string()),
        );
        event.set_field(
            "field=with=equals".to_string(),
            Dynamic::from("value2".to_string()),
        );
        event.set_field(
            "field\twith\ttabs".to_string(),
            Dynamic::from("value3".to_string()),
        );
        event.set_field(
            "field\nwith\nnewlines".to_string(),
            Dynamic::from("value4".to_string()),
        );
        event.set_field(
            "field\rwith\rcarriage".to_string(),
            Dynamic::from("value5".to_string()),
        );
        event.set_field(
            "normal_field".to_string(),
            Dynamic::from("value6".to_string()),
        );
        event.set_field(
            "field-with-dashes".to_string(),
            Dynamic::from("value7".to_string()),
        );
        event.set_field(
            "field.with.dots".to_string(),
            Dynamic::from("value8".to_string()),
        );

        let formatter = LogfmtFormatter::new();
        let result = formatter.format(&event);

        // Keys should be sanitized by replacing problematic characters with underscores
        assert!(result.contains("field_with_spaces=value1"));
        assert!(result.contains("field_with_equals=value2"));
        assert!(result.contains("field_with_tabs=value3"));
        assert!(result.contains("field_with_newlines=value4"));
        assert!(result.contains("field_with_carriage=value5"));
        assert!(result.contains("normal_field=value6"));

        // Non-problematic characters should be preserved
        assert!(result.contains("field-with-dashes=value7"));
        assert!(result.contains("field.with.dots=value8"));

        // Ensure the result can be parsed by the logfmt parser
        let parser = crate::parsers::logfmt::LogfmtParser::new();
        let parsed = crate::pipeline::EventParser::parse(&parser, &result);
        assert!(
            parsed.is_ok(),
            "Sanitized logfmt output should be parseable: {}",
            result
        );

        let parsed_event = parsed.unwrap();
        // Verify that sanitized keys preserve the data
        assert_eq!(
            parsed_event
                .fields
                .get("field_with_spaces")
                .unwrap()
                .to_string(),
            "value1"
        );
        assert_eq!(
            parsed_event
                .fields
                .get("field_with_equals")
                .unwrap()
                .to_string(),
            "value2"
        );
        assert_eq!(
            parsed_event.fields.get("normal_field").unwrap().to_string(),
            "value6"
        );
    }

    #[test]
    fn test_sanitize_logfmt_key_function() {
        // Test the sanitize_logfmt_key function directly
        assert_eq!(sanitize_logfmt_key("normal_field"), "normal_field");
        assert_eq!(
            sanitize_logfmt_key("field with spaces"),
            "field_with_spaces"
        );
        assert_eq!(
            sanitize_logfmt_key("field=with=equals"),
            "field_with_equals"
        );
        assert_eq!(sanitize_logfmt_key("field\twith\ttabs"), "field_with_tabs");
        assert_eq!(
            sanitize_logfmt_key("field\nwith\nnewlines"),
            "field_with_newlines"
        );
        assert_eq!(
            sanitize_logfmt_key("field\rwith\rcarriage"),
            "field_with_carriage"
        );
        assert_eq!(
            sanitize_logfmt_key("field-with-dashes"),
            "field-with-dashes"
        );
        assert_eq!(sanitize_logfmt_key("field.with.dots"), "field.with.dots");
        assert_eq!(
            sanitize_logfmt_key("field_with_underscores"),
            "field_with_underscores"
        );
        assert_eq!(sanitize_logfmt_key(""), "");

        // Test edge cases
        assert_eq!(sanitize_logfmt_key("==="), "___");
        assert_eq!(sanitize_logfmt_key("   "), "___");
        assert_eq!(sanitize_logfmt_key(" = \t = \n = \r "), "_____________");
    }

    #[test]
    fn test_levelmap_formatter_emits_full_line() {
        let formatter = LevelmapFormatter::with_width(3);
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        let mut event1 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event1.set_field("level".to_string(), Dynamic::from("info"));
        assert!(formatter.format(&event1).is_empty());

        let mut event2 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event2.set_field("level".to_string(), Dynamic::from("debug"));
        assert!(formatter.format(&event2).is_empty());

        let mut event3 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event3.set_field("level".to_string(), Dynamic::from("trace"));
        let line = formatter.format(&event3);
        assert_eq!(line, "1970-01-01T00:00:00.000Z idt");

        assert!(formatter.finish().is_none());

        let ts2 = Utc.timestamp_millis_opt(1_000).unwrap();
        let mut event4 = Event {
            parsed_ts: Some(ts2),
            ..Event::default()
        };
        event4.set_field("level".to_string(), Dynamic::from("warn"));
        assert!(formatter.format(&event4).is_empty());

        let trailing = formatter
            .finish()
            .expect("should flush trailing levelmap line");
        assert_eq!(trailing, "1970-01-01T00:00:01.000Z w");
    }

    #[test]
    fn test_levelmap_formatter_unknown_level() {
        let formatter = LevelmapFormatter::with_width(1);
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        let event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };

        let line = formatter.format(&event);
        assert_eq!(line, "1970-01-01T00:00:00.000Z ?");
    }

    #[test]
    fn test_keymap_formatter_emits_full_line() {
        let formatter = KeymapFormatter::with_width(3, Some("status".to_string()));
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        let mut event1 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event1.set_field("status".to_string(), Dynamic::from("ok"));
        assert!(formatter.format(&event1).is_empty());

        let mut event2 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event2.set_field("status".to_string(), Dynamic::from("error"));
        assert!(formatter.format(&event2).is_empty());

        let mut event3 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event3.set_field("status".to_string(), Dynamic::from("warn"));
        let line = formatter.format(&event3);
        assert_eq!(line, "1970-01-01T00:00:00.000Z oew");

        assert!(formatter.finish().is_none());

        let ts2 = Utc.timestamp_millis_opt(1_000).unwrap();
        let mut event4 = Event {
            parsed_ts: Some(ts2),
            ..Event::default()
        };
        event4.set_field("status".to_string(), Dynamic::from("pending"));
        assert!(formatter.format(&event4).is_empty());

        let trailing = formatter
            .finish()
            .expect("should flush trailing keymap line");
        assert_eq!(trailing, "1970-01-01T00:00:01.000Z p");
    }

    #[test]
    fn test_keymap_formatter_empty_field() {
        let formatter = KeymapFormatter::with_width(1, Some("status".to_string()));
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        let event = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };

        let line = formatter.format(&event);
        assert_eq!(line, "1970-01-01T00:00:00.000Z .");
    }

    #[test]
    fn test_keymap_formatter_custom_field() {
        let formatter = KeymapFormatter::with_width(4, Some("method".to_string()));
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        let mut event1 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event1.set_field("method".to_string(), Dynamic::from("GET"));
        assert!(formatter.format(&event1).is_empty());

        let mut event2 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event2.set_field("method".to_string(), Dynamic::from("POST"));
        assert!(formatter.format(&event2).is_empty());

        let mut event3 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event3.set_field("method".to_string(), Dynamic::from("PUT"));
        assert!(formatter.format(&event3).is_empty());

        let mut event4 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event4.set_field("method".to_string(), Dynamic::from("DELETE"));
        let line = formatter.format(&event4);
        assert_eq!(line, "1970-01-01T00:00:00.000Z GPPD");
    }

    #[test]
    fn test_keymap_formatter_non_string_fields() {
        let formatter = KeymapFormatter::with_width(5, Some("value".to_string()));
        let ts = Utc.timestamp_millis_opt(0).unwrap();

        // Test with integer
        let mut event1 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event1.set_field("value".to_string(), Dynamic::from(42_i64));
        assert!(formatter.format(&event1).is_empty());

        // Test with float
        let mut event2 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event2.set_field("value".to_string(), Dynamic::from(9.87));
        assert!(formatter.format(&event2).is_empty());

        // Test with boolean true
        let mut event3 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event3.set_field("value".to_string(), Dynamic::from(true));
        assert!(formatter.format(&event3).is_empty());

        // Test with boolean false
        let mut event4 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event4.set_field("value".to_string(), Dynamic::from(false));
        assert!(formatter.format(&event4).is_empty());

        // Test with negative number
        let mut event5 = Event {
            parsed_ts: Some(ts),
            ..Event::default()
        };
        event5.set_field("value".to_string(), Dynamic::from(-99_i64));
        let line = formatter.format(&event5);
        // Should show: 4, 9, t, f, - (first chars of "42", "9.87", "true", "false", "-99")
        assert_eq!(line, "1970-01-01T00:00:00.000Z 49tf-");
    }

    #[test]
    fn test_hide_formatter() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
        event.set_field("msg".to_string(), Dynamic::from("Test message".to_string()));
        event.set_field("user".to_string(), Dynamic::from("alice".to_string()));

        let formatter = HideFormatter::new();
        let result = formatter.format(&event);
        assert_eq!(result, "");
    }

    #[test]
    fn test_hide_formatter_empty_event() {
        let event = Event::default();
        let formatter = HideFormatter::new();
        let result = formatter.format(&event);
        assert_eq!(result, "");
    }

    #[test]
    fn test_null_formatter_behavior() {
        // Null format uses HideFormatter, so test that it produces empty strings
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("ERROR".to_string()));
        event.set_field(
            "msg".to_string(),
            Dynamic::from("Critical error".to_string()),
        );

        let formatter = HideFormatter::new(); // Null format uses HideFormatter
        let result = formatter.format(&event);
        assert_eq!(result, ""); // Should be empty for null format
    }

    #[test]
    fn test_shared_escaping_utilities() {
        // Test escape_logfmt_string
        assert_eq!(escape_logfmt_string("simple"), "simple");
        assert_eq!(escape_logfmt_string("with\"quotes"), "with\\\"quotes");
        assert_eq!(escape_logfmt_string("with\nnewline"), "with\\nnewline");
        assert_eq!(escape_logfmt_string("with\ttab"), "with\\ttab");
        assert_eq!(escape_logfmt_string("with\\backslash"), "with\\\\backslash");

        // Test needs_logfmt_quoting
        assert!(!needs_logfmt_quoting("simple"));
        assert!(needs_logfmt_quoting("with spaces"));
        assert!(needs_logfmt_quoting(""));
        assert!(needs_logfmt_quoting("with=equals"));
        assert!(needs_logfmt_quoting("with\"quotes"));
        assert!(needs_logfmt_quoting("with\ttab"));

        // Test format_dynamic_value
        assert_eq!(
            format_dynamic_value(&Dynamic::from("test")),
            ("test".to_string(), true)
        );
        assert_eq!(
            format_dynamic_value(&Dynamic::from(42i64)),
            ("42".to_string(), false)
        );
        assert_eq!(
            format_dynamic_value(&Dynamic::from(true)),
            ("true".to_string(), false)
        );
    }

    #[test]
    fn test_csv_formatter_basic() {
        let keys = vec!["name".to_string(), "age".to_string(), "city".to_string()];
        let formatter = CsvFormatter::new(keys);

        let mut event = Event::default();
        event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
        event.set_field("age".to_string(), Dynamic::from(25i64));
        event.set_field("city".to_string(), Dynamic::from("New York".to_string()));

        let result = formatter.format(&event);

        // Should include header and data
        assert!(result.contains("name,age,city"));
        assert!(result.contains("Alice,25,New York"));
    }

    #[test]
    fn test_csv_formatter_with_quoting() {
        let keys = vec!["name".to_string(), "msg".to_string()];
        let formatter = CsvFormatter::new(keys);

        let mut event = Event::default();
        event.set_field("name".to_string(), Dynamic::from("Smith, John".to_string()));
        event.set_field(
            "msg".to_string(),
            Dynamic::from("He said \"hello\"".to_string()),
        );

        let result = formatter.format(&event);

        // Should properly quote values with commas and quotes
        assert!(result.contains("\"Smith, John\""));
        assert!(result.contains("\"He said \"\"hello\"\"\""));
    }

    #[test]
    fn test_tsv_formatter_basic() {
        let keys = vec!["name".to_string(), "age".to_string()];
        let formatter = CsvFormatter::new_tsv(keys);

        let mut event = Event::default();
        event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
        event.set_field("age".to_string(), Dynamic::from(25i64));

        let result = formatter.format(&event);

        // Should use tab separator
        assert!(result.contains("name\tage"));
        assert!(result.contains("Alice\t25"));
    }

    #[test]
    fn test_csv_formatter_no_header() {
        let keys = vec!["name".to_string(), "age".to_string()];
        let formatter = CsvFormatter::new_csv_no_header(keys);

        let mut event = Event::default();
        event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
        event.set_field("age".to_string(), Dynamic::from(25i64));

        let result = formatter.format(&event);

        // Should not include header
        assert!(!result.contains("name,age"));
        assert_eq!(result, "Alice,25");
    }

    #[test]
    fn test_csv_formatter_missing_fields() {
        let keys = vec!["name".to_string(), "age".to_string(), "city".to_string()];
        let formatter = CsvFormatter::new_csv_no_header(keys);

        let mut event = Event::default();
        event.set_field("name".to_string(), Dynamic::from("Alice".to_string()));
        // age is missing
        event.set_field("city".to_string(), Dynamic::from("Boston".to_string()));

        let result = formatter.format(&event);

        // Should have empty field for missing age
        assert_eq!(result, "Alice,,Boston");
    }

    #[test]
    fn test_csv_escaping_utilities() {
        // Test needs_csv_quoting
        assert!(!needs_csv_quoting("simple", ','));
        assert!(needs_csv_quoting("with,comma", ','));
        assert!(needs_csv_quoting("with\"quote", ','));
        assert!(needs_csv_quoting("with\nnewline", ','));
        assert!(needs_csv_quoting("", ','));
        assert!(needs_csv_quoting(" leading", ','));
        assert!(needs_csv_quoting("trailing ", ','));

        // Test with tab delimiter
        assert!(!needs_csv_quoting("with,comma", '\t'));
        assert!(needs_csv_quoting("with\ttab", '\t'));

        // Test escape_csv_value
        assert_eq!(escape_csv_value("simple", ','), "simple");
        assert_eq!(escape_csv_value("with,comma", ','), "\"with,comma\"");
        assert_eq!(escape_csv_value("with\"quote", ','), "\"with\"\"quote\"");
        assert_eq!(escape_csv_value("", ','), "\"\"");
    }

    #[test]
    fn test_default_formatter_wrapping_disabled() {
        let mut event = Event::default();
        event.set_field("level".to_string(), Dynamic::from("INFO".to_string()));
        event.set_field(
            "message".to_string(),
            Dynamic::from("This is a very long message that would normally wrap".to_string()),
        );
        event.set_field("user".to_string(), Dynamic::from("alice".to_string()));

        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            false, // wrapping disabled
            false,
            0, // No quiet mode
        );
        let result = formatter.format(&event);

        // Should be single line when wrapping is disabled
        assert!(!result.contains('\n'));
        assert!(result.contains("level='INFO'"));
        assert!(result.contains("message='This is a very long message that would normally wrap'"));
        assert!(result.contains("user='alice'"));
    }

    #[test]
    fn test_default_formatter_wrapping_enabled() {
        let mut event = Event::default();
        event.set_field("field1".to_string(), Dynamic::from("value1".to_string()));
        event.set_field("field2".to_string(), Dynamic::from("value2".to_string()));
        event.set_field(
            "very_long_field_name".to_string(),
            Dynamic::from(
                "a very long field value that will definitely cause wrapping".to_string(),
            ),
        );
        event.set_field("field4".to_string(), Dynamic::from("value4".to_string()));

        // Override terminal width for consistent testing
        let formatter = DefaultFormatter {
            colors: crate::colors::ColorScheme::new(false),
            level_keys: vec!["level"],
            brief: false,
            timestamp_formatting: crate::config::TimestampFormatConfig::default(),
            enable_wrapping: true,
            terminal_width: 50, // Small width to force wrapping
            pretty_nested: false,
            use_emoji: false,
            quiet_level: 0, // No quiet mode
        };

        let result = formatter.format(&event);

        // Should wrap when width is exceeded
        assert!(result.contains('\n'));
        assert!(result.contains("  ")); // Should have indentation

        // All fields should still be present
        assert!(result.contains("field1='value1'"));
        assert!(result.contains("field2='value2'"));
        assert!(result.contains(
            "very_long_field_name='a very long field value that will definitely cause wrapping'"
        ));
        assert!(result.contains("field4='value4'"));
    }

    #[test]
    fn test_default_formatter_wrapping_brief_mode() {
        let mut event = Event::default();
        event.set_field("field1".to_string(), Dynamic::from("short".to_string()));
        event.set_field(
            "field2".to_string(),
            Dynamic::from("this is a much longer value that should cause wrapping".to_string()),
        );
        event.set_field("field3".to_string(), Dynamic::from("end".to_string()));

        let formatter = DefaultFormatter {
            colors: crate::colors::ColorScheme::new(false),
            level_keys: vec![],
            brief: true,
            timestamp_formatting: crate::config::TimestampFormatConfig::default(),
            enable_wrapping: true,
            terminal_width: 30, // Very small width
            pretty_nested: false,
            use_emoji: false,
            quiet_level: 0, // No quiet mode
        };

        let result = formatter.format(&event);

        // Brief mode should still wrap properly
        assert!(result.contains('\n'));
        assert!(result.contains("  ")); // Should have indentation

        // In brief mode, only values are shown (no key= parts)
        assert!(result.contains("short"));
        assert!(result.contains("this is a much longer value that should cause wrapping"));
        assert!(result.contains("end"));
        assert!(!result.contains("field1="));
        assert!(!result.contains("field2="));
        assert!(!result.contains("field3="));
    }

    #[test]
    fn test_display_length_ignores_ansi_codes() {
        let formatter = DefaultFormatter::new_with_wrapping(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            true,
            false,
            0, // No quiet mode
        );

        // Test string with ANSI color codes
        let colored_text = "\x1b[31mred text\x1b[0m";
        assert_eq!(formatter.display_length(colored_text), 8); // "red text" = 8 chars

        let plain_text = "red text";
        assert_eq!(formatter.display_length(plain_text), 8);

        // Empty string
        assert_eq!(formatter.display_length(""), 0);

        // Only ANSI codes
        assert_eq!(formatter.display_length("\x1b[31m\x1b[0m"), 0);
    }

    #[test]
    fn test_wrapping_preserves_field_boundaries() {
        let mut event = Event::default();
        event.set_field("a".to_string(), Dynamic::from("value".to_string()));
        event.set_field("b".to_string(), Dynamic::from("value".to_string()));
        event.set_field("c".to_string(), Dynamic::from("value".to_string()));

        let formatter = DefaultFormatter {
            colors: crate::colors::ColorScheme::new(false),
            level_keys: vec![],
            brief: false,
            timestamp_formatting: crate::config::TimestampFormatConfig::default(),
            enable_wrapping: true,
            terminal_width: 20, // Force wrapping
            pretty_nested: false,
            use_emoji: false,
            quiet_level: 0, // No quiet mode
        };

        let result = formatter.format(&event);

        // Should never break within a field, only between fields
        assert!(!result.contains("a='val\n  ue'")); // Would be bad
        assert!(result.contains("a='value'")); // Should be complete

        // Should have proper line structure
        let lines: Vec<&str> = result.split('\n').collect();
        assert!(lines.len() > 1); // Should have multiple lines

        // Continuation lines should be indented
        for (i, line) in lines.iter().enumerate() {
            if i > 0 && !line.is_empty() {
                assert!(
                    line.starts_with("  "),
                    "Line {} should be indented: '{}'",
                    i,
                    line
                );
            }
        }
    }

    #[test]
    fn test_default_formatter_new_constructor_enables_wrapping_by_default() {
        let mut event = Event::default();
        event.set_field("field1".to_string(), Dynamic::from("value1".to_string()));
        event.set_field(
            "very_long_field_name_that_exceeds_width".to_string(),
            Dynamic::from(
                "a very long field value that should definitely cause wrapping in most terminals"
                    .to_string(),
            ),
        );
        event.set_field("field3".to_string(), Dynamic::from("value3".to_string()));

        // Use the basic constructor (should enable wrapping by default now)
        let mut formatter = DefaultFormatter::new(
            false,
            false,
            false,
            crate::config::TimestampFormatConfig::default(),
            false,
            0, // No quiet mode
        );

        // Default constructor should have wrapping enabled
        assert!(formatter.enable_wrapping);

        // Force a small terminal width to make wrapping deterministic in tests
        formatter.terminal_width = 80;

        let result = formatter.format(&event);

        // Should wrap by default now
        assert!(
            result.contains('\n'),
            "Default constructor should enable wrapping"
        );
        assert!(
            result.contains("  "),
            "Should have indentation when wrapping"
        );

        // All fields should still be present
        assert!(result.contains("field1='value1'"));
        assert!(result.contains("very_long_field_name_that_exceeds_width="));
        assert!(result.contains("field3='value3'"));
    }

    #[test]
    fn test_gap_tracker_inserts_marker_for_large_delta() {
        let mut tracker = GapTracker::new(ChronoDuration::minutes(30), false);

        let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
        let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());

        assert!(tracker.check(first).is_none());
        let marker = tracker.check(second).expect("marker line");
        assert!(marker.starts_with('_'));
        assert!(marker.ends_with('_'));
        assert!(marker.contains("time gap: 2 hours"));
    }

    #[test]
    fn test_gap_tracker_skips_small_delta() {
        let mut tracker = GapTracker::new(ChronoDuration::hours(2), false);

        let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
        let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 12, 0, 0).unwrap());

        assert!(tracker.check(first).is_none());
        assert!(tracker.check(second).is_none());
    }

    #[test]
    fn test_gap_tracker_handles_missing_timestamp() {
        let mut tracker = GapTracker::new(ChronoDuration::minutes(45), false);

        assert!(tracker.check(None).is_none());

        let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 12, 0, 0).unwrap());
        assert!(tracker.check(second).is_none());

        let third = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());
        let marker = tracker.check(third).expect("marker line");
        assert!(marker.contains("time gap: 1 hour"));
        assert!(marker.starts_with('_'));
    }

    #[test]
    fn test_gap_tracker_handles_reverse_order() {
        let mut tracker = GapTracker::new(ChronoDuration::milliseconds(1), false);

        let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
        let earlier = Some(Utc.with_ymd_and_hms(2024, 2, 5, 10, 59, 59).unwrap());

        assert!(tracker.check(first).is_none());
        let marker = tracker.check(earlier).expect("marker for backwards jump");
        assert!(marker.contains("time gap"));
    }

    #[test]
    fn test_gap_tracker_colors_marker_when_enabled() {
        let mut tracker = GapTracker::new(ChronoDuration::minutes(30), true);

        let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
        let second = Some(Utc.with_ymd_and_hms(2024, 2, 5, 13, 0, 0).unwrap());

        assert!(tracker.check(first).is_none());
        let marker = tracker.check(second).expect("colored marker");
        assert!(marker.contains("\x1b[34m"));
        assert!(marker.contains("\x1b[0m"));
        assert!(marker.starts_with("\x1b[34m_"));
        let reset_index = marker.rfind("\x1b[0m").expect("reset sequence");
        assert!(marker[..reset_index].ends_with('_'));
        assert!(marker.contains("time gap: 2 hours"));
    }

    #[test]
    fn test_gap_tracker_formats_fractional_microseconds_compactly() {
        let mut tracker = GapTracker::new(ChronoDuration::milliseconds(1), false);

        let first = Some(Utc.with_ymd_and_hms(2024, 2, 5, 11, 0, 0).unwrap());
        let second = first.map(|ts| ts + ChronoDuration::microseconds(1_230_000));

        assert!(tracker.check(first).is_none());
        let marker = tracker.check(second).expect("fractional marker");
        assert!(marker.contains("time gap: 1.23 seconds"));
    }
}
