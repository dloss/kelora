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

- `ts`, `_ts`, `timestamp`, `at`, `time`, `@timestamp`, `log_timestamp`, `event_time`, `datetime`, `date_time`, `created_at`, `logged_at`, `_t`, `@t`, `t`
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
    -e 'e.event_time = to_datetime(e.log.extract_regex(r"at (\S+)", 1))'

# Parse with custom format
echo '{"log": "Event at 15/Jan/2024:10:30:45"}' | \
    kelora -j \
    -e 'e.event_time = to_datetime(e.log.extract_regex(r"at (\S+)", 1), "%d/%b/%Y:%H:%M:%S")'

# Parse with timezone hint
echo '{"log": "Event at 2024-01-15 10:30:00"}' | \
    kelora -j \
    -e 'e.event_time = to_datetime(e.log.extract_regex(r"at (.+)$", 1), "%Y-%m-%d %H:%M:%S", "Europe/Berlin")'
```

## Step 8: Using Pre-Parsed Timestamps with `meta.parsed_ts`

Kelora automatically parses timestamps during ingestion and exposes them via `meta.parsed_ts` as a DateTime object. This is more efficient than calling `to_datetime()` repeatedly:

```bash
# Extract time components using meta.parsed_ts (faster!)
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'e.hour = meta.parsed_ts.hour();
        e.day = meta.parsed_ts.day();
        e.month = meta.parsed_ts.month();
        e.year = meta.parsed_ts.year()' \
    -k hour,day,month,year
```

**Why use `meta.parsed_ts`?**

- ✅ Already parsed - no conversion overhead
- ✅ Uses Kelora's detected timestamp field automatically
- ✅ Respects `--ts-field` and `--ts-format` options
- ✅ One source of truth for all time operations

### Real-World Example: Detecting Monitoring Gaps

A practical use case that can't be done with `--since`/`--until` or `--span` alone: finding gaps in time-series data where events stop arriving:

```bash
# Create sample monitoring data with a gap
cat > /tmp/monitoring.jsonl << 'EOF'
{"timestamp": "2024-01-15T10:00:00Z", "host": "server1", "cpu": 45}
{"timestamp": "2024-01-15T10:01:00Z", "host": "server1", "cpu": 48}
{"timestamp": "2024-01-15T10:02:00Z", "host": "server1", "cpu": 52}
{"timestamp": "2024-01-15T10:08:00Z", "host": "server1", "cpu": 61}
{"timestamp": "2024-01-15T10:09:00Z", "host": "server1", "cpu": 59}
EOF

# Detect gaps > 2 minutes between events using a 2-event window
kelora -j /tmp/monitoring.jsonl \
    --window 2 \
    -e 'if window.len() >= 2 {
            let gap = meta.parsed_ts - to_datetime(window[1].timestamp);
            if gap.as_seconds() > 120 {
                e.alert = "GAP DETECTED";
                e.gap_duration = gap.to_string();
            }
        }' \
    --filter 'e.has("alert")' \
    -k timestamp,gap_duration,alert
```

**Output:**
```
timestamp='2024-01-15T10:08:00Z' gap_duration='6m' alert='GAP DETECTED'
```

This example:
- Uses `--window 2` to access the previous event via `window[1]`
- Compares `meta.parsed_ts` (current, pre-parsed) against the previous event's timestamp
- Detects a 6-minute gap where monitoring stopped reporting
- Can't be done with `--since`/`--until` (filters absolute time ranges)
- Can't be done with `--span` (aggregates by time windows, not gaps between events)

**Note:** The window contains event snapshots before exec modifications, so we use `meta.parsed_ts` for the current event and `to_datetime(window[1].timestamp)` for the previous event.

### Another Example: Request Rate Bucketing by Hour

Analyze request patterns by hour of day (useful for capacity planning):

```bash
# Sample API logs
cat > /tmp/api_requests.jsonl << 'EOF'
{"timestamp": "2024-01-15T08:30:00Z", "endpoint": "/api/users", "status": 200}
{"timestamp": "2024-01-15T08:45:00Z", "endpoint": "/api/orders", "status": 200}
{"timestamp": "2024-01-15T14:15:00Z", "endpoint": "/api/users", "status": 200}
{"timestamp": "2024-01-15T14:30:00Z", "endpoint": "/api/products", "status": 200}
{"timestamp": "2024-01-15T14:35:00Z", "endpoint": "/api/users", "status": 500}
{"timestamp": "2024-01-15T20:10:00Z", "endpoint": "/api/orders", "status": 200}
EOF

# Count requests per hour using meta.parsed_ts
kelora -j /tmp/api_requests.jsonl \
    -e 'track_freq("requests_by_hour", meta.parsed_ts.hour())' \
    -m
```

**Output:**
```
requests_by_hour_8  = 2
requests_by_hour_14 = 3
requests_by_hour_20 = 1
```

This shows traffic patterns across the day, helping identify peak hours for capacity planning.

## Step 9: DateTime Operations

Extract components and format timestamps:

```bash
# Extract time components (using meta.parsed_ts is more efficient)
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'e.hour = meta.parsed_ts.hour();
        e.day = meta.parsed_ts.day();
        e.month = meta.parsed_ts.month();
        e.year = meta.parsed_ts.year()' \
    -k hour,day,month,year

# Format timestamp
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp);
        e.formatted = dt.format("%b %d, %Y at %I:%M %p")'

# Convert timezone
echo '{"timestamp": "2024-01-15T10:30:45Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp);
        e.utc = dt.to_utc().to_iso();
        e.berlin = dt.to_timezone("Europe/Berlin").to_iso();
        e.ny = dt.to_timezone("America/New_York").to_iso()'
```

**Available methods:**

- `.year()`, `.month()`, `.day()` - Date components
- `.hour()`, `.minute()`, `.second()` - Time components
- `.format(fmt)` - Custom formatting
- `.to_iso()` - ISO 8601 string
- `.to_utc()`, `.to_local()` - Timezone conversion
- `.to_timezone(name)` - Named timezone conversion
- `.timezone_name()` - Get timezone name

## Step 10: Duration Calculations

Calculate time differences between events:

```bash
# Calculate duration between timestamps
echo '{"start": "2024-01-15T10:00:00Z", "end": "2024-01-15T10:30:00Z"}' | \
    kelora -j \
    -e 'let start_dt = to_datetime(e.start);
        let end_dt = to_datetime(e.end);
        let duration = end_dt - start_dt;
        e.duration_seconds = duration.as_seconds();
        e.duration_minutes = duration.as_minutes();
        e.duration_human = duration.to_string()' \
    -k duration_seconds,duration_minutes,duration_human

# Add duration to timestamp
echo '{"timestamp": "2024-01-15T10:00:00Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp);
        let hour_later = dt + to_duration("1h");
        e.plus_1h = hour_later.to_iso()'

# Duration from number
echo '{"timestamp": "2024-01-15T10:00:00Z"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp);
        let offset = duration_from_minutes(90);
        e.plus_90m = (dt + offset).to_iso()'
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

## Step 10: Time Bucketing for Aggregation

Group events into time buckets for histograms and time-series analysis using `round_to()`:

```bash
# Create sample timestamped data
cat > requests.jsonl <<'EOF'
{"timestamp": "2024-01-15T10:03:45Z", "status": 200}
{"timestamp": "2024-01-15T10:07:23Z", "status": 200}
{"timestamp": "2024-01-15T10:08:12Z", "status": 500}
{"timestamp": "2024-01-15T10:13:56Z", "status": 200}
{"timestamp": "2024-01-15T10:17:34Z", "status": 404}
{"timestamp": "2024-01-15T10:22:01Z", "status": 200}
EOF

# Group requests into 5-minute buckets
kelora -j requests.jsonl \
    -e 'let dt = to_datetime(e.timestamp);
        e.bucket = dt.round_to("5m").to_iso()' \
    -m --exec 'track_freq("requests_per_5min", e.bucket)'
```

**Output:**
```
requests_per_5min:
  2024-01-15T10:00:00+00:00: 2
  2024-01-15T10:05:00+00:00: 2
  2024-01-15T10:10:00+00:00: 1
  2024-01-15T10:15:00+00:00: 1
  2024-01-15T10:20:00+00:00: 1
```

**Different time granularities:**

```bash
# Hourly buckets for daily patterns
kelora -j api_logs.jsonl \
    -e 'let dt = to_datetime(e.timestamp);
        e.hour = dt.round_to("1h").format("%Y-%m-%d %H:00")' \
    -m --exec 'track_freq("hour", e.hour)'

# Daily buckets for weekly trends
kelora -j api_logs.jsonl \
    -e 'e.date = to_datetime(e.timestamp).round_to("1d").format("%Y-%m-%d")' \
    -m --exec 'track_freq("requests_per_day", e.date)'

# 15-minute buckets for fine-grained analysis
kelora -j api_logs.jsonl \
    -e 'e.bucket = to_datetime(e.timestamp).round_to("15m").to_iso()' \
    -m --exec 'track_freq("errors", e.bucket)' \
    --filter 'e.level == "ERROR"'
```

**How `round_to()` works:**

```rhai
// Rounds DOWN to the nearest interval boundary (floor operation)
let dt = to_datetime("2024-01-15T10:34:56Z");

dt.round_to("5m")   // → 2024-01-15T10:30:00Z
dt.round_to("1h")   // → 2024-01-15T10:00:00Z
dt.round_to("1d")   // → 2024-01-15T00:00:00Z
```

**`ceil_to()` rounds UP** to the next boundary (stays if already on one):

```rhai
let dt = to_datetime("2024-01-15T10:34:56Z");

dt.ceil_to("5m")   // → 2024-01-15T10:35:00Z
dt.ceil_to("1h")   // → 2024-01-15T11:00:00Z

// Compute bucket edges
e.bucket_start = dt.round_to("1h").to_iso();
e.bucket_end = dt.ceil_to("1h").to_iso();
```

**Common use cases:**
- **Latency histograms** - Group response times into time windows
- **Traffic patterns** - Analyze request volume over time
- **Error trending** - Track error rates by hour/day
- **Capacity planning** - Identify peak usage periods

## Step 11: Real-World Example - Request Duration Analysis

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
    --filter 'e.duration_ms > 1000' \
    -e 'let dt = to_datetime(e.timestamp);
        e.duration_human = humanize_duration(e.duration_ms);
        e.hour = dt.hour();
        e.formatted_time = dt.format("%H:%M:%S")' \
    -k formatted_time,endpoint,duration_human
```

## Step 12: Time-Based Filtering with Business Hours

Filter logs during business hours across timezones:

```bash
# Filter for events during business hours (9 AM - 5 PM)
kelora -j app.log \
    --input-tz America/New_York \
    -e 'let dt = to_datetime(e.timestamp);
        e.hour = dt.hour()' \
    --filter 'e.hour >= 9 && e.hour < 17'

# Weekend vs weekday analysis
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp);
        let dow = dt.format("%w").to_int();
        e.is_weekend = dow == 0 || dow == 6' \
    --filter 'e.is_weekend'
```

## Step 13: Comparing Timestamps

Use datetime comparison in filters:

```bash
# Events after a specific time
echo '{"timestamp": "2024-01-15T10:30:00Z", "message": "Event 1"}
{"timestamp": "2024-01-15T11:00:00Z", "message": "Event 2"}
{"timestamp": "2024-01-15T11:30:00Z", "message": "Event 3"}' | \
    kelora -j \
    -e 'let dt = to_datetime(e.timestamp);
        let cutoff = to_datetime("2024-01-15T11:00:00Z")' \
    --filter 'dt > cutoff'

# Events within time window
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp);
        let start = to_datetime("2024-01-15T10:00:00Z");
        let end = to_datetime("2024-01-15T11:00:00Z")' \
    --filter 'dt >= start && dt <= end'
```

**Comparison operators:**

- `==`, `!=` - Equality
- `>`, `<` - Greater/less than
- `>=`, `<=` - Greater/less or equal

## Step 14: Current Time Function

Use `now()` for relative time calculations:

```bash
# Find events in last 5 minutes using script
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp);
        let cutoff = now() - to_duration("5m")' \
    --filter 'dt > cutoff'

# Add processing timestamp
kelora -j app.log \
    -e 'e.processed_at = now().to_iso()'

# Calculate event age
kelora -j app.log \
    -e 'let dt = to_datetime(e.timestamp);
        let age = now() - dt;
        e.age_minutes = age.as_minutes()'
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
    -e 'let start = to_datetime(e.start_time);
        let end = to_datetime(e.end_time);
        let duration = end - start;
        e.duration_ms = duration.as_milliseconds()'
```

### Pattern 5: Business Hours Analysis

```bash
# Filter for business hours in specific timezone
kelora -j app.log \
    --input-tz America/New_York \
    -e 'let dt = to_datetime(e.timestamp);
        let hour = dt.hour()' \
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

### Prefer `meta.parsed_ts` Over `to_datetime()`

```bash
# Best - use pre-parsed timestamp (already a DateTime object)
kelora -j app.log \
    -e 'e.hour = meta.parsed_ts.hour();
        e.day = meta.parsed_ts.day();
        e.formatted = meta.parsed_ts.format("%Y-%m-%d")'

# Good - parse once, use multiple times (only if you need a non-detected field)
kelora -j app.log \
    -e 'let dt = to_datetime(e.custom_time);
        e.hour = dt.hour();
        e.day = dt.day()'

# Avoid - parse multiple times (wasteful)
kelora -j app.log \
    -e 'e.hour = to_datetime(e.timestamp).hour();
        e.day = to_datetime(e.timestamp).day()'
```

**Key principle:** Use `meta.parsed_ts` for the primary timestamp field (the one Kelora detects/filters on). Only use `to_datetime()` for additional timestamp fields in your events.

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
    -e 'print("Start: " + e.start_time + ", End: " + e.end_time);
        let start = to_datetime(e.start_time);
        let end = to_datetime(e.end_time);
        e.duration = (end - start).as_seconds()' \
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
