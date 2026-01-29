/// Print timestamp format help
pub fn print_time_format_help() {
    let help_text = r#"
Time Format Reference for --ts-format:

Use with:
  --ts-format <FMT>     Describe how timestamps are parsed
  --input-tz <TZ>       Supply a timezone for inputs without offsets (e.g., --input-tz UTC)
  --multiline timestamp:format=FMT  Use the same chrono format for header detection

Basic date/time components:
%Y  Year with century (e.g., 2024)
%y  Year without century (00-99)
%m  Month as zero-padded decimal (01-12)
%b  Month as abbreviated name (Jan, Feb, ..., Dec)
%B  Month as full name (January, February, ..., December)
%d  Day of month as zero-padded decimal (01-31)
%j  Day of year as zero-padded decimal (001-366)
%H  Hour (24-hour) as zero-padded decimal (00-23)
%I  Hour (12-hour) as zero-padded decimal (01-12)
%p  AM/PM indicator
%M  Minute as zero-padded decimal (00-59)
%S  Second as zero-padded decimal (00-59)

Subsecond precision cheatsheet:
%f   Microseconds (000000-999999)
%3f  Milliseconds (000-999)
%6f  Microseconds (000000-999999)
%9f  Nanoseconds (000000000-999999999)
%.f  Auto-match subseconds with flexible precision

Time zone tokens:
%z  UTC offset (+HHMM or -HHMM)
%Z  Time zone name (if available)
%:z UTC offset with colon (+HH:MM or -HH:MM)

Weekday helpers:
%w  Weekday as decimal (0=Sunday, 6=Saturday)
%a  Weekday as abbreviated name (Sun, Mon, ..., Sat)
%A  Weekday as full name (Sunday, Monday, ..., Saturday)

Week numbers:
%W  Week number (Monday as first day of week)
%U  Week number (Sunday as first day of week)

Common examples:
%Y-%m-%d %H:%M:%S           2024-01-15 14:30:45
%Y-%m-%dT%H:%M:%S%z         2024-01-15T14:30:45+0000
%Y-%m-%d %H:%M:%S%.f        2024-01-15 14:30:45.123456
%b %d %H:%M:%S              Jan 15 14:30:45 (syslog format)
%d/%b/%Y:%H:%M:%S %z        15/Jan/2024:14:30:45 +0000 (Apache access log)
%Y-%m-%d %H:%M:%S,%3f       2024-01-15 14:30:45,123 (Python logging)

Naive timestamp + timezone example:
  kelora app.log --ts-format "%Y-%m-%d %H:%M:%S" --input-tz Europe/Berlin
  (parses local timestamps and normalises them internally)

Shell tip: wrap the entire format in single quotes or escape % symbols to keep
  your shell from expanding them.

Unix epoch timestamps (auto-detected):
  Integer format:    1735566123         # Seconds (10 digits)
                     1735566123000      # Milliseconds (13 digits)
                     1735566123000000   # Microseconds (16 digits)
  Float format:      1735566123.456     # Seconds with fractional milliseconds
                     1735566123.456789  # Seconds with fractional microseconds

Timestamp filtering with --since and --until:
  kelora --since "2024-01-15T10:00:00Z" app.log   # Events after timestamp
  kelora --until "yesterday" app.log              # Events before yesterday
  kelora --since 1h app.log                       # Last hour (1h, 30m, 2d, etc.)
  kelora --since +1h app.log                      # Future events (+ means ahead)
  kelora --since 1735566123 app.log               # Events after Unix timestamp
  kelora --since 1735566123.456 app.log           # Float Unix timestamps work too

  Anchored timestamps (relative to the other boundary):
  kelora --since 10:00 --until start+30m app.log  # 30 minutes starting at 10:00
  kelora --since end-1h --until 11:00 app.log     # 1 hour ending at 11:00
  kelora --since -2h --until start+1h app.log     # 1 hour starting 2 hours ago

  'start' anchors to --since, 'end' anchors to --until
  Cannot use both anchors in the same command (e.g., --since end-1h --until start+1h)

  Common timestamp field names are auto-detected:
    ts, _ts, timestamp, at, time, @timestamp, log_timestamp, event_time,
    datetime, date_time, created_at, logged_at, _t, @t, t
  Events without valid timestamps are filtered out in resilient mode (default)
  Use --strict to abort processing on missing/invalid timestamps
  Use --verbose to see detailed timestamp parsing errors

For the full chrono format reference, see:
https://docs.rs/chrono/latest/chrono/format/strftime/index.html

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
