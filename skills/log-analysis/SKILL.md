---
name: log-analysis
description: Analyze, filter, transform, and convert log files using Kelora. Use when users need to parse logs, extract patterns, investigate incidents, calculate metrics, or convert between log formats (JSON, logfmt, syslog, CSV, etc.).
compatibility: Requires Kelora CLI installed and accessible via `kelora` command
metadata:
  author: kelora
  version: "1.0"
---

# Log Analysis with Kelora

You are helping users analyze and process log files using the Kelora CLI tool. Kelora is a streaming log processor with Rhai scripting support.

## Quick Reference

### Supported Input Formats
- **Auto-detect**: `-f auto` (default)
- **JSON/JSONL**: `-f json` or `-j`
- **Logfmt**: `-f logfmt` (key=value pairs)
- **Syslog**: `-f syslog` (RFC3164/5424)
- **CEF**: `-f cef` (Common Event Format)
- **CSV/TSV**: `-f csv`, `-f tsv`, `-f csvnh` (no headers)
- **Combined**: `-f combined` (Apache/Nginx access logs)
- **Plain text**: `-f line`
- **Regex**: `-f 'regex:<pattern>'` with named captures
- **Columns**: `-f 'cols:<spec>'` for fixed-width

### Output Formats
- **Human-readable**: default (colorized)
- **JSON**: `-F json` or `-J`
- **Logfmt**: `-F logfmt`
- **CSV/TSV**: `-F csv`, `-F tsv`
- **Inspect**: `-F inspect` (debug view)

## Common Tasks

### 1. Filter Logs

Filter by log level:
```bash
kelora -l ERROR,WARN app.log           # Only errors and warnings
kelora -L DEBUG,TRACE app.log          # Exclude verbose levels
```

Filter by expression:
```bash
kelora --filter "e.status >= 500" access.log
kelora --filter "e.duration > 1000" api.log
kelora --filter 'e.message.contains("timeout")' app.log
kelora --filter 'e.user_id == "u123"' events.jsonl
```

Filter by time range:
```bash
kelora --since "1 hour ago" app.log
kelora --since "2024-01-15" --until "2024-01-16" app.log
kelora --since "09:00" --until "17:00" app.log
```

Combine multiple filters (AND logic):
```bash
kelora --filter "e.level == 'ERROR'" --filter "e.service == 'api'" app.log
```

### 2. Transform and Enrich

Add/modify fields:
```bash
kelora -e 'e.env = "production"' app.log
kelora -e 'e.duration_sec = e.duration_ms / 1000.0' api.log
kelora -e 'e.short_msg = e.message.slice(0, 100)' app.log
```

Extract data from fields:
```bash
kelora -e 'e.ips = e.message.extract_ips()' security.log
kelora -e 'e.domain = e.url.extract_domain()' access.log
kelora -e 'e.absorb_json("data")' events.log  # Parse JSON in field
kelora -e 'e.absorb_kv("message")' app.log    # Parse key=value pairs
```

Normalize timestamps:
```bash
kelora --normalize-ts app.log
kelora --ts-format "%Y-%m-%d %H:%M:%S" --input-tz "America/New_York" app.log
```

### 3. Convert Between Formats

JSON to logfmt:
```bash
kelora -j -F logfmt events.jsonl > events.log
```

Apache logs to JSON:
```bash
kelora -f combined -J access.log > access.jsonl
```

Syslog to CSV:
```bash
kelora -f syslog -F csv -k timestamp,level,message syslog.log > syslog.csv
```

Any format to JSON (preserving structure):
```bash
kelora -J input.log > output.jsonl
```

### 4. Aggregate and Analyze

Get statistics:
```bash
kelora -s app.log                    # Summary stats
kelora -m app.log                    # Detailed metrics
kelora -m json app.log               # Metrics as JSON
```

Track custom metrics:
```bash
kelora --metrics -e '
  track_count("requests");
  track_avg("latency", e.duration);
  track_percentiles("response_time", e.ms, [50, 95, 99]);
  track_unique("users", e.user_id);
  track_top("endpoints", e.path, 10);
' api.log
```

Group by field:
```bash
kelora --metrics -e 'track_count("by_level:" + e.level)' app.log
kelora --metrics -e 'track_count("by_status:" + e.status)' access.log
```

### 5. Pattern Discovery

Find log templates (Drain algorithm):
```bash
kelora --drain app.log               # Discover message patterns
kelora --drain full app.log          # With example messages
kelora --drain json app.log          # As JSON for processing
```

Use in scripts:
```bash
kelora -e 'e.template_id = drain_template(e.message)' app.log
```

### 6. Incident Investigation

Context around errors:
```bash
kelora -C 5 --filter "e.level == 'ERROR'" app.log   # 5 lines before/after
kelora -B 10 --filter 'e.message.contains("crash")' app.log
```

Time-window analysis:
```bash
kelora --since "10:30" --until "10:45" --filter "e.level != 'DEBUG'" app.log
```

Correlate by request ID:
```bash
kelora --filter 'e.request_id == "abc123"' *.log
```

Session/span analysis:
```bash
kelora --span request_id -e '
  e.event_count = window.len();
  e.total_duration = window.pluck_as_nums("duration").sum();
' api.log
```

### 7. Multi-file Processing

Process multiple files:
```bash
kelora app.log api.log worker.log
kelora /var/log/app/*.log
kelora **/*.jsonl                    # Recursive glob
```

Add file metadata:
```bash
kelora -e 'e.source = e._filename' *.log
```

### 8. Streaming and Pipes

Tail and process:
```bash
tail -f /var/log/app.log | kelora --filter "e.level == 'ERROR'"
```

Pipeline with other tools:
```bash
kelora -J app.log | jq '.message'
cat *.log | kelora -f auto -J | gzip > combined.jsonl.gz
```

## Field Access

Access event fields with `e.fieldname` or `e["field-name"]` for special characters:

```rhai
e.level                    // Direct access
e["@timestamp"]            // Fields with special chars
e.nested.field             // Nested access
e.get_path("a.b.c")        // Safe nested access (returns () if missing)
e.has("field")             // Check field exists
```

## Useful Rhai Functions

### String Operations
- `e.msg.contains("error")` - Substring check
- `e.msg.like("*timeout*")` - Glob pattern
- `e.msg.matches("\\d{3}")` - Regex match
- `e.msg.extract_regex("user=(.+)")` - Capture groups
- `e.msg.after("error: ")` - Text after marker
- `e.msg.between("[", "]")` - Text between delimiters

### Parsing
- `parse_json(text)` - Parse JSON string
- `parse_logfmt(text)` - Parse key=value
- `parse_url(url)` - URL components
- `parse_user_agent(ua)` - Browser/OS info

### IP/Network
- `is_ipv4(ip)`, `is_ipv6(ip)` - Validate
- `is_private_ip(ip)` - Check RFC1918
- `is_in_cidr(ip, "10.0.0.0/8")` - CIDR check
- `mask_ip(ip, 16)` - Anonymize

### DateTime
- `e.ts.format("%Y-%m-%d")` - Format timestamp
- `e.ts.to_utc()`, `e.ts.to_local()` - Timezone
- `now()` - Current time
- `to_duration("5m")` - Parse duration

### Arrays
- `arr.filter(|x| x > 0)` - Filter
- `arr.map(|x| x * 2)` - Transform
- `arr.pluck("field")` - Extract field from objects
- `arr.unique()`, `arr.sorted()` - Dedupe/sort

## Output Control

```bash
kelora -q app.log              # Suppress events, show only stats
kelora -k level,message app.log  # Only specific fields
kelora -K _raw,_line app.log   # Exclude fields
kelora -b app.log              # Brief: values only
kelora -n 100 app.log          # First 100 events
kelora --head 1000 app.log     # Read only first 1000 lines (fast)
```

## Best Practices

1. **Start simple**: Use `-f auto` and let Kelora detect the format
2. **Test filters**: Use `-n 10` to preview before processing large files
3. **Use --head for exploration**: `--head 100` stops reading early, much faster than `-n`
4. **Combine with -q**: When you only need metrics, use `-q` to suppress event output
5. **Quote complex filters**: Use single quotes around Rhai expressions
6. **Check the built-in help**: Run `kelora --help-functions` for all 150+ functions

## See Also

- [Command Reference](references/COMMANDS.md) - Extended command examples
- Run `kelora --help-examples` for more patterns
- Run `kelora --help-functions` for all available functions
