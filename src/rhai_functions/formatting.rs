use rhai::Engine;
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Process-wide flag governing whether color helpers emit ANSI escape
/// sequences. Defaults to false (disabled) so that library users and tests
/// get uncolored output unless explicitly opted in. The runner calls
/// `set_colors_enabled()` at startup based on --color / NO_COLOR / TTY.
static COLORS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable ANSI color output from Rhai color helpers. Called
/// once at pipeline startup by the runner.
pub fn set_colors_enabled(enabled: bool) {
    COLORS_ENABLED.store(enabled, Ordering::Relaxed);
}

fn colors_enabled() -> bool {
    COLORS_ENABLED.load(Ordering::Relaxed)
}

const RESET: &str = "\x1b[0m";

fn wrap(text: &str, code: &str) -> String {
    if colors_enabled() {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn register_functions(engine: &mut Engine) {
    // human_bytes: format byte count with binary (IEC) units (B, KiB, MiB, ...).
    // human_bytes_si: format byte count with decimal (SI) units (B, KB, MB, ...).
    engine.register_fn("human_bytes", |n: i64| -> String {
        human_bytes_impl(n as f64, false)
    });
    engine.register_fn("human_bytes", |n: f64| -> String {
        human_bytes_impl(n, false)
    });
    engine.register_fn("human_bytes_si", |n: i64| -> String {
        human_bytes_impl(n as f64, true)
    });
    engine.register_fn("human_bytes_si", |n: f64| -> String {
        human_bytes_impl(n, true)
    });

    // format_decimals: format number as string with exactly N digits after the
    // decimal point. Returns a string.
    engine.register_fn("format_decimals", |value: f64, decimals: i64| -> String {
        format_decimals_impl(value, decimals)
    });
    engine.register_fn("format_decimals", |value: i64, decimals: i64| -> String {
        format_decimals_impl(value as f64, decimals)
    });

    // format_percent: multiply ratio by 100 and render as string with N decimals
    // followed by a '%'. Returns a string.
    engine.register_fn("format_percent", |ratio: f64, decimals: i64| -> String {
        format_percent_impl(ratio, decimals)
    });
    engine.register_fn("format_percent", |ratio: i64, decimals: i64| -> String {
        format_percent_impl(ratio as f64, decimals)
    });

    // ljust: left-justify, pad right with spaces (or fill char) to reach
    // display width n. Already-wide strings are returned unchanged.
    engine.register_fn("ljust", |s: &str, n: i64| -> String {
        ljust_impl(s, n, ' ')
    });
    engine.register_fn("ljust", |s: &str, n: i64, fill: &str| -> String {
        ljust_impl(s, n, fill_char(fill))
    });

    // rjust: right-justify, pad left to reach display width n.
    engine.register_fn("rjust", |s: &str, n: i64| -> String {
        rjust_impl(s, n, ' ')
    });
    engine.register_fn("rjust", |s: &str, n: i64, fill: &str| -> String {
        rjust_impl(s, n, fill_char(fill))
    });

    // center: center within width n; extra padding goes on the right.
    engine.register_fn("center", |s: &str, n: i64| -> String {
        center_impl(s, n, ' ')
    });
    engine.register_fn("center", |s: &str, n: i64, fill: &str| -> String {
        center_impl(s, n, fill_char(fill))
    });

    // shorten: if the string exceeds width n, keep the start and append a
    // marker (default "…"). Width-aware (counts display columns).
    engine.register_fn("shorten", |s: &str, n: i64| -> String {
        shorten_impl(s, n, "…")
    });
    engine.register_fn("shorten", |s: &str, n: i64, marker: &str| -> String {
        shorten_impl(s, n, marker)
    });

    // shorten_middle: if the string exceeds width n, keep both ends and
    // insert a marker (default "…") in the middle. Front gets the extra
    // column when the remaining budget is odd.
    engine.register_fn("shorten_middle", |s: &str, n: i64| -> String {
        shorten_middle_impl(s, n, "…")
    });
    engine.register_fn(
        "shorten_middle",
        |s: &str, n: i64, marker: &str| -> String { shorten_middle_impl(s, n, marker) },
    );

    // ANSI color helpers. Wrap the string with an SGR code and a reset. When
    // colors are disabled (no TTY / NO_COLOR / --no-color / default), these
    // return the string unchanged so scripts work transparently in pipes.
    // Bright variants are used for primary colors to match the existing
    // logfmt output palette in src/colors.rs.
    engine.register_fn("red", |s: &str| -> String { wrap(s, "\x1b[91m") });
    engine.register_fn("green", |s: &str| -> String { wrap(s, "\x1b[92m") });
    engine.register_fn("yellow", |s: &str| -> String { wrap(s, "\x1b[93m") });
    engine.register_fn("blue", |s: &str| -> String { wrap(s, "\x1b[34m") });
    engine.register_fn("cyan", |s: &str| -> String { wrap(s, "\x1b[36m") });
    engine.register_fn("magenta", |s: &str| -> String { wrap(s, "\x1b[95m") });
    engine.register_fn("bold", |s: &str| -> String { wrap(s, "\x1b[1m") });
    engine.register_fn("dim", |s: &str| -> String { wrap(s, "\x1b[2m") });
}

/// Format a byte count as a human-readable string.
///
/// When `si` is false, uses binary (IEC) units with base 1024: B, KiB, MiB,
/// GiB, TiB, PiB, EiB. When `si` is true, uses decimal units with base 1000:
/// B, KB, MB, GB, TB, PB, EB.
///
/// Bytes (values below one unit step) are rendered without decimals; larger
/// units use one decimal place. Negative values render with a leading minus.
fn human_bytes_impl(n: f64, si: bool) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    let negative = n.is_sign_negative();
    let mut value = n.abs();

    let (base, units): (f64, &[&str]) = if si {
        (1000.0, &["B", "KB", "MB", "GB", "TB", "PB", "EB"])
    } else {
        (1024.0, &["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"])
    };

    let mut idx = 0;
    while value >= base && idx < units.len() - 1 {
        value /= base;
        idx += 1;
    }

    let sign = if negative { "-" } else { "" };
    if idx == 0 {
        // Bytes: no decimals
        format!("{}{} {}", sign, value.round() as i64, units[idx])
    } else {
        format!("{}{:.1} {}", sign, value, units[idx])
    }
}

/// Format a floating-point value as a string with exactly `decimals` digits
/// after the decimal point. Negative decimal counts are treated as zero; very
/// large values are capped at 20 to avoid pathological allocations.
fn format_decimals_impl(value: f64, decimals: i64) -> String {
    let d = decimals.clamp(0, 20) as usize;
    format!("{:.*}", d, value)
}

/// Format a ratio as a percentage string with exactly `decimals` digits after
/// the decimal point, followed by a '%' character. The input is multiplied by
/// 100, so pass 0.042 to render "4.2%".
fn format_percent_impl(ratio: f64, decimals: i64) -> String {
    let d = decimals.clamp(0, 20) as usize;
    format!("{:.*}%", d, ratio * 100.0)
}

/// Treat the first character of the user-supplied fill string as the fill
/// character, falling back to space if the string is empty or its first
/// character has non-unit display width (wide / CJK / zero-width).
fn fill_char(fill: &str) -> char {
    fill.chars()
        .next()
        .filter(|c| UnicodeWidthChar::width(*c) == Some(1))
        .unwrap_or(' ')
}

/// Left-justify `s` to display width `target`, padding with `fill` on the
/// right. Strings already at or beyond the target width are returned
/// unchanged. Negative widths are treated as zero.
fn ljust_impl(s: &str, target: i64, fill: char) -> String {
    let target = target.max(0) as usize;
    let w = UnicodeWidthStr::width(s);
    if w >= target {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + (target - w));
    out.push_str(s);
    for _ in 0..(target - w) {
        out.push(fill);
    }
    out
}

/// Right-justify `s` to display width `target`, padding with `fill` on the
/// left.
fn rjust_impl(s: &str, target: i64, fill: char) -> String {
    let target = target.max(0) as usize;
    let w = UnicodeWidthStr::width(s);
    if w >= target {
        return s.to_string();
    }
    let pad = target - w;
    let mut out = String::with_capacity(s.len() + pad);
    for _ in 0..pad {
        out.push(fill);
    }
    out.push_str(s);
    out
}

/// Center `s` within display width `target`. Extra padding (when the
/// difference is odd) goes on the right side.
fn center_impl(s: &str, target: i64, fill: char) -> String {
    let target = target.max(0) as usize;
    let w = UnicodeWidthStr::width(s);
    if w >= target {
        return s.to_string();
    }
    let total_pad = target - w;
    let left_pad = total_pad / 2;
    let right_pad = total_pad - left_pad;
    let mut out = String::with_capacity(s.len() + total_pad);
    for _ in 0..left_pad {
        out.push(fill);
    }
    out.push_str(s);
    for _ in 0..right_pad {
        out.push(fill);
    }
    out
}

/// Take a prefix of `s` whose display width does not exceed `budget`.
/// Returns the prefix as a string and the exact display width of that
/// prefix. Zero-width chars are included greedily at the tail as long as
/// no other char is pushed past the budget.
fn take_prefix_by_width(s: &str, budget: usize) -> (String, usize) {
    let mut out = String::new();
    let mut width = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > budget {
            break;
        }
        out.push(c);
        width += cw;
    }
    (out, width)
}

/// Take a suffix of `s` whose display width does not exceed `budget`.
fn take_suffix_by_width(s: &str, budget: usize) -> (String, usize) {
    let mut chars: Vec<char> = Vec::new();
    let mut width = 0usize;
    for c in s.chars().rev() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > budget {
            break;
        }
        chars.push(c);
        width += cw;
    }
    chars.reverse();
    (chars.into_iter().collect(), width)
}

/// If `s` exceeds display width `target`, cut from the end and append
/// `marker`. If the marker alone does not fit, falls back to a hard
/// width-truncated prefix without a marker.
fn shorten_impl(s: &str, target: i64, marker: &str) -> String {
    let target = target.max(0) as usize;
    let w = UnicodeWidthStr::width(s);
    if w <= target {
        return s.to_string();
    }
    let mw = UnicodeWidthStr::width(marker);
    if mw >= target {
        return take_prefix_by_width(s, target).0;
    }
    let budget = target - mw;
    let (prefix, _) = take_prefix_by_width(s, budget);
    format!("{prefix}{marker}")
}

/// If `s` exceeds display width `target`, keep the start and end and insert
/// `marker` in the middle. The front half gets the extra column when the
/// remaining budget is odd. Falls back to a hard prefix cut if the marker
/// alone does not fit.
fn shorten_middle_impl(s: &str, target: i64, marker: &str) -> String {
    let target = target.max(0) as usize;
    let w = UnicodeWidthStr::width(s);
    if w <= target {
        return s.to_string();
    }
    let mw = UnicodeWidthStr::width(marker);
    if mw >= target {
        return take_prefix_by_width(s, target).0;
    }
    let budget = target - mw;
    let front_budget = budget.div_ceil(2);
    let back_budget = budget - front_budget;
    let (front, _) = take_prefix_by_width(s, front_budget);
    let (back, _) = take_suffix_by_width(s, back_budget);
    format!("{front}{marker}{back}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_bytes_binary_basic() {
        assert_eq!(human_bytes_impl(0.0, false), "0 B");
        assert_eq!(human_bytes_impl(1.0, false), "1 B");
        assert_eq!(human_bytes_impl(500.0, false), "500 B");
        assert_eq!(human_bytes_impl(1023.0, false), "1023 B");
        assert_eq!(human_bytes_impl(1024.0, false), "1.0 KiB");
        assert_eq!(human_bytes_impl(1536.0, false), "1.5 KiB");
        assert_eq!(human_bytes_impl(1048576.0, false), "1.0 MiB");
        assert_eq!(human_bytes_impl(1073741824.0, false), "1.0 GiB");
    }

    #[test]
    fn test_human_bytes_si_basic() {
        assert_eq!(human_bytes_impl(0.0, true), "0 B");
        assert_eq!(human_bytes_impl(999.0, true), "999 B");
        assert_eq!(human_bytes_impl(1000.0, true), "1.0 KB");
        assert_eq!(human_bytes_impl(1500.0, true), "1.5 KB");
        assert_eq!(human_bytes_impl(1_000_000.0, true), "1.0 MB");
        assert_eq!(human_bytes_impl(1_500_000_000.0, true), "1.5 GB");
    }

    #[test]
    fn test_human_bytes_negative() {
        assert_eq!(human_bytes_impl(-500.0, false), "-500 B");
        assert_eq!(human_bytes_impl(-1536.0, false), "-1.5 KiB");
        assert_eq!(human_bytes_impl(-1_500_000.0, true), "-1.5 MB");
    }

    #[test]
    fn test_human_bytes_large_values() {
        // Saturate at the largest defined unit
        let huge = 1024.0_f64.powi(7); // 1024^7 bytes → past EiB
        let result = human_bytes_impl(huge, false);
        assert!(result.ends_with(" EiB"), "got {result}");
    }

    #[test]
    fn test_human_bytes_special_floats() {
        assert_eq!(human_bytes_impl(f64::NAN, false), "NaN");
        assert_eq!(human_bytes_impl(f64::INFINITY, false), "inf");
        assert_eq!(human_bytes_impl(f64::NEG_INFINITY, false), "-inf");
    }

    #[test]
    fn test_format_decimals_basic() {
        assert_eq!(format_decimals_impl(1.23456, 2), "1.23");
        assert_eq!(format_decimals_impl(1.23456, 3), "1.235");
        assert_eq!(format_decimals_impl(1.23456, 0), "1");
        assert_eq!(format_decimals_impl(1.0, 2), "1.00");
        assert_eq!(format_decimals_impl(1.5, 0), "2"); // banker's rounding via format!
    }

    #[test]
    fn test_format_decimals_negative_value() {
        assert_eq!(format_decimals_impl(-2.75, 1), "-2.8");
        assert_eq!(format_decimals_impl(-0.5, 2), "-0.50");
    }

    #[test]
    fn test_format_decimals_clamps_decimals_arg() {
        // Negative decimals treated as 0
        assert_eq!(format_decimals_impl(1.23456, -1), "1");
        // Very large decimals clamped to 20
        let result = format_decimals_impl(1.0, 100);
        assert_eq!(result.len(), "1.".len() + 20);
    }

    #[test]
    fn test_format_decimals_zero() {
        assert_eq!(format_decimals_impl(0.0, 2), "0.00");
        assert_eq!(format_decimals_impl(0.0, 0), "0");
    }

    #[test]
    fn test_format_percent_basic() {
        assert_eq!(format_percent_impl(0.0, 1), "0.0%");
        assert_eq!(format_percent_impl(0.5, 0), "50%");
        assert_eq!(format_percent_impl(0.042, 1), "4.2%");
        assert_eq!(format_percent_impl(1.0, 0), "100%");
        assert_eq!(format_percent_impl(0.12345, 2), "12.35%");
    }

    #[test]
    fn test_format_percent_over_one() {
        // Ratios > 1 are valid (e.g., growth rates)
        assert_eq!(format_percent_impl(1.5, 0), "150%");
        assert_eq!(format_percent_impl(2.25, 1), "225.0%");
    }

    #[test]
    fn test_format_percent_negative() {
        assert_eq!(format_percent_impl(-0.1, 1), "-10.0%");
    }

    #[test]
    fn test_format_percent_clamps_decimals_arg() {
        assert_eq!(format_percent_impl(0.5, -1), "50%");
    }

    // ---- padding / justification ----

    #[test]
    fn test_ljust_basic() {
        assert_eq!(ljust_impl("hi", 5, ' '), "hi   ");
        assert_eq!(ljust_impl("hello", 5, ' '), "hello");
        assert_eq!(ljust_impl("hello", 3, ' '), "hello"); // already too wide
        assert_eq!(ljust_impl("", 3, '-'), "---");
    }

    #[test]
    fn test_rjust_basic() {
        assert_eq!(rjust_impl("hi", 5, ' '), "   hi");
        assert_eq!(rjust_impl("42", 6, '0'), "000042");
        assert_eq!(rjust_impl("hello", 3, ' '), "hello");
    }

    #[test]
    fn test_center_basic() {
        assert_eq!(center_impl("hi", 6, ' '), "  hi  ");
        // odd remainder: extra goes to the right
        assert_eq!(center_impl("hi", 5, ' '), " hi  ");
        assert_eq!(center_impl("hi", 4, '-'), "-hi-");
        assert_eq!(center_impl("hello", 3, ' '), "hello");
    }

    #[test]
    fn test_padding_negative_width() {
        assert_eq!(ljust_impl("hi", -5, ' '), "hi");
        assert_eq!(rjust_impl("hi", -1, ' '), "hi");
        assert_eq!(center_impl("hi", -3, ' '), "hi");
    }

    #[test]
    fn test_padding_unicode_width_aware() {
        // "日本" has display width 4 (two wide chars)
        assert_eq!(UnicodeWidthStr::width("日本"), 4);
        assert_eq!(ljust_impl("日本", 6, ' '), "日本  ");
        assert_eq!(rjust_impl("日本", 6, ' '), "  日本");
        assert_eq!(center_impl("日本", 6, ' '), " 日本 ");
    }

    #[test]
    fn test_fill_char_defaults_to_space() {
        // empty fill → space
        assert_eq!(fill_char(""), ' ');
        // wide char → space (not allowed as fill)
        assert_eq!(fill_char("日"), ' ');
        // single narrow char → that char
        assert_eq!(fill_char("-"), '-');
        assert_eq!(fill_char("0"), '0');
        // multi-char: first narrow char
        assert_eq!(fill_char("abc"), 'a');
    }

    // ---- shorten ----

    #[test]
    fn test_shorten_basic() {
        assert_eq!(shorten_impl("hello world", 20, "…"), "hello world");
        assert_eq!(shorten_impl("hello world", 11, "…"), "hello world");
        assert_eq!(shorten_impl("hello world", 8, "…"), "hello w…");
        assert_eq!(shorten_impl("hello world", 5, "…"), "hell…");
    }

    #[test]
    fn test_shorten_ascii_marker() {
        assert_eq!(shorten_impl("hello world", 8, "..."), "hello...");
        assert_eq!(shorten_impl("hello world", 6, "..."), "hel...");
    }

    #[test]
    fn test_shorten_empty_marker() {
        // Empty marker = hard truncate
        assert_eq!(shorten_impl("hello world", 5, ""), "hello");
    }

    #[test]
    fn test_shorten_marker_too_wide() {
        // marker ">>>>" has width 4, target is 3 → fall back to hard truncate
        assert_eq!(shorten_impl("hello world", 3, ">>>>"), "hel");
    }

    #[test]
    fn test_shorten_unicode() {
        // "日本語テスト" is 6 chars × 2 cols each = 12 cols
        assert_eq!(UnicodeWidthStr::width("日本語テスト"), 12);
        // target 7: marker "…" is 1 col → budget 6 → fits 3 wide chars
        assert_eq!(shorten_impl("日本語テスト", 7, "…"), "日本語…");
        // target 5: budget 4 → fits 2 wide chars
        assert_eq!(shorten_impl("日本語テスト", 5, "…"), "日本…");
    }

    #[test]
    fn test_shorten_negative_width() {
        assert_eq!(shorten_impl("hello", -1, "…"), "");
    }

    // ---- shorten_middle ----

    #[test]
    fn test_shorten_middle_basic() {
        // "abcdefghij" width 10, target 6, marker "…" width 1 → budget 5
        // front = ceil(5/2) = 3, back = 2 → "abc…ij"
        assert_eq!(shorten_middle_impl("abcdefghij", 6, "…"), "abc…ij");
    }

    #[test]
    fn test_shorten_middle_even_budget() {
        // target 7, marker "…" width 1 → budget 6 → front=3, back=3
        assert_eq!(shorten_middle_impl("abcdefghij", 7, "…"), "abc…hij");
    }

    #[test]
    fn test_shorten_middle_ascii_marker() {
        // marker "..." width 3, target 8, budget 5, front=3 back=2
        assert_eq!(shorten_middle_impl("abcdefghij", 8, "..."), "abc...ij");
    }

    #[test]
    fn test_shorten_middle_no_truncation() {
        assert_eq!(shorten_middle_impl("short", 20, "…"), "short");
        assert_eq!(shorten_middle_impl("hello", 5, "…"), "hello");
    }

    #[test]
    fn test_shorten_middle_path_example() {
        let path = "/home/user/projects/kelora/src/rhai_functions/formatting.rs";
        let result = shorten_middle_impl(path, 30, "…");
        assert!(UnicodeWidthStr::width(result.as_str()) <= 30);
        assert!(result.starts_with('/'));
        assert!(result.ends_with("formatting.rs"));
        assert!(result.contains('…'));
    }

    #[test]
    fn test_shorten_middle_marker_too_wide() {
        // marker width 4, target 3 → fall back to hard prefix cut
        assert_eq!(shorten_middle_impl("abcdefghij", 3, ">>>>"), "abc");
    }

    #[test]
    fn test_shorten_middle_unicode() {
        // "日本語テスト" is 12 cols. target 8, marker "…" width 1 → budget 7
        // front = ceil(7/2)=4 → 2 wide chars; back = 3 → 1 wide char (width 2, next would be 4 > 3)
        // Result: "日本…ト" = 2+2+1+2 = 7 cols, leq 8 ✓
        assert_eq!(shorten_middle_impl("日本語テスト", 8, "…"), "日本…ト");
    }

    #[test]
    fn test_shorten_width_invariant() {
        // Output width never exceeds target
        for target in 0..15 {
            let out = shorten_impl("/some/path/to/a/file.rs", target, "…");
            assert!(
                UnicodeWidthStr::width(out.as_str()) <= target as usize,
                "shorten(target={target}) exceeded: {out:?}"
            );
            let out = shorten_middle_impl("/some/path/to/a/file.rs", target, "…");
            assert!(
                UnicodeWidthStr::width(out.as_str()) <= target as usize,
                "shorten_middle(target={target}) exceeded: {out:?}"
            );
        }
    }

    // ---- colors ----

    // A minimal test fixture: uses its own setter, restores after. The tests
    // use Relaxed ordering and only flip a single atomic, so cross-test bleed
    // is acceptable (we always set explicitly before asserting).

    #[test]
    fn test_colors_disabled_by_default_returns_plain() {
        set_colors_enabled(false);
        assert_eq!(wrap("hi", "\x1b[91m"), "hi");
    }

    #[test]
    fn test_colors_enabled_wraps_with_ansi() {
        set_colors_enabled(true);
        assert_eq!(wrap("hi", "\x1b[91m"), "\x1b[91mhi\x1b[0m");
        set_colors_enabled(false);
    }

    #[test]
    fn test_colors_empty_string() {
        set_colors_enabled(true);
        assert_eq!(wrap("", "\x1b[1m"), "\x1b[1m\x1b[0m");
        set_colors_enabled(false);
    }

    #[test]
    fn test_colors_set_and_get() {
        set_colors_enabled(true);
        assert!(colors_enabled());
        set_colors_enabled(false);
        assert!(!colors_enabled());
    }

    #[test]
    fn test_padding_width_invariant() {
        // ljust/rjust/center output width is exactly max(input_width, target)
        for target in 0..12 {
            for s in &["", "x", "hi", "hello"] {
                let input_w = UnicodeWidthStr::width(*s);
                let expected = input_w.max(target as usize);
                assert_eq!(
                    UnicodeWidthStr::width(ljust_impl(s, target, ' ').as_str()),
                    expected
                );
                assert_eq!(
                    UnicodeWidthStr::width(rjust_impl(s, target, ' ').as_str()),
                    expected
                );
                assert_eq!(
                    UnicodeWidthStr::width(center_impl(s, target, ' ').as_str()),
                    expected
                );
            }
        }
    }
}
