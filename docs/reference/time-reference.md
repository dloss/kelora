# Time Reference

Complete reference for timestamp detection, parsing, formatting, and time-based operations in Kelora.

## Automatic Timestamp Detection

Kelora automatically detects timestamp fields in structured formats (JSON, logfmt, CEF, syslog, etc.) using a standard set of field names.

### Auto-Detected Field Names

The following field names are recognized as timestamps (case-insensitive):

- `ts`
- `_ts`
- `timestamp`
- `at`
- `time`
- `@timestamp`
- `log_timestamp`
- `event_time`
- `datetime`
- `date_time`
- `created_at`
- `logged_at`
- `_t`
- `@t`
- `t`

**Behavior:**

- Detection is case-insensitive (`Timestamp`, `TIMESTAMP`, etc. all match)
- First matching field in the event is used
- To override auto-detection, use `--ts-field <field_name>`

**Example:**
```bash
# Auto-detects "timestamp" field
echo '{"timestamp": "2024-01-15T10:30:00Z", "level": "INFO"}' | kelora -j

# Override to use "created_at" field
echo '{"timestamp": "2024-01-15T10:30:00Z", "created_at": "2024-01-15T10:31:00Z"}' \
  | kelora -j --ts-field created_at
```

## Timestamp Format Parsing

Kelora uses [chrono format strings](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) for parsing and formatting timestamps.

### Date and Time Components

| Token | Example | Description |
|-------|---------|-------------|
| `%Y` | `2024` | 4-digit year |
| `%y` | `24` | 2-digit year |
| `%m` | `01` | Month (01-12) |
| `%b` | `Jan` | Abbreviated month name |
| `%B` | `January` | Full month name |
| `%d` | `15` | Day of month (01-31) |
| `%j` | `015` | Day of year (001-366) |
| `%H` | `14` | Hour (00-23) |
| `%I` | `02` | Hour (01-12) |
| `%p` | `PM` | AM/PM |
| `%M` | `30` | Minute (00-59) |
| `%S` | `45` | Second (00-59) |

### Subsecond Precision

| Token | Example | Description |
|-------|---------|-------------|
| `%f` | `123456789` | Nanoseconds (9 digits) |
| `%3f` | `123` | Milliseconds (3 digits) |
| `%6f` | `123456` | Microseconds (6 digits) |
| `%9f` | `123456789` | Nanoseconds (9 digits) |
| `%.f` | `.123` or `.123456` | Auto-match subseconds with dot |

### Timezone Tokens

| Token | Example | Description |
|-------|---------|-------------|
| `%z` | `+0000` | Numeric offset (+HHMM) |
| `%:z` | `+00:00` | Numeric offset with colon |
| `%Z` | `UTC`, `EST` | Timezone abbreviation |

### Weekday Formats

| Token | Example | Description |
|-------|---------|-------------|
| `%a` | `Mon` | Abbreviated weekday |
| `%A` | `Monday` | Full weekday name |
| `%w` | `1` | Weekday number (0=Sunday) |

### Week Numbers

| Token | Example | Description |
|-------|---------|-------------|
| `%W` | `03` | Week number (Monday as first day) |
| `%U` | `03` | Week number (Sunday as first day) |

### Common Timestamp Format Examples

```bash
# ISO 8601 / RFC3339
kelora --ts-format '%Y-%m-%dT%H:%M:%S%.f%:z' app.log

# Apache/Nginx logs
kelora --ts-format '%d/%b/%Y:%H:%M:%S %z' access.log

# Syslog
kelora --ts-format '%b %d %H:%M:%S' syslog.log

# Python logging
kelora --ts-format '%Y-%m-%d %H:%M:%S,%3f' app.log

# Custom date and time
kelora --ts-format '%Y-%m-%d %H:%M:%S' app.log

# Unix timestamp (numeric)
# No format needed - auto-detected

# US format with 12-hour time
kelora --ts-format '%m/%d/%Y %I:%M:%S %p' app.log
```

### Naive Timestamps and Timezone Handling

If your timestamps lack timezone information, specify the input timezone:

```bash
# Timestamps are in UTC
kelora --ts-format '%Y-%m-%d %H:%M:%S' --input-tz UTC app.log

# Timestamps are in local time
kelora --ts-format '%Y-%m-%d %H:%M:%S' --input-tz local app.log

# Timestamps are in specific timezone
kelora --ts-format '%Y-%m-%d %H:%M:%S' --input-tz America/New_York app.log
```

**Timezone Options:**

- `UTC` - Coordinated Universal Time
- `local` - System local timezone
- `America/New_York`, `Europe/London`, etc. - Named IANA timezones

## CLI Timestamp Options

### Parsing Configuration

| Flag | Description | Example |
|------|-------------|---------|
| `--ts-field <FIELD>` | Override auto-detected timestamp field | `--ts-field created_at` |
| `--ts-format <FORMAT>` | Custom timestamp format (chrono syntax) | `--ts-format '%Y-%m-%d %H:%M:%S'` |
| `--input-tz <TZ>` | Timezone for naive timestamps | `--input-tz America/New_York` |

### Time Range Filtering

| Flag | Description | Example |
|------|-------------|---------|
| `--since <TIME>` | Include events from this time onward | `--since '1h'` or `--since '2024-01-15T10:00:00Z'` |
| `--until <TIME>` | Include events until this time | `--until '30m'` or `--until '2024-01-15T11:00:00Z'` |

**Relative Time Formats:**

- `1h` - 1 hour ago
- `30m` - 30 minutes ago
- `2d` - 2 days ago
- `1w` - 1 week ago
- `+1h` - 1 hour in the future (prefix `+` for future times)
- Combine: `1h30m` - 1 hour 30 minutes ago

**Absolute Time Formats:**

- ISO 8601: `2024-01-15T10:30:00Z`
- RFC3339: `2024-01-15T10:30:00+00:00`
- Unix timestamps: `1705318200`
- Date only: `2024-01-15` (assumes 00:00:00)
- Time only: `10:30:00` (assumes today)
- Special values: `now`, `today`, `yesterday`, `tomorrow`

**Anchored Timestamps:**

Anchor one boundary to the other for duration-based windows:

- `since+DURATION` - Duration after `--since` value
- `since-DURATION` - Duration before `--since` value
- `until+DURATION` - Duration after `--until` value
- `until-DURATION` - Duration before `--until` value
- `now+DURATION` - Duration from current time (future)
- `now-DURATION` - Duration from current time (past)

```bash
# Show 30 minutes starting at 10:00
kelora --since "10:00" --until "since+30m" app.log

# Show 1 hour ending at 11:00
kelora --since "until-1h" --until "11:00" app.log

# Show 1 hour starting from 2 hours ago
kelora --since "-2h" --until "since+1h" app.log

# Show 45 minutes starting at a specific timestamp
kelora --since "2024-01-15T10:00:00Z" --until "since+45m" app.log

# Show next 5 minutes (using now anchor)
kelora --until "now+5m" app.log

# Show from 1 hour ago to 5 minutes from now
kelora --since "now-1h" --until "now+5m" app.log
```

**Important Notes:**

- `since` anchors to the `--since` value, `until` anchors to the `--until` value
- `now` anchors to the current time (doesn't require --since or --until to be set)
- Cannot use both anchors in the same command (e.g., `--since until-1h --until since+1h` is an error)
- The anchor target must be specified (e.g., `--until since+30m` requires `--since` to be set)

**Basic Examples:**
```bash
# Last hour
kelora -j --since 1h app.log

# Last 30 minutes
kelora -j --since 30m app.log

# Between two times
kelora -j --since '2024-01-15T10:00:00Z' --until '2024-01-15T11:00:00Z' app.log

# Since absolute time
kelora -j --since '2024-01-15T10:00:00Z' app.log

# Future events (1 hour from now)
kelora -j --since +1h app.log
```

### Timestamp Display and Conversion

| Flag | Description | Example |
|------|-------------|---------|
| `-z` | Display timestamps as local RFC3339 (display only) | `kelora -j -z app.log` |
| `-Z` | Display timestamps as UTC RFC3339 (display only) | `kelora -j -Z app.log` |
| `--normalize-ts` | Normalize timestamp field to RFC3339 (modifies event) | `kelora -j --normalize-ts app.log` |

**Difference between `-z/-Z` and `--normalize-ts`:**

- `-z` and `-Z`: Display-only formatting, doesn't modify event data
- `--normalize-ts`: Converts the timestamp field in the event itself to RFC3339

```bash
# Display in local time (doesn't modify events)
kelora -j -z app.log -F json

# Convert timestamp field to RFC3339 in events
kelora -j --normalize-ts app.log -F json
```

### Time-Based Features

| Flag | Description | Example |
|------|-------------|---------|
| `--span <DURATION>` | Time-based span windows for aggregation | `--span 1h` |
| `--mark-gaps <DURATION>` | Insert markers when time delta exceeds duration | `--mark-gaps 5m` |

**Example:**
```bash
# 1-hour aggregation windows
kelora -j --span 1h --exec 'emit_span(|s| {count: s.len()})' app.log

# Mark gaps longer than 5 minutes
kelora -j --mark-gaps 5m app.log
```

## Rhai DateTime Functions

Complete reference for working with dates and times in Rhai scripts.

### Parsing and Creation

| Function | Description | Example |
|----------|-------------|---------|
| `to_datetime(text)` | Parse ISO 8601 timestamp (auto-format) | `to_datetime("2024-01-15T10:30:00Z")` |
| `to_datetime(text, fmt)` | Parse with custom format | `to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S")` |
| `to_datetime(text, fmt, tz)` | Parse with format and timezone | `to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S", "America/New_York")` |
| `now()` | Current time (UTC) | `now()` |

### DateTime Components

Access components of a DateTime value:

| Method | Returns | Description |
|--------|---------|-------------|
| `.year()` | Integer | Year (e.g., 2024) |
| `.month()` | Integer | Month (1-12) |
| `.day()` | Integer | Day of month (1-31) |
| `.hour()` | Integer | Hour (0-23) |
| `.minute()` | Integer | Minute (0-59) |
| `.second()` | Integer | Second (0-59) |
| `.ts_nanos()` | Integer | Unix timestamp in nanoseconds |

**Example:**
```rhai
let dt = to_datetime(e.timestamp);
if dt.hour() >= 9 && dt.hour() < 17 {
    print("Business hours");
}
```

### DateTime Formatting

| Method | Description | Example |
|--------|-------------|---------|
| `.to_iso()` | Convert to ISO 8601 string | `dt.to_iso()` → `"2024-01-15T10:30:00Z"` |
| `.format(fmt)` | Format with custom pattern | `dt.format("%Y-%m-%d")` → `"2024-01-15"` |

### Timezone Conversion

| Method | Description | Example |
|--------|-------------|---------|
| `.to_utc()` | Convert to UTC | `dt.to_utc()` |
| `.to_local()` | Convert to local timezone | `dt.to_local()` |
| `.to_timezone(name)` | Convert to named timezone | `dt.to_timezone("America/New_York")` |
| `.timezone_name()` | Get timezone name | `dt.timezone_name()` → `"UTC"` |

### DateTime Comparison

DateTime values support all comparison operators:

```rhai
let dt1 = to_datetime("2024-01-15T10:30:00Z");
let dt2 = to_datetime("2024-01-15T11:00:00Z");

dt1 == dt2  // false
dt1 != dt2  // true
dt1 < dt2   // true
dt1 <= dt2  // true
dt1 > dt2   // false
dt1 >= dt2  // false
```

### DateTime Arithmetic

```rhai
// Add duration to datetime
let dt = to_datetime("2024-01-15T10:00:00Z");
let later = dt + to_duration("1h30m");  // 2024-01-15T11:30:00Z

// Subtract duration from datetime
let earlier = dt - to_duration("30m");  // 2024-01-15T09:30:00Z

// Difference between two datetimes (returns Duration)
let dt1 = to_datetime("2024-01-15T10:00:00Z");
let dt2 = to_datetime("2024-01-15T11:30:00Z");
let elapsed = dt2 - dt1;  // Duration: 1h30m
```

## Rhai Duration Functions

### Parsing and Creation

| Function | Description | Example |
|----------|-------------|---------|
| `to_duration(text)` | Parse duration string | `to_duration("1h30m")` |
| `duration_from_seconds(n)` | Create from seconds | `duration_from_seconds(3600)` |
| `duration_from_milliseconds(n)` | Create from milliseconds | `duration_from_milliseconds(5000)` |
| `duration_from_nanoseconds(n)` | Create from nanoseconds | `duration_from_nanoseconds(1000000)` |
| `duration_from_minutes(n)` | Create from minutes | `duration_from_minutes(30)` |
| `duration_from_hours(n)` | Create from hours | `duration_from_hours(2)` |
| `duration_from_days(n)` | Create from days | `duration_from_days(7)` |

**Duration String Format:**

- `1h` - 1 hour
- `30m` - 30 minutes
- `45s` - 45 seconds
- `500ms` - 500 milliseconds
- Combine: `1h30m45s` - 1 hour, 30 minutes, 45 seconds

### Duration Conversion

| Method | Returns | Description |
|--------|---------|-------------|
| `.as_seconds()` | Float | Duration in seconds |
| `.as_milliseconds()` | Integer | Duration in milliseconds |
| `.as_nanoseconds()` | Integer | Duration in nanoseconds |
| `.as_minutes()` | Float | Duration in minutes |
| `.as_hours()` | Float | Duration in hours |
| `.as_days()` | Float | Duration in days |

### Duration Formatting

| Function | Description | Example |
|----------|-------------|---------|
| `humanize_duration(ms)` | Format milliseconds as human-readable | `humanize_duration(5000)` → `"5s"` |
| `.to_string()` | Convert duration to string | `dur.to_string()` → `"1h30m"` |

**Example:**
```rhai
let dur = to_duration("1h30m");
print(`${dur.as_minutes()} minutes`);  // 90 minutes
print(`${dur.as_seconds()} seconds`);  // 5400 seconds
```

### Duration Arithmetic

```rhai
// Add durations
let d1 = to_duration("1h");
let d2 = to_duration("30m");
let total = d1 + d2;  // 1h30m

// Subtract durations
let diff = d1 - d2;  // 30m

// Multiply duration
let doubled = d1 * 2;  // 2h

// Divide duration
let half = d1 / 2;  // 30m
```

### Duration Comparison

Duration values support all comparison operators:

```rhai
let d1 = to_duration("1h");
let d2 = to_duration("30m");

d1 == d2  // false
d1 != d2  // true
d1 > d2   // true
d1 >= d2  // true
d1 < d2   // false
d1 <= d2  // false
```

## Common Patterns

### Calculate Request Duration

```rhai
// Parse timestamps and calculate duration
let start = to_datetime(e.start_time);
let end = to_datetime(e.end_time);
let duration = end - start;
e.duration_ms = duration.as_milliseconds();
```

### Filter Business Hours

```rhai
// Use inside a --filter stage to keep only business hours (9 AM - 5 PM)
let dt = to_datetime(e.timestamp);
dt.hour() >= 9 && dt.hour() < 17
```

### Compare Against Current Time

```rhai
// Find events older than 1 hour
let dt = to_datetime(e.timestamp);
let age = now() - dt;
if age > to_duration("1h") {
    e.is_old = true;
}
```

### Format Timestamp for Display

```rhai
// Convert to custom display format
let dt = to_datetime(e.timestamp);
e.display_time = dt.format("%Y-%m-%d %I:%M %p");
// Result: "2024-01-15 10:30 AM"
```

### Time-Based Aggregation

```rhai
// Group by hour of day
let dt = to_datetime(e.timestamp);
e.hour_of_day = dt.hour();
```

## Troubleshooting

### Timestamp Not Detected

**Problem:** Timestamps not being parsed automatically.

**Solutions:**

1. Check if field name is in auto-detected list (see above)
2. Specify field explicitly: `--ts-field your_field_name`
3. Check field value is a string (not nested object)

### Parse Errors

**Problem:** "Failed to parse timestamp" errors.

**Solutions:**

1. Check if format matches exactly: `--ts-format '%Y-%m-%d %H:%M:%S'`
2. Verify timezone handling: add `--input-tz UTC` for naive timestamps
3. Check for subsecond precision: use `%.f` for auto-matching
4. Look for Python comma separator: automatic conversion to period

### Timezone Issues

**Problem:** Times appear in wrong timezone.

**Solutions:**

1. For display: Use `-z` (local) or `-Z` (UTC)
2. For naive inputs: Set `--input-tz America/New_York`
3. Check TZ environment variable if using "local"
4. Verify timestamp includes timezone: `+00:00` or `Z`

### Relative Time Parsing

**Problem:** `--since 1h` not working as expected.

**Solutions:**

1. Check timestamp is properly detected/parsed first
2. Ensure timestamps are in chronological order (for best results)
3. Verify format: `1h`, `30m`, `2d` (no spaces)
4. Use absolute times if relative parsing fails

### Syslog Year Inference

**Problem:** Syslog timestamps missing year.

**Solution:**

- Kelora infers year from current time
- For historical logs, ensure system clock is correct
- Use custom format with explicit year if available

## See Also

- [Working with Time Tutorial](../tutorials/working-with-time.md) - Step-by-step guide with examples
- [Functions Reference](functions.md) - Complete Rhai function documentation
- [CLI Reference](cli-reference.md) - All command-line flags
- [Script Variables](script-variables.md) - Available variables in Rhai scripts
- [Chrono Format Strings](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) - Complete format token reference
