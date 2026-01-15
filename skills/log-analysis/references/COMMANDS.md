# Kelora Command Reference

Extended examples and patterns for common log analysis tasks.

## Format Conversion Examples

### JSON to Other Formats

```bash
# JSON to logfmt
kelora -j -F logfmt events.jsonl

# JSON to CSV with specific columns
kelora -j -F csv -k timestamp,level,service,message events.jsonl

# JSON to human-readable (default)
kelora -j events.jsonl

# Flatten nested JSON for CSV export
kelora -j -e 'e = e.flattened()' -F csv data.jsonl
```

### Text Logs to JSON

```bash
# Auto-detect format, output JSON
kelora -J app.log

# Apache/Nginx combined logs
kelora -f combined -J access.log

# Syslog to JSON
kelora -f syslog -J /var/log/syslog

# Logfmt to JSON
kelora -f logfmt -J app.log

# Custom regex to JSON
kelora -f 'regex:(?P<ts>\S+) \[(?P<level>\w+)\] (?P<msg>.*)' -J app.log
```

### CSV/TSV Processing

```bash
# CSV to JSON
kelora -f csv -J data.csv

# TSV to JSON
kelora -f tsv -J data.tsv

# CSV without headers (columns named col0, col1, ...)
kelora -f csvnh -J data.csv

# JSON to TSV
kelora -j -F tsv -k timestamp,level,message events.jsonl
```

## Filtering Patterns

### By Log Level

```bash
# Errors only
kelora -l ERROR app.log

# Errors and warnings
kelora -l ERROR,WARN app.log

# Everything except debug/trace
kelora -L DEBUG,TRACE app.log

# Filter by level field (for non-standard level names)
kelora --filter 'e.severity == "critical"' app.log
```

### By Time Range

```bash
# Last hour
kelora --since "1 hour ago" app.log

# Last 30 minutes
kelora --since "30 minutes ago" app.log

# Specific date range
kelora --since "2024-01-15 09:00" --until "2024-01-15 17:00" app.log

# Today only
kelora --since "today" app.log

# Yesterday
kelora --since "yesterday" --until "today" app.log
```

### By Field Values

```bash
# Exact match
kelora --filter 'e.status == 500' access.log
kelora --filter 'e.user_id == "u12345"' events.jsonl

# Numeric comparisons
kelora --filter 'e.duration > 1000' api.log
kelora --filter 'e.response_size >= 1000000' access.log

# String contains
kelora --filter 'e.message.contains("timeout")' app.log
kelora --filter 'e.path.contains("/api/")' access.log

# Regex match
kelora --filter 'e.message.matches("error|fail|exception")' app.log

# Glob pattern
kelora --filter 'e.path.like("/api/v*/users/*")' access.log

# Field exists
kelora --filter 'e.has("error_code")' app.log

# Field is null/missing
kelora --filter '!e.has("user_id")' events.jsonl

# Multiple conditions (AND)
kelora --filter 'e.level == "ERROR"' --filter 'e.service == "api"' app.log

# OR conditions (in single filter)
kelora --filter 'e.level == "ERROR" || e.level == "FATAL"' app.log
```

### By IP/Network

```bash
# Specific IP
kelora --filter 'e.client_ip == "192.168.1.100"' access.log

# IP range (CIDR)
kelora --filter 'is_in_cidr(e.ip, "10.0.0.0/8")' access.log

# Private IPs only
kelora --filter 'is_private_ip(e.client_ip)' access.log

# Public IPs only
kelora --filter '!is_private_ip(e.client_ip)' access.log
```

## Transformation Patterns

### Add/Modify Fields

```bash
# Add static field
kelora -e 'e.env = "production"' app.log

# Compute new field
kelora -e 'e.duration_sec = e.duration_ms / 1000.0' api.log

# Conditional field
kelora -e 'e.is_slow = e.duration > 1000' api.log

# Copy/rename field
kelora -e 'e.timestamp = e["@timestamp"]' events.jsonl
```

### Extract Data

```bash
# Extract IPs from message
kelora -e 'e.ips = e.message.extract_ips()' security.log

# Extract domain from URL
kelora -e 'e.domain = e.url.extract_domain()' access.log

# Extract emails
kelora -e 'e.emails = e.body.extract_email()' mail.log

# Parse JSON embedded in field
kelora -e 'e.absorb_json("data")' events.log

# Parse key=value pairs in message
kelora -e 'e.absorb_kv("message")' app.log

# Regex capture groups
kelora -e 'e.user = e.message.extract_regex("user=([^ ]+)")' app.log

# Text between delimiters
kelora -e 'e.request_id = e.message.between("[", "]")' app.log
```

### Clean/Normalize Data

```bash
# Truncate long messages
kelora -e 'e.message = e.message.slice(0, 500)' app.log

# Remove fields
kelora -e 'e._raw = ()' -e 'e._line = ()' app.log

# Lowercase field
kelora -e 'e.method = e.method.lower()' access.log

# Strip whitespace
kelora -e 'e.message = e.message.strip()' app.log

# Mask sensitive IPs
kelora -e 'e.client_ip = mask_ip(e.client_ip, 16)' access.log

# Pseudonymize user IDs
kelora -e 'e.user_id = pseudonym(e.user_id)' events.jsonl
```

## Aggregation and Metrics

### Basic Statistics

```bash
# Summary stats
kelora -s app.log

# Detailed metrics
kelora -m app.log

# Metrics as JSON
kelora -m json app.log

# Suppress events, only show metrics
kelora -q -m app.log
```

### Custom Metrics

```bash
# Count events
kelora --metrics -e 'track_count("total")' app.log

# Count by field value
kelora --metrics -e 'track_count("by_level:" + e.level)' app.log

# Track average
kelora --metrics -e 'track_avg("avg_duration", e.duration)' api.log

# Track min/max
kelora --metrics -e '
  track_min("min_latency", e.latency);
  track_max("max_latency", e.latency);
' api.log

# Percentiles
kelora --metrics -e 'track_percentiles("latency", e.ms, [50, 90, 95, 99])' api.log

# Unique count (cardinality)
kelora --metrics -e 'track_unique("unique_users", e.user_id)' events.jsonl

# Top N values
kelora --metrics -e 'track_top("top_endpoints", e.path, 10)' access.log

# Bottom N values
kelora --metrics -e 'track_bottom("slowest_endpoints", e.path, 10, e.duration)' api.log
```

### Complex Metrics

```bash
# Multiple metrics at once
kelora --metrics -e '
  track_count("requests");
  track_count("errors:" + e.level);
  track_avg("latency", e.duration);
  track_percentiles("response_time", e.ms, [50, 95, 99]);
  track_unique("users", e.user_id);
  track_top("endpoints", e.path, 10);
' api.log

# Bucket distribution
kelora --metrics -e '
  track_bucket("latency_bucket", e.duration, [0, 100, 500, 1000, 5000]);
' api.log

# Write metrics to file
kelora --metrics --metrics-file metrics.json api.log
```

## Pattern Discovery

### Drain Template Mining

```bash
# Discover message templates
kelora --drain app.log

# Full output with examples
kelora --drain full app.log

# JSON output
kelora --drain json app.log

# Just template IDs
kelora --drain id app.log
```

### Use Templates in Processing

```bash
# Add template ID to each event
kelora -e 'e.template_id = drain_template(e.message)' app.log

# Group by template
kelora --metrics -e 'track_count("tpl:" + drain_template(e.message))' app.log
```

## Incident Investigation

### Context Around Events

```bash
# 5 lines before and after errors
kelora -C 5 --filter 'e.level == "ERROR"' app.log

# 10 lines before crashes
kelora -B 10 --filter 'e.message.contains("crash")' app.log

# 3 lines after warnings
kelora -A 3 --filter 'e.level == "WARN"' app.log
```

### Correlation

```bash
# Find all events with same request ID
kelora --filter 'e.request_id == "abc-123-def"' *.log

# Find events around a timestamp
kelora --since "2024-01-15 10:30:00" --until "2024-01-15 10:35:00" app.log

# Find related events in multiple files
kelora --filter 'e.trace_id == "xyz"' api.log worker.log db.log
```

### Session/Span Analysis

```bash
# Group by request ID
kelora --span request_id api.log

# Aggregate session data
kelora --span session_id -e '
  e.event_count = window.len();
  e.errors = window.filter(|x| x.level == "ERROR").len();
  e.duration = window.pluck_as_nums("duration").sum();
' app.log

# Time-based spans (5 minute windows)
kelora --span 5m -e '
  e.count = window.len();
' app.log
```

## Multi-file Processing

```bash
# Multiple specific files
kelora app.log api.log worker.log

# Glob pattern
kelora /var/log/app/*.log

# Recursive glob
kelora **/*.jsonl

# Add source file info
kelora -e 'e.source = e._filename' *.log

# Process files in order
kelora --file-order mtime *.log      # By modification time
kelora --file-order name *.log       # Alphabetical
```

## Performance Tips

```bash
# Preview with limited output
kelora -n 10 huge.log

# Read only first N lines (stops I/O early)
kelora --head 1000 huge.log

# Parallel processing for large files
kelora --parallel huge.log

# Suppress output when only need metrics
kelora -q --metrics huge.log

# Sample events (every 100th)
kelora --filter 'sample_every(100)' huge.log

# Deterministic sampling (consistent hash)
kelora --filter 'bucket(e.request_id, 100) < 10' huge.log  # 10% sample
```

## Output Control

```bash
# Specific fields only
kelora -k timestamp,level,message app.log

# Exclude fields
kelora -K _raw,_line_num,_filename app.log

# Brief output (values only)
kelora -b -k message app.log

# No colors
kelora --no-color app.log

# JSON output to file
kelora -J -o output.jsonl app.log

# Quiet mode (metrics only)
kelora -q -m app.log
```

## Configuration

```bash
# Save command as alias
kelora --save-alias errors -l ERROR --filter 'e.service == "api"'

# Use saved alias
kelora -a errors app.log

# Show current config
kelora --show-config

# Edit config
kelora --edit-config

# Ignore config file
kelora --ignore-config app.log
```
