# Composed Pipelines: Building Powerful Log Analysis Workflows

Learn to build sophisticated log analysis pipelines by composing Kelora's features into multi-stage workflows. This tutorial demonstrates real-world patterns that combine section isolation, multiline reconstruction, format parsing, filtering, metrics, and span aggregation into powerful analytical recipes.

## Overview

Most production incidents require more than a single command. You need to:

1. **Isolate** the relevant section (time window, service, severity)
2. **Reconstruct** multi-line context (stack traces, JSON payloads)
3. **Parse** the format correctly
4. **Filter and transform** to extract insights
5. **Aggregate** with metrics and span rollups

This tutorial shows how to compose these capabilities into complete workflows that answer complex questions.

![Composed Pipeline Overview](../images/composed-pipeline-overview.png#only-light)
![Composed Pipeline Overview](../images/composed-pipeline-overview-dark.png#only-dark)

## What You'll Learn

- Isolate log sections by time, service, or severity
- Reconstruct multi-line stack traces and payloads
- Chain filters and transformations for progressive refinement
- Combine metrics tracking with span-based aggregation
- Build reusable pipeline patterns for common scenarios
- Export results at different pipeline stages

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Essential CLI usage
- [Introduction to Rhai Scripting](intro-to-rhai.md) - Rhai fundamentals
- [Metrics and Tracking](metrics-and-tracking.md) - Understanding `track_*()` functions
- [Span Aggregation](span-aggregation.md) - Time-based and count-based windows
- **Time:** ~30 minutes

## Sample Data

This tutorial uses:

- `examples/multiline_stacktrace.log` - Application logs with stack traces
- `examples/api_logs.jsonl` - API gateway structured logs
- `examples/incident_story.log` - Simulated deployment incident

---

## Pattern 1: Time Window Isolation + Error Analysis

**Scenario**: A deployment happened at 12:32. Analyze errors in the 5-minute window after deployment.

### Step 1: Isolate the Time Section

First, narrow down to the relevant time window:

=== "Command"

    ```bash
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --stats
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --stats
    ```

**What to look for:**
- Event count in the window
- Time span confirmation
- Field availability

### Step 2: Filter to Errors + Extract Structure

Now parse the format and filter to errors:

=== "Command"

    ```bash
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --exec 'e.absorb_kv("line")' \
      --filter 'e.level == "ERROR"' \
      -k timestamp,level,pod,detail
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --exec 'e.absorb_kv("line")' \
      --filter 'e.level == "ERROR"' \
      -k timestamp,level,pod,detail
    ```

**Pipeline flow:**
1. `--since`/`--until`: Isolate time window
2. `--exec`: Parse key=value pairs from the line first
3. `--filter`: Then keep only ERROR level (can access parsed fields)
4. `-k`: Display specific fields

### Step 3: Add Metrics for Summary

Combine with metrics to get error statistics:

=== "Command"

    ```bash
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --exec 'e.absorb_kv("line")' \
      --filter 'e.level == "ERROR"' \
      --exec '
        track_count("total_errors");
        track_top("error_source", e.get_path("pod", "unknown"), 10);
      ' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/incident_story.log \
      --since "2024-06-19T12:32:00Z" \
      --until "2024-06-19T12:37:00Z" \
      --exec 'e.absorb_kv("line")' \
      --filter 'e.level == "ERROR"' \
      --exec '
        track_count("total_errors");
        track_top("error_source", e.get_path("pod", "unknown"), 10);
      ' \
      --metrics
    ```

**Key insight:** Chaining time filtering → error filtering → parsing → metrics gives you both detailed events and aggregate statistics in a single pass.

---

## Pattern 2: Multiline Reconstruction + Pattern Analysis

**Scenario**: Extract and analyze stack traces from application logs to identify root causes.

### Step 1: Reconstruct Stack Traces

Use multiline joining to reconstruct complete stack traces:

=== "Command"

    ```bash
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --take 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --take 3
    ```

**What's happening:**
- `--multiline`: Lines not matching the timestamp pattern are joined to the previous event
- Stack traces become part of the error event's `line` field
- Now we have complete context for each error

### Step 2: Parse and Extract Error Details

Parse the timestamp and level from the reconstructed line, then extract error types:

=== "Command"

    ```bash
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --exec '
        // Extract timestamp and level from first line
        let parts = e.line.substring(0, 25).split(" ");
        e.timestamp = parts[0] + " " + parts[1];
        e.level = parts[2];

        // Extract error type from stack trace
        let lines = e.line.split(" ");
        e.error_summary = parts.len() > 3 ? parts[3] : "Unknown";
      ' \
      -k timestamp,level,error_summary \
      --take 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --exec '
        // Extract timestamp and level from first line
        let parts = e.line.substring(0, 25).split(" ");
        e.timestamp = parts[0] + " " + parts[1];
        e.level = parts[2];

        // Extract error type from stack trace
        let lines = e.line.split(" ");
        e.error_summary = parts.len() > 3 ? parts[3] : "Unknown";
      ' \
      -k timestamp,level,error_summary \
      --take 3
    ```

### Step 3: Aggregate with Drain Pattern Discovery

Use drain to find common error patterns in the reconstructed stack traces:

=== "Command"

    ```bash
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --drain -k line
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/multiline_stacktrace.log \
      --multiline 'regex:match=^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
      --filter 'e.line.contains("ERROR")' \
      --drain -k line
    ```

**Complete workflow:**
1. Reconstruct multi-line stack traces
2. Filter to ERROR events
3. Extract error message from reconstructed line
4. Use drain to discover patterns

---

## Pattern 3: Service Isolation + Span-Based Rollup

**Scenario**: Analyze API errors per service with 1-minute rollups to identify problem services.

### Step 1: Isolate Service and Level

Filter to a specific service and error level:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      -k timestamp,service,message,status
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      -k timestamp,service,message,status
    ```

### Step 2: Add Metrics Tracking

Track error statistics:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      --exec '
        track_count("errors");
        track_stats("response_time", e.get_path("response_time", 0.0));
        track_top("error_type", e.message, 5);
      ' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      --exec '
        track_count("errors");
        track_stats("response_time", e.get_path("response_time", 0.0));
        track_top("error_type", e.message, 5);
      ' \
      --metrics
    ```

### Step 3: Add Span-Based Time Rollup

Now add time-based spans for per-minute summaries:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      --exec '
        track_count("errors");
        track_stats("response_time", e.get_path("response_time", 0.0));
      ' \
      --span 1m \
      --span-close '
        let m = span.metrics;
        let errors = m.get_path("errors", 0);
        let avg_time = m.get_path("response_time_avg", 0);
        print(`${span.start.to_iso()}: ${errors} errors, avg ${avg_time}s response time`);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.service == "auth-service" && e.level == "ERROR"' \
      --exec '
        track_count("errors");
        track_stats("response_time", e.get_path("response_time", 0.0));
      ' \
      --span 1m \
      --span-close '
        let m = span.metrics;
        let errors = m.get_path("errors", 0);
        let avg_time = m.get_path("response_time_avg", 0);
        print(`${span.start.to_iso()}: ${errors} errors, avg ${avg_time}s response time`);
      '
    ```

**Composed pipeline:**
1. Filter to specific service and level
2. Track error metrics per event
3. Group into 1-minute windows
4. Emit per-minute summaries with aggregates

---

## Pattern 4: Progressive Filtering + Multi-Stage Transformation

**Scenario**: Find slow API requests, enrich with computed fields, then analyze by endpoint.

### Step 1: Initial Filter for Slow Requests

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      -k timestamp,service,path,response_time
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      -k timestamp,service,path,response_time
    ```

### Step 2: Add Computed Fields

Classify response times into buckets:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      --exec '
        let rt = e.get_path("response_time", 0.0);
        e.latency_class = if rt > 5.0 {
          "critical"
        } else if rt > 2.0 {
          "high"
        } else {
          "moderate"
        };
      ' \
      -k timestamp,service,response_time,latency_class
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      --exec '
        let rt = e.get_path("response_time", 0.0);
        e.latency_class = if rt > 5.0 {
          "critical"
        } else if rt > 2.0 {
          "high"
        } else {
          "moderate"
        };
      ' \
      -k timestamp,service,response_time,latency_class
    ```

### Step 3: Filter to Critical Cases + Aggregate

Add another filter for critical cases and track:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      --exec '
        let rt = e.get_path("response_time", 0.0);
        e.latency_class = if rt > 5.0 {
          "critical"
        } else if rt > 2.0 {
          "high"
        } else {
          "moderate"
        };
      ' \
      --filter 'e.latency_class == "critical"' \
      --exec '
        track_count("critical_requests");
        track_top("service", e.service, 10);
        track_stats("latency", e.response_time);
      ' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.get_path("response_time", 0.0) > 1.0' \
      --exec '
        let rt = e.get_path("response_time", 0.0);
        e.latency_class = if rt > 5.0 {
          "critical"
        } else if rt > 2.0 {
          "high"
        } else {
          "moderate"
        };
      ' \
      --filter 'e.latency_class == "critical"' \
      --exec '
        track_count("critical_requests");
        track_top("service", e.service, 10);
        track_stats("latency", e.response_time);
      ' \
      --metrics
    ```

**Multi-stage approach:**
1. First filter: Identify slow requests (>1s)
2. First exec: Classify into latency buckets
3. Second filter: Narrow to critical cases
4. Second exec: Track metrics on critical subset

This progressive refinement lets you work with smaller datasets at each stage.

---

## Pattern 5: Format Detection + Mixed Content Handling

**Scenario**: Logs contain both structured JSON and unstructured text. Extract errors from both.

### Approach 1: Process Each Format Separately

For best results with mixed formats, use preprocessing:

```bash
# Extract and analyze JSON errors
grep '^{' examples/mixed_format.log | \
  kelora -f json --filter 'e.level == "ERROR"' -k timestamp,message

# Extract and analyze text errors
grep -v '^{' examples/mixed_format.log | \
  kelora -f line --filter 'e.line.contains("ERROR")' -k line
```

### Approach 2: Fallback Parsing

Handle as line format and parse JSON where possible:

```bash
kelora examples/mixed_format.log \
  -f line \
  --exec '
    // Try to parse as JSON
    if e.line.starts_with("{") {
      let parsed = e.line.parse_json();
      if parsed != () {
        e.level = parsed.get_path("level", "UNKNOWN");
        e.message = parsed.get_path("message", "");
      }
    } else {
      // Plain text - extract level
      if e.line.contains("ERROR") {
        e.level = "ERROR";
      }
    }
  ' \
  --filter 'e.get_path("level") == "ERROR"' \
  -k line_num,level,message
```

**Best practice:** Separate formats upstream with `grep` for best performance and accuracy.

---

## Pattern 6: Complete Incident Analysis Workflow

**Scenario**: End-to-end analysis of an API incident with time isolation, metrics, and rollup.

This brings together everything we've learned:

=== "Command"

    ```bash
    kelora -j examples/api_logs.jsonl \
      --since "2025-01-15T10:24:00Z" \
      --until "2025-01-15T10:30:00Z" \
      --filter 'e.level == "ERROR" || e.get_path("response_time", 0.0) > 2.0' \
      --exec '
        // Classify issue type
        e.issue_type = if e.level == "ERROR" {
          "error"
        } else {
          "latency"
        };

        // Track by type and service
        track_count(e.issue_type);
        track_top("service_issue", e.service + ":" + e.issue_type, 10);
        track_stats("response_time", e.get_path("response_time", 0.0));
      ' \
      --span 2m \
      --span-close '
        let m = span.metrics;
        print(`\n=== Window: ${span.start.to_iso()} ===$()`);
        print(`  Total issues: ${span.size}`);
        print(`  Errors: ${m.get_path("error", 0)}`);
        print(`  Latency: ${m.get_path("latency", 0)}`);
        print(`  Avg response: ${m.get_path("response_time_avg", 0)}s`);
        print(`  P95 response: ${m.get_path("response_time_p95", 0)}s`);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --since "2025-01-15T10:24:00Z" \
      --until "2025-01-15T10:30:00Z" \
      --filter 'e.level == "ERROR" || e.get_path("response_time", 0.0) > 2.0' \
      --exec '
        // Classify issue type
        e.issue_type = if e.level == "ERROR" {
          "error"
        } else {
          "latency"
        };

        // Track by type and service
        track_count(e.issue_type);
        track_top("service_issue", e.service + ":" + e.issue_type, 10);
        track_stats("response_time", e.get_path("response_time", 0.0));
      ' \
      --span 2m \
      --span-close '
        let m = span.metrics;
        print(`\n=== Window: ${span.start.to_iso()} ===$()`);
        print(`  Total issues: ${span.size}`);
        print(`  Errors: ${m.get_path("error", 0)}`);
        print(`  Latency: ${m.get_path("latency", 0)}`);
        print(`  Avg response: ${m.get_path("response_time_avg", 0)}s`);
        print(`  P95 response: ${m.get_path("response_time_p95", 0)}s`);
      '
    ```

**Complete pipeline stages:**
1. **Isolation**: Time window (--since/--until)
2. **Filtering**: Errors or slow requests
3. **Classification**: Compute issue types
4. **Metrics**: Track by category and service
5. **Aggregation**: 2-minute window rollups
6. **Summary**: Per-window statistics

---

## Pattern 7: Export Pipeline at Multiple Stages

**Scenario**: Export filtered data for external tools while also computing local metrics.

### Export Filtered Events

Save filtered events to a file:

```bash
kelora -j examples/api_logs.jsonl \
  --filter 'e.level == "ERROR"' \
  --exec 'e.absorb_kv("message")' \
  -F json > errors-export.jsonl
```

### Export Metrics to JSON

Compute metrics and save to file:

```bash
kelora -j examples/api_logs.jsonl \
  --filter 'e.level == "ERROR"' \
  --exec '
    track_count("total");
    track_top("service", e.service, 10);
    track_stats("response_time", e.get_path("response_time", 0.0));
  ' \
  --metrics-file incident-metrics.json \
  --silent
```

### Combined: Events + Metrics

Export events to stdout, metrics to file:

```bash
kelora -j examples/api_logs.jsonl \
  --filter 'e.level == "ERROR"' \
  --exec 'track_count(e.service)' \
  --metrics-file metrics.json \
  -F json > events.jsonl
```

**Use cases:**
- Forward events to external systems (Elasticsearch, S3)
- Save metrics for trending/dashboards
- Share analysis results with team

---

## Common Mistakes

**❌ Problem:** Forgetting multiline reconstruction loses context
```bash
kelora stack-traces.log --filter 'e.line.contains("ERROR")'
# Stack traces are split across events
```
**✅ Solution:** Use `--multiline` to reconstruct:
```bash
kelora stack-traces.log --multiline 'regex:match=^[0-9]{4}-' --filter 'e.line.contains("ERROR")'
```

---

**❌ Problem:** Wrong filter order processes too much data
```bash
kelora huge.log --exec 'expensive_transform(e)' --filter 'e.level == "ERROR"'
# Transform runs on ALL events, then filters
```
**✅ Solution:** Filter first, then transform:
```bash
kelora huge.log --filter 'e.level == "ERROR"' --exec 'expensive_transform(e)'
```

---

**❌ Problem:** Not using safe field access causes crashes
```bash
kelora api.log --filter 'e.response_time > 1.0'
# Crashes if response_time field is missing
```
**✅ Solution:** Use `.get_path()` with defaults:
```bash
kelora api.log --filter 'e.get_path("response_time", 0.0) > 1.0'
```

---

**❌ Problem:** Span mode incompatible with parallel processing
```bash
kelora huge.log --parallel --span 5m --span-close '...'
# Error: --span incompatible with --parallel
```
**✅ Solution:** Remove --parallel for span processing:
```bash
kelora huge.log --span 5m --span-close '...'
```

---

**❌ Problem:** Time filters on unsorted logs miss events
```bash
kelora unsorted.log --since "2024-01-15T10:00:00Z"
# May miss events if timestamps are out of order
```
**✅ Solution:** Pre-sort by timestamp or use line-based filtering:
```bash
sort -t'"' -k4 unsorted.log | kelora -j --since "2024-01-15T10:00:00Z"
```

---

## Tips & Best Practices

### Pipeline Design Principles

1. **Filter early, transform late**: Reduce data volume as soon as possible
2. **Use progressive refinement**: Multiple simple filters beat one complex filter
3. **Safe field access**: Always use `.get_path(field, default)` for optional fields
4. **Reconstruct context first**: Apply multiline joins before other processing
5. **Combine filters and metrics**: Single-pass analysis is more efficient

### Composition Patterns

**Pattern: Funnel Analysis**
```bash
# Wide → narrow with metrics at each stage
kelora app.log \
  --exec 'track_count("total")' \
  --filter 'e.level == "ERROR"' \
  --exec 'track_count("errors")' \
  --filter 'e.service == "api"' \
  --exec 'track_count("api_errors")' \
  --metrics
```

**Pattern: Enrich Then Filter**
```bash
# Add computed fields, then filter on them
kelora api.log \
  --exec 'e.is_slow = e.response_time > 1.0' \
  --exec 'e.is_error = e.status >= 500' \
  --filter 'e.is_slow && e.is_error' \
  --metrics
```

**Pattern: Multi-Dimensional Aggregation**
```bash
# Track multiple dimensions in one pass
kelora app.log \
  --exec '
    track_count(e.level);
    track_count(e.service);
    track_count(e.level + ":" + e.service);
    track_stats("latency", e.response_time);
  ' \
  --metrics
```

### Performance Optimization

1. **Filter before expensive operations**: Parsing, transformations, tracking
2. **Use format-specific options**: `-j` for JSON, `-f logfmt` for key=value
3. **Leverage parallel mode**: For large files without spans: `--parallel`
4. **Sample huge datasets**: `--filter 'sample_every(100)'` for multi-TB logs
5. **Limit output early**: Use `--take N` to stop after N events

### Debugging Pipelines

**Check each stage:**
```bash
# Stage 1: Verify time filtering
kelora app.log --since "2024-01-15T10:00:00Z" --stats

# Stage 2: Add error filtering
kelora app.log --since "2024-01-15T10:00:00Z" --filter 'e.level == "ERROR"' --stats

# Stage 3: Add parsing
kelora app.log --since "2024-01-15T10:00:00Z" --filter 'e.level == "ERROR"' --exec 'e.absorb_kv("line")' -J

# Stage 4: Add metrics
kelora app.log --since "2024-01-15T10:00:00Z" --filter 'e.level == "ERROR"' --exec 'track_count("total")' -m
```

**Use inspect format:**
```bash
kelora app.log --take 1 -F inspect
# Shows all fields and their types
```

### Reusability

Save common pipelines as shell functions:

```bash
# In ~/.bashrc or ~/.zshrc
kelora-errors() {
  kelora "$@" \
    --filter 'e.level == "ERROR"' \
    --exec 'track_count("total"); track_top("service", e.service, 10)' \
    --metrics
}

# Usage
kelora-errors app.log
```

Or use Kelora's configuration system:

```ini
# In .kelora.ini
[alias.errors]
filter = e.level == "ERROR"
exec = track_count("total"); track_top("service", e.service, 10)
metrics = true
```

Then run: `kelora +errors app.log`

---

## Summary

You've learned to compose powerful log analysis pipelines:

- ✅ **Section isolation** with time filters and service/level filtering
- ✅ **Multiline reconstruction** for stack traces and context
- ✅ **Progressive filtering** to refine datasets efficiently
- ✅ **Multi-stage transformation** to enrich and classify events
- ✅ **Metrics tracking** for aggregate statistics
- ✅ **Span-based rollups** for time-window summaries
- ✅ **Export at multiple stages** for external tools
- ✅ **Debugging techniques** to verify each pipeline stage

**Key composition patterns:**

| Pattern | Use Case | Example |
|---------|----------|---------|
| Time + Filter + Metrics | Incident analysis | `--since X --filter Y --exec 'track_*()' -m` |
| Multiline + Parse + Drain | Stack trace analysis | `--multiline 'regex:match=...' --drain` |
| Filter + Enrich + Filter | Progressive refinement | `--filter X --exec 'e.field=...' --filter Y` |
| Service + Metrics + Span | Per-service rollups | `--filter 'service==X' --exec 'track_*()' --span 1m` |
| Export + Metrics | External + local analysis | `-F json > file.json --metrics-file m.json` |

## Next Steps

Now that you understand pipeline composition, explore advanced topics:

- **[Configuration and Reusability](configuration-and-reusability.md)** - Save pipelines as reusable aliases
- **[How-To: Incident Response Playbooks](../how-to/incident-response-playbooks.md)** - Real-world incident patterns
- **[How-To: Power-User Techniques](../how-to/power-user-techniques.md)** - Advanced Rhai functions
- **[Concepts: Pipeline Model](../concepts/pipeline-model.md)** - Deep dive into execution model

**Related guides:**

- [Multiline Strategies](../concepts/multiline-strategies.md) - All multiline handling approaches
- [Script Variables](../reference/script-variables.md) - Complete variable reference
- [CLI Reference](../reference/cli-reference.md) - All command-line options
