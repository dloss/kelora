//! Hide formatter
//!
//! Suppresses all event output. Used for null/hide output format.

use crate::event::Event;
use crate::pipeline;

/// Hide formatter - suppresses all event output
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
