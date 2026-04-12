#[derive(Debug, Clone)]
pub struct DebugConfig {
    pub verbosity: u8,
    pub show_timing: bool,
    pub trace_events: bool,
    pub use_emoji: bool,
}

impl DebugConfig {
    pub fn new(verbose_count: u8) -> Self {
        DebugConfig {
            verbosity: verbose_count,
            show_timing: verbose_count >= 1,
            trace_events: verbose_count >= 2,
            use_emoji: true, // Default to true, will be overridden
        }
    }

    pub fn with_emoji(mut self, use_emoji: bool) -> Self {
        self.use_emoji = use_emoji;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.verbosity > 0
    }
}
