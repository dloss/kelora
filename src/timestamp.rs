#![allow(dead_code)]
use chrono::{DateTime, Datelike, Local, TimeZone, Utc};

/// Adaptive timestamp parser that dynamically reorders formats based on success
/// Each thread should have its own instance to avoid contention
pub struct AdaptiveTsParser {
    /// Format list with successful formats moved to front
    formats: Vec<String>,
}

impl AdaptiveTsParser {
    pub fn new() -> Self {
        Self {
            formats: get_initial_timestamp_formats(),
        }
    }

    /// Parse timestamp using adaptive format ordering
    /// Successful formats are moved to front of the list for faster future parsing
    pub fn parse_ts(&mut self, ts_str: &str) -> Option<DateTime<Utc>> {
        self.parse_ts_with_config(ts_str, None, None)
    }

    /// Parse timestamp with full configuration support
    pub fn parse_ts_with_config(
        &mut self,
        ts_str: &str,
        custom_format: Option<&str>,
        default_timezone: Option<&str>,
    ) -> Option<DateTime<Utc>> {
        let ts_str = ts_str.trim();

        // Try custom format first if provided
        if let Some(format) = custom_format {
            if let Some(parsed) = try_parse_with_format(ts_str, format, default_timezone) {
                return Some(parsed);
            }
        }

        // Handle special values (journalctl-compatible)
        match ts_str {
            "now" => return Some(Utc::now()),
            "today" => {
                let local_today = Local::now().date_naive();
                if let Some(naive_datetime) = local_today.and_hms_opt(0, 0, 0) {
                    return Some(naive_datetime.and_utc());
                }
            }
            "yesterday" => {
                let local_yesterday = Local::now().date_naive() - chrono::Duration::days(1);
                if let Some(naive_datetime) = local_yesterday.and_hms_opt(0, 0, 0) {
                    return Some(naive_datetime.and_utc());
                }
            }
            "tomorrow" => {
                let local_tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
                if let Some(naive_datetime) = local_tomorrow.and_hms_opt(0, 0, 0) {
                    return Some(naive_datetime.and_utc());
                }
            }
            _ => {}
        }

        // Handle relative times (e.g., "-1h", "+30m", "-2d", "1h", "30m")
        if ts_str.starts_with('-') || ts_str.starts_with('+') || looks_like_relative_time(ts_str) {
            if let Ok(dt) = parse_relative_time(ts_str) {
                return Some(dt);
            }
        }

        // Try Unix timestamp parsing for numeric-only strings
        if ts_str.chars().all(|c| c.is_ascii_digit()) {
            if let Some(parsed) = try_parse_unix_timestamp(ts_str) {
                return Some(parsed);
            }
        }

        // Try standard RFC formats (these are very fast)
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(dt) = DateTime::parse_from_rfc2822(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }

        // Try date-only formats and assume 00:00:00
        if let Some(dt) = parse_date_only(ts_str) {
            return Some(dt);
        }

        // Try time-only formats and assume today's date
        if let Some(dt) = parse_time_only(ts_str) {
            return Some(dt);
        }

        // Try format list with adaptive reordering
        self.try_formats_with_reordering(ts_str, default_timezone)
    }

    /// Try formats with timezone configuration and move successful ones to front
    fn try_formats_with_reordering(
        &mut self,
        ts_str: &str,
        default_timezone: Option<&str>,
    ) -> Option<DateTime<Utc>> {
        for (index, format) in self.formats.iter().enumerate() {
            if let Some(parsed) = try_parse_with_format(ts_str, format, default_timezone) {
                // Move successful format to front if it's not already there
                if index > 0 {
                    let successful_format = self.formats.remove(index);
                    self.formats.insert(0, successful_format);
                }
                return Some(parsed);
            }
        }

        None
    }

    /// Reset the format ordering to initial state (for testing/debugging)
    #[allow(dead_code)]
    pub fn reset_ordering(&mut self) {
        self.formats = get_initial_timestamp_formats();
    }

    /// Get current format ordering (for debugging/monitoring)
    #[allow(dead_code)]
    pub fn get_format_ordering(&self) -> Vec<String> {
        self.formats.clone()
    }
}

impl Default for AdaptiveTsParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to apply timezone configuration to a naive datetime
fn apply_timezone_to_naive(
    naive_dt: chrono::NaiveDateTime,
    default_timezone: Option<&str>,
) -> Option<DateTime<Utc>> {
    use chrono_tz::Tz;

    match default_timezone {
        Some("UTC") => Some(naive_dt.and_utc()),
        Some(tz_str) => {
            // Try to parse as named timezone
            if let Ok(tz) = tz_str.parse::<Tz>() {
                if let Some(dt) = tz.from_local_datetime(&naive_dt).single() {
                    return Some(dt.with_timezone(&Utc));
                }
            }
            // Fall back to local time if timezone parsing fails
            chrono::Local
                .from_local_datetime(&naive_dt)
                .single()
                .map(|local_dt| local_dt.with_timezone(&Utc))
        }
        None => {
            // Default: interpret as local time
            chrono::Local
                .from_local_datetime(&naive_dt)
                .single()
                .map(|local_dt| local_dt.with_timezone(&Utc))
        }
    }
}

/// Choose the best timestamp from multiple candidates
/// Prefers timestamps that are in the past and closest to now
fn choose_best_timestamp(candidates: &[DateTime<Utc>], now: DateTime<Utc>) -> DateTime<Utc> {
    use chrono::Duration;

    // Allow small future tolerance for clock skew (24 hours)
    const FUTURE_TOLERANCE_HOURS: i64 = 24;
    let future_cutoff = now + Duration::hours(FUTURE_TOLERANCE_HOURS);

    // Filter to reasonable timestamps (not far in future)
    let reasonable: Vec<_> = candidates
        .iter()
        .filter(|dt| **dt <= future_cutoff)
        .collect();

    // If we have reasonable candidates, pick the one closest to now
    let candidates_to_use = if !reasonable.is_empty() {
        reasonable
    } else {
        // Fallback: use all candidates if none are reasonable
        candidates.iter().collect()
    };

    // Return the timestamp closest to now
    **candidates_to_use
        .iter()
        .min_by_key(|dt| {
            let diff = ***dt - now;
            diff.num_seconds().abs()
        })
        .expect("candidates should not be empty")
}

/// Try to parse a timestamp with a specific format and timezone configuration
fn try_parse_with_format(
    ts_str: &str,
    format: &str,
    default_timezone: Option<&str>,
) -> Option<DateTime<Utc>> {
    // Strip brackets if present (common in Apache/Nginx logs)
    let ts_str = ts_str.trim();
    let ts_str = if ts_str.starts_with('[') && ts_str.ends_with(']') {
        &ts_str[1..ts_str.len() - 1]
    } else {
        ts_str
    };

    // Handle comma-separated fractional seconds (Python logging format)
    let (processed_ts_str, processed_format) = if format.contains(",%f") {
        // Convert comma to dot for chrono compatibility and handle milliseconds properly.
        // Only normalize when the fractional part is all digits; otherwise, leave untouched
        // so parsing simply fails instead of panicking on unexpected characters.
        if let Some(comma_pos) = ts_str.rfind(',') {
            let base_part = &ts_str[..comma_pos];
            let frac_part = &ts_str[comma_pos + 1..];

            if !frac_part.is_empty() && frac_part.chars().all(|c| c.is_ascii_digit()) {
                // Pad or truncate fractional part to 9 digits (nanoseconds)
                let frac_nanos = if frac_part.len() <= 3 {
                    // Treat as milliseconds, pad to nanoseconds
                    format!("{:0<9}", format!("{:0<3}", frac_part))
                } else if frac_part.len() <= 6 {
                    // Treat as microseconds, pad to nanoseconds
                    format!("{:0<9}", format!("{:0<6}", frac_part))
                } else {
                    // Limit to nanosecond precision while keeping at least 9 digits
                    let mut truncated: String = frac_part.chars().take(9).collect();
                    if truncated.len() < 9 {
                        truncated = format!("{:0<9}", truncated);
                    }
                    truncated
                };

                let new_ts_str = format!("{}.{}", base_part, frac_nanos);
                let new_format = format.replace(",%f", ".%f");
                (new_ts_str, new_format)
            } else {
                (ts_str.to_string(), format.to_string())
            }
        } else {
            (ts_str.to_string(), format.to_string())
        }
    } else {
        (ts_str.to_string(), format.to_string())
    };

    // Try timezone-aware parsing first
    if let Ok(dt) = DateTime::parse_from_str(&processed_ts_str, &processed_format) {
        return Some(dt.with_timezone(&Utc));
    }

    // Special handling for year-less timestamps (syslog, custom formats, etc.)
    // Check if format has month and day but no year
    let has_month = processed_format.contains("%b")   // Abbreviated month (Jan, Feb, ...)
        || processed_format.contains("%B")            // Full month (January, February, ...)
        || processed_format.contains("%m");           // Numeric month (01-12)
    let has_day = processed_format.contains("%d")     // Day with leading zero (01-31)
        || processed_format.contains("%e");           // Day without leading zero (1-31)
    let has_year = processed_format.contains("%Y")    // 4-digit year (2024)
        || processed_format.contains("%y");           // 2-digit year (24)

    if has_month && has_day && !has_year
    {
        let now = chrono::Utc::now();
        let current_year = now.year();
        let format_with_year = format!("%Y {}", processed_format);

        let mut candidates = Vec::new();

        // Try current year, previous year, and next year
        // This handles edge cases around year boundaries
        for year_offset in [-1, 0, 1] {
            let year = current_year + year_offset;
            let ts_with_year = format!("{} {}", year, processed_ts_str);

            if let Ok(naive_dt) =
                chrono::NaiveDateTime::parse_from_str(&ts_with_year, &format_with_year)
            {
                if let Some(dt) = apply_timezone_to_naive(naive_dt, default_timezone) {
                    candidates.push(dt);
                }
            }
        }

        // If we have multiple valid candidates, choose the most reasonable one
        // Prefer timestamps in the past and closest to now
        if !candidates.is_empty() {
            return Some(choose_best_timestamp(&candidates, now));
        }
    }

    // Try naive parsing
    if let Ok(naive_dt) =
        chrono::NaiveDateTime::parse_from_str(&processed_ts_str, &processed_format)
    {
        return apply_timezone_to_naive(naive_dt, default_timezone);
    }

    None
}

/// Try to parse Unix timestamp based on string length
fn try_parse_unix_timestamp(ts_str: &str) -> Option<DateTime<Utc>> {
    let timestamp_int = ts_str.parse::<i64>().ok()?;

    // Detect Unix timestamp precision by string length
    let dt = match ts_str.len() {
        10 => {
            // Seconds (1735566123)
            DateTime::from_timestamp(timestamp_int, 0)
        }
        13 => {
            // Milliseconds (1735566123000)
            DateTime::from_timestamp(
                timestamp_int / 1000,
                (timestamp_int % 1000) as u32 * 1_000_000,
            )
        }
        16 => {
            // Microseconds (1735566123000000)
            DateTime::from_timestamp(
                timestamp_int / 1_000_000,
                (timestamp_int % 1_000_000) as u32 * 1_000,
            )
        }
        19 => {
            // Nanoseconds (1735566123000000000)
            DateTime::from_timestamp(
                timestamp_int / 1_000_000_000,
                (timestamp_int % 1_000_000_000) as u32,
            )
        }
        _ => None,
    };

    dt.map(|dt| dt.with_timezone(&Utc))
}

/// Get initial list of timestamp formats
/// Ordered by likelihood/performance, with most common formats first
fn get_initial_timestamp_formats() -> Vec<String> {
    vec![
        // ISO 8601 variants (most common in logs)
        "%Y-%m-%dT%H:%M:%S%.fZ".to_string(), // ISO 8601 with subseconds
        "%Y-%m-%dT%H:%M:%SZ".to_string(),    // ISO 8601
        "%Y-%m-%dT%H:%M:%S%.f%:z".to_string(), // ISO 8601 with timezone
        "%Y-%m-%dT%H:%M:%S%:z".to_string(),  // ISO 8601 with timezone
        "%Y-%m-%dT%H:%M:%S%.f".to_string(),  // ISO 8601 without timezone (with subseconds)
        "%Y-%m-%dT%H:%M:%S".to_string(),     // ISO 8601 without timezone
        // Space-separated ISO variants (common in many log formats)
        "%Y-%m-%d %H:%M:%S%.f".to_string(), // Common log format with subseconds
        "%Y-%m-%d %H:%M:%S".to_string(),    // Common log format
        "%Y-%m-%d %H:%M:%S%.fZ".to_string(), // Space-separated with Z
        "%Y-%m-%d %H:%M:%SZ".to_string(),   // Space-separated with Z
        "%Y-%m-%d %H:%M:%S%z".to_string(),  // Space-separated with timezone
        "%Y-%m-%d %H:%M:%S%.f%z".to_string(), // Space-separated with fractional + timezone
        // Syslog and server log formats
        "%b %d %H:%M:%S".to_string(),       // Syslog format
        "%b %d %Y %H:%M:%S".to_string(),    // BSD syslog with year
        "%d/%b/%Y:%H:%M:%S %z".to_string(), // Apache log format
        // Application-specific formats
        "%Y-%m-%d %H:%M:%S,%f".to_string(), // Python logging format
        "%Y/%m/%d %H:%M:%S".to_string(),    // Nginx error log format
        "%m/%d/%Y %H:%M:%S".to_string(),    // US slash format with time
        "%d.%m.%Y %H:%M:%S".to_string(),    // German format
        "%y%m%d %H:%M:%S".to_string(),      // MySQL legacy format
        // Less common but valid formats
        "%d %b %Y, %H:%M".to_string(),         // "12 Feb 2006, 19:17"
        "%a %b %d %H:%M:%S %Y".to_string(),    // Classic Unix timestamp
        "%d-%b-%y %I:%M:%S.%f %p".to_string(), // Oracle format
        "%b %d, %Y %I:%M:%S %p".to_string(),   // Java SimpleDateFormat
    ]
}

/// Configuration for timestamp field identification and parsing
#[derive(Debug, Clone)]
pub struct TsConfig {
    /// Custom timestamp field name (overrides auto-detection)
    pub custom_field: Option<String>,
    /// Custom timestamp format string
    pub custom_format: Option<String>,
    /// Default timezone for naive timestamps (None = local time)
    pub default_timezone: Option<String>,
    /// Whether to automatically parse timestamps from events (reserved for future features)
    #[allow(dead_code)]
    pub auto_parse: bool,
}

impl Default for TsConfig {
    fn default() -> Self {
        Self {
            custom_field: None,
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        }
    }
}

/// Identify and extract timestamp from event fields
pub fn identify_timestamp_field(
    fields: &indexmap::IndexMap<String, rhai::Dynamic>,
    config: &TsConfig,
) -> Option<(String, String)> {
    // If custom field is specified, try that first
    if let Some(ref custom_field) = config.custom_field {
        if let Some(value) = fields.get(custom_field) {
            if let Ok(ts_str) = value.clone().into_string() {
                return Some((custom_field.clone(), ts_str));
            }
        }
        // Custom field explicitly requested but absent or not string-convertible;
        // do not fall back to built-in candidates.
        return None;
    }

    // Otherwise, try the standard timestamp field names
    for ts_key in crate::event::TIMESTAMP_FIELD_NAMES {
        if let Some(value) = fields.get(*ts_key) {
            if let Ok(ts_str) = value.clone().into_string() {
                return Some((ts_key.to_string(), ts_str));
            }
        }
    }

    None
}

/// Parse timestamp arguments (--since, --until) in journalctl-compatible format
/// Uses the enhanced adaptive parser with journalctl support
pub fn parse_timestamp_arg_with_timezone(
    arg: &str,
    default_timezone: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    let mut parser = AdaptiveTsParser::new();
    parser
        .parse_ts_with_config(arg, None, default_timezone)
        .ok_or_else(|| format!("Could not parse timestamp: {}", arg))
}

/// Parse anchored timestamp expressions like "start+30m", "end-1h"
/// Requires the corresponding anchor timestamp to be provided
pub fn parse_anchored_timestamp(
    arg: &str,
    start_anchor: Option<DateTime<Utc>>,
    end_anchor: Option<DateTime<Utc>>,
    default_timezone: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    // Try to parse as anchor reference
    if let Some(offset_part) = arg.strip_prefix("start+") {
        let start = start_anchor
            .ok_or_else(|| "'start' anchor requires --since to be specified".to_string())?;
        let duration = parse_duration(&format!("+{}", offset_part))?;
        start
            .checked_add_signed(duration)
            .ok_or_else(|| "Anchored timestamp is out of supported range".to_string())
    } else if let Some(offset_part) = arg.strip_prefix("start-") {
        let start = start_anchor
            .ok_or_else(|| "'start' anchor requires --since to be specified".to_string())?;
        let duration = parse_duration(&format!("-{}", offset_part))?;
        start
            .checked_add_signed(duration)
            .ok_or_else(|| "Anchored timestamp is out of supported range".to_string())
    } else if let Some(offset_part) = arg.strip_prefix("end+") {
        let end = end_anchor
            .ok_or_else(|| "'end' anchor requires --until to be specified".to_string())?;
        let duration = parse_duration(&format!("+{}", offset_part))?;
        end.checked_add_signed(duration)
            .ok_or_else(|| "Anchored timestamp is out of supported range".to_string())
    } else if let Some(offset_part) = arg.strip_prefix("end-") {
        let end = end_anchor
            .ok_or_else(|| "'end' anchor requires --until to be specified".to_string())?;
        let duration = parse_duration(&format!("-{}", offset_part))?;
        end.checked_add_signed(duration)
            .ok_or_else(|| "Anchored timestamp is out of supported range".to_string())
    } else {
        // Not an anchored timestamp, parse normally
        parse_timestamp_arg_with_timezone(arg, default_timezone)
    }
}

/// Check if a string looks like a relative time expression (e.g., "1h", "30m", "2d", "1 hour")
fn looks_like_relative_time(arg: &str) -> bool {
    // Must start with a digit
    if !arg.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }

    // Find where numbers end (may have spaces before unit)
    let num_end = arg.find(|c: char| !c.is_ascii_digit()).unwrap_or(arg.len());
    let remainder = &arg[num_end..].trim_start();

    if remainder.is_empty() {
        return false;
    }

    // Check if it's a valid time unit
    matches!(
        *remainder,
        "s" | "sec"
            | "secs"
            | "second"
            | "seconds"
            | "m"
            | "min"
            | "mins"
            | "minute"
            | "minutes"
            | "h"
            | "hour"
            | "hours"
            | "d"
            | "day"
            | "days"
            | "w"
            | "week"
            | "weeks"
    )
}

/// Parse a duration string like "+30m", "-1h", "2d" into a chrono::Duration
/// Positive values use +, negative values use -, unsigned defaults to negative (past)
fn parse_duration(arg: &str) -> Result<chrono::Duration, String> {
    let (sign, rest) = if let Some(stripped) = arg.strip_prefix('-') {
        (-1, stripped)
    } else if let Some(stripped) = arg.strip_prefix('+') {
        (1, stripped)
    } else {
        // Unsigned times default to past (e.g., "1h" means "1 hour ago")
        (-1, arg)
    };

    if rest.is_empty() {
        return Err("Empty relative time".to_string());
    }

    // Parse number and unit (handle spaces between them)
    let (num_str, unit) = if let Some(pos) = rest.find(|c: char| !c.is_ascii_digit()) {
        let num_part = &rest[..pos];
        let unit_part = rest[pos..].trim_start();
        if unit_part.is_empty() || !unit_part.chars().next().unwrap().is_alphabetic() {
            return Err("Relative time must have a valid unit (h, m, d, etc.)".to_string());
        }
        (num_part, unit_part)
    } else {
        return Err("Relative time must have a unit (h, m, d, etc.)".to_string());
    };

    let num: i64 = num_str
        .parse()
        .map_err(|_| "Invalid number in relative time")?;
    let signed_num = sign * num;

    let seconds_factor: i64 = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 604_800,
        _ => return Err(format!("Unknown time unit: {}", unit)),
    };

    let total_seconds = signed_num
        .checked_mul(seconds_factor)
        .ok_or_else(|| "Relative time is out of supported range".to_string())?;
    chrono::Duration::try_seconds(total_seconds)
        .ok_or_else(|| "Relative time is out of supported range".to_string())
}

/// Parse relative time expressions like "-1h", "+30m", "-2d", "1h", "30m"
/// Unsigned times default to past (e.g., "1h" means "1 hour ago")
fn parse_relative_time(arg: &str) -> Result<DateTime<Utc>, String> {
    let duration = parse_duration(arg)?;
    Utc::now()
        .checked_add_signed(duration)
        .ok_or_else(|| "Relative time is out of supported range".to_string())
}

/// Parse date-only strings and assume 00:00:00
fn parse_date_only(arg: &str) -> Option<DateTime<Utc>> {
    // Try YYYY-MM-DD format
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%Y-%m-%d") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    // Try YYYY/MM/DD format
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%Y/%m/%d") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    // Try MM/DD/YYYY format
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%m/%d/%Y") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    // Try DD.MM.YYYY format
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%d.%m.%Y") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    // Try Month Day, Year format with long month name
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%B %d, %Y") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    // Try Day Month Year format with long month name
    if let Ok(date) = chrono::NaiveDate::parse_from_str(arg, "%d %B %Y") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some(naive_dt.and_utc());
        }
    }

    None
}

/// Parse time-only strings and assume today's date
fn parse_time_only(arg: &str) -> Option<DateTime<Utc>> {
    let today = Local::now().date_naive();

    // Try HH:MM:SS format
    if let Ok(time) = chrono::NaiveTime::parse_from_str(arg, "%H:%M:%S") {
        let naive_dt = today.and_time(time);
        return Some(naive_dt.and_utc());
    }

    // Try HH:MM format (assume :00 seconds)
    if let Ok(time) = chrono::NaiveTime::parse_from_str(arg, "%H:%M") {
        let naive_dt = today.and_time(time);
        return Some(naive_dt.and_utc());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};
    use proptest::prelude::*;

    fn arb_utc_datetime() -> impl Strategy<Value = DateTime<Utc>> {
        (-2208988800i64..=253402300799i64, 0u32..1_000_000_000).prop_map(|(secs, nanos)| {
            chrono::Utc
                .timestamp_opt(secs, nanos)
                .single()
                .expect("valid timestamp")
        })
    }

    #[test]
    fn test_adaptive_parser_basic() {
        let mut parser = AdaptiveTsParser::new();

        // Test ISO 8601 parsing
        let result = parser.parse_ts("2023-07-04T12:34:56Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 4);
    }

    #[test]
    fn test_format_reordering() {
        let mut parser = AdaptiveTsParser::new();

        // Parse first timestamp - should move successful format to front
        let result1 = parser.parse_ts("2023-07-04 12:34:56");
        assert!(result1.is_some());

        // Check that format was moved to front
        let formats = parser.get_format_ordering();
        assert!(!formats.is_empty());
        // The successful format should now be at index 0
        assert_eq!(formats[0], "%Y-%m-%d %H:%M:%S%.f");

        // Parse second timestamp with same format - should be faster (first try)
        let result2 = parser.parse_ts("2023-07-05 13:45:07");
        assert!(result2.is_some());
    }

    #[test]
    fn test_unix_timestamp_parsing() {
        let mut parser = AdaptiveTsParser::new();

        // Test seconds precision
        let result = parser.parse_ts("1735566123");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2024);

        // Test milliseconds precision
        let result = parser.parse_ts("1735566123000");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2024);
    }

    #[test]
    fn test_timestamp_field_identification() {
        use indexmap::IndexMap;
        use rhai::Dynamic;

        let mut fields = IndexMap::new();
        fields.insert("ts".to_string(), Dynamic::from("2023-07-04T12:34:56Z"));
        fields.insert("message".to_string(), Dynamic::from("test message"));

        let config = TsConfig::default();
        let result = identify_timestamp_field(&fields, &config);

        assert!(result.is_some());
        let (field_name, value) = result.unwrap();
        assert_eq!(field_name, "ts");
        assert_eq!(value, "2023-07-04T12:34:56Z");
    }

    #[test]
    fn test_custom_timestamp_field() {
        use indexmap::IndexMap;
        use rhai::Dynamic;

        let mut fields = IndexMap::new();
        fields.insert(
            "custom_time".to_string(),
            Dynamic::from("2023-07-04T12:34:56Z"),
        );
        fields.insert("ts".to_string(), Dynamic::from("other_timestamp"));

        let config = TsConfig {
            custom_field: Some("custom_time".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        let result = identify_timestamp_field(&fields, &config);

        assert!(result.is_some());
        let (field_name, value) = result.unwrap();
        assert_eq!(field_name, "custom_time");
        assert_eq!(value, "2023-07-04T12:34:56Z");
    }

    #[test]
    fn test_custom_timestamp_field_missing_does_not_fallback() {
        use indexmap::IndexMap;
        use rhai::Dynamic;

        let mut fields = IndexMap::new();
        fields.insert("ts".to_string(), Dynamic::from("2023-07-04T12:34:56Z"));

        let config = TsConfig {
            custom_field: Some("custom_time".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        let result = identify_timestamp_field(&fields, &config);

        assert!(result.is_none());
    }

    #[test]
    fn test_custom_timestamp_field_non_string_does_not_fallback() {
        use indexmap::IndexMap;
        use rhai::Dynamic;

        let mut fields = IndexMap::new();
        fields.insert("custom_time".to_string(), Dynamic::from(42));
        fields.insert("ts".to_string(), Dynamic::from("2023-07-04T12:34:56Z"));

        let config = TsConfig {
            custom_field: Some("custom_time".to_string()),
            custom_format: None,
            default_timezone: None,
            auto_parse: true,
        };

        let result = identify_timestamp_field(&fields, &config);

        assert!(result.is_none());
    }

    #[test]
    fn test_ordering_reset() {
        let mut parser = AdaptiveTsParser::new();

        // Parse to change ordering
        parser.parse_ts("2023-07-04 12:34:56");
        let formats_after = parser.get_format_ordering();
        assert_eq!(formats_after[0], "%Y-%m-%d %H:%M:%S%.f");

        // Reset ordering
        parser.reset_ordering();
        let formats_reset = parser.get_format_ordering();
        // Should be back to initial order
        assert_eq!(formats_reset[0], "%Y-%m-%dT%H:%M:%S%.fZ");
    }

    #[test]
    fn test_invalid_timestamp() {
        let mut parser = AdaptiveTsParser::new();

        let result = parser.parse_ts("not-a-timestamp");
        assert!(result.is_none());

        let result = parser.parse_ts("2023-13-45T25:70:70Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_fractional_part_with_non_digits_does_not_panic() {
        let mut parser = AdaptiveTsParser::new();
        let result = parser.parse_ts("4050-01-01T0,:00:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_multiple_format_reordering() {
        let mut parser = AdaptiveTsParser::new();

        // Parse different formats to test reordering
        parser.parse_ts("2023-07-04 12:34:56"); // Common format
        parser.parse_ts("2023-07-05T13:45:07Z"); // ISO format
        parser.parse_ts("2023-07-06 14:56:08"); // Common format again

        let formats = parser.get_format_ordering();
        // Most recently used format should be at front
        assert_eq!(formats[0], "%Y-%m-%d %H:%M:%S%.f");
    }

    #[test]
    fn test_journalctl_compatible_formats() {
        let mut parser = AdaptiveTsParser::new();

        // Test special values
        let now = parser.parse_ts("now");
        assert!(now.is_some());

        let today = parser.parse_ts("today");
        assert!(today.is_some());

        let yesterday = parser.parse_ts("yesterday");
        assert!(yesterday.is_some());

        let tomorrow = parser.parse_ts("tomorrow");
        assert!(tomorrow.is_some());

        // Test relative times
        let one_hour_ago = parser.parse_ts("-1h");
        assert!(one_hour_ago.is_some());

        let thirty_min_from_now = parser.parse_ts("+30m");
        assert!(thirty_min_from_now.is_some());

        // Test date-only formats
        let date_only = parser.parse_ts("2023-07-04");
        assert!(date_only.is_some());

        // Test time-only formats
        let time_only = parser.parse_ts("15:30:45");
        assert!(time_only.is_some());
    }

    #[test]
    fn test_parse_timestamp_arg_with_timezone() {
        // Test that the wrapper function works
        let result = parse_timestamp_arg_with_timezone("2023-07-04T12:34:56Z", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("now", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("-1h", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("invalid-timestamp", None);
        assert!(result.is_err());

        // Test bracketed timestamps (common in Apache/Nginx logs)
        let result = parse_timestamp_arg_with_timezone("[9/Feb/2017:10:34:12 -0700]", None);
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2017);
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 9);
    }

    #[test]
    fn test_relative_time_parsing() {
        // Test various relative time formats
        let result = parse_timestamp_arg_with_timezone("-30m", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("+2h", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("-1d", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("-2w", None);
        assert!(result.is_ok());

        // Test invalid relative times
        let result = parse_timestamp_arg_with_timezone("-", None);
        assert!(result.is_err());

        let result = parse_timestamp_arg_with_timezone("-1x", None);
        assert!(result.is_err());

        let result = parse_timestamp_arg_with_timezone("1h", None); // Unsigned defaults to past
        assert!(result.is_ok());
    }

    #[test]
    fn test_relative_time_out_of_range_returns_error() {
        assert!(parse_relative_time("111111111111h").is_err());
    }

    #[test]
    fn test_parse_duration() {
        // Test positive durations
        let duration = parse_duration("+30m").unwrap();
        assert_eq!(duration.num_minutes(), 30);

        let duration = parse_duration("+1h").unwrap();
        assert_eq!(duration.num_hours(), 1);

        // Test negative durations
        let duration = parse_duration("-30m").unwrap();
        assert_eq!(duration.num_minutes(), -30);

        // Test unsigned (defaults to negative)
        let duration = parse_duration("1h").unwrap();
        assert_eq!(duration.num_hours(), -1);
    }

    #[test]
    fn test_parse_anchored_timestamp_start_plus() {
        let base = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
        let result = parse_anchored_timestamp("start+30m", Some(base), None, None).unwrap();

        let expected = chrono::Utc
            .with_ymd_and_hms(2024, 1, 15, 10, 30, 0)
            .unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_anchored_timestamp_start_minus() {
        let base = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
        let result = parse_anchored_timestamp("start-1h", Some(base), None, None).unwrap();

        let expected = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 9, 0, 0).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_anchored_timestamp_end_plus() {
        let base = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 11, 0, 0).unwrap();
        let result = parse_anchored_timestamp("end+1h", None, Some(base), None).unwrap();

        let expected = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_anchored_timestamp_end_minus() {
        let base = chrono::Utc.with_ymd_and_hms(2024, 1, 15, 11, 0, 0).unwrap();
        let result = parse_anchored_timestamp("end-30m", None, Some(base), None).unwrap();

        let expected = chrono::Utc
            .with_ymd_and_hms(2024, 1, 15, 10, 30, 0)
            .unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_anchored_timestamp_missing_anchor() {
        // start anchor required but not provided
        let result = parse_anchored_timestamp("start+30m", None, None, None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("'start' anchor requires --since"));

        // end anchor required but not provided
        let result = parse_anchored_timestamp("end+30m", None, None, None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("'end' anchor requires --until"));
    }

    #[test]
    fn test_parse_anchored_timestamp_fallback_to_normal() {
        // Non-anchored timestamps should fall back to normal parsing
        let result = parse_anchored_timestamp("2024-01-15T10:00:00Z", None, None, None);
        assert!(result.is_ok());

        let dt = result.unwrap();
        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 15);
    }

    #[test]
    fn test_date_only_parsing() {
        let result = parse_timestamp_arg_with_timezone("2023-07-04", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("07/04/2023", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("04.07.2023", None);
        assert!(result.is_ok());

        // Invalid date formats
        let result = parse_timestamp_arg_with_timezone("2023-13-45", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_time_only_parsing() {
        let result = parse_timestamp_arg_with_timezone("15:30:45", None);
        assert!(result.is_ok());

        let result = parse_timestamp_arg_with_timezone("09:15", None);
        assert!(result.is_ok());

        // Invalid time formats
        let result = parse_timestamp_arg_with_timezone("25:70:80", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_comma_separated_milliseconds() {
        let mut parser = AdaptiveTsParser::new();

        // Test 3-digit milliseconds (most common) - use UTC timezone to avoid local time conversion
        let result = parser.parse_ts_with_config("2010-04-24 07:52:09,487", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2010);
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 24);
        assert_eq!(dt.hour(), 7);
        assert_eq!(dt.minute(), 52);
        assert_eq!(dt.second(), 9);
        // 487 milliseconds should equal 487,000,000 nanoseconds
        assert_eq!(
            dt.timestamp_nanos_opt().unwrap() % 1_000_000_000,
            487_000_000
        );

        // Test 2-digit centiseconds
        let result = parser.parse_ts_with_config("2010-04-24 07:52:09,12", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        // 12 centiseconds = 120 milliseconds = 120,000,000 nanoseconds
        assert_eq!(
            dt.timestamp_nanos_opt().unwrap() % 1_000_000_000,
            120_000_000
        );

        // Test 1-digit deciseconds
        let result = parser.parse_ts_with_config("2010-04-24 07:52:09,5", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        // 5 deciseconds = 500 milliseconds = 500,000,000 nanoseconds
        assert_eq!(
            dt.timestamp_nanos_opt().unwrap() % 1_000_000_000,
            500_000_000
        );
    }

    #[test]
    fn test_unix_timestamp_parsing_enhanced() {
        let mut parser = AdaptiveTsParser::new();

        // Test various Unix timestamp precisions
        let result = parser.parse_ts("1735566123"); // seconds
        assert!(result.is_some());

        let result = parser.parse_ts("1735566123000"); // milliseconds
        assert!(result.is_some());

        let result = parser.parse_ts("1735566123000000"); // microseconds
        assert!(result.is_some());

        let result = parser.parse_ts("1735566123000000000"); // nanoseconds
        assert!(result.is_some());

        // Test invalid Unix timestamps
        let result = parser.parse_ts("123"); // too short
        assert!(result.is_none());

        let result = parser.parse_ts("12345678901234567890123"); // too long
        assert!(result.is_none());
    }

    #[test]
    fn test_bracketed_timestamp_parsing() {
        let mut parser = AdaptiveTsParser::new();

        // Test Apache/Nginx log format with brackets
        let result = parser.parse_ts("[9/Feb/2017:10:34:12 -0700]");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2017);
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 9);
        assert_eq!(dt.hour(), 17); // 10:34 -0700 = 17:34 UTC
        assert_eq!(dt.minute(), 34);
        assert_eq!(dt.second(), 12);

        // Test without brackets (should still work)
        let result = parser.parse_ts("9/Feb/2017:10:34:12 -0700");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2017);
        assert_eq!(dt.month(), 2);

        // Test other bracketed formats
        let result = parser.parse_ts("[2023-07-04T12:34:56Z]");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);

        let result = parser.parse_ts("[2023-07-04 12:34:56]");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);

        // Edge case: malformed brackets are outside our scope -
        // our improvement handles the common case of properly bracketed timestamps
    }

    proptest! {
        #[test]
        fn prop_parse_rfc3339_roundtrip(dt in arb_utc_datetime()) {
            let mut parser = AdaptiveTsParser::new();
            let input = dt.to_rfc3339();
            let parsed = parser
                .parse_ts_with_config(&input, None, None)
                .expect("parser should accept RFC3339 timestamp");

            prop_assert_eq!(parsed, dt);
        }

        #[test]
        fn prop_parse_ignores_surrounding_whitespace(dt in arb_utc_datetime(), prefix in "[ \t]{0,3}", suffix in "[ \t]{0,3}") {
            let mut parser = AdaptiveTsParser::new();
            let raw = format!("{}{}{}", prefix, dt.to_rfc3339(), suffix);
            let parsed = parser
                .parse_ts_with_config(&raw, None, None)
                .expect("parser should ignore surrounding whitespace");

            prop_assert_eq!(parsed, dt);
        }
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Timezone Handling
    // ============================================================================

    #[test]
    fn test_timezone_named_zones() {
        let mut parser = AdaptiveTsParser::new();

        // Test parsing with named timezone (America/New_York)
        let result =
            parser.parse_ts_with_config("2023-07-04 12:00:00", None, Some("America/New_York"));
        assert!(result.is_some());
        let dt = result.unwrap();
        // 12:00 EDT (UTC-4 in summer) = 16:00 UTC
        assert_eq!(dt.hour(), 16);

        // Test parsing with Europe/London
        let result =
            parser.parse_ts_with_config("2023-07-04 12:00:00", None, Some("Europe/London"));
        assert!(result.is_some());
        let dt = result.unwrap();
        // 12:00 BST (UTC+1 in summer) = 11:00 UTC
        assert_eq!(dt.hour(), 11);

        // Test parsing with Asia/Tokyo
        let result = parser.parse_ts_with_config("2023-07-04 12:00:00", None, Some("Asia/Tokyo"));
        assert!(result.is_some());
        let dt = result.unwrap();
        // 12:00 JST (UTC+9) = 03:00 UTC
        assert_eq!(dt.hour(), 3);

        // Test that invalid timezone falls back to local time
        let result = parser.parse_ts_with_config("2023-07-04 12:00:00", None, Some("Invalid/Zone"));
        assert!(result.is_some());
        // Should not error, just fall back to local time parsing
    }

    #[test]
    fn test_timezone_utc_explicit() {
        let mut parser = AdaptiveTsParser::new();

        // Test explicit UTC timezone
        let result = parser.parse_ts_with_config("2023-07-04 12:00:00", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_dst_transition_spring_forward() {
        // Test DST transition - Spring forward (2:00 AM doesn't exist)
        // In America/New_York, March 12, 2023 at 2:00 AM clocks jump to 3:00 AM
        let mut parser = AdaptiveTsParser::new();

        // 1:59 AM exists (before spring forward)
        let result =
            parser.parse_ts_with_config("2023-03-12 01:59:00", None, Some("America/New_York"));
        assert!(result.is_some());

        // 2:30 AM doesn't exist (in the gap)
        // chrono will handle this - it may return None or map to a valid time
        let _result =
            parser.parse_ts_with_config("2023-03-12 02:30:00", None, Some("America/New_York"));
        // The behavior here depends on chrono's DST handling
        // We just verify it doesn't panic

        // 3:00 AM exists (after spring forward)
        let result =
            parser.parse_ts_with_config("2023-03-12 03:00:00", None, Some("America/New_York"));
        assert!(result.is_some());
    }

    #[test]
    fn test_dst_transition_fall_back() {
        // Test DST transition - Fall back (2:00 AM happens twice)
        // In America/New_York, November 5, 2023 at 2:00 AM clocks fall back to 1:00 AM
        let mut parser = AdaptiveTsParser::new();

        // 1:30 AM exists (before fall back)
        let result =
            parser.parse_ts_with_config("2023-11-05 01:30:00", None, Some("America/New_York"));
        assert!(result.is_some());

        // 2:30 AM is ambiguous (happens twice)
        // chrono's from_local_datetime will pick one (typically the first occurrence)
        let result =
            parser.parse_ts_with_config("2023-11-05 02:30:00", None, Some("America/New_York"));
        assert!(result.is_some());

        // 3:00 AM exists (after fall back)
        let result =
            parser.parse_ts_with_config("2023-11-05 03:00:00", None, Some("America/New_York"));
        assert!(result.is_some());
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Edge Dates
    // ============================================================================

    #[test]
    fn test_edge_dates_epoch_boundaries() {
        let mut parser = AdaptiveTsParser::new();

        // Unix epoch (1970-01-01 00:00:00 UTC)
        let result = parser.parse_ts("1970-01-01T00:00:00Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.timestamp(), 0);

        // Just after epoch
        let result = parser.parse_ts("1970-01-01T00:00:01Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.timestamp(), 1);

        // Before epoch (negative timestamp)
        let result = parser.parse_ts("1969-12-31T23:59:59Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.timestamp(), -1);
    }

    #[test]
    fn test_edge_dates_year_boundaries() {
        let mut parser = AdaptiveTsParser::new();

        // Year 2000 (Y2K boundary)
        let result = parser.parse_ts("2000-01-01T00:00:00Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2000);

        // Year 9999 (maximum reasonable year)
        let result = parser.parse_ts("9999-12-31T23:59:59Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 9999);

        // Year 1 (early date)
        let result = parser.parse_ts("0001-01-01T00:00:00Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 1);
    }

    #[test]
    fn test_leap_year_february_29() {
        let mut parser = AdaptiveTsParser::new();

        // Leap year - February 29, 2020
        let result = parser.parse_ts("2020-02-29T12:00:00Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 29);

        // Leap year - February 29, 2000 (divisible by 400)
        let result = parser.parse_ts("2000-02-29T12:00:00Z");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 29);

        // Non-leap year - February 29, 2019 (should fail)
        let result = parser.parse_ts("2019-02-29T12:00:00Z");
        assert!(result.is_none());

        // Century year not divisible by 400 - February 29, 1900 (should fail)
        let result = parser.parse_ts("1900-02-29T12:00:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_unix_timestamp_negative() {
        let mut parser = AdaptiveTsParser::new();

        // Negative Unix timestamps are not supported by the digit-only parser
        // (it only handles positive integers)
        let result = parser.parse_ts("-1");
        // Should parse as relative time "-1" which is invalid
        assert!(result.is_none());
    }

    #[test]
    fn test_unix_timestamp_overflow() {
        let mut parser = AdaptiveTsParser::new();

        // Test Unix timestamp at maximum i64 - this is 19 digits so treated as nanoseconds
        // The parser will handle it but the resulting DateTime may not be representable
        let _result = parser.parse_ts("9223372036854775807"); // i64::MAX
                                                              // This may succeed or fail depending on DateTime::from_timestamp limits
                                                              // We just verify it doesn't panic

        // Test very large timestamp that would overflow i64 parsing
        let result = parser.parse_ts("99999999999999999999"); // 20 digits, too large for i64
        assert!(result.is_none()); // Should fail at parsing stage
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Custom Format Strings
    // ============================================================================

    #[test]
    fn test_custom_format_with_timezone() {
        let mut parser = AdaptiveTsParser::new();

        // Custom format: DD/MM/YYYY HH:MM:SS (parse as UTC to avoid local timezone issues)
        let custom_format = "%d/%m/%Y %H:%M:%S";
        let result =
            parser.parse_ts_with_config("15/07/2023 14:30:45", Some(custom_format), Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);

        // Custom format with timezone
        let result = parser.parse_ts_with_config(
            "15/07/2023 14:30:45",
            Some(custom_format),
            Some("America/New_York"),
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_custom_format_invalid() {
        let mut parser = AdaptiveTsParser::new();

        // Invalid custom format (wrong pattern) but input still parseable by standard formats
        let custom_format = "%invalid%format";
        let result = parser.parse_ts_with_config("2023-07-15 14:30:45", Some(custom_format), None);
        // Will fall back to standard formats and succeed
        assert!(result.is_some());

        // Custom format doesn't match input but input isn't parseable by any format
        let custom_format = "%Y-%m-%d";
        let result =
            parser.parse_ts_with_config("not-a-valid-timestamp", Some(custom_format), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_syslog_year_rollover() {
        let mut parser = AdaptiveTsParser::new();

        // Syslog format without year - should add current year (parse as UTC to avoid local timezone issues)
        let result = parser.parse_ts_with_config("Dec 31 23:59:59", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 31);

        // Should also try previous year if it's early January
        let result = parser.parse_ts_with_config("Jan 1 00:00:01", None, Some("UTC"));
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Invalid Formats
    // ============================================================================

    #[test]
    fn test_invalid_month_values() {
        let mut parser = AdaptiveTsParser::new();

        // Month 0 (invalid)
        let result = parser.parse_ts("2023-00-15T12:00:00Z");
        assert!(result.is_none());

        // Month 13 (invalid)
        let result = parser.parse_ts("2023-13-15T12:00:00Z");
        assert!(result.is_none());

        // Month 99 (invalid)
        let result = parser.parse_ts("2023-99-15T12:00:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_day_values() {
        let mut parser = AdaptiveTsParser::new();

        // Day 0 (invalid)
        let result = parser.parse_ts("2023-07-00T12:00:00Z");
        assert!(result.is_none());

        // Day 32 in July (invalid - July has 31 days)
        let result = parser.parse_ts("2023-07-32T12:00:00Z");
        assert!(result.is_none());

        // Day 31 in February (invalid)
        let result = parser.parse_ts("2023-02-31T12:00:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_time_values() {
        let mut parser = AdaptiveTsParser::new();

        // Hour 24 (invalid - should be 0-23)
        let result = parser.parse_ts("2023-07-15T24:00:00Z");
        assert!(result.is_none());

        // Hour 25 (invalid)
        let result = parser.parse_ts("2023-07-15T25:00:00Z");
        assert!(result.is_none());

        // Minute 60 (invalid - should be 0-59)
        let result = parser.parse_ts("2023-07-15T12:60:00Z");
        assert!(result.is_none());

        // Second 60 (invalid - should be 0-59, 60 is only for leap seconds)
        let _result = parser.parse_ts("2023-07-15T12:00:60Z");
        // Note: Some parsers accept 60 for leap seconds
        // We just verify it doesn't panic

        // Second 61 (invalid)
        let result = parser.parse_ts("2023-07-15T12:00:61Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_malformed_timestamps() {
        let mut parser = AdaptiveTsParser::new();

        // Empty string
        let result = parser.parse_ts("");
        assert!(result.is_none());

        // Whitespace only
        let result = parser.parse_ts("   ");
        assert!(result.is_none());

        // Partial timestamp
        let result = parser.parse_ts("2023-07");
        assert!(result.is_none());

        // Missing separators
        let result = parser.parse_ts("20230715120000");
        assert!(result.is_none());

        // Mixed formats
        let _result = parser.parse_ts("2023-07-15 12:00:00Z");
        // This might actually parse since space-separated ISO is supported
        // We just verify it doesn't panic

        // Garbage input
        let result = parser.parse_ts("not-a-timestamp-at-all");
        assert!(result.is_none());

        // Special characters
        let result = parser.parse_ts("2023@07@15T12:00:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn test_very_long_timestamp_strings() {
        let mut parser = AdaptiveTsParser::new();

        // Very long string (should not cause issues)
        let mut long_string = "2023-07-15T12:00:00Z".to_string();
        long_string.push_str(&"x".repeat(10000));
        let result = parser.parse_ts(&long_string);
        assert!(result.is_none());

        // String with embedded valid timestamp
        let result = parser.parse_ts("prefix 2023-07-15T12:00:00Z suffix");
        // Should fail since we expect exact match (after trim)
        assert!(result.is_none());
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - RFC2822 Format
    // ============================================================================

    #[test]
    fn test_rfc2822_parsing() {
        let mut parser = AdaptiveTsParser::new();

        // Standard RFC2822 format
        let result = parser.parse_ts("Tue, 1 Jul 2003 10:52:37 +0200");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2003);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 1);

        // RFC2822 with different timezone - Sat, 15 Jul 2023 14:30:00 -0700
        let result = parser.parse_ts("Sat, 15 Jul 2023 14:30:00 -0700");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 15);
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Duration Parsing Edge Cases
    // ============================================================================

    #[test]
    fn test_duration_parsing_edge_cases() {
        // Zero duration
        let result = parse_duration("0s");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_seconds(), 0);

        // Very large duration
        let result = parse_duration("999999h");
        assert!(result.is_ok());

        // Empty unit (should fail)
        let result = parse_duration("100");
        assert!(result.is_err());

        // Invalid unit (should fail)
        let result = parse_duration("100x");
        assert!(result.is_err());

        // Space between number and unit
        let result = parse_duration("10 hours");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_hours(), -10); // Defaults to negative (past)

        // Multiple spaces (actually supported due to trim_start)
        let result = parse_duration("10   hours");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_hours(), -10);

        // Negative zero
        let result = parse_duration("-0h");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_seconds(), 0);
    }

    #[test]
    fn test_duration_overflow_protection() {
        // Duration that would overflow i64
        let result = parse_duration("999999999999999999999999999999h");
        assert!(result.is_err());
        // Error message could be about parsing the number or range overflow
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("out of supported range") || err_msg.contains("Invalid number"));
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Relative Time Edge Cases
    // ============================================================================

    #[test]
    fn test_looks_like_relative_time_edge_cases() {
        // Valid relative times
        assert!(looks_like_relative_time("1h"));
        assert!(looks_like_relative_time("30m"));
        assert!(looks_like_relative_time("2d"));
        assert!(looks_like_relative_time("1 hour"));
        assert!(looks_like_relative_time("30 minutes"));

        // Invalid relative times
        assert!(!looks_like_relative_time("h")); // No number
        assert!(!looks_like_relative_time("abc")); // No number at start
        assert!(!looks_like_relative_time("1")); // No unit
        assert!(!looks_like_relative_time("1x")); // Invalid unit
        assert!(!looks_like_relative_time("")); // Empty
        assert!(!looks_like_relative_time("  ")); // Whitespace only
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Date/Time Only Parsing Edge Cases
    // ============================================================================

    #[test]
    fn test_date_only_various_formats() {
        // YYYY-MM-DD
        let result = parse_date_only("2023-07-15");
        assert!(result.is_some());

        // YYYY/MM/DD
        let result = parse_date_only("2023/07/15");
        assert!(result.is_some());

        // MM/DD/YYYY (US format)
        let result = parse_date_only("07/15/2023");
        assert!(result.is_some());

        // DD.MM.YYYY (German format)
        let result = parse_date_only("15.07.2023");
        assert!(result.is_some());

        // Month name formats
        let result = parse_date_only("July 15, 2023");
        assert!(result.is_some());

        let result = parse_date_only("15 July 2023");
        assert!(result.is_some());

        // Invalid date formats
        let result = parse_date_only("2023-13-45");
        assert!(result.is_none());

        let result = parse_date_only("not-a-date");
        assert!(result.is_none());
    }

    #[test]
    fn test_time_only_various_formats() {
        // HH:MM:SS
        let result = parse_time_only("14:30:45");
        assert!(result.is_some());

        // HH:MM
        let result = parse_time_only("14:30");
        assert!(result.is_some());

        // Invalid time formats
        let result = parse_time_only("25:00:00");
        assert!(result.is_none());

        let result = parse_time_only("14:60:00");
        assert!(result.is_none());

        let result = parse_time_only("not-a-time");
        assert!(result.is_none());
    }

    // ============================================================================
    // P0 EDGE CASE TESTS - Fractional Seconds Edge Cases
    // ============================================================================

    #[test]
    fn test_fractional_seconds_various_precisions() {
        let mut parser = AdaptiveTsParser::new();

        // Milliseconds (3 digits)
        let result = parser.parse_ts("2023-07-15T12:00:00.123Z");
        assert!(result.is_some());

        // Microseconds (6 digits)
        let result = parser.parse_ts("2023-07-15T12:00:00.123456Z");
        assert!(result.is_some());

        // Nanoseconds (9 digits)
        let result = parser.parse_ts("2023-07-15T12:00:00.123456789Z");
        assert!(result.is_some());

        // Single digit (deciseconds)
        let result = parser.parse_ts("2023-07-15T12:00:00.1Z");
        assert!(result.is_some());

        // Two digits (centiseconds)
        let result = parser.parse_ts("2023-07-15T12:00:00.12Z");
        assert!(result.is_some());

        // More than 9 digits (should truncate or handle gracefully)
        let result = parser.parse_ts("2023-07-15T12:00:00.123456789012Z");
        // Should still parse, possibly truncating extra precision
        assert!(result.is_some());
    }

    #[test]
    fn test_microsecond_unix_timestamp() {
        let mut parser = AdaptiveTsParser::new();

        // 16-digit microsecond timestamp
        let result = parser.parse_ts("1689422400000000"); // 2023-07-15 12:00:00 UTC in microseconds
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 15);
    }

    #[test]
    fn test_nanosecond_unix_timestamp() {
        let mut parser = AdaptiveTsParser::new();

        // 19-digit nanosecond timestamp
        let result = parser.parse_ts("1689422400000000000"); // 2023-07-15 12:00:00 UTC in nanoseconds
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 15);
    }
}
