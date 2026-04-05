use rhai::Engine;

pub fn register_functions(engine: &mut Engine) {
    // human_bytes: format byte count with binary (IEC) units by default.
    // Takes i64 or f64. Overload with unit string selects "si" for decimal units.
    engine.register_fn("human_bytes", |n: i64| -> String {
        human_bytes_impl(n as f64, false)
    });
    engine.register_fn("human_bytes", |n: f64| -> String {
        human_bytes_impl(n, false)
    });
    engine.register_fn("human_bytes", |n: i64, units: &str| -> String {
        human_bytes_impl(n as f64, units.eq_ignore_ascii_case("si"))
    });
    engine.register_fn("human_bytes", |n: f64, units: &str| -> String {
        human_bytes_impl(n, units.eq_ignore_ascii_case("si"))
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
}
