//! Display-width helpers shared by the summary modes that render aligned
//! tables (`--discover`, `--drain-diff`).
//!
//! Column alignment has to count *display* columns, not bytes or `char`s, or a
//! CJK field name silently shifts every column to its right. These live here
//! rather than in one mode's module so both use the same definition.

use unicode_width::UnicodeWidthStr;

/// Width assumed when output is redirected and `COLUMNS` says nothing. Wide
/// enough that a table written to a file keeps its long columns intact instead
/// of being truncated to a guess about someone's terminal.
pub const REDIRECTED_TABLE_WIDTH: usize = 200;

/// The width a summary table should lay itself out for.
///
/// An explicit `COLUMNS` wins even when redirected — it is the only way to ask
/// for a specific width in a pipeline — otherwise a terminal is measured and a
/// redirect falls back to [`REDIRECTED_TABLE_WIDTH`].
pub fn output_width() -> usize {
    if crate::tty::is_stdout_tty() {
        crate::tty::get_terminal_width()
    } else {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&c| c > 0)
            .unwrap_or(REDIRECTED_TABLE_WIDTH)
    }
}

/// Truncate a string to `max_chars` with an ellipsis suffix, preserving valid
/// UTF-8 boundaries. `ellipsis` is the suffix to append when truncation occurs
/// (`…` normally, `...` under `--no-emoji`).
pub fn truncate_for_display(s: &str, max_chars: usize, ellipsis: &str) -> String {
    let ell_width = ellipsis.chars().count();
    if max_chars <= ell_width {
        return ".".repeat(max_chars);
    }
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }

    let keep = max_chars - ell_width;
    let mut out = s.chars().take(keep).collect::<String>();
    out.push_str(ellipsis);
    out
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn pad_right_display(s: &str, width: usize) -> String {
    let current = display_width(s);
    if current >= width {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(width - current))
}

pub fn pad_left_display(s: &str, width: usize) -> String {
    let current = display_width(s);
    if current >= width {
        return s.to_string();
    }
    format!("{}{s}", " ".repeat(width - current))
}

/// Punctuation glyphs chosen by whether Unicode output is allowed
/// (`--no-emoji` falls back to ASCII).
#[derive(Debug, Clone, Copy)]
pub struct Glyphs {
    /// Truncation / "more values exist" marker: `…` or `...`.
    pub ellipsis: &'static str,
    /// Placeholder for a value that does not exist: `—` or `-`.
    pub em_dash: &'static str,
    /// Multiplication sign in a rate multiple (`14× more`): `×` or `x`.
    pub times: &'static str,
}

impl Glyphs {
    pub fn new(use_unicode: bool) -> Self {
        if use_unicode {
            Self {
                ellipsis: "\u{2026}",
                em_dash: "\u{2014}",
                times: "\u{d7}",
            }
        } else {
            Self {
                ellipsis: "...",
                em_dash: "-",
                times: "x",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_appends_ellipsis_only_when_needed() {
        assert_eq!(truncate_for_display("abcdef", 10, "…"), "abcdef");
        assert_eq!(truncate_for_display("abcdef", 4, "…"), "abc…");
        // Degenerate widths must not panic or overflow.
        assert_eq!(truncate_for_display("abcdef", 1, "…"), ".");
        assert_eq!(truncate_for_display("abcdef", 0, "…"), "");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let s = "häuser-überlauf";
        let out = truncate_for_display(s, 6, "…");
        assert_eq!(out, "häuse…");
    }

    #[test]
    fn padding_uses_display_width() {
        assert_eq!(pad_left_display("7", 3), "  7");
        assert_eq!(pad_right_display("ab", 4), "ab  ");
        // Already at or over the target width is returned untouched.
        assert_eq!(pad_left_display("abcd", 2), "abcd");
    }

    #[test]
    fn glyphs_fall_back_to_ascii() {
        let ascii = Glyphs::new(false);
        assert_eq!(ascii.ellipsis, "...");
        assert_eq!(ascii.times, "x");
        let unicode = Glyphs::new(true);
        assert_eq!(unicode.ellipsis, "\u{2026}");
        assert_eq!(unicode.times, "\u{d7}");
    }
}
