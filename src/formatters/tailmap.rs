use crate::event::Event;
use crate::pipeline;

use super::compact_map::utils as compact_map_utils;

use std::sync::Mutex;
use tdigests::TDigest;

// Tailmap formatter - visualizes numeric field distribution focused on tail latencies
// Uses percentile-based bucketing: '_' (< p90), '1' (p90-p95), '2' (p95-p99), '3' (>= p99), '.' (missing)
struct TailmapEntry {
    timestamp: String,
    value: Option<f64>,
}

pub struct TailmapFormatter {
    state: Mutex<TailmapState>,
    terminal_width: usize,
    buffer_width_override: Option<usize>,
    field_name: String,
    emoji_mode: crate::config::EmojiMode,
    color_mode: crate::config::ColorMode,
    show_legend: bool,
}

struct TailmapState {
    entries: Vec<TailmapEntry>,
    digest: Option<TDigest>,
}

impl TailmapState {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            digest: None,
        }
    }
}

impl TailmapFormatter {
    const FALLBACK_TERMINAL_WIDTH: usize = 80;

    pub fn new(
        field_name: Option<String>,
        emoji_mode: crate::config::EmojiMode,
        color_mode: crate::config::ColorMode,
        show_legend: bool,
    ) -> Self {
        let detected_width = crate::tty::get_terminal_width();
        let terminal_width = if detected_width == 0 {
            Self::FALLBACK_TERMINAL_WIDTH
        } else {
            detected_width
        };

        Self {
            state: Mutex::new(TailmapState::new()),
            terminal_width,
            buffer_width_override: None,
            field_name: field_name.unwrap_or_else(|| "value".to_string()),
            emoji_mode,
            color_mode,
            show_legend,
        }
    }

    #[cfg(test)]
    pub fn with_width(width: usize, field_name: Option<String>) -> Self {
        Self {
            state: Mutex::new(TailmapState::new()),
            terminal_width: 80,
            buffer_width_override: Some(width),
            field_name: field_name.unwrap_or_else(|| "value".to_string()),
            emoji_mode: crate::config::EmojiMode::Never,
            color_mode: crate::config::ColorMode::Never,
            show_legend: true,
        }
    }

    #[cfg(test)]
    pub fn with_width_and_legend(
        width: usize,
        field_name: Option<String>,
        show_legend: bool,
    ) -> Self {
        Self {
            state: Mutex::new(TailmapState::new()),
            terminal_width: 80,
            buffer_width_override: Some(width),
            field_name: field_name.unwrap_or_else(|| "value".to_string()),
            emoji_mode: crate::config::EmojiMode::Never,
            color_mode: crate::config::ColorMode::Never,
            show_legend,
        }
    }

    fn available_width(&self, timestamp: Option<&String>) -> usize {
        if let Some(override_width) = self.buffer_width_override {
            return override_width;
        }

        let timestamp_len = timestamp.map(|ts| ts.len() + 1).unwrap_or(0);

        if self.terminal_width > timestamp_len {
            self.terminal_width - timestamp_len
        } else {
            1
        }
    }

    fn extract_numeric_value(&self, event: &Event) -> Option<f64> {
        event.fields.get(&self.field_name).and_then(|value| {
            if value.is_float() {
                value.as_float().ok()
            } else if value.is_int() {
                value.as_int().ok().map(|i| i as f64)
            } else {
                None
            }
        })
    }

    fn value_to_bucket(&self, value: f64, digest: &TDigest) -> char {
        if !value.is_finite() {
            return '.';
        }

        // Tail-focused percentile thresholds: p90, p95, p99
        let p90 = digest.estimate_quantile(0.90);
        let p95 = digest.estimate_quantile(0.95);
        let p99 = digest.estimate_quantile(0.99);

        if value < p90 {
            '_' // Bottom 90% - normal
        } else if value < p95 {
            '1' // p90-p95 - slow
        } else if value < p99 {
            '2' // p95-p99 - slower
        } else {
            '3' // >= p99 - worst
        }
    }
}

impl pipeline::Formatter for TailmapFormatter {
    fn format(&self, event: &Event) -> String {
        let mut state = self.state.lock().expect("tailmap formatter mutex poisoned");

        let timestamp = compact_map_utils::extract_timestamp(event);
        let value = self.extract_numeric_value(event);

        if let Some(v) = value {
            if v.is_finite() {
                let new_digest = TDigest::from_values(vec![v]);
                state.digest = Some(if let Some(existing) = state.digest.take() {
                    existing.merge(&new_digest)
                } else {
                    new_digest
                });
            }
        }

        state.entries.push(TailmapEntry { timestamp, value });

        String::new()
    }

    fn finish(&self) -> Option<String> {
        let state = self.state.lock().expect("tailmap formatter mutex poisoned");

        if state.entries.is_empty() {
            return None;
        }

        // Need digest to compute percentiles
        let Some(ref digest) = state.digest else {
            // No valid values to compute percentiles from - all missing
            return None;
        };

        let mut output = String::new();
        let mut current_timestamp: Option<String> = None;
        let mut buffer = String::new();
        let mut visible_len = 0;

        for entry in &state.entries {
            if current_timestamp.is_none() {
                current_timestamp = Some(entry.timestamp.clone());
            }

            let available_width = self.available_width(current_timestamp.as_ref());

            let display_char = if let Some(value) = entry.value {
                self.value_to_bucket(value, digest)
            } else {
                '.'
            };

            buffer.push(display_char);
            visible_len += 1;

            if visible_len >= available_width {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&compact_map_utils::format_line(
                    current_timestamp.as_ref(),
                    &buffer,
                ));
                buffer.clear();
                visible_len = 0;
                current_timestamp = Some(entry.timestamp.clone());
            }
        }

        if visible_len > 0 {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&compact_map_utils::format_line(
                current_timestamp.as_ref(),
                &buffer,
            ));
        }

        if output.is_empty() {
            return None;
        }

        if !self.show_legend {
            return Some(output);
        }

        // Add legend with percentile thresholds
        let p90 = digest.estimate_quantile(0.90);
        let p95 = digest.estimate_quantile(0.95);
        let p99 = digest.estimate_quantile(0.99);
        let min = digest.estimate_quantile(0.0);
        let max = digest.estimate_quantile(1.0);

        let valid_count = state.entries.iter().filter(|e| e.value.is_some()).count();

        // Add emoji prefix if enabled
        let use_emoji = crate::tty::should_use_emoji_with_mode(&self.emoji_mode, &self.color_mode);
        let prefix = if use_emoji { "🔹 " } else { "" };

        output.push_str(&format!(
            "\n\n{}{}: {} events, range {:.1} to {:.1}, p90={:.1}, p95={:.1}, p99={:.1}\n_ = below p90 | 1 = p90-p95 | 2 = p95-p99 | 3 = above p99 | . = missing",
            prefix, self.field_name, valid_count, min, max, p90, p95, p99
        ));

        Some(output)
    }
}
