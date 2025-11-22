# Working With Time

Master Kelora's timestamp parsing, filtering, and timezone handling.

## What You'll Learn

By the end of this tutorial, you'll be able to:

- Parse timestamps with custom formats using `--ts-format`
- Filter logs by time ranges with `--since` and `--until`
- Handle timezones correctly with `--input-tz`
- Convert and format timestamps using datetime functions
- Calculate durations between events
- Use timezone-aware datetime operations

## Prerequisites

- Completed the [Quickstart](../quickstart.md)
- Basic understanding of timestamp formats and timezones
- Familiarity with command-line operations

## Overview

Time handling is critical for log analysis. Kelora provides powerful timestamp parsing, filtering, and manipulation capabilities with proper timezone support.

**Time:** ~15 minutes

## Step 1: Understanding Timestamp Detection

Kelora automatically detects common timestamp field names in your logs:

=== "Command"

    ```bash
    echo '{"ts": "2024-01-15T10:30:00Z", "message": "User login"}
    {"timestamp": "2024-01-15T10:31:00Z", "message": "Request processed"}
    {"time": "2024-01-15T10:32:00Z", "message": "Response sent"}' | kelora -j --stats
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"ts": "2024-01-15T10:30:00Z", "message": "User login"}
    {"timestamp": "2024-01-15T10:31:00Z", "message": "Request processed"}
    {"time": "2024-01-15T10:32:00Z", "message": "Response sent"}' | kelora -j --stats
    ```

**Auto-detected field names:**

- `ts`, `timestamp`, `time`, `@timestamp`
- Case-insensitive detection
- First matching field in the event is used

## Step 2: Time Range Filtering

Use `--since` and `--until` to filter logs by time range:

```bash
# Last hour of logs
kelora -j --since 1h app.log

# Last 30 minutes
kelora -j --since 30m app.log

# Last 2 days
kelora -j --since 2d app.log

# Specific timestamp range
kelora -j --since "2024-01-15T10:00:00Z" --until "2024-01-15T11:00:00Z" app.log

# Natural language (yesterday)
kelora -j --since yesterday app.log
```

**Duration syntax:**

- `1h` - One hour ago
- `30m` - Thirty minutes ago
- `2d` - Two days ago
- `1h30m` - One hour and thirty minutes ago

**Future filtering:**

- `--since +1h` - Events starting one hour from now
- `--until +2d` - Events up to two days from now

**Anchored timestamps** (duration windows):

Anchor one boundary to the other to specify durations:

```bash
# 30 minutes starting at 10:00
kelora -j --since "10:00" --until "start+30m" app.log

# 1 hour ending at 11:00
kelora -j --since "end-1h" --until "11:00" app.log

# 2 hours starting from yesterday
kelora -j --since "yesterday" --until "start+2h" app.log

# 45 minutes starting from a specific timestamp
kelora -j --since "2024-01-15T10:00:00Z" --until "start+45m" app.log
```

**Anchor syntax:**

- `start+DURATION` or `start-DURATION` - relative to `--since` value
- `end+DURATION` or `end-DURATION` - relative to `--until` value

**Note:** Cannot use both anchors in the same command (e.g., `--since end-1h --until start+1h` will error).

## Step 3: Custom Timestamp Formats

When your timestamps don't match standard formats, use `--ts-format`:

```bash
# Python logging format with milliseconds
echo '2024-01-15 10:30:45,123 INFO User login' | \
    kelora -f 'cols:timestamp(2) level *message' \
    --ts-field timestamp \
    --ts-format '%Y-%m-%d %H:%M:%S,%3f'

# Apache access log format
echo '15/Jan/2024:10:30:45 +0000 GET /api/users 200' | \
    kelora -f 'cols:timestamp(2) method path status:int' \
    --ts-field timestamp \
    --ts-format '%d/%b/%Y:%H:%M:%S %z'

# Syslog format without year
echo 'Jan 15 10:30:45 webserver nginx: Connection accepted' | \
    kelora -f syslog --ts-format '%b %d %H:%M:%S'
```

**Common format tokens:**

- `%Y` - Year with century (2024)
- `%m` - Month (01-12)
- `%d` - Day (01-31)
- `%H` - Hour 24h (00-23)
- `%M` - Minute (00-59)
- `%S` - Second (00-59)
- `%3f` - Milliseconds (000-999)
- `%6f` - Microseconds (000000-999999)
- `%z` - UTC offset (+0000, -0500)

See `kelora --help-time` for complete format reference.

## Step 4: Timezone Handling

Handle naive timestamps (without timezone info) using `--input-tz`:

=== "Command"

    ```bash
    # Parse timestamps with mixed timezones, all normalized to UTC display
    kelora -f 'cols:timestamp *message' examples/timezones_mixed.log \
        --ts-field timestamp -Z -n 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    # Parse timestamps with mixed timezones, all normalized to UTC display
    kelora -f 'cols:timestamp *message' examples/timezones_mixed.log \
        --ts-field timestamp -Z -n 5
    ```

Other timezone examples:

```bash
# Parse naive timestamps as UTC
kelora -j --input-tz UTC app.log

# Parse naive timestamps as local time
kelora -j --input-tz local app.log

# Parse naive timestamps in specific timezone
kelora -j --input-tz Europe/Berlin app.log
kelora -j --input-tz America/New_York app.log
```

**Timezone precedence:**

1. `--input-tz` flag (highest priority)
2. `TZ` environment variable
3. UTC (default)

**Important:** `--input-tz` only affects naive timestamps. Timestamps with explicit timezone info (like `2024-01-15T10:30:00+01:00`) preserve their original timezone.

## Step 5: Converting Timestamps in Events

Use `--normalize-ts` to normalize the primary timestamp (the same field Kelora uses for filtering and stats) to RFC3339:

```bash
# Convert the detected timestamp field to RFC3339
echo '{"ts": "2024-01-15 10:30:00", "user": "alice"}' | \
    kelora -j --input-tz UTC --normalize-ts

# Respect a custom timestamp field provided via --ts-field
echo '{"created_at": "2024-01-15 10:45:00", "user": "bob"}' | \
    kelora -j --ts-field created_at --input-tz UTC --normalize-ts
```

**Output example:**
```
ts="2024-01-15T10:30:00+00:00" user="alice"
created_at="2024-01-15T10:45:00+00:00" user="bob"
```

This modifies the event data itself, affecting all output formats.

## Step 6: Display Formatting vs Data Conversion

Understand the difference between data conversion and display formatting:

```bash
# --normalize-ts: Modifies event data (affects all formats)
echo '{"ts": "2024-01-15 10:30:00"}' | \
    kelora -j --normalize-ts -F json

# -z: Display formatting only (default format only)
echo '{"ts": "2024-01-15T10:30:00Z"}' | \
    kelora -j -z

# -Z: Display as UTC (default format only)
echo '{"ts": "2024-01-15T10:30:00Z"}' | \
    kelora -j -Z
```

**Key differences:**

- `--normalize-ts` - Changes the event data
- `-z / -Z` - Only affects default formatter display
- JSON/CSV output ignores `-z/-Z` flags

## Step 7: Working with DateTime in Scripts

Use `to_datetime()` to parse timestamps in Rhai scripts:

```bash
# Parse timestamp from string
echo '{"log": "Event at 2024-01-15T10:30:00Z completed"}' | \
    kelora -j \
    -e 'e.event_time = to_datetime(e.log.extract_re(r"at (\S+)", 1))'

# Parse with custom format
echo '{"log": "Event at 15/Jan/2024:10:30:45"}' | \
    kelora -j \
    -e 'e.event_time = to_datetime(e.log.extract_re(r"at (\S+)", 1), "%d/%b/%Y:%H:%M:%S")'

# Parse with timezone hint
echo '{"log": "Event at 2024-01-15 10:30:00"}' | \
    kelora -j \
    -e 'e.event_time = to_datetime(e.log.extract_re(r"at (.+)$", 1), "%Y-%m-%d %H:%M:%S", "Europe/Berlin")'
```

## Step 8: DateTime Operations

Extract components and format timestamps:

```bash
# Extract time components
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.hour = dt.hour()' \
    -e 'e.day = dt.day()' \
    -e 'e.month = dt.month()' \
    -e 'e.year = dt.year()' \
    -k hour,day,month,year

# Format timestamp
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.formatted = dt.format("%b %d, %Y at %I:%M %p")'

# Convert timezone
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.utc = dt.to_utc().to_iso()' \
    -e 'e.berlin = dt.to_timezone("Europe/Berlin").to_iso()' \
    -e 'e.ny = dt.to_timezone("America/New_York").to_iso()'
```

**Available methods:**

- `.year()`, `.month()`, `.day()` - Date components
- `.hour()`, `.minute()`, `.second()` - Time components
- `.format(fmt)` - Custom formatting
- `.to_iso()` - ISO 8601 string
- `.to_utc()`, `.to_local()` - Timezone conversion
- `.to_timezone(name)` - Named timezone conversion
- `.timezone_name()` - Get timezone name

## Step 9: Duration Calculations

Calculate time differences between events:

```bash
# Calculate duration between timestamps
echo '{"start": "2024-01-15T10:00:00Z", "end": "2024-01-15T10:30:00Z"}' | \
    kelora -j \
    -e 'let start_dt = to_datetime(e.start)' \
    -e 'let end_dt = to_datetime(e.end)' \
    -e 'let duration = end_dt - start_dt' \
    -e 'e.duration_seconds = duration.as_seconds()' \
    -e 'e.duration_minutes = duration.as_minutes()' \
    -e 'e.duration_human = duration.to_string()' \
    -k duration_seconds,duration_minutes,duration_human

# Add duration to timestamp
echo '{"timestamp": "2024-01-15T10:00:00Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let hour_later = dt + to_duration("1h")' \
    -e 'e.plus_1h = hour_later.to_iso()'

# Duration from number
echo '{"timestamp": "2024-01-15T10:00:00Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let offset = duration_from_minutes(90)' \
    -e 'e.plus_90m = (dt + offset).to_iso()'
```

**Duration functions:**

- `to_duration("1h30m")` - Parse duration string
- `duration_from_seconds(n)`, `duration_from_minutes(n)`
- `duration_from_hours(n)`, `duration_from_days(n)`
- `duration_from_ms(n)`, `duration_from_ns(n)`

**Duration methods:**

- `.as_seconds()`, `.as_milliseconds()`, `.as_nanoseconds()`
- `.as_minutes()`, `.as_hours()`, `.as_days()`
- `.to_string()` - Human-readable format

## Step 10: Real-World Example - Request Duration Analysis

Analyze API request durations with proper time handling:

```bash
# Sample log data
cat api_logs.json
{"timestamp": "2024-01-15T10:00:00Z", "endpoint": "/api/users", "duration_ms": 45}
{"timestamp": "2024-01-15T10:00:05Z", "endpoint": "/api/orders", "duration_ms": 230}
{"timestamp": "2024-01-15T10:00:10Z", "endpoint": "/api/users", "duration_ms": 1200}
{"timestamp": "2024-01-15T10:00:15Z", "endpoint": "/api/products", "duration_ms": 89}

# Analyze slow requests in the last hour
kelora -j api_logs.json \
    --since 1h \
    -e 'e.duration_human = humanize_duration(e.duration_ms)' \
    --filter 'e.duration_ms > 1000' \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.hour = dt.hour()' \
    -e 'e.formatted_time = dt.format("%H:%M:%S")' \
    -k formatted_time,endpoint,duration_human
```

## Step 11: Time-Based Filtering with Business Hours

Filter logs during business hours across timezones:

```bash
# Filter for events during business hours (9 AM - 5 PM)
kelora -j app.log \
    --input-tz America/New_York \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.hour = dt.hour()' \
    --filter 'e.hour >= 9 && e.hour < 17'

# Weekend vs weekday analysis
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let dow = dt.format("%w").to_int()' \
    -e 'e.is_weekend = dow == 0 || dow == 6' \
    --filter 'e.is_weekend'
```

## Step 12: Comparing Timestamps

Use datetime comparison in filters:

```bash
# Events after a specific time
echo '{"timestamp": "2024-01-15T10:30:00Z", "message": "Event 1"}
{"timestamp": "2024-01-15T11:00:00Z", "message": "Event 2"}
{"timestamp": "2024-01-15T11:30:00Z", "message": "Event 3"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let cutoff = to_datetime("2024-01-15T11:00:00Z")' \
    --filter 'dt > cutoff'

# Events within time window
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let start = to_datetime("2024-01-15T10:00:00Z")' \
    -e 'let end = to_datetime("2024-01-15T11:00:00Z")' \
    --filter 'dt >= start && dt <= end'
```

**Comparison operators:**

- `==`, `!=` - Equality
- `>`, `<` - Greater/less than
- `>=`, `<=` - Greater/less or equal

## Step 13: Current Time Function

Use `now()` for relative time calculations:

```bash
# Find events in last 5 minutes using script
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let cutoff = now() - to_duration("5m")' \
    --filter 'dt > cutoff'

# Add processing timestamp
kelora -j app.log \
    -e 'e.processed_at = now().to_iso()'

# Calculate event age
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let age = now() - dt' \
    -e 'e.age_minutes = age.as_minutes()'
```

## Common Patterns

### Pattern 1: Parse Non-Standard Timestamps

```bash
# Custom application format
kelora -j app.log \
    --ts-format '%Y-%m-%d %H:%M:%S,%3f' \
    --input-tz UTC
```

### Pattern 2: Filter by Time Range

```bash
# Last hour of errors
kelora -j app.log \
    --since 1h \
    -l error
```

### Pattern 3: Convert Timezone for Display

```bash
# Show timestamps in local timezone
kelora -j app.log -z

# Show timestamps in UTC
kelora -j app.log -Z
```

### Pattern 4: Calculate Request Duration

```bash
# Add duration between start and end timestamps
kelora -j app.log \
    -e 'let start = to_datetime(e.start_time)' \
    -e 'let end = to_datetime(e.end_time)' \
    -e 'let duration = end - start' \
    -e 'e.duration_ms = duration.as_milliseconds()'
```

### Pattern 5: Business Hours Analysis

```bash
# Filter for business hours in specific timezone
kelora -j app.log \
    --input-tz America/New_York \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'let hour = dt.hour()' \
    --filter 'hour >= 9 && hour < 17'
```

### Pattern 6: Humanize Durations

```bash
# Convert milliseconds to human-readable format
kelora -j app.log \
    -e 'e.duration_human = humanize_duration(e.response_time_ms)' \
    -k timestamp,endpoint,duration_human
```

## Tips and Best Practices

### Always Specify Timezone for Naive Timestamps

```bash
# Good - explicit timezone
kelora -j --input-tz UTC app.log

# Avoid - relies on defaults
kelora -j app.log
```

### Use --ts-format for Custom Formats

```bash
# Good - explicit format
kelora -f syslog --ts-format '%b %d %H:%M:%S' app.log

# Avoid - relies on auto-detection
kelora -f syslog app.log
```

### Filter Early with --since/--until

```bash
# Good - filter at input stage
kelora -j --since 1h app.log

# Less efficient - filter in script
kelora -j app.log -e 'let dt = to_datetime(e.timestamp)' --filter 'now() - dt < to_duration("1h")'
```

### Store Parsed DateTime in Variable

```bash
# Good - parse once, use multiple times
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp)' \
    -e 'e.hour = dt.hour()' \
    -e 'e.day = dt.day()' \
    -e 'e.formatted = dt.format("%Y-%m-%d")'

# Less efficient - parse multiple times
kelora -j app.log \
    -e 'e.hour = to_datetime(e.timestamp).hour()' \
    -e 'e.day = to_datetime(e.timestamp).day()'
```

### Use ISO Format for Interoperability

```bash
# Good - ISO 8601 format
kelora -j app.log -e 'e.timestamp = to_datetime(e.ts).to_iso()'

# Less portable - custom format
kelora -j app.log -e 'e.timestamp = to_datetime(e.ts).format("%Y-%m-%d %H:%M:%S")'
```

## Troubleshooting

### Timestamps Not Being Detected

**Problem:** Time filtering not working.

**Solution:** Check field names and add explicit timestamp field:
```bash
# Debug: Show detected timestamp
kelora -j app.log -n 3

# Point Kelora at your timestamp field explicitly
kelora -f 'cols:my_time level *message' app.log --since 1h --ts-field my_time
```

### Timezone Confusion

**Problem:** Timestamps showing unexpected times.

**Solution:** Verify input timezone and display options:
```bash
# Check what timezone is being used
kelora -j --input-tz UTC app.log -n 1 -z

# Verify timestamp includes timezone info
kelora -j --normalize-ts app.log -n 1 -F json
```

### Custom Format Not Parsing

**Problem:** `--ts-format` not working.

**Solution:** Test format with sample data:
```bash
# Test format with verbose errors
echo '2024-01-15 10:30:45,123 Test' | \
    kelora -f 'cols:timestamp(2) *message' \
    --ts-field timestamp \
    --ts-format '%Y-%m-%d %H:%M:%S,%3f' \
    --verbose

# Check format string escaping
kelora --ts-format '%Y-%m-%d %H:%M:%S' app.log  # Ensure proper quoting
```

### Duration Calculations Wrong

**Problem:** Negative or incorrect durations.

**Solution:** Verify timestamp order and timezone consistency:
```bash
# Check both timestamps are parsed correctly
kelora -j app.log \
    -e 'print("Start: " + e.start_time + ", End: " + e.end_time)' \
    -e 'let start = to_datetime(e.start_time)' \
    -e 'let end = to_datetime(e.end_time)' \
    -e 'e.duration = (end - start).as_seconds()' \
    -n 3
```

## Next Steps

Now that you understand time handling in Kelora, explore:

- **[Advanced Scripting](advanced-scripting.md)** - Advanced Rhai scripting techniques
- **[Metrics and Tracking](metrics-and-tracking.md)** - Time-based metric aggregation
- **[Time Format Reference](../reference/cli-reference.md#timestamp-options)** - Complete format documentation

## See Also

- `kelora --help-time` - Complete timestamp format reference
- `kelora --help-functions` - DateTime function reference
- [CLI Reference](../reference/cli-reference.md) - All timestamp-related flags
