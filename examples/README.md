# Kelora Examples

This directory contains sample files for testing Kelora with different log formats, edge cases, and real-world scenarios. Use these files to experiment with filters, transformations, and parsing strategies before processing your own logs.

For detailed guides and tutorials, see the [documentation](https://kelora.dev).

## Quick Start

New to Kelora? Try these first:

```bash
# Basic filtering and transformation
kelora examples/quickstart.log --filter 'e.line.contains("ERROR")'

# JSON log analysis
kelora -f json examples/simple_json.jsonl --filter 'e.level == "ERROR"' --exec '#{level: e.level, message: e.message}'

# Web access log parsing
kelora examples/web_access.log --filter 'e.status >= 400'

# Using Rhai helper functions for enrichment
kelora --include examples/helpers.rhai examples/api_logs.jsonl \
  --exec 'e.severity = classify_severity(e.level, e.get_path("response_time", 0.0))'
```

Then run `kelora --help-examples` for common patterns and usage recipes.

## Pattern Discovery with Drain

Automatically discover log message templates using the `--drain` flag:
For lightweight bucketing without Drain, you can pre-normalize a field with `normalized()` in `--exec`.

```bash
# Find common patterns (clean output)
kelora examples/app_monitoring.jsonl --drain -k message

# Show line numbers, samples, and IDs for each pattern
kelora examples/app_monitoring.jsonl --drain=full -k message

# Show stable ID list for diffs
kelora examples/app_monitoring.jsonl --drain=id -k message

# Export as JSON for analysis
kelora examples/app_monitoring.jsonl --drain=json -k message
```

**Example output:**
```
templates (18 items):
  6: Connection timeout to database host <fqdn>
  6: Failed login attempt for user alice from <ipv4>
  4: GET <path> completed in <duration> with status <num>
  4: Rate limit exceeded for API key <num> on endpoint <path>
  ...
```

The `--drain=full` format adds context:
```
  6: Connection timeout to database host <fqdn>
     id: v1:5f3c7a9b1d2e4f6a
     lines: 1-36
     sample: "Connection timeout to database host db-primary-01.prod.internal:5432"
```

Use drain to quickly understand log patterns before writing filters or building dashboards.

## Filter Patterns (Boolean Logic)

```bash
# Parentheses to combine OR within an AND
kelora -j examples/api_logs.jsonl --filter '(e.service == "auth-service" || e.service == "api-gateway") && e.get_path("status", 0) >= 500'

# Array membership with mixed thresholds
kelora -j examples/api_logs.jsonl --filter '["POST","PUT"].contains(e.get_path("method")) && (e.get_path("status", 0) >= 500 || e.get_path("response_time", 0.0) > 1.5)'

# Guard against missing fields before comparing
kelora -j examples/api_logs.jsonl --filter 'e.get_path("stack_trace") != () && e.level == "ERROR"'

# Chain multiple filters for readability (order preserved)
kelora -j examples/api_logs.jsonl \
  --filter 'e.get_path("status", 0) >= 400' \
  --filter 'e.service == "auth-service" || e.get_path("metadata.subscription.tier") == "premium"' \
  --filter 'e.get_path("response_time", 0.0) > 0.2'
```

## File Organization

Examples follow a naming convention for easy discovery:

### Basic Format Examples (`simple_*`)

Start here to understand Kelora's format auto-detection:

- `simple_json.jsonl` - Structured JSON logs
- `simple_csv.csv` - Comma-separated values with headers
- `simple_tsv.tsv` - Tab-separated values
- `simple_logfmt.log` - Logfmt key=value format
- `simple_syslog.log` - Standard syslog messages
- `simple_combined.log` - Apache combined log format
- `simple_cef.log` - Common Event Format
- `simple_line.log` - Unstructured text logs

### Error Handling & Edge Cases (`errors_*`)

Test Kelora's robustness with malformed or unusual input:

- `errors_json_mixed.jsonl` - Mixed valid/invalid JSON
- `errors_json_types.jsonl` - Type handling edge cases
- `errors_csv_ragged.csv` - Rows with varying column counts
- `errors_empty_lines.log` - Empty lines and whitespace
- `errors_unicode.log` - Unicode handling
- `errors_filter_runtime.jsonl` - Filter expression errors
- `errors_exec_transform.jsonl` - Transformation errors

### Mixed Format Handling

Real-world logs often contain multiple formats in the same file (Docker prefixes, stack traces mixed with JSON, etc.):

- `mixed_format.log` - Sample file with JSON and plain text intermixed
- `mixed_format_demo.sh` - Demonstrates preprocessing techniques for handling mixed formats

**Key Insight:** Kelora excels at processing single-format structured logs. For mixed formats, the recommended approach is to **preprocess logs** to split by format before processing:

```bash
# Extract and process JSON lines only
grep '^{' mixed_format.log | kelora -f json --filter 'e.level == "error"'

# Process plain text lines separately
grep -v '^{' mixed_format.log | kelora -f line --filter 'e.line.contains("ERROR")'
```

See `mixed_format_demo.sh` for more examples of handling mixed-format logs using standard Unix tools.

### Multiline Handling (`multiline_*`)

Different strategies for parsing multi-line log entries:

- `multiline_stacktrace.log` - Stack traces and exceptions
- `multiline_continuation.log` - Line continuation patterns
- `multiline_indent.log` - Indentation-based grouping
- `multiline_boundary.log` - Delimiter-based boundaries
- `multiline_json_arrays.log` - JSON arrays spanning lines

See `kelora --help-multiline` for detailed multiline strategies.

### Real-World Scenarios

Production-like log files for testing realistic use cases:

- `api_logs.jsonl` - API gateway requests with nested metadata
- `app_monitoring.jsonl` - Application monitoring logs with repeated patterns (great for `--drain`)
- `web_access.log` - Web server access logs
- `security_audit.jsonl` - Security audit events
- `k8s_security.jsonl` - Kubernetes security logs
- `auth_burst.jsonl` - Authentication burst patterns
- `payments_latency.jsonl` - Payment processing latency
- `email_logs.log` - Email delivery logs
- `duration_logs.jsonl` - Performance timing analysis
- `uptime_windows.jsonl` - Service uptime windows
- `incident_story.log` - Simulated incident timeline
- And many more...

### Power-User Technique Examples

Examples for advanced features from the [Power-User Techniques](https://kelora.dev/how-to/power-user-techniques/) guide:

- `production-errors.jsonl` - Pattern normalization with `normalized()`
- `user-activity.jsonl` - Deterministic sampling with `bucket()`
- `deeply-nested.jsonl` - Structure flattening with `flattened()`
- `auth-logs.jsonl` - JWT parsing with `parse_jwt()`
- `error-logs.jsonl` - Fuzzy matching with `edit_distance()`
- `user-data.jsonl` - Multi-algorithm hashing
- `analytics.jsonl` - Privacy-preserving pseudonymization
- `user-events.jsonl` - Stateful processing with `state` map

### Performance Monitoring (Tailmap Visualization)

Visualize numeric field distributions over time using tailmap format:

- `api_latency_incident.jsonl` - API performance degradation and recovery
- `database_queries.jsonl` - Database query performance analysis

```bash
# Visualize API latency incident timeline
kelora -j examples/api_latency_incident.jsonl -F tailmap --keys response_time_ms

# Find slow database queries
kelora -j examples/database_queries.jsonl -F tailmap --keys query_time_ms
```

Tailmap uses percentile-based symbols: `_` (below p90), `1` (p90-p95), `2` (p95-p99), `3` (above p99)

### Specialized Formats

- `cols_fixed.log`, `cols_mixed.log` - Fixed-width columns
- `csv_typed.csv` - CSV with type inference
- `prefix_docker.log` - Docker container logs with prefixes
- `prefix_custom.log` - Custom prefix patterns
- `custom_timestamps.log` - Non-standard timestamp formats
- `timezones_mixed.log` - Mixed timezone handling
- `kv_pairs.log` - Key-value pair extraction
- `regex_apache_style.log` - Custom regex parsing
- `regex_custom_format.log` - User-defined patterns
- `fan_out_batches.jsonl` - Flattening nested arrays
- `json_nested_deep.jsonl` - Deep object nesting
- `json_arrays.jsonl` - Array handling
- `window_metrics.jsonl` - Time window aggregation
- `sampling_hash.jsonl.gz` - Deterministic sampling (compressed)
- `web_access_large.log.gz` - Large file processing (compressed)

### Stress Tests (`nightmare_*`)

Complex scenarios for testing performance and correctness:

- `nightmare_mixed_formats.log` - Multiple formats in one file
- `nightmare_deeply_nested_transform.jsonl` - Complex nested transformations

## Rhai Helper Scripts

Reusable Rhai functions that you can include in your pipelines with `--include`:

### `helpers.rhai`

Common utility functions for log analysis:

```bash
# Enrich events with computed severity
kelora --include examples/helpers.rhai examples/api_logs.jsonl \
  --exec 'e.severity = classify_severity(e.level, e.get_path("response_time", 0.0))'

# Mask sensitive fields
kelora --include examples/helpers.rhai examples/api_logs.jsonl \
  --exec 'if e.has("email") { e.email = mask_sensitive(e.email); }'
```

Functions:
- `is_problem(event)` - Check if event is an error or slow
- `classify_severity(level, value)` - Categorize severity
- `extract_domain(text)` - Extract domain from URL/email
- `mask_sensitive(value)` - Mask sensitive data

**Note:** For filtering, prefer `--filter` with inline expressions. Use `e = ()` in `--exec` only when you need helper functions for complex logic that can't be expressed inline.

### `enrich_events.rhai`

Example of event enrichment and transformation patterns.

### `resolve_fields.rhai`

Semantic field resolution for cross-format log analysis. Different log formats use different field names for the same concepts (e.g., `response_time` vs `latency` vs `duration_ms`). This module provides functions to resolve these concepts regardless of the actual field name used.

```bash
# Filter slow requests regardless of field naming convention
kelora --include examples/resolve_fields.rhai -f json logs.jsonl \
  --exec 'if resolve_duration(e) > 1000 { e } else { () }'

# Aggregate by user across mixed log sources
kelora --include examples/resolve_fields.rhai -f json *.jsonl \
  --exec 'track_stats(resolve_user(e) ?? "unknown", resolve_duration(e) ?? 0)'

# Check for errors using multiple indicators (fields, level, status)
kelora --include examples/resolve_fields.rhai -f json logs.jsonl \
  --filter 'has_error(e)'
```

Functions:
- `resolve_duration(e)` / `resolve_duration_name(e)` - response_time, latency, elapsed, etc.
- `resolve_user(e)` / `resolve_user_name(e)` - user_id, userId, username, etc.
- `resolve_client_ip(e)` / `resolve_client_ip_name(e)` - ip, client_ip, remote_addr, etc.
- `resolve_error(e)` / `resolve_error_name(e)` - error, exception, fault, etc.
- `resolve_request_id(e)` / `resolve_request_id_name(e)` - request_id, trace_id, correlation_id, etc.
- `resolve_status(e)` / `resolve_status_name(e)` - status, status_code, http_status, etc.
- `has_error(e)` - Check if event has any error indicators (fields, level, or status code)
- `resolve_field_concepts()` - List available concepts

Copy and customize the file to add organization-specific field names.

### `patterns.rhai`

Pattern detection and extraction using curated regex patterns for common data types (IPs, emails, URLs, durations, UUIDs, etc.). See file header for usage examples.

## Finding the Right Example

- **By format**: Look for `simple_<format>.*` files
- **By use case**: Browse real-world scenario files (api_logs, web_access, etc.)
- **By feature**: Use prefixes (multiline_, errors_, etc.)
- **By complexity**: Start with `simple_*`, progress to real-world, test with `nightmare_*`

Use `grep`, `ls`, or your editor's file search to quickly locate examples.

## Using Examples

All examples work with Kelora's CLI:

```bash
# Auto-detect format
kelora examples/simple_json.jsonl

# Specify format explicitly
kelora -f json examples/api_logs.jsonl

# Chain operations
kelora examples/web_access.log --filter 'e.status >= 400' --exec '#{ip: e.client_ip, path: e.path}'

# Include helper scripts for enrichment
kelora --include examples/helpers.rhai examples/api_logs.jsonl \
  --exec 'e.domain = extract_domain(e.get_path("email", ""))'

# Compressed files work too
kelora examples/web_access_large.log.gz
```

## Next Steps

- [Documentation](https://kelora.dev) - How-to guides and tutorials
- `kelora --help` - Complete CLI reference
- `kelora --help-functions` - All 150+ built-in Rhai functions
- `kelora --help-examples` - Common usage patterns
- `kelora --help-rhai` - Rhai scripting guide
