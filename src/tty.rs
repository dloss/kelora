use crate::config::{ColorMode, EmojiMode};
use std::io::IsTerminal;

/// Check if stdout is connected to a TTY
pub fn is_stdout_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Check if stdin is connected to a TTY
pub fn is_stdin_tty() -> bool {
    std::io::stdin().is_terminal()
}

/// Check if stderr is connected to a TTY
pub fn is_stderr_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// Determine if colors should be used based on CLI color mode and environment
pub fn should_use_colors_with_mode(color_mode: &ColorMode) -> bool {
    match color_mode {
        ColorMode::Never => false,
        ColorMode::Always => {
            // --force-color should override NO_COLOR environment variable
            true
        }
        ColorMode::Auto => should_use_colors_auto(),
    }
}

/// Auto color detection logic
fn should_use_colors_auto() -> bool {
    // Check FORCE_COLOR first for CI environments that support colors
    if std::env::var("FORCE_COLOR").is_ok() {
        return true;
    }

    // Don't use colors if not on TTY
    if !is_stdout_tty() {
        return false;
    }

    // Respect NO_COLOR environment variable (https://no-color.org/)
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Default: use colors for TTY
    true
}

/// Auto color detection for stderr messages (errors/warnings)
pub fn should_use_colors_for_stderr() -> bool {
    // Check FORCE_COLOR first for CI environments that support colors
    if std::env::var("FORCE_COLOR").is_ok() {
        return true;
    }

    // Don't use colors if stderr not on TTY
    if !is_stderr_tty() {
        return false;
    }

    // Respect NO_COLOR environment variable (https://no-color.org/)
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Default: use colors for TTY
    true
}

/// Determine if emoji should be used based on emoji mode, color settings, and environment
pub fn should_use_emoji_with_mode(emoji_mode: &EmojiMode, color_mode: &ColorMode) -> bool {
    match emoji_mode {
        EmojiMode::Never => false,
        EmojiMode::Always => true,
        EmojiMode::Auto => {
            // Emoji requires colors to be enabled
            let use_colors = should_use_colors_with_mode(color_mode);
            if !use_colors {
                return false;
            }

            // Check NO_EMOJI environment variable
            if std::env::var("NO_EMOJI").is_ok() {
                return false;
            }

            // Default: use emoji when colors are enabled
            true
        }
    }
}

/// Auto emoji detection for stderr messages (based on color detection)
pub fn should_use_emoji_for_stderr() -> bool {
    // Emoji requires colors to be enabled
    let use_colors = should_use_colors_for_stderr();
    if !use_colors {
        return false;
    }

    // Check NO_EMOJI environment variable
    if std::env::var("NO_EMOJI").is_ok() {
        return false;
    }

    // Default: use emoji when colors are enabled
    true
}

/// Get terminal width for word-wrapping, with fallback to default width
/// Checks in order: COLUMNS environment variable, terminal size detection, default (100)
pub fn get_terminal_width() -> usize {
    // Priority 1: Check COLUMNS environment variable
    if let Ok(columns_str) = std::env::var("COLUMNS") {
        if let Ok(columns) = columns_str.parse::<usize>() {
            if columns > 0 {
                return columns;
            }
        }
    }

    // Priority 2: Detect terminal size
    if let Some((terminal_size::Width(width), _)) = terminal_size::terminal_size() {
        width as usize
    } else {
        // Priority 3: Default fallback width
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            let vars = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            Self { vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.vars {
                if let Some(v) = value {
                    std::env::set_var(key, v);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    fn with_env_lock<F: FnOnce()>(keys: &[&'static str], f: F) {
        let _lock = crate::test_env::lock_env();
        let _guard = EnvGuard::new(keys);
        f();
    }

    #[test]
    fn color_mode_never_disables_colors() {
        assert!(!should_use_colors_with_mode(&ColorMode::Never));
    }

    #[test]
    fn color_mode_always_overrides_no_color_environment() {
        with_env_lock(&["NO_COLOR"], || {
            std::env::set_var("NO_COLOR", "1");
            assert!(should_use_colors_with_mode(&ColorMode::Always));
        });
    }

    #[test]
    fn terminal_width_uses_columns_env_var() {
        with_env_lock(&["COLUMNS"], || {
            std::env::set_var("COLUMNS", "120");
            assert_eq!(get_terminal_width(), 120);
        });
    }

    #[test]
    fn terminal_width_ignores_invalid_columns() {
        with_env_lock(&["COLUMNS"], || {
            // Non-numeric value should be ignored
            std::env::set_var("COLUMNS", "invalid");
            let width = get_terminal_width();
            // Should fall back to terminal size detection or default (100)
            // We can't assert the exact value as it depends on actual terminal size
            assert!(width > 0);
        });
    }

    #[test]
    fn terminal_width_ignores_zero_columns() {
        with_env_lock(&["COLUMNS"], || {
            std::env::set_var("COLUMNS", "0");
            let width = get_terminal_width();
            // Should fall back to terminal size detection or default (100)
            assert!(width > 0);
        });
    }

    #[test]
    fn terminal_width_fallback_when_columns_unset() {
        with_env_lock(&["COLUMNS"], || {
            std::env::remove_var("COLUMNS");
            let width = get_terminal_width();
            // Should use terminal size detection or default fallback
            assert!(width > 0);
        });
    }
}
