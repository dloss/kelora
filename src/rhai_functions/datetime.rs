use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use rhai::{Engine, EvalAltResult, Position};
use std::cell::RefCell;
use std::fmt;
use std::sync::OnceLock;

/// Wrapper for chrono::DateTime to provide Rhai integration
#[derive(Debug, Clone)]
pub struct DateTimeWrapper {
    pub inner: DateTime<Tz>,
}

impl DateTimeWrapper {
    pub fn new(dt: DateTime<Tz>) -> Self {
        Self { inner: dt }
    }

    pub fn from_utc(dt: DateTime<Utc>) -> Self {
        Self {
            inner: dt.with_timezone(&Tz::UTC),
        }
    }
}

impl fmt::Display for DateTimeWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner.to_rfc3339())
    }
}

/// Wrapper for chrono::Duration to provide Rhai integration
#[derive(Debug, Clone)]
pub struct DurationWrapper {
    pub inner: Duration,
}

impl DurationWrapper {
    pub fn new(dur: Duration) -> Self {
        // Ensure durations are always non-negative as per spec
        Self { inner: dur.abs() }
    }

    pub fn from_seconds(secs: i64) -> Self {
        Self::new(Duration::seconds(secs))
    }

    pub fn from_minutes(mins: i64) -> Self {
        Self::new(Duration::minutes(mins))
    }

    pub fn from_hours(hours: i64) -> Self {
        Self::new(Duration::hours(hours))
    }

    pub fn from_days(days: i64) -> Self {
        Self::new(Duration::days(days))
    }

    pub fn from_milliseconds(ms: i64) -> Self {
        Self::new(Duration::milliseconds(ms))
    }

    pub fn from_nanoseconds(ns: i64) -> Self {
        Self::new(Duration::nanoseconds(ns))
    }
}

impl fmt::Display for DurationWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_seconds = self.inner.num_seconds();
        if total_seconds < 60 {
            write!(f, "{}s", total_seconds)
        } else if total_seconds < 3600 {
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            if seconds == 0 {
                write!(f, "{}m", minutes)
            } else {
                write!(f, "{}m {}s", minutes, seconds)
            }
        } else if total_seconds < 86400 {
            let hours = total_seconds / 3600;
            let remaining = total_seconds % 3600;
            let minutes = remaining / 60;
            if minutes == 0 {
                write!(f, "{}h", hours)
            } else {
                write!(f, "{}h {}m", hours, minutes)
            }
        } else {
            let days = total_seconds / 86400;
            let remaining = total_seconds % 86400;
            let hours = remaining / 3600;
            if hours == 0 {
                write!(f, "{}d", days)
            } else {
                write!(f, "{}d {}h", days, hours)
            }
        }
    }
}

// Thread-local adaptive parser for Rhai timestamp parsing
thread_local! {
    static RHAI_TS_PARSER: RefCell<crate::timestamp::AdaptiveTsParser> =
        RefCell::new(crate::timestamp::AdaptiveTsParser::new());
}

/// Convert a string into a `DateTimeWrapper` using optional format and timezone hints.
pub fn to_datetime(
    s: &str,
    format: Option<&str>,
    tz: Option<&str>,
) -> Result<DateTimeWrapper, Box<EvalAltResult>> {
    // Default timezone
    let default_tz = if let Some(tz_str) = tz {
        tz_str.parse::<Tz>().map_err(|e| {
            Box::new(EvalAltResult::ErrorRuntime(
                format!("Invalid timezone '{}': {}", tz_str, e).into(),
                Position::NONE,
            ))
        })?
    } else {
        Tz::UTC
    };

    // Try explicit format first
    if let Some(fmt) = format {
        if let Ok(naive_dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTimeWrapper::new(
                default_tz.from_utc_datetime(&naive_dt),
            ));
        }
        // Also try with timezone-aware parsing for explicit format
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Ok(DateTimeWrapper::new(dt.with_timezone(&default_tz)));
        }

        // If explicit format was provided but failed, return error immediately
        // Don't fall back to adaptive parsing
        return Err(Box::new(EvalAltResult::ErrorRuntime(
            format!("Failed to parse '{}' with format '{}'", s, fmt).into(),
            Position::NONE,
        )));
    }

    // For auto-parsing (no explicit format), use the adaptive parser
    // Rhai scripts use UTC interpretation for consistency
    let parsed_utc = RHAI_TS_PARSER.with(|parser| {
        parser
            .borrow_mut()
            .parse_ts_with_config(s, None, Some("UTC"))
    });

    if let Some(utc_dt) = parsed_utc {
        // Convert to the requested timezone
        let tz_dt = if default_tz == Tz::UTC {
            utc_dt.with_timezone(&Tz::UTC)
        } else {
            utc_dt.with_timezone(&default_tz)
        };
        return Ok(DateTimeWrapper::new(tz_dt));
    }

    Err(Box::new(EvalAltResult::ErrorRuntime(
        format!("Unable to parse timestamp: '{}'", s).into(),
        Position::NONE,
    )))
}

/// Convert a string like "1h 30m" or "2d" into a `DurationWrapper`.
pub fn to_duration(s: &str) -> Result<DurationWrapper, Box<EvalAltResult>> {
    let mut total_duration = Duration::zero();
    let mut current_number = String::new();
    let mut current_unit = String::new();
    let mut found_unit = false;

    fn push_duration(
        total: &mut Duration,
        number: &str,
        unit: &str,
    ) -> Result<(), Box<EvalAltResult>> {
        if number.is_empty() || unit.is_empty() {
            return Err(Box::new(EvalAltResult::ErrorRuntime(
                "Incomplete duration segment".into(),
                Position::NONE,
            )));
        }

        let value: f64 = number.parse().map_err(|_| {
            Box::new(EvalAltResult::ErrorRuntime(
                format!("Invalid number in duration: '{}'", number).into(),
                Position::NONE,
            ))
        })?;

        let unit_norm = unit.to_lowercase();
        let nanos_per_unit: f64 = match unit_norm.as_str() {
            "ns" | "nsec" | "nsecs" | "nanosecond" | "nanoseconds" => 1.0,
            "us" | "µs" | "usec" | "usecs" | "microsecond" | "microseconds" => 1_000.0,
            "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => 1_000_000.0,
            "s" | "sec" | "secs" | "second" | "seconds" => 1_000_000_000.0,
            "m" | "min" | "mins" | "minute" | "minutes" => 60.0 * 1_000_000_000.0,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3_600.0 * 1_000_000_000.0,
            "d" | "day" | "days" => 86_400.0 * 1_000_000_000.0,
            "w" | "week" | "weeks" => 604_800.0 * 1_000_000_000.0,
            _ => {
                return Err(Box::new(EvalAltResult::ErrorRuntime(
                    format!("Unknown duration unit: '{}'", unit).into(),
                    Position::NONE,
                )))
            }
        };

        let nanos = (value * nanos_per_unit).round();
        if !nanos.is_finite() {
            return Err(Box::new(EvalAltResult::ErrorRuntime(
                "Duration value out of range".into(),
                Position::NONE,
            )));
        }

        let nanos_i128 = nanos as i128;
        if nanos_i128 > i64::MAX as i128 || nanos_i128 < i64::MIN as i128 {
            return Err(Box::new(EvalAltResult::ErrorRuntime(
                "Duration value out of range".into(),
                Position::NONE,
            )));
        }

        *total += Duration::nanoseconds(nanos_i128 as i64);
        Ok(())
    }

    let mut chars = s.chars().peekable();
    let mut number_has_decimal = false;
    while let Some(ch) = chars.next() {
        if ch.is_ascii_whitespace() {
            if !current_unit.is_empty() {
                push_duration(&mut total_duration, &current_number, &current_unit)?;
                current_number.clear();
                current_unit.clear();
                number_has_decimal = false;
                found_unit = true;
            }
            continue;
        }

        if ch.is_ascii_digit() || ch == '.' {
            if ch == '.' {
                if number_has_decimal {
                    return Err(Box::new(EvalAltResult::ErrorRuntime(
                        "Multiple decimal points in duration number".into(),
                        Position::NONE,
                    )));
                }
                if current_number.is_empty() {
                    return Err(Box::new(EvalAltResult::ErrorRuntime(
                        "Duration numbers cannot start with a decimal point".into(),
                        Position::NONE,
                    )));
                }
                number_has_decimal = true;
            }

            if !current_unit.is_empty() {
                push_duration(&mut total_duration, &current_number, &current_unit)?;
                current_number.clear();
                current_unit.clear();
                number_has_decimal = ch == '.';
                found_unit = true;
                if ch == '.' {
                    return Err(Box::new(EvalAltResult::ErrorRuntime(
                        "Duration numbers cannot start with a decimal point".into(),
                        Position::NONE,
                    )));
                }
            }

            current_number.push(ch);
        } else if ch.is_ascii_alphabetic() || ch == 'µ' {
            if current_number.is_empty() {
                return Err(Box::new(EvalAltResult::ErrorRuntime(
                    "Duration unit must follow a number".into(),
                    Position::NONE,
                )));
            }
            current_unit.push(ch);

            if let Some(next) = chars.peek() {
                if next.is_ascii_whitespace() {
                    continue;
                }
                if next.is_ascii_digit() || *next == '.' {
                    push_duration(&mut total_duration, &current_number, &current_unit)?;
                    current_number.clear();
                    current_unit.clear();
                    number_has_decimal = false;
                    found_unit = true;
                }
            }
        } else {
            return Err(Box::new(EvalAltResult::ErrorRuntime(
                format!("Invalid character in duration: '{}'", ch).into(),
                Position::NONE,
            )));
        }
    }

    if !current_unit.is_empty() {
        push_duration(&mut total_duration, &current_number, &current_unit)?;
        found_unit = true;
    }

    if !found_unit {
        return Err(Box::new(EvalAltResult::ErrorRuntime(
            format!("Unable to parse duration: '{}'", s).into(),
            Position::NONE,
        )));
    }

    Ok(DurationWrapper::new(total_duration))
}

/// Floor-divide: always round toward negative infinity.
/// Rust's `/` truncates toward zero, which is wrong for negative timestamps.
fn floor_nanos(timestamp_nanos: i64, interval_nanos: i64) -> i64 {
    let d = timestamp_nanos / interval_nanos;
    let r = timestamp_nanos % interval_nanos;
    // If remainder is negative, we truncated toward zero — adjust down by one interval
    let floored_d = if r < 0 { d - 1 } else { d };
    floored_d * interval_nanos
}

fn parse_positive_interval_nanos(interval: &str) -> Result<i64, Box<EvalAltResult>> {
    let duration = to_duration(interval)?;
    let nanos = duration.inner.num_nanoseconds().ok_or_else(|| {
        EvalAltResult::ErrorRuntime("Duration out of range".into(), Position::NONE)
    })?;
    if nanos <= 0 {
        return Err(Box::new(EvalAltResult::ErrorRuntime(
            "Interval must be positive".into(),
            Position::NONE,
        )));
    }
    Ok(nanos)
}

/// Round a datetime down to the nearest interval
pub fn round_to(
    dt: &mut DateTimeWrapper,
    interval: &str,
) -> Result<DateTimeWrapper, Box<EvalAltResult>> {
    let interval_nanos = parse_positive_interval_nanos(interval)?;
    let timestamp_nanos = dt.inner.timestamp_nanos_opt().ok_or_else(|| {
        EvalAltResult::ErrorRuntime("Timestamp out of range".into(), Position::NONE)
    })?;

    let floored = floor_nanos(timestamp_nanos, interval_nanos);
    let rounded_dt = Utc
        .timestamp_nanos(floored)
        .with_timezone(&dt.inner.timezone());

    Ok(DateTimeWrapper::new(rounded_dt))
}

/// Round a datetime up to the next interval boundary (ceiling)
///
/// If the timestamp is already exactly on a boundary, it stays unchanged.
/// Otherwise it advances to the next boundary.
pub fn ceil_to(
    dt: &mut DateTimeWrapper,
    interval: &str,
) -> Result<DateTimeWrapper, Box<EvalAltResult>> {
    let interval_nanos = parse_positive_interval_nanos(interval)?;
    let timestamp_nanos = dt.inner.timestamp_nanos_opt().ok_or_else(|| {
        EvalAltResult::ErrorRuntime("Timestamp out of range".into(), Position::NONE)
    })?;

    let floored = floor_nanos(timestamp_nanos, interval_nanos);
    let ceiled = if floored == timestamp_nanos {
        floored
    } else {
        floored + interval_nanos
    };

    let ceiled_dt = Utc
        .timestamp_nanos(ceiled)
        .with_timezone(&dt.inner.timezone());

    Ok(DateTimeWrapper::new(ceiled_dt))
}

/// Resolve the system's local timezone for `to_local()`.
///
/// Resolution order:
/// 1. The `TZ` environment variable, if it holds a valid IANA name
///    (e.g. `Europe/Berlin`). `iana-time-zone` ignores `TZ` on most
///    platforms, so we honor it explicitly to match Unix expectations and
///    to keep the behavior testable.
/// 2. The OS-reported local timezone via `iana-time-zone`.
///
/// Returns `None` if neither source yields a parseable IANA timezone.
fn resolve_local_timezone() -> Option<Tz> {
    if let Ok(tz) = std::env::var("TZ") {
        if let Ok(parsed) = tz.parse::<Tz>() {
            return Some(parsed);
        }
    }
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
}

/// The system's local timezone, resolved once and cached for the process.
///
/// Falls back to UTC with a single diagnostic warning if the local timezone
/// cannot be determined. Caching avoids re-resolving on every `to_local()`
/// call in the streaming pipeline.
fn local_timezone() -> Tz {
    static LOCAL_TZ: OnceLock<Tz> = OnceLock::new();
    *LOCAL_TZ.get_or_init(|| {
        resolve_local_timezone().unwrap_or_else(|| {
            eprintln!(
                "kelora: could not determine system local timezone; to_local() falling back to UTC"
            );
            Tz::UTC
        })
    })
}

/// Register all datetime functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // Parsing functions
    engine.register_fn(
        "to_datetime",
        |s: &str| -> Result<DateTimeWrapper, Box<EvalAltResult>> { to_datetime(s, None, None) },
    );

    engine.register_fn(
        "to_datetime",
        |s: &str, format: &str| -> Result<DateTimeWrapper, Box<EvalAltResult>> {
            to_datetime(s, Some(format), None)
        },
    );

    engine.register_fn(
        "to_datetime",
        |s: &str, format: &str, tz: &str| -> Result<DateTimeWrapper, Box<EvalAltResult>> {
            to_datetime(s, Some(format), Some(tz))
        },
    );

    engine.register_fn("to_duration", to_duration);

    // Current time helper
    engine.register_fn("now", || DateTimeWrapper::from_utc(Utc::now()));

    // Duration creation functions
    engine.register_fn("duration_from_seconds", DurationWrapper::from_seconds);
    engine.register_fn("duration_from_minutes", DurationWrapper::from_minutes);
    engine.register_fn("duration_from_hours", DurationWrapper::from_hours);
    engine.register_fn("duration_from_days", DurationWrapper::from_days);
    engine.register_fn(
        "duration_from_milliseconds",
        DurationWrapper::from_milliseconds,
    );
    engine.register_fn(
        "duration_from_nanoseconds",
        DurationWrapper::from_nanoseconds,
    );

    // Humanize milliseconds to readable duration format
    engine.register_fn("humanize_duration", |ms: i64| -> String {
        DurationWrapper::from_milliseconds(ms).to_string()
    });

    // Register the custom types
    engine
        .register_type::<DateTimeWrapper>()
        .register_type::<DurationWrapper>();

    // DateTime methods
    engine.register_fn("to_utc", |dt: &mut DateTimeWrapper| {
        DateTimeWrapper::new(dt.inner.with_timezone(&Tz::UTC))
    });

    engine.register_fn("to_local", |dt: &mut DateTimeWrapper| {
        DateTimeWrapper::new(dt.inner.with_timezone(&local_timezone()))
    });

    engine.register_fn(
        "to_timezone",
        |dt: &mut DateTimeWrapper, tz: &str| -> Result<DateTimeWrapper, Box<EvalAltResult>> {
            let timezone = tz.parse::<Tz>().map_err(|e| {
                Box::new(EvalAltResult::ErrorRuntime(
                    format!("Invalid timezone '{}': {}", tz, e).into(),
                    Position::NONE,
                ))
            })?;
            Ok(DateTimeWrapper::new(dt.inner.with_timezone(&timezone)))
        },
    );

    engine.register_fn("to_iso", |dt: &mut DateTimeWrapper| -> String {
        dt.inner.to_rfc3339()
    });

    engine.register_fn("format", |dt: &mut DateTimeWrapper, fmt: &str| -> String {
        dt.inner.format(fmt).to_string()
    });

    engine.register_fn("year", |dt: &mut DateTimeWrapper| dt.inner.year() as i64);
    engine.register_fn("month", |dt: &mut DateTimeWrapper| dt.inner.month() as i64);
    engine.register_fn("day", |dt: &mut DateTimeWrapper| dt.inner.day() as i64);
    engine.register_fn("hour", |dt: &mut DateTimeWrapper| dt.inner.hour() as i64);
    engine.register_fn("minute", |dt: &mut DateTimeWrapper| {
        dt.inner.minute() as i64
    });
    engine.register_fn("second", |dt: &mut DateTimeWrapper| {
        dt.inner.second() as i64
    });
    engine.register_fn("ts_nanos", |dt: &mut DateTimeWrapper| {
        dt.inner.timestamp_nanos_opt().unwrap_or(0)
    });
    engine.register_fn("timezone_name", |dt: &mut DateTimeWrapper| {
        dt.inner.timezone().to_string()
    });

    // Time bucketing
    engine.register_fn("round_to", round_to);
    engine.register_fn("ceil_to", ceil_to);

    // Duration methods
    engine.register_fn("as_seconds", |dur: &mut DurationWrapper| {
        dur.inner.num_seconds()
    });
    engine.register_fn("as_milliseconds", |dur: &mut DurationWrapper| {
        dur.inner.num_milliseconds()
    });
    engine.register_fn("as_nanoseconds", |dur: &mut DurationWrapper| {
        dur.inner.num_nanoseconds().unwrap_or(0)
    });
    engine.register_fn("as_minutes", |dur: &mut DurationWrapper| {
        dur.inner.num_minutes()
    });
    engine.register_fn("as_hours", |dur: &mut DurationWrapper| {
        dur.inner.num_hours()
    });
    engine.register_fn("as_days", |dur: &mut DurationWrapper| dur.inner.num_days());

    // Duration string conversion - enables to_string() and print() formatting
    engine.register_fn("to_string", |dur: &mut DurationWrapper| -> String {
        dur.to_string()
    });
    engine.register_fn("to_debug", |dur: &mut DurationWrapper| -> String {
        dur.to_string()
    });

    // DateTime arithmetic
    engine.register_fn("+", |dt: DateTimeWrapper, dur: DurationWrapper| {
        DateTimeWrapper::new(dt.inner + dur.inner)
    });

    engine.register_fn("-", |dt: DateTimeWrapper, dur: DurationWrapper| {
        DateTimeWrapper::new(dt.inner - dur.inner)
    });

    engine.register_fn("-", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        DurationWrapper::new((dt1.inner - dt2.inner).abs())
    });

    // Duration arithmetic
    engine.register_fn("+", |dur1: DurationWrapper, dur2: DurationWrapper| {
        DurationWrapper::new(dur1.inner + dur2.inner)
    });

    engine.register_fn("-", |dur1: DurationWrapper, dur2: DurationWrapper| {
        DurationWrapper::new((dur1.inner - dur2.inner).abs())
    });

    engine.register_fn("*", |dur: DurationWrapper, n: i64| {
        DurationWrapper::new(dur.inner * n as i32)
    });

    engine.register_fn("/", |dur: DurationWrapper, n: i64| {
        DurationWrapper::new(dur.inner / n as i32)
    });

    // Duration comparison
    engine.register_fn("==", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner == dur2.inner
    });
    engine.register_fn("!=", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner != dur2.inner
    });
    engine.register_fn(">", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner > dur2.inner
    });
    engine.register_fn("<", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner < dur2.inner
    });
    engine.register_fn(">=", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner >= dur2.inner
    });
    engine.register_fn("<=", |dur1: DurationWrapper, dur2: DurationWrapper| {
        dur1.inner <= dur2.inner
    });

    // DateTime comparison
    engine.register_fn("==", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner == dt2.inner
    });
    engine.register_fn("!=", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner != dt2.inner
    });
    engine.register_fn(">", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner > dt2.inner
    });
    engine.register_fn("<", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner < dt2.inner
    });
    engine.register_fn(">=", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner >= dt2.inner
    });
    engine.register_fn("<=", |dt1: DateTimeWrapper, dt2: DateTimeWrapper| {
        dt1.inner <= dt2.inner
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes tests that mutate the process-wide `TZ` variable and restores
    // its previous value afterwards. Uses the crate-wide environment lock so it
    // also serializes against env-mutating tests in other modules.
    struct TzGuard {
        previous: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TzGuard {
        fn set(value: &str) -> Self {
            let lock = crate::test_env::lock_env();
            let previous = std::env::var("TZ").ok();
            std::env::set_var("TZ", value);
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for TzGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("TZ", v),
                None => std::env::remove_var("TZ"),
            }
        }
    }

    #[test]
    fn test_resolve_local_timezone_honors_tz_env() {
        let _guard = TzGuard::set("America/New_York");
        assert_eq!(resolve_local_timezone(), Some(Tz::America__New_York));
    }

    #[test]
    fn test_to_local_converts_using_resolved_timezone() {
        // Regression test: to_local() must not silently stay in UTC.
        let _guard = TzGuard::set("America/New_York");
        let tz = resolve_local_timezone().expect("TZ should resolve");
        let dt = to_datetime("2023-07-04T12:00:00Z", None, None).unwrap();
        let local = dt.inner.with_timezone(&tz);
        // New York observes EDT (UTC-4) in July, so 12:00 UTC -> 08:00 local.
        assert_eq!(local.hour(), 8);
        assert_eq!(local.day(), 4);
    }

    #[test]
    fn test_duration_wrapper_always_positive() {
        // Test that negative durations become positive
        let negative_dur = Duration::seconds(-100);
        let wrapper = DurationWrapper::new(negative_dur);
        assert_eq!(wrapper.inner.num_seconds(), 100);
    }

    #[test]
    fn test_duration_display_formatting() {
        // Test various duration display formats
        assert_eq!(DurationWrapper::from_seconds(30).to_string(), "30s");
        assert_eq!(DurationWrapper::from_seconds(60).to_string(), "1m");
        assert_eq!(DurationWrapper::from_seconds(90).to_string(), "1m 30s");
        assert_eq!(DurationWrapper::from_seconds(3600).to_string(), "1h");
        assert_eq!(DurationWrapper::from_seconds(3660).to_string(), "1h 1m");
        assert_eq!(DurationWrapper::from_seconds(86400).to_string(), "1d");
        assert_eq!(DurationWrapper::from_seconds(90000).to_string(), "1d 1h");
    }

    #[test]
    fn test_to_datetime_edge_cases() {
        // Test empty string
        assert!(to_datetime("", None, None).is_err());

        // Test invalid formats
        assert!(to_datetime("not-a-date", None, None).is_err());
        assert!(to_datetime("2023-13-01T12:00:00Z", None, None).is_err()); // Invalid month
        assert!(to_datetime("2023-02-30T12:00:00Z", None, None).is_err()); // Invalid day

        // Test valid edge cases
        assert!(to_datetime("2023-01-01T00:00:00Z", None, None).is_ok());
        assert!(to_datetime("2023-12-31T23:59:59Z", None, None).is_ok());
    }

    #[test]
    fn test_to_datetime_with_explicit_format() {
        // Test custom format parsing
        let result = to_datetime("2023/07/04 12:34:56", Some("%Y/%m/%d %H:%M:%S"), None);
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);

        // Test invalid format with explicit format
        assert!(to_datetime("2023-07-04", Some("%Y/%m/%d"), None).is_err());
    }

    #[test]
    fn test_to_datetime_with_timezone() {
        // Test parsing with valid timezone
        let result = to_datetime(
            "2023-07-04 12:34:56",
            Some("%Y-%m-%d %H:%M:%S"),
            Some("UTC"),
        );
        assert!(result.is_ok());

        // Test parsing with invalid timezone
        let result = to_datetime(
            "2023-07-04 12:34:56",
            Some("%Y-%m-%d %H:%M:%S"),
            Some("INVALID"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_to_duration_edge_cases() {
        // Test empty string
        assert!(to_duration("").is_err());

        // Test invalid characters
        assert!(to_duration("1x").is_err());
        assert!(to_duration("1h@30m").is_err());

        // Test invalid numbers
        assert!(to_duration("ah").is_err());

        // Test zero duration
        assert!(to_duration("0s").is_ok());

        // Test complex valid durations
        assert!(to_duration("1d 2h 3m 4s").is_ok());
        assert!(to_duration("100h").is_ok());
    }

    #[test]
    fn test_to_duration_various_formats() {
        // Test single units
        let dur_s = to_duration("30s").unwrap();
        assert_eq!(dur_s.inner.num_seconds(), 30);

        let dur_m = to_duration("5m").unwrap();
        assert_eq!(dur_m.inner.num_minutes(), 5);

        let dur_h = to_duration("2h").unwrap();
        assert_eq!(dur_h.inner.num_hours(), 2);

        let dur_d = to_duration("3d").unwrap();
        assert_eq!(dur_d.inner.num_days(), 3);

        let dur_ms = to_duration("250ms").unwrap();
        assert_eq!(dur_ms.inner.num_milliseconds(), 250);

        let dur_us = to_duration("500us").unwrap();
        assert_eq!(dur_us.inner.num_microseconds().unwrap(), 500);

        let dur_ns = to_duration("750ns").unwrap();
        assert_eq!(dur_ns.inner.num_nanoseconds().unwrap(), 750);

        // Test mixed units
        let dur_mixed = to_duration("1h 30m").unwrap();
        assert_eq!(dur_mixed.inner.num_minutes(), 90);

        // Test with extra spaces
        let dur_spaced = to_duration("  1h   30m  ").unwrap();
        assert_eq!(dur_spaced.inner.num_minutes(), 90);

        // Test compact format without spaces
        let dur_compact = to_duration("1m30s").unwrap();
        assert_eq!(dur_compact.inner.num_seconds(), 90);

        // Test millisecond subsequence with additional unit
        let dur_combo = to_duration("2s500ms").unwrap();
        assert_eq!(dur_combo.inner.num_milliseconds(), 2500);

        // Test fractional values
        let dur_fractional = to_duration("1.5s").unwrap();
        assert_eq!(dur_fractional.inner.num_milliseconds(), 1500);

        let dur_fractional_ms = to_duration("0.25ms").unwrap();
        assert_eq!(dur_fractional_ms.inner.num_nanoseconds().unwrap(), 250_000);

        let dur_fractional_minutes = to_duration("1.25m").unwrap();
        assert_eq!(dur_fractional_minutes.inner.num_seconds(), 75);
    }

    #[test]
    fn test_duration_arithmetic_non_negative() {
        let dur1 = DurationWrapper::from_hours(2);
        let dur2 = DurationWrapper::from_hours(3);

        // Subtraction that would normally be negative becomes positive
        let result = DurationWrapper::new((dur1.inner - dur2.inner).abs());
        assert_eq!(result.inner.num_hours(), 1);
    }

    #[test]
    fn test_datetime_wrapper_display() {
        let dt = to_datetime("2023-07-04T12:34:56Z", None, None).unwrap();
        let display_str = dt.to_string();
        assert!(display_str.contains("2023-07-04"));
        assert!(display_str.contains("12:34:56"));
    }

    #[test]
    fn test_datetime_component_access() {
        let dt = to_datetime("2023-07-04T12:34:56Z", None, None).unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);
    }

    #[test]
    fn test_duration_conversions() {
        let dur = DurationWrapper::from_hours(2);
        assert_eq!(dur.inner.num_hours(), 2);
        assert_eq!(dur.inner.num_minutes(), 120);
        assert_eq!(dur.inner.num_seconds(), 7200);

        let dur_ms = DurationWrapper::from_milliseconds(5000);
        assert_eq!(dur_ms.inner.num_seconds(), 5);
    }

    #[test]
    fn test_rfc3339_and_rfc2822_parsing() {
        // RFC3339
        let rfc3339_result = to_datetime("2023-07-04T12:34:56+00:00", None, None);
        assert!(rfc3339_result.is_ok());

        // RFC2822
        let rfc2822_result = to_datetime("Tue, 04 Jul 2023 12:34:56 +0000", None, None);
        assert!(rfc2822_result.is_ok());
    }

    #[test]
    fn test_standard_format_parsing() {
        // Apache log format
        let apache_result = to_datetime("04/Jul/2023:12:34:56 +0000", None, None);
        assert!(apache_result.is_ok());

        // Common log format
        let common_result = to_datetime("2023-07-04 12:34:56", None, None);
        assert!(common_result.is_ok());

        // ISO 8601 variants
        let iso_result = to_datetime("2023-07-04T12:34:56.123Z", None, None);
        assert!(iso_result.is_ok());
    }

    #[test]
    fn test_large_duration_values() {
        // Test very large durations
        let large_dur = DurationWrapper::from_days(365);
        assert_eq!(large_dur.inner.num_days(), 365);

        // Test nanosecond precision
        let nano_dur = DurationWrapper::from_nanoseconds(1_000_000_000);
        assert_eq!(nano_dur.inner.num_seconds(), 1);
    }

    #[test]
    fn test_boundary_conditions() {
        // Test leap year
        let leap_year_result = to_datetime("2024-02-29T12:00:00Z", None, None);
        assert!(leap_year_result.is_ok());

        // Test non-leap year (should fail)
        let non_leap_result = to_datetime("2023-02-29T12:00:00Z", None, None);
        assert!(non_leap_result.is_err());

        // Test year boundaries
        let y2k_result = to_datetime("2000-01-01T00:00:00Z", None, None);
        assert!(y2k_result.is_ok());
    }

    #[test]
    fn test_unix_timestamp_parsing() {
        // Test Unix timestamp in seconds (10 digits)
        let unix_seconds = to_datetime("1735566123", None, None);
        assert!(unix_seconds.is_ok());
        let dt = unix_seconds.unwrap();
        assert_eq!(dt.inner.year(), 2024);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 30);

        // Test Unix timestamp in milliseconds (13 digits)
        let unix_millis = to_datetime("1735566123000", None, None);
        assert!(unix_millis.is_ok());
        let dt = unix_millis.unwrap();
        assert_eq!(dt.inner.year(), 2024);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 30);

        // Test Unix timestamp in microseconds (16 digits)
        let unix_micros = to_datetime("1735566123000000", None, None);
        assert!(unix_micros.is_ok());
        let dt = unix_micros.unwrap();
        assert_eq!(dt.inner.year(), 2024);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 30);

        // Test Unix timestamp in nanoseconds (19 digits)
        let unix_nanos = to_datetime("1735566123000000000", None, None);
        assert!(unix_nanos.is_ok());
        let dt = unix_nanos.unwrap();
        assert_eq!(dt.inner.year(), 2024);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 30);

        // Test invalid Unix timestamp (wrong length)
        let invalid_unix = to_datetime("12345", None, None);
        assert!(invalid_unix.is_err());

        // Test Unix timestamp with non-numeric characters
        let invalid_chars = to_datetime("1735566123a", None, None);
        assert!(invalid_chars.is_err());
    }

    #[test]
    fn test_new_timestamp_formats() {
        // Test Python logging format with comma separator
        let python_result = to_datetime("2023-07-04 12:34:56,123", None, None);
        assert!(python_result.is_ok());
        let dt = python_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Note: Ambiguous formats like "7/4/2023 12:34:56 PM" are not supported
        // in automatic parsing due to month/day ambiguity. Use explicit format instead.

        // Test MySQL legacy format
        let mysql_result = to_datetime("230704 12:34:56", None, None);
        assert!(mysql_result.is_ok());
        let dt = mysql_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test Nginx error log format
        let nginx_result = to_datetime("2023/07/04 12:34:56", None, None);
        assert!(nginx_result.is_ok());
        let dt = nginx_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test BSD syslog with year
        let bsd_result = to_datetime("Jul 04 2023 12:34:56", None, None);
        assert!(bsd_result.is_ok());
        let dt = bsd_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test Java SimpleDateFormat
        let java_result = to_datetime("Jul 04, 2023 12:34:56 PM", None, None);
        assert!(java_result.is_ok());
        let dt = java_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test German format with dots
        let german_result = to_datetime("04.07.2023 12:34:56", None, None);
        assert!(german_result.is_ok());
        let dt = german_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);
    }

    #[test]
    fn test_klp_inspired_formats() {
        // Test space-separated ISO 8601 with fractional seconds and Z
        let space_iso_frac_z = to_datetime("2023-07-04 12:34:56.123Z", None, None);
        assert!(space_iso_frac_z.is_ok());
        let dt = space_iso_frac_z.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test space-separated ISO 8601 without fractional seconds but with Z
        let space_iso_z = to_datetime("2023-07-04 12:34:56Z", None, None);
        assert!(space_iso_z.is_ok());
        let dt = space_iso_z.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test space-separated ISO 8601 with timezone offset
        let space_iso_tz = to_datetime("2023-07-04 12:34:56+0000", None, None);
        assert!(space_iso_tz.is_ok());
        let dt = space_iso_tz.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test space-separated ISO 8601 with fractional seconds and timezone
        let space_iso_frac_tz = to_datetime("2023-07-04 12:34:56.123+0000", None, None);
        assert!(space_iso_frac_tz.is_ok());
        let dt = space_iso_frac_tz.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);

        // Test classic Unix timestamp with weekday
        // July 4th, 2023 was a Tuesday
        let unix_weekday = to_datetime("Tue Jul 04 12:34:56 2023", None, None);
        assert!(unix_weekday.is_ok());
        let dt = unix_weekday.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);
    }

    #[test]
    fn test_oracle_format_parsing() {
        // Test Oracle format - this one is complex and may need adjustment
        let oracle_result = to_datetime("04-JUL-23 12:34:56.123 PM", None, None);
        assert!(oracle_result.is_ok());
        let dt = oracle_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7);
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);
        assert_eq!(dt.inner.minute(), 34);
        assert_eq!(dt.inner.second(), 56);
    }

    #[test]
    fn test_adaptive_parsing_in_rhai() {
        // Test that the Rhai to_datetime function benefits from adaptive parsing
        // by parsing similar formats multiple times

        // First parse - should learn the format
        let result1 = to_datetime("2023-07-04 12:34:56", None, None);
        assert!(result1.is_ok());
        let dt1 = result1.unwrap();
        assert_eq!(dt1.inner.year(), 2023);
        assert_eq!(dt1.inner.month(), 7);
        assert_eq!(dt1.inner.day(), 4);

        // Second parse with same format - should be faster due to learning
        let result2 = to_datetime("2023-07-05 13:45:07", None, None);
        assert!(result2.is_ok());
        let dt2 = result2.unwrap();
        assert_eq!(dt2.inner.year(), 2023);
        assert_eq!(dt2.inner.month(), 7);
        assert_eq!(dt2.inner.day(), 5);

        // Third parse with same format - should still work efficiently
        let result3 = to_datetime("2023-07-06 14:56:08", None, None);
        assert!(result3.is_ok());
        let dt3 = result3.unwrap();
        assert_eq!(dt3.inner.year(), 2023);
        assert_eq!(dt3.inner.month(), 7);
        assert_eq!(dt3.inner.day(), 6);
    }

    #[test]
    fn test_explicit_format_for_ambiguous_dates() {
        // Test US format (M/D/YYYY) with explicit format
        let us_result = to_datetime("7/4/2023 12:34:56 PM", Some("%m/%d/%Y %I:%M:%S %p"), None);
        assert!(us_result.is_ok());
        let dt = us_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 7); // July (US interpretation)
        assert_eq!(dt.inner.day(), 4);
        assert_eq!(dt.inner.hour(), 12);

        // Test European format (D/M/YYYY) with explicit format
        let eu_result = to_datetime("7/4/2023 12:34:56", Some("%d/%m/%Y %H:%M:%S"), None);
        assert!(eu_result.is_ok());
        let dt = eu_result.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 4); // April (European interpretation)
        assert_eq!(dt.inner.day(), 7);
        assert_eq!(dt.inner.hour(), 12);

        // Test that ambiguous formats fail without explicit format
        let ambiguous_result = to_datetime("7/4/2023 12:34:56 PM", None, None);
        assert!(
            ambiguous_result.is_err(),
            "Ambiguous date format should fail without explicit format"
        );

        // Test Windows Event Log format variations with explicit format
        let windows_us = to_datetime("12/31/2023 11:59:59 PM", Some("%m/%d/%Y %I:%M:%S %p"), None);
        assert!(windows_us.is_ok());
        let dt = windows_us.unwrap();
        assert_eq!(dt.inner.year(), 2023);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 31);
        assert_eq!(dt.inner.hour(), 23); // 11 PM = 23:00
    }

    #[test]
    fn test_unix_timestamp_edge_cases() {
        // Test earliest Unix timestamp (1970-01-01 00:00:00)
        let epoch_result = to_datetime("0", None, None);
        assert!(epoch_result.is_err()); // Single digit should fail

        let epoch_result = to_datetime("0000000000", None, None);
        assert!(epoch_result.is_ok());
        let dt = epoch_result.unwrap();
        assert_eq!(dt.inner.year(), 1970);
        assert_eq!(dt.inner.month(), 1);
        assert_eq!(dt.inner.day(), 1);

        // Test year 2038 problem boundary
        let y2038_result = to_datetime("2147483647", None, None);
        assert!(y2038_result.is_ok());
        let dt = y2038_result.unwrap();
        assert_eq!(dt.inner.year(), 2038);
        assert_eq!(dt.inner.month(), 1);
        assert_eq!(dt.inner.day(), 19);

        // Test millisecond precision
        let millis_result = to_datetime("1735566123456", None, None);
        assert!(millis_result.is_ok());
        let dt = millis_result.unwrap();
        assert_eq!(dt.inner.year(), 2024);
        assert_eq!(dt.inner.month(), 12);
        assert_eq!(dt.inner.day(), 30);
    }

    #[test]
    fn test_round_to_minutes() {
        // 2023-07-04 12:34:56 UTC
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();

        // Round to 5 minutes - should go to 12:30:00
        let rounded = round_to(&mut dt, "5m").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 30);
        assert_eq!(rounded.inner.second(), 0);

        // Round to 1 minute - should go to 12:34:00
        let rounded = round_to(&mut dt, "1m").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 34);
        assert_eq!(rounded.inner.second(), 0);
    }

    #[test]
    fn test_round_to_hours() {
        // 2023-07-04 12:34:56 UTC
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();

        // Round to 1 hour - should go to 12:00:00
        let rounded = round_to(&mut dt, "1h").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 0);
        assert_eq!(rounded.inner.second(), 0);

        // Round to 6 hours - should go to 12:00:00
        let rounded = round_to(&mut dt, "6h").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 0);
        assert_eq!(rounded.inner.second(), 0);

        // Test 15:34:56 rounds to 12:00:00 with 6h interval
        let mut dt2 = to_datetime("2023-07-04 15:34:56Z", None, None).unwrap();
        let rounded = round_to(&mut dt2, "6h").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
    }

    #[test]
    fn test_round_to_days() {
        // 2023-07-04 12:34:56 UTC
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();

        // Round to 1 day - should go to 2023-07-04 00:00:00
        let rounded = round_to(&mut dt, "1d").unwrap();
        assert_eq!(rounded.inner.year(), 2023);
        assert_eq!(rounded.inner.month(), 7);
        assert_eq!(rounded.inner.day(), 4);
        assert_eq!(rounded.inner.hour(), 0);
        assert_eq!(rounded.inner.minute(), 0);
        assert_eq!(rounded.inner.second(), 0);
    }

    #[test]
    fn test_round_to_seconds() {
        // 2023-07-04 12:34:56.789Z
        let mut dt = to_datetime("2023-07-04T12:34:56.789Z", None, None).unwrap();

        // Round to 10 seconds - should go to 12:34:50.000
        let rounded = round_to(&mut dt, "10s").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 34);
        assert_eq!(rounded.inner.second(), 50);

        // Round to 30 seconds - should go to 12:34:30.000
        let rounded = round_to(&mut dt, "30s").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 34);
        assert_eq!(rounded.inner.second(), 30);
    }

    #[test]
    fn test_round_to_preserves_timezone() {
        // Parse as UTC then convert to America/New_York
        // This creates 2023-07-04 12:34:56 UTC, which is 08:34:56 EDT
        let dt_utc = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        let mut dt = DateTimeWrapper::new(
            dt_utc
                .inner
                .with_timezone(&"America/New_York".parse::<Tz>().unwrap()),
        );

        let rounded = round_to(&mut dt, "1h").unwrap();
        assert_eq!(rounded.inner.timezone().to_string(), "America/New_York");
        // 12:34:56 UTC = 08:34:56 EDT, rounds to 08:00:00 EDT
        assert_eq!(rounded.inner.hour(), 8);
        assert_eq!(rounded.inner.minute(), 0);
    }

    #[test]
    fn test_round_to_error_cases() {
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();

        // Invalid duration format
        assert!(round_to(&mut dt, "invalid").is_err());

        // Empty string
        assert!(round_to(&mut dt, "").is_err());
    }

    #[test]
    fn test_round_to_edge_cases() {
        // Test rounding exactly on the boundary
        let mut dt = to_datetime("2023-07-04 12:30:00Z", None, None).unwrap();
        let rounded = round_to(&mut dt, "5m").unwrap();
        assert_eq!(rounded.inner.hour(), 12);
        assert_eq!(rounded.inner.minute(), 30);
        assert_eq!(rounded.inner.second(), 0);

        // Test midnight crossing with days
        let mut dt2 = to_datetime("2023-07-05 00:00:00Z", None, None).unwrap();
        let rounded = round_to(&mut dt2, "1d").unwrap();
        assert_eq!(rounded.inner.day(), 5);
        assert_eq!(rounded.inner.hour(), 0);
    }

    #[test]
    fn test_ceil_to_minutes() {
        // 12:34:56 ceil to 5m -> 12:35:00
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "5m").unwrap();
        assert_eq!(ceiled.inner.hour(), 12);
        assert_eq!(ceiled.inner.minute(), 35);
        assert_eq!(ceiled.inner.second(), 0);
    }

    #[test]
    fn test_ceil_to_on_boundary() {
        // Already on a 5m boundary -> stays unchanged
        let mut dt = to_datetime("2023-07-04 12:30:00Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "5m").unwrap();
        assert_eq!(ceiled.inner.hour(), 12);
        assert_eq!(ceiled.inner.minute(), 30);
        assert_eq!(ceiled.inner.second(), 0);
    }

    #[test]
    fn test_ceil_to_hours() {
        // 12:34:56 ceil to 1h -> 13:00:00
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "1h").unwrap();
        assert_eq!(ceiled.inner.hour(), 13);
        assert_eq!(ceiled.inner.minute(), 0);
        assert_eq!(ceiled.inner.second(), 0);
    }

    #[test]
    fn test_ceil_to_on_hour_boundary() {
        // Already on hour boundary -> stays unchanged
        let mut dt = to_datetime("2023-07-04 12:00:00Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "1h").unwrap();
        assert_eq!(ceiled.inner.hour(), 12);
        assert_eq!(ceiled.inner.minute(), 0);
    }

    #[test]
    fn test_ceil_to_days() {
        // 12:34:56 ceil to 1d -> next day 00:00:00
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "1d").unwrap();
        assert_eq!(ceiled.inner.day(), 5);
        assert_eq!(ceiled.inner.hour(), 0);
        assert_eq!(ceiled.inner.minute(), 0);
    }

    #[test]
    fn test_ceil_to_error_cases() {
        let mut dt = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        assert!(ceil_to(&mut dt, "invalid").is_err());
        assert!(ceil_to(&mut dt, "").is_err());
    }

    #[test]
    fn test_ceil_to_preserves_timezone() {
        let dt_utc = to_datetime("2023-07-04 12:34:56Z", None, None).unwrap();
        let mut dt = DateTimeWrapper::new(
            dt_utc
                .inner
                .with_timezone(&"America/New_York".parse::<Tz>().unwrap()),
        );

        let ceiled = ceil_to(&mut dt, "1h").unwrap();
        assert_eq!(ceiled.inner.timezone().to_string(), "America/New_York");
    }

    // Pre-epoch tests: verify floor-toward-negative-infinity (not toward zero)

    #[test]
    fn test_round_to_pre_epoch() {
        // 1969-12-31 23:47:00 UTC — 13 minutes before epoch
        let mut dt = to_datetime("1969-12-31 23:47:00Z", None, None).unwrap();

        // Floor to 5m: should go to 23:45:00, not 23:50:00
        let rounded = round_to(&mut dt, "5m").unwrap();
        assert_eq!(rounded.inner.hour(), 23);
        assert_eq!(rounded.inner.minute(), 45);
        assert_eq!(rounded.inner.second(), 0);
    }

    #[test]
    fn test_round_to_pre_epoch_on_boundary() {
        // Exactly on a 5m boundary before epoch
        let mut dt = to_datetime("1969-12-31 23:45:00Z", None, None).unwrap();
        let rounded = round_to(&mut dt, "5m").unwrap();
        assert_eq!(rounded.inner.hour(), 23);
        assert_eq!(rounded.inner.minute(), 45);
    }

    #[test]
    fn test_ceil_to_pre_epoch() {
        // 1969-12-31 23:47:00 UTC
        let mut dt = to_datetime("1969-12-31 23:47:00Z", None, None).unwrap();

        // Ceil to 5m: should go to 23:50:00
        let ceiled = ceil_to(&mut dt, "5m").unwrap();
        assert_eq!(ceiled.inner.hour(), 23);
        assert_eq!(ceiled.inner.minute(), 50);
        assert_eq!(ceiled.inner.second(), 0);
    }

    #[test]
    fn test_ceil_to_pre_epoch_on_boundary() {
        // Exactly on a 5m boundary before epoch — should stay
        let mut dt = to_datetime("1969-12-31 23:45:00Z", None, None).unwrap();
        let ceiled = ceil_to(&mut dt, "5m").unwrap();
        assert_eq!(ceiled.inner.hour(), 23);
        assert_eq!(ceiled.inner.minute(), 45);
    }

    #[test]
    fn test_floor_nanos_positive() {
        assert_eq!(floor_nanos(7, 5), 5);
        assert_eq!(floor_nanos(10, 5), 10);
        assert_eq!(floor_nanos(0, 5), 0);
    }

    #[test]
    fn test_floor_nanos_negative() {
        // -7 floored by 5 should be -10, not -5
        assert_eq!(floor_nanos(-7, 5), -10);
        assert_eq!(floor_nanos(-10, 5), -10); // exactly on boundary
        assert_eq!(floor_nanos(-1, 5), -5);
    }
}
