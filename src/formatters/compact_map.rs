use crate::colors::ColorScheme;
use crate::event::Event;
use crate::pipeline;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use rhai::Dynamic;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// Maximum number of distinct labels listed per legend glyph before truncating.
const MAX_LEGEND_LABELS: usize = 6;
/// Maximum displayed length of a single legend label.
const MAX_LEGEND_LABEL_LEN: usize = 32;

/// Accumulates the mapping from a displayed glyph to the source values that
/// produced it, so a data-driven legend can be rendered when the stream ends.
#[derive(Default)]
struct LegendAccumulator {
    entries: BTreeMap<char, LegendBucket>,
}

#[derive(Default)]
struct LegendBucket {
    labels: BTreeSet<String>,
    truncated: bool,
}

impl LegendAccumulator {
    fn record(&mut self, glyph: char, label: &str) {
        let bucket = self.entries.entry(glyph).or_default();
        if bucket.labels.len() < MAX_LEGEND_LABELS || bucket.labels.contains(label) {
            bucket.labels.insert(truncate_label(label));
        } else {
            bucket.truncated = true;
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Render a single-line legend, e.g. `🔹 E = error | I = info | W = warn`.
    /// `color_for_label` returns the ANSI color for a label ("" = no color);
    /// `reset` is the ANSI reset sequence (ignored when no color is applied).
    fn render(
        &self,
        use_emoji: bool,
        reset: &str,
        color_for_label: impl Fn(&str) -> &'static str,
    ) -> Option<String> {
        if self.is_empty() {
            return None;
        }

        let wrap = |s: &str, color: &str| {
            if color.is_empty() {
                s.to_string()
            } else {
                format!("{color}{s}{reset}")
            }
        };

        let mut parts = Vec::with_capacity(self.entries.len());
        for (glyph, bucket) in &self.entries {
            let glyph_color = bucket
                .labels
                .iter()
                .next()
                .map(|label| color_for_label(label))
                .unwrap_or("");
            let labels: Vec<String> = bucket
                .labels
                .iter()
                .map(|label| wrap(label, color_for_label(label)))
                .collect();
            let mut joined = labels.join(",");
            if bucket.truncated {
                joined.push_str(",…");
            }
            parts.push(format!(
                "{} = {}",
                wrap(&glyph.to_string(), glyph_color),
                joined
            ));
        }

        let prefix = if use_emoji { "🔹 " } else { "" };
        Some(format!("{prefix}{}", parts.join(" | ")))
    }
}

/// Truncate an over-long legend label so a single noisy value can't blow up the line.
fn truncate_label(label: &str) -> String {
    if label.chars().count() > MAX_LEGEND_LABEL_LEN {
        let head: String = label.chars().take(MAX_LEGEND_LABEL_LEN - 1).collect();
        format!("{head}…")
    } else {
        label.to_string()
    }
}

// Shared state and utilities for compact map formatters (levelmap, keymap)
struct CompactMapState {
    current_timestamp: Option<String>,
    buffer: String,
    visible_len: usize,
    legend: LegendAccumulator,
}

impl CompactMapState {
    fn new(initial_capacity: usize) -> Self {
        let base_capacity = initial_capacity.max(1) * 4;
        Self {
            current_timestamp: None,
            buffer: String::with_capacity(base_capacity),
            visible_len: 0,
            legend: LegendAccumulator::default(),
        }
    }

    // Reset only the per-line state; the legend accumulates across the whole run.
    fn reset(&mut self) {
        self.current_timestamp = None;
        self.buffer.clear();
        self.visible_len = 0;
    }

    fn push_rendered(&mut self, rendered: &str) {
        self.buffer.push_str(rendered);
        self.visible_len += 1;
    }

    /// Combine the trailing partial line with the optional legend block.
    fn finish_with_legend(
        &mut self,
        trailing: Option<String>,
        legend: Option<String>,
    ) -> Option<String> {
        match (trailing, legend) {
            (Some(t), Some(l)) => Some(format!("{t}\n\n{l}")),
            (Some(t), None) => Some(t),
            (None, Some(l)) => Some(format!("\n{l}")),
            (None, None) => None,
        }
    }
}

// Shared utility functions for compact map formatters
pub(super) mod compact_map_utils {
    use super::*;

    pub(super) fn dynamic_to_trimmed_string(value: &Dynamic) -> Option<String> {
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

    pub(crate) fn format_line(timestamp: Option<&String>, buffer: &str) -> String {
        match timestamp {
            Some(ts) if !ts.is_empty() => format!("{} {}", ts, buffer),
            _ => buffer.to_string(),
        }
    }

    pub(crate) fn extract_timestamp(event: &Event) -> String {
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

    pub(super) fn format_timestamp(ts: DateTime<Utc>) -> String {
        ts.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

pub struct LevelmapFormatter {
    state: Mutex<CompactMapState>,
    terminal_width: usize,
    buffer_width_override: Option<usize>,
    colors: ColorScheme,
    use_emoji: bool,
    show_legend: bool,
}

impl LevelmapFormatter {
    const FALLBACK_TERMINAL_WIDTH: usize = 80;

    pub fn new(use_colors: bool, use_emoji: bool, show_legend: bool) -> Self {
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
            use_emoji,
            show_legend,
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize) -> Self {
        Self::with_width_and_legend(width, false, false)
    }

    #[cfg(test)]
    pub fn with_width_and_legend(width: usize, use_colors: bool, show_legend: bool) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(CompactMapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            colors: ColorScheme::new(use_colors),
            use_emoji: false,
            show_legend,
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
                if let Some(level) = compact_map_utils::dynamic_to_trimmed_string(value) {
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

    fn level_color(&self, level: &str) -> &'static str {
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
            state.current_timestamp = Some(compact_map_utils::extract_timestamp(event));
        }

        let available_width = self.available_width(state.current_timestamp.as_ref());

        let level_string = Self::extract_level_string(event);
        let display_char = level_string
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('?');
        if self.show_legend {
            let label = level_string.as_deref().unwrap_or("(none)");
            state.legend.record(display_char, label);
        }
        let rendered = self.render_level_char(level_string.as_deref(), display_char);
        state.push_rendered(&rendered);

        if state.visible_len >= available_width {
            let line =
                compact_map_utils::format_line(state.current_timestamp.as_ref(), &state.buffer);
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

        let trailing = if state.visible_len > 0 {
            let line =
                compact_map_utils::format_line(state.current_timestamp.as_ref(), &state.buffer);
            (!line.is_empty()).then_some(line)
        } else {
            None
        };
        state.reset();

        let legend = if self.show_legend {
            state
                .legend
                .render(self.use_emoji, self.colors.reset, |label| {
                    self.level_color(label)
                })
        } else {
            None
        };

        state.finish_with_legend(trailing, legend)
    }
}

pub struct KeymapFormatter {
    state: Mutex<CompactMapState>,
    terminal_width: usize,
    buffer_width_override: Option<usize>,
    field_name: String,
    use_emoji: bool,
    show_legend: bool,
}

impl KeymapFormatter {
    const FALLBACK_TERMINAL_WIDTH: usize = 80;

    pub fn new(field_name: Option<String>, use_emoji: bool, show_legend: bool) -> Self {
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
            use_emoji,
            show_legend,
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize, field_name: Option<String>) -> Self {
        Self::with_width_and_legend(width, field_name, false)
    }

    #[cfg(test)]
    pub fn with_width_and_legend(
        width: usize,
        field_name: Option<String>,
        show_legend: bool,
    ) -> Self {
        let effective_width = width.max(1);
        Self {
            state: Mutex::new(CompactMapState::new(effective_width)),
            terminal_width: effective_width,
            buffer_width_override: Some(effective_width),
            field_name: field_name.unwrap_or_else(|| "level".to_string()),
            use_emoji: false,
            show_legend,
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
            compact_map_utils::dynamic_to_trimmed_string(value)
        } else {
            None
        }
    }
}

impl pipeline::Formatter for KeymapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self.state.lock().expect("keymap formatter mutex poisoned");

        if state.current_timestamp.is_none() {
            state.current_timestamp = Some(compact_map_utils::extract_timestamp(event));
        }

        let available_width = self.available_width(state.current_timestamp.as_ref());

        let field_string = self.extract_field_string(event);
        let display_char = field_string
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('.');
        if self.show_legend {
            let label = field_string.as_deref().unwrap_or("(missing)");
            state.legend.record(display_char, label);
        }
        state.push_rendered(&display_char.to_string());

        if state.visible_len >= available_width {
            let line =
                compact_map_utils::format_line(state.current_timestamp.as_ref(), &state.buffer);
            state.reset();
            line
        } else {
            String::new()
        }
    }

    fn finish(&self) -> Option<String> {
        let mut state = self.state.lock().expect("keymap formatter mutex poisoned");

        let trailing = if state.visible_len > 0 {
            let line =
                compact_map_utils::format_line(state.current_timestamp.as_ref(), &state.buffer);
            (!line.is_empty()).then_some(line)
        } else {
            None
        };
        state.reset();

        // keymap glyphs are uncolored, so the legend stays plain too.
        let legend = if self.show_legend {
            state.legend.render(self.use_emoji, "", |_| "")
        } else {
            None
        };

        state.finish_with_legend(trailing, legend)
    }
}

pub(super) use compact_map_utils as utils;
