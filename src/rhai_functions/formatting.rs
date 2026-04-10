use rhai::{Array, Dynamic, Engine};
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
    // bar(value, max, width): render a horizontal bar of display width `width`
    // columns representing value/max as a fraction, using Unicode eighth-blocks
    // for sub-cell resolution. Overflow (value > max) clamps to full; negative
    // values and a non-positive max render as an empty-width pad.
    //
    // This is the single, unambiguous form. For a pre-normalized ratio, pass
    // max as 1 (e.g. `bar(0.42, 1, 20)`).
    engine.register_fn("bar", |value: f64, max: f64, width: i64| -> String {
        bar_impl(value / max, width)
    });
    engine.register_fn("bar", |value: i64, max: i64, width: i64| -> String {
        let ratio = if max == 0 {
            0.0
        } else {
            value as f64 / max as f64
        };
        bar_impl(ratio, width)
    });
    engine.register_fn("bar", |value: f64, max: i64, width: i64| -> String {
        let ratio = if max == 0 { 0.0 } else { value / max as f64 };
        bar_impl(ratio, width)
    });
    engine.register_fn("bar", |value: i64, max: f64, width: i64| -> String {
        bar_impl(value as f64 / max, width)
    });

    // sparkline(array): render a sparkline scaled 0..max(array) from an
    // array of numbers. Empty arrays return "". Non-numeric elements are
    // treated as 0.
    engine.register_fn("sparkline", |arr: Array| -> String { sparkline_impl(&arr) });

    engine.register_fn("red", |s: &str| -> String { wrap(s, "\x1b[91m") });
    engine.register_fn("green", |s: &str| -> String { wrap(s, "\x1b[92m") });
    engine.register_fn("yellow", |s: &str| -> String { wrap(s, "\x1b[93m") });
    engine.register_fn("blue", |s: &str| -> String { wrap(s, "\x1b[94m") });
    engine.register_fn("cyan", |s: &str| -> String { wrap(s, "\x1b[96m") });
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

    // If formatting to one decimal place would round up to the base value
    // (e.g. 1023.999... → "1024.0"), bump to the next unit so the output
    // stays sensible (e.g. "1.0 GiB" rather than "1024.0 MiB").
    if idx < units.len() - 1 && (value * 10.0).round() >= base * 10.0 {
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

/// Unicode eighth-blocks, ordered from empty (0/8) to full (8/8). Used by
/// `bar` and `sparkline` to render sub-cell resolution. Index `i` represents
/// `i` eighths filled.
const EIGHTHS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

/// Sparkline ticks from low to high. Eight levels give smooth visual
/// gradations; index 0 is used for zero/empty values (rendered as a space).
const SPARKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render a horizontal bar of display width `width` columns from a ratio in
/// 0..1. Ratios outside that range clamp. NaN ratios render as an empty
/// (all-space) bar. A non-positive width returns an empty string.
fn bar_impl(ratio: f64, width: i64) -> String {
    let width = width.max(0) as usize;
    if width == 0 {
        return String::new();
    }
    let r = if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    };
    // Total eighths to fill out of width * 8.
    let total_eighths = (r * (width as f64) * 8.0).round() as usize;
    let full_cells = total_eighths / 8;
    let partial = total_eighths % 8;
    let mut out = String::with_capacity(width * 3);
    for _ in 0..full_cells {
        out.push_str(EIGHTHS[8]);
    }
    if full_cells < width {
        out.push_str(EIGHTHS[partial]);
        // Pad the remainder with spaces so the bar has fixed display width.
        for _ in (full_cells + 1)..width {
            out.push(' ');
        }
    }
    out
}

/// Convert a Dynamic to f64, treating non-numeric values as 0.0.
fn dyn_to_f64(value: &Dynamic) -> f64 {
    if value.is_int() {
        value.as_int().unwrap_or(0) as f64
    } else if value.is_float() {
        value.as_float().unwrap_or(0.0)
    } else if value.is_bool() {
        if value.as_bool().unwrap_or(false) {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Render an array of numbers as a sparkline scaled 0..max. Empty arrays and
/// all-zero / all-negative arrays return a string of spaces of the same
/// length as the input, preserving column count so callers can align output.
fn sparkline_impl(arr: &[Dynamic]) -> String {
    if arr.is_empty() {
        return String::new();
    }
    let values: Vec<f64> = arr.iter().map(dyn_to_f64).map(|v| v.max(0.0)).collect();
    let max = values.iter().copied().fold(0.0_f64, f64::max);
    let mut out = String::with_capacity(arr.len() * 3);
    if max <= 0.0 {
        for _ in 0..arr.len() {
            out.push(' ');
        }
        return out;
    }
    let levels = SPARKS.len() as f64; // 8
    for v in values {
        if v <= 0.0 {
            out.push(' ');
            continue;
        }
        // Map (0, max] to indices 0..=7.
        let scaled = (v / max * levels).ceil() as usize;
        let idx = scaled.clamp(1, SPARKS.len()) - 1;
        out.push(SPARKS[idx]);
    }
    out
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
    fn test_human_bytes_binary_rounding_boundary() {
        // Values just below a unit threshold must not display as "1024.0 KiB"
        // (which equals the next unit) — they should bump up instead.
        assert_eq!(human_bytes_impl(1048575.0, false), "1.0 MiB"); // 1 MiB - 1 B
        assert_eq!(human_bytes_impl(1073741823.0, false), "1.0 GiB"); // 1 GiB - 1 B
                                                                      // Values that round down should stay in the lower unit
        assert_eq!(human_bytes_impl(1_047_552.0, false), "1023.0 KiB"); // 1023 * 1024 exactly
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
    fn test_human_bytes_si_rounding_boundary() {
        // Same rounding-at-boundary check for SI units.
        assert_eq!(human_bytes_impl(999_999.0, true), "1.0 MB"); // 1 MB - 1 B
        assert_eq!(human_bytes_impl(999_999_999.0, true), "1.0 GB"); // 1 GB - 1 B
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

    // ---- bar ----

    #[test]
    fn test_bar_empty_and_full() {
        assert_eq!(bar_impl(0.0, 10), "          ");
        assert_eq!(bar_impl(1.0, 10), "██████████");
    }

    #[test]
    fn test_bar_half() {
        // 0.5 * 10 * 8 = 40 eighths → 5 full cells + pad
        assert_eq!(bar_impl(0.5, 10), "█████     ");
    }

    #[test]
    fn test_bar_eighth_resolution() {
        // 1/8 of one cell out of a 1-wide bar → first partial block
        assert_eq!(bar_impl(0.125, 1), "▏");
        // 1/8 of a 4-wide bar: 0.125 * 4 * 8 = 4 eighths → half block in first cell
        assert_eq!(bar_impl(0.125, 4), "▌   ");
        // 3/8 in a 1-wide bar
        assert_eq!(bar_impl(0.375, 1), "▍");
        // 7/8 in a 1-wide bar
        assert_eq!(bar_impl(0.875, 1), "▉");
    }

    #[test]
    fn test_bar_clamps_overflow_and_negative() {
        assert_eq!(bar_impl(2.0, 5), "█████");
        assert_eq!(bar_impl(-0.5, 5), "     ");
    }

    #[test]
    fn test_bar_nan_and_zero_width() {
        assert_eq!(bar_impl(f64::NAN, 5), "     ");
        assert_eq!(bar_impl(0.5, 0), "");
        assert_eq!(bar_impl(0.5, -3), "");
    }

    #[test]
    fn test_bar_display_width_invariant() {
        // Every bar of width N has exactly N display columns.
        for width in 0..20_i64 {
            for &ratio in &[-1.0, 0.0, 0.1, 0.33, 0.5, 0.75, 0.99, 1.0, 2.0] {
                let out = bar_impl(ratio, width);
                assert_eq!(
                    UnicodeWidthStr::width(out.as_str()),
                    width.max(0) as usize,
                    "bar(ratio={ratio}, width={width}) = {out:?}"
                );
            }
        }
    }

    // ---- sparkline ----

    #[test]
    fn test_sparkline_empty() {
        let arr: Vec<Dynamic> = vec![];
        assert_eq!(sparkline_impl(&arr), "");
    }

    #[test]
    fn test_sparkline_all_zero() {
        let arr: Vec<Dynamic> = vec![
            Dynamic::from(0_i64),
            Dynamic::from(0_i64),
            Dynamic::from(0_i64),
        ];
        assert_eq!(sparkline_impl(&arr), "   ");
    }

    #[test]
    fn test_sparkline_monotonic() {
        let arr: Vec<Dynamic> = (1_i64..=8).map(Dynamic::from).collect();
        // Scaled 1..8 to 8 levels: 1/8,2/8,...,8/8 → all eight chars.
        assert_eq!(sparkline_impl(&arr), "▁▂▃▄▅▆▇█");
    }

    #[test]
    fn test_sparkline_mixed_types() {
        let arr: Vec<Dynamic> = vec![
            Dynamic::from(0_i64),
            Dynamic::from(5.0_f64),
            Dynamic::from(10_i64),
        ];
        // max=10: 0→space, 5→ceil(5/10*8)=4→'▄', 10→'█'
        assert_eq!(sparkline_impl(&arr), " ▄█");
    }

    #[test]
    fn test_sparkline_negative_clamped() {
        let arr: Vec<Dynamic> = vec![
            Dynamic::from(-3_i64),
            Dynamic::from(5_i64),
            Dynamic::from(10_i64),
        ];
        // negatives become 0 (rendered as space)
        assert_eq!(sparkline_impl(&arr), " ▄█");
    }

    #[test]
    fn test_sparkline_single_value() {
        let arr: Vec<Dynamic> = vec![Dynamic::from(42_i64)];
        // max = value → full height
        assert_eq!(sparkline_impl(&arr), "█");
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
