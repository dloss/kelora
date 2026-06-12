/// ANSI color codes for logfmt output formatting
#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub key: &'static str,             // Green for field names
    pub equals: &'static str,          // No color for = separator
    pub string: &'static str,          // No color for quoted strings
    pub level_trace: &'static str,     // Cyan for trace levels
    pub level_debug: &'static str,     // Bright cyan for debug levels
    pub level_info: &'static str,      // Bright green for info levels
    pub level_warn: &'static str,      // Bright yellow for warn levels
    pub level_error: &'static str,     // Bright red for error levels
    pub context_before: &'static str,  // Blue for context prefix before markers
    pub context_match: &'static str,   // Bright magenta for context prefix match markers
    pub context_after: &'static str,   // Blue for context prefix after markers
    pub context_overlap: &'static str, // Cyan for overlapping context markers
    pub reset: &'static str,           // Reset to default color
}

impl ColorScheme {
    /// Create color scheme for readable logfmt output
    pub fn new(use_colors: bool) -> Self {
        if use_colors {
            Self {
                key: "\x1b[32m",             // Green for field names
                equals: "",                  // No color for equals signs
                string: "",                  // No color for quoted values
                level_trace: "\x1b[36m",     // Cyan for trace/finest
                level_debug: "\x1b[96m",     // Bright cyan for debug/finer/config
                level_info: "\x1b[92m",      // Bright green for info/informational/notice
                level_warn: "\x1b[93m",      // Bright yellow for warn/warning
                level_error: "\x1b[91m",     // Bright red for error/fatal/panic/etc
                context_before: "\x1b[34m",  // Blue for before context markers
                context_match: "\x1b[95m",   // Bright magenta for match context markers
                context_after: "\x1b[34m",   // Blue for after context markers
                context_overlap: "\x1b[36m", // Cyan for overlapping context markers
                reset: "\x1b[0m",            // Reset
            }
        } else {
            // All empty strings for no-color mode
            Self {
                key: "",
                equals: "",
                string: "",
                level_trace: "",
                level_debug: "",
                level_info: "",
                level_warn: "",
                level_error: "",
                context_before: "",
                context_match: "",
                context_after: "",
                context_overlap: "",
                reset: "",
            }
        }
    }

    /// Map a log level string to its ANSI color (`""` when unrecognized).
    ///
    /// Recognizes full level words and their common synonyms, plus glog/klog's
    /// single-letter levels (`I`/`W`/`E`/`F`). The single-letter arms are scoped
    /// to glog's alphabet on purpose: coloring is cosmetic, so a wrong color is
    /// harmless, but we still avoid claiming letters glog never emits.
    pub fn level_color(&self, level: &str) -> &'static str {
        match level.to_lowercase().as_str() {
            // Error levels (incl. glog E=error, F=fatal)
            "error" | "err" | "fatal" | "panic" | "alert" | "crit" | "critical" | "emerg"
            | "emergency" | "severe" | "e" | "f" => self.level_error,
            // Warning levels (incl. glog W=warning)
            "warn" | "warning" | "w" => self.level_warn,
            // Info levels (incl. glog I=info)
            "info" | "informational" | "notice" | "i" => self.level_info,
            // Debug levels
            "debug" | "finer" | "config" => self.level_debug,
            // Trace levels
            "trace" | "finest" => self.level_trace,
            // Unknown levels get no color
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glog_single_letter_levels_match_word_levels() {
        let c = ColorScheme::new(true);
        // glog/klog emits I/W/E/F; each should color like its full-word level.
        assert_eq!(c.level_color("E"), c.level_color("ERROR"));
        assert_eq!(c.level_color("F"), c.level_color("FATAL"));
        assert_eq!(c.level_color("W"), c.level_color("WARN"));
        assert_eq!(c.level_color("I"), c.level_color("INFO"));
        // Case-insensitive, like the word arms.
        assert_eq!(c.level_color("e"), c.level_error);
    }

    #[test]
    fn unknown_and_non_glog_single_letters_stay_uncolored() {
        let c = ColorScheme::new(true);
        // glog has no debug/trace letters, so we don't claim them.
        assert_eq!(c.level_color("d"), "");
        assert_eq!(c.level_color("t"), "");
        assert_eq!(c.level_color("xyz"), "");
    }

    #[test]
    fn no_color_mode_yields_empty_for_known_levels() {
        let c = ColorScheme::new(false);
        assert_eq!(c.level_color("E"), "");
        assert_eq!(c.level_color("ERROR"), "");
    }
}
