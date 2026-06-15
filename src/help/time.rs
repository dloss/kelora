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

Timezone policy (how the zone is decided):
  - Numeric offset present (e.g. +0200, -0700): always honored as-is.
  - No offset (naive: syslog, log4j, python-logging, glog, apache-error,
    postgres, ...): resolved with --input-tz, which defaults to UTC. Set
    --input-tz <zone> (or the TZ env var) if your source logs local time,
    otherwise every timestamp is shifted silently and so are --since/--until,
    --span boundaries, and ordering.
  - Zone abbreviation present (CEST, PST, ...): not parsed (abbreviations are
    ambiguous and cannot encode DST) and treated as naive. Use --input-tz.
  When timestamps are naive and no zone was chosen, kelora prints a one-time
  stderr hint if a time filter, --span, or --normalize-ts relies on the
  assumption (suppress with --no-diagnostics / --silent).

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

  Anchored timestamps: define one bound relative to the other (or to now).
  The keywords stand for the values you pass on the command line:
    since  = the value given to --since
    until  = the value given to --until
    now    = the current time
  Append +DURATION or -DURATION to shift from that anchor:

  kelora --since 10:00 --until since+30m app.log  # until = since+30m  -> 10:00..10:30
  kelora --since until-1h --until 11:00 app.log   # since = until-1h   -> 10:00..11:00
  kelora --since -2h --until since+1h app.log     # 1-hour window starting 2 hours ago
  kelora --since now-15m app.log                  # the last 15 minutes

  Only one bound may anchor to the other: --since and --until cannot both
  reference each other (e.g., --since until-1h --until since+1h is rejected).

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
