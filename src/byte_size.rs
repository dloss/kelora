//! Parse and format human-friendly byte sizes for size-valued CLI options
//! (currently `--max-line-bytes`).
//!
//! Accepts a plain byte count or an IEC/SI suffix, case-insensitively:
//! `64MiB`, `64M`, `64mb`, `1GiB`, `1048576`. All multipliers are 1024-based
//! (binary), matching the `human_bytes` Rhai helper, so `MB` and `MiB` are
//! treated alike. The sentinels `0`, `off`, `none`, and `unlimited` disable the
//! limit (return `0`).

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

/// Parse a byte-size string into a count of bytes. `0`/`off`/`none`/`unlimited`
/// yield `0` (disabled). Returns a human-readable error on bad input.
pub fn parse_byte_size(input: &str) -> Result<usize, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("empty size value".to_string());
    }

    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "0" | "off" | "none" | "unlimited") {
        return Ok(0);
    }

    // Split the trailing unit (letters) from the leading number.
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num_part, unit_part) = s.split_at(split);
    let num_part = num_part.trim();

    let value: f64 = num_part
        .parse()
        .map_err(|_| format!("invalid size '{input}': '{num_part}' is not a number"))?;
    if value < 0.0 {
        return Err(format!("invalid size '{input}': must not be negative"));
    }

    let multiplier: u64 = match unit_part.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => KIB,
        "m" | "mb" | "mib" => MIB,
        "g" | "gb" | "gib" => GIB,
        other => {
            return Err(format!(
                "invalid size '{input}': unknown unit '{other}' (use B, KiB, MiB, GiB)"
            ))
        }
    };

    Ok((value * multiplier as f64) as usize)
}

/// Render a byte count using a binary (IEC) unit when it divides evenly, else
/// as a plain byte count. Used in diagnostics so the cap reads back the way it
/// was configured (`64 MiB` rather than `67108864`).
pub fn format_byte_size(bytes: u64) -> String {
    if bytes >= GIB && bytes % GIB == 0 {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB && bytes % MIB == 0 {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB && bytes % KIB == 0 {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_suffixed() {
        assert_eq!(parse_byte_size("1048576").unwrap(), 1024 * 1024);
        assert_eq!(parse_byte_size("64MiB").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_byte_size("64M").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_byte_size("64mb").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_byte_size("1GiB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_byte_size("2 kib").unwrap(), 2048);
    }

    #[test]
    fn sentinels_disable() {
        for s in ["0", "off", "none", "unlimited", "OFF"] {
            assert_eq!(parse_byte_size(s).unwrap(), 0, "{s}");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_byte_size("abc").is_err());
        assert!(parse_byte_size("12xb").is_err());
        assert!(parse_byte_size("-5MiB").is_err());
        assert!(parse_byte_size("").is_err());
    }

    #[test]
    fn formats_round_trip() {
        assert_eq!(format_byte_size(64 * 1024 * 1024), "64 MiB");
        assert_eq!(format_byte_size(1024 * 1024 * 1024), "1 GiB");
        assert_eq!(format_byte_size(2048), "2 KiB");
        assert_eq!(format_byte_size(1500), "1500 bytes");
    }
}
