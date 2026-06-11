# Span Aggregation: Time-Based and Count-Based Windows

Group events into non-overlapping spans for periodic rollups, time-series analysis, and windowed statistics. This tutorial shows you how to aggregate logs into fixed windows without losing per-event processing power.

## Overview

Kelora's span feature groups filtered events into logical blocks during log processing. Events can be grouped by count, time windows, or field changes. The system tracks metadata for each span and allows emission of per-span summaries via a hook script when each span closes.

![Span Aggregation Overview](../images/span-aggregation-overview.png#only-light)
![Span Aggregation Overview](../images/span-aggregation-overview-dark.png#only-dark)

The diagram above illustrates the complete span aggregation pipeline, showing how events flow through parsing, filtering, and per-event scripts before being grouped into spans. Three span modes are available: count-based (batching N events), time-based (fixed time windows), and field-change based (grouping by field values). The span lifecycle manages event buffering, tracks metadata, and triggers span-close hooks for summaries and analytics.

## What You'll Learn

- Create count-based spans to batch every N events
- Build time-aligned windows for dashboard rollups
- Access span events and per-span metrics in `--span-close` hooks
- Handle late arrivals and missing timestamps gracefully
- Tag events with span metadata for downstream tools
- Choose between count and time spans for your use case

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Basic CLI usage
- [Introduction to Rhai Scripting](intro-to-rhai.md) - Rhai fundamentals
- [Begin and End Stages](begin-end-stages.md) - Understanding --begin and --end
- **Time:** ~20 minutes

## Sample Data

This tutorial uses:

- `examples/simple_json.jsonl` - Application logs with timestamps
- Generated data for demonstrations

All commands use `markdown-exec` format, so output is live from the actual CLI.

---

## Step 1: Count-Based Spans – Batch Every N Events

The simplest span mode: close a span every N events that survive your filters.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 5 \
      --span-close 'print("Span " + span.id + " had " + span.size.to_string() + " events")'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 5 \
      --span-close 'print("Span " + span.id + " had " + span.size.to_string() + " events")'
    ```

**What's happening:**

- `--span 5` batches every 5 events
- Spans are numbered sequentially: `#0`, `#1`, `#2`, etc.
- `--span-close` runs when each span finishes
- `span.id` and `span.size` provide basic span info

**Use case:** "Every 1000 errors, emit a summary line" or "Batch API calls into groups of 100 for rate limiting."

### Count Spans Operate Post-Filter

Spans count events that *survive* your filters:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --filter 'e.level == "ERROR"' \
      --span 2 \
      --span-close 'print("Error batch " + span.id + ": " + span.size.to_string() + " errors")'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --filter 'e.level == "ERROR"' \
      --span 2 \
      --span-close 'print("Error batch " + span.id + ": " + span.size.to_string() + " errors")'
    ```

The filter removes non-ERROR events, then spans batch every 2 *remaining* errors.

---

## Step 2: Accessing Span Events – Iterate Over the Batch

The `span.events` array contains all events that were included in the span:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 5 \
      --span-close '
        print("=== Span " + span.id + " ===");
        for evt in span.events {
          print("  " + evt.service + ": " + evt.message);
        }
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 5 \
      --span-close '
        print("=== Span " + span.id + " ===");
        for evt in span.events {
          print("  " + evt.service + ": " + evt.message);
        }
      '
    ```

**What's in `span.events`:**

- Each element is a full event map with all fields
- Includes original fields plus `line`, `line_num`, `filename` (if applicable)
- Includes span metadata: `span_status`, `span_id`, `span_start`, `span_end`

### Span Script Variable Reference

Inside `--span-close` the important bindings are `span.id`, `span.start`, `span.end`, `span.size`, `span.events`, `span.metrics`, and the global `metrics` map. Use `span.metrics` for per-span deltas and `metrics` for cumulative totals. See the [Script Variables reference](../reference/script-variables.md) for the full scope matrix, including helpers such as `meta.span_status`, `meta.span_id`, and when `window` is available.

> `--span-close` executes outside the per-event pipeline, so treat it as a summary hook: rely on `span.*` and `metrics` for aggregates, and loop over `span.events` when you need per-event details.

**Use case:** Collect request IDs for correlation, extract specific fields for detailed reports, or forward events to external systems.

### Extract Specific Fields from Events

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --filter 'e.level == "ERROR" || e.level == "WARN"' \
      --span 3 \
      --span-close '
        let services = span.events.map(|e| e.service).join(", ");
        print("Span " + span.id + " services: " + services);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --filter 'e.level == "ERROR" || e.level == "WARN"' \
      --span 3 \
      --span-close '
        let services = span.events.map(|e| e.service).join(", ");
        print("Span " + span.id + " services: " + services);
      '
    ```

The `.map()` function extracts a field from each event, creating an array of values you can process.

---

## Step 3: Time-Based Spans – Aligned Windows

Use durations (`5m`, `1h`, `30s`) to create fixed wall-clock windows:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 1m \
      --span-close 'print("Window: " + span.id + " (" + span.size.to_string() + " events)")'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 1m \
      --span-close 'print("Window: " + span.id + " (" + span.size.to_string() + " events)")'
    ```

**Time span properties:**

- Span IDs are `ISO-timestamp/duration` format: `2024-01-15T10:00:00Z/1m`
- Windows align to duration boundaries (not first event timestamp)
- `span.start` and `span.end` are DateTime objects
- Requires events to have timestamps (auto-detected or parsed)

### Working with Span Start and End Times

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 2m \
      --span-close '
        print("Window: " + span.id);
        print("  Start: " + span.start.to_iso());
        print("  End: " + span.end.to_iso());
        print("  Events: " + span.size.to_string());
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --span 2m \
      --span-close '
        print("Window: " + span.id);
        print("  Start: " + span.start.to_iso());
        print("  End: " + span.end.to_iso());
        print("  Events: " + span.size.to_string());
      '
    ```

`span.start` and `span.end` are DateTime objects with full date/time methods available.

**Use case:** Generate 5-minute rollups for dashboards, create hourly error summaries, build time-series metrics aligned to fixed intervals.

---

## Field-Based Spans – Value Changes

Open a new span whenever a field value changes. This is great for per-request or per-session rollups without pre-sorting.

```bash
kelora -j examples/simple_json.jsonl \
  --span request_id \
  --span-close 'print("Request " + span.id + ": " + span.size.to_string() + " events")'
```

**Notes:**

- Missing fields stay in the current span (error with `--strict`); the first missing field opens an `(unset)` span.
- Single-active-span model: interleaved IDs (`req-1, req-2, req-1`) create multiple spans per ID.

---

## Idle-Based Spans – Inactivity Timeouts

Detect sessions or bursts separated by quiet periods.

```bash
kelora -j examples/simple_json.jsonl \
  --span-idle 5m \
  --span-close 'print("Session " + span.id + ": " + span.size.to_string() + " events")'
```

**Notes:**

- Requires timestamps; missing timestamps are `unassigned` (error with `--strict`).
- Only forward-time gaps close spans. Out-of-order events are included in the current span; sort input first if you need strict ordering.
- `--span-idle` is mutually exclusive with `--span`.

---

## Step 4: Per-Span Metrics – Automatic Deltas

Combine `track_*()` functions in `--exec` with `span.metrics` in `--span-close` for automatic per-window aggregation:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec '
        track_sum("total", 1);
        if e.level == "ERROR" { track_sum("errors", 1); }
        if e.level == "WARN" { track_sum("warnings", 1); }
      ' \
      --span 5 \
      --span-close '
        let m = span.metrics;
        let total = m.get_path("total", 0);
        let errors = m.get_path("errors", 0);
        let warnings = m.get_path("warnings", 0);
        print(`Span ${span.id}: ${total} total, ${errors} errors, ${warnings} warnings`);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec '
        track_sum("total", 1);
        if e.level == "ERROR" { track_sum("errors", 1); }
        if e.level == "WARN" { track_sum("warnings", 1); }
      ' \
      --span 5 \
      --span-close '
        let m = span.metrics;
        let total = m.get_path("total", 0);
        let errors = m.get_path("errors", 0);
        let warnings = m.get_path("warnings", 0);
        print(`Span ${span.id}: ${total} total, ${errors} errors, ${warnings} warnings`);
      '
    ```

**How span metrics work:**

1. Kelora snapshots metrics at span open (baseline)
2. Your `--exec` scripts call `track_*()` functions per event
3. At span close, Kelora computes deltas: `current - baseline`
4. Only deltas appear in `span.metrics` (zeros are omitted)
5. Metrics auto-reset after each span (no manual clearing needed)

**Note:** Use `.get_path(key, default)` to safely extract values, since only non-zero deltas are included.

### Time-Based Metrics Example

Calculate error rates per 1-minute window:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec '
        track_sum("requests", 1);
        if e.level == "ERROR" { track_sum("errors", 1); }
      ' \
      --span 1m \
      --span-close '
        let m = span.metrics;
        let requests = m.get_path("requests", 0);
        let errors = m.get_path("errors", 0);
        let rate = if requests > 0 { (errors * 100) / requests } else { 0 };
        print(`${span.start.to_iso()}: ${errors}/${requests} errors (${rate}%)`);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec '
        track_sum("requests", 1);
        if e.level == "ERROR" { track_sum("errors", 1); }
      ' \
      --span 1m \
      --span-close '
        let m = span.metrics;
        let requests = m.get_path("requests", 0);
        let errors = m.get_path("errors", 0);
        let rate = if requests > 0 { (errors * 100) / requests } else { 0 };
        print(`${span.start.to_iso()}: ${errors}/${requests} errors (${rate}%)`);
      '
    ```

---

## Step 5: Global Metrics – Compare Spans to Cumulative Totals

Inside `--span-close` you have access to **two** metrics maps:

- **`span.metrics`** – Delta for this span only (resets after each span)
- **`metrics`** – Cumulative totals across the entire run

This lets you compare per-window activity against overall trends:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'track_sum("total", 1); if e.level == "ERROR" { track_sum("errors", 1); }' \
      --span 5 \
      --span-close '
        let span_err = span.metrics.get_path("errors", 0);
        let total_err = metrics.get_path("errors", 0);
        eprint(`Span ${span.id}: +${span_err} errors (total: ${total_err})`);
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'track_sum("total", 1); if e.level == "ERROR" { track_sum("errors", 1); }' \
      --span 5 \
      --span-close '
        let span_err = span.metrics.get_path("errors", 0);
        let total_err = metrics.get_path("errors", 0);
        eprint(`Span ${span.id}: +${span_err} errors (total: ${total_err})`);
      '
    ```

**Key differences:**

| Feature | `span.metrics` | `metrics` |
|---------|----------------|-----------|
| Scope | Current span only | Entire run |
| Values | Deltas (what changed) | Cumulative totals |
| Reset | After each span | Never |

### Spike Detection – Compare to Average

Detect anomalies by comparing span activity to overall rates:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'track_sum("requests", 1); if e.level == "ERROR" { track_sum("errors", 1); }' \
      --span 3 \
      --span-close '
        let span_rate = span.metrics.get_path("errors", 0) * 100 / span.size;
        let total_rate = metrics.get_path("errors", 0) * 100 /
                         metrics.get_path("requests", 1);
        if span_rate > total_rate * 2 {
          eprint(`🚨 Spike in ${span.id}: ${span_rate}% (avg: ${total_rate}%)`);
        }
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'track_sum("requests", 1); if e.level == "ERROR" { track_sum("errors", 1); }' \
      --span 3 \
      --span-close '
        let span_rate = span.metrics.get_path("errors", 0) * 100 / span.size;
        let total_rate = metrics.get_path("errors", 0) * 100 /
                         metrics.get_path("requests", 1);
        if span_rate > total_rate * 2 {
          eprint(`🚨 Spike in ${span.id}: ${span_rate}% (avg: ${total_rate}%)`);
        }
      '
    ```

**Use case:** Detect sudden increases in error rates, latency spikes, or unusual traffic patterns relative to baseline.

---

## Step 6: Late Events and Missing Timestamps

Time-based spans track event status for better handling of out-of-order or missing data.

### Detecting Late Events

Events arriving earlier than the current span window are marked as "late":

=== "Command"

    ```bash
    echo '{"ts":"2024-01-15T10:05:00Z","msg":"Event 1"}
    {"ts":"2024-01-15T10:06:00Z","msg":"Event 2"}
    {"ts":"2024-01-15T10:04:00Z","msg":"Late event"}
    {"ts":"2024-01-15T10:07:00Z","msg":"Event 3"}' | \
    kelora -j \
      --span 1m \
      --exec '
        if meta.span_status == "late" {
          eprint(`⚠️  Late event: ${e.msg} at ${e.ts}`);
        }
      ' \
      --span-close 'print("Window " + span.id + ": " + span.size.to_string() + " events")'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"ts":"2024-01-15T10:05:00Z","msg":"Event 1"}
    {"ts":"2024-01-15T10:06:00Z","msg":"Event 2"}
    {"ts":"2024-01-15T10:04:00Z","msg":"Late event"}
    {"ts":"2024-01-15T10:07:00Z","msg":"Event 3"}' | \
    kelora -j \
      --span 1m \
      --exec '
        if meta.span_status == "late" {
          eprint(`⚠️  Late event: ${e.msg} at ${e.ts}`);
        }
      ' \
      --span-close 'print("Window " + span.id + ": " + span.size.to_string() + " events")'
    ```

**Late event behavior:**

- Late events still pass through your `--filter` and `--exec` scripts
- They are tagged with `meta.span_status == "late"`
- They do NOT reopen closed spans
- They do NOT appear in `span.events` of closed spans

**Best practice:** Pre-sort logs by timestamp if you need accurate time windows. Or use late event detection to emit warnings/corrections.

### Handling Missing Timestamps

Events without timestamps are marked as "unassigned":

=== "Command"

    ```bash
    echo '{"ts":"2024-01-15T10:05:00Z","msg":"Has timestamp"}
    {"msg":"No timestamp"}
    {"ts":"2024-01-15T10:06:00Z","msg":"Has timestamp 2"}' | \
    kelora -j \
      --span 1m \
      --exec '
        if meta.span_status == "unassigned" {
          eprint("⚠️  No timestamp: " + e.msg);
        }
      ' \
      --span-close 'print("Window " + span.id + ": " + span.size.to_string() + " events")'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"ts":"2024-01-15T10:05:00Z","msg":"Has timestamp"}
    {"msg":"No timestamp"}
    {"ts":"2024-01-15T10:06:00Z","msg":"Has timestamp 2"}' | \
    kelora -j \
      --span 1m \
      --exec '
        if meta.span_status == "unassigned" {
          eprint("⚠️  No timestamp: " + e.msg);
        }
      ' \
      --span-close 'print("Window " + span.id + ": " + span.size.to_string() + " events")'
    ```

**Unassigned event behavior:**

- Cannot be assigned to a time window
- Tagged with `meta.span_status == "unassigned"`
- Still pass through filters and exec scripts
- Do NOT appear in `span.events`

**Strict mode:** Use `--strict` to abort on missing timestamps instead of continuing:

```bash
kelora -j --span 5m --strict app.log
# Error: event missing required timestamp for --span
```

---

## Step 7: Tagging Events Without --span-close

You can use `--span` to tag events with window metadata without writing a close hook:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      --span 2m \
      --exec 'e.window_id = meta.span_id' \
      --take 5 \
      -k timestamp,service,window_id
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      --span 2m \
      --exec 'e.window_id = meta.span_id' \
      --take 5 \
      -k timestamp,service,window_id
    ```

**Use case:** Forward events to external tools (DuckDB, jq, spreadsheets) that will do their own grouping:

```bash
kelora -j logs/*.jsonl --span 5m --exec 'e.window = meta.span_id' -F json |
  jq -sc 'group_by(.window) | map({window: .[0].window, count: length})'
```

This is **lightweight** – Kelora doesn't buffer events or compute metrics, just tags them with span IDs.

---

## Step 8: Combining Filters, Execs, and Spans

Spans work seamlessly with multi-stage pipelines:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'e.is_error = (e.level == "ERROR")' \
      --filter 'e.is_error' \
      --exec 'track_count("service", e.service)' \
      --span 2 \
      --span-close '
        print("Error batch " + span.id + ":");
        let m = span.metrics;
        for key in m.keys() {
          print("  " + key + ": " + m[key].to_string());
        }
      '
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -q \
      --exec 'e.is_error = (e.level == "ERROR")' \
      --filter 'e.is_error' \
      --exec 'track_count("service", e.service)' \
      --span 2 \
      --span-close '
        print("Error batch " + span.id + ":");
        let m = span.metrics;
        for key in m.keys() {
          print("  " + key + ": " + m[key].to_string());
        }
      '
    ```

**Pipeline flow:**

1. First `--exec`: Add computed field
2. `--filter`: Keep only errors
3. Second `--exec`: Track metrics (only errors reach this)
4. `--span`: Batch every 2 errors
5. `--span-close`: Report per-span metrics

Spans operate on the *result* of your filters, so you get precise control over what gets batched.

---

## Summary

You've learned to use span aggregation for time-windowed and count-based rollups:

- ✅ **Count spans** (`--span N`) batch every N post-filter events
- ✅ **Time spans** (`--span 5m`) create aligned wall-clock windows
- ✅ **span.id** identifies spans (`#0` for count, `ISO/duration` for time)
- ✅ **span.size** counts events that survived
- ✅ **span.events** provides full event arrays for detailed analysis
- ✅ **span.metrics** gives automatic per-span metric deltas
- ✅ **metrics** provides cumulative totals across all spans
- ✅ **meta.span_status** detects late/unassigned/filtered events
- ✅ Spans work seamlessly with filters, execs, and tracking functions

**Key properties:**

| Property | Count Spans | Time Spans |
|----------|-------------|------------|
| Span ID | `#0`, `#1`, ... | `2024-01-15T10:00:00Z/5m` |
| span.start | `()` (not applicable) | DateTime object |
| span.end | `()` (not applicable) | DateTime object |
| span.size | Event count | Event count |
| span.events | Full event array | Full event array |
| span.metrics | Per-span deltas | Per-span deltas |
| metrics | Global cumulative totals | Global cumulative totals |

## Common Mistakes

**❌ Problem:** Trying to use spans with `--parallel`
```bash
kelora -j --span 5 --parallel app.log
# Error: --span incompatible with --parallel
```
**✅ Solution:** Spans require sequential mode. Remove `--parallel`.

---

**❌ Problem:** Accessing span metrics in `--exec`
```bash
kelora -j --span 5 --exec 'print(span.metrics)' app.log
# Error: span not available in exec stage
```
**✅ Solution:** `span.metrics` is only available in `--span-close`.

---

**❌ Problem:** Late events with unsorted logs
```bash
# Logs are not sorted by timestamp
kelora -j random-order.log --span 5m --span-close '...'
# Many events marked as "late"
```
**✅ Solution:** Pre-sort logs by timestamp:
```bash
kelora -j random-order.log --output jsonl | sort -t'"' -k4 | kelora -j --span 5m --span-close '...'
```

---

**❌ Problem:** Missing timestamps in time spans
```bash
kelora -f logfmt app.log --span 5m
# Events without timestamps are "unassigned"
```
**✅ Solution:** Ensure logs have timestamp fields, or use `--span N` (count-based) instead.

---

**❌ Problem:** Large count spans consuming memory
```bash
kelora -j huge.log --span 1000000 --span-close 'for evt in span.events { ... }'
# High memory usage
```
**✅ Solution:** Use smaller spans, or switch to time-based spans which naturally limit buffering. If you don't need `span.events`, omit `--span-close` to avoid buffering.

---

**❌ Problem:** Forgetting to use `.get_path()` with span.metrics
```bash
--span-close 'let errors = span.metrics["errors"]'
# Crashes if "errors" key doesn't exist (zero delta omitted)
```
**✅ Solution:** Always use `.get_path()` with a default:
```bash
--span-close 'let errors = span.metrics.get_path("errors", 0)'
```

## Tips & Best Practices

### Quick Span Type Reference

Choose the right span mode for your use case:

```bash
--span N              # Count-based: Every N events (batch processing)
--span 5m             # Time-based: Fixed time windows (dashboards, time-series)
--span field_name     # Field-based: Group by field value (requests, sessions, IDs)
--span-idle 10m       # Idle-based: Detect inactivity gaps (sessions, bursts)
```

### Choosing Count vs Time Spans

**Use count spans when:**

- You want fixed-size batches (every 1000 errors, every 100 requests)
- Timestamps are unavailable or unreliable
- Event ordering doesn't matter
- You're doing arrival-order analysis

**Use time spans when:**

- You need dashboard/time-series rollups
- You want aligned windows (every 5 minutes on the clock)
- You're correlating with external time-based systems
- Order matters (financial transactions, audit logs)

### Memory Considerations

- **Count spans** buffer all events until `span.size` is reached
- **Time spans** buffer events within the current time window
- If you don't need `span.events`, skip `--span-close` to save memory
- Consider using `--span-close` without accessing `span.events` if you only need metrics

### Sorting Recommendations

For time-based spans with out-of-order logs:

```bash
# Sort upstream before span processing
cat *.log | sort | kelora -j --span 5m --span-close '...'

# Or use GNU sort with large files
sort -S 2G --parallel=4 huge.log | kelora -j --span 1h --span-close '...'
```

### Using Both Metrics Maps Effectively

Combine `span.metrics` and `metrics` for context-aware reporting:

```rhai
// Pattern 1: Running commentary with context
let span_errors = span.metrics.get_path("errors", 0);
let total_errors = metrics.get_path("errors", 0);
eprint(`+${span_errors} errors (total: ${total_errors})`);
```

```rhai
// Pattern 2: Threshold alerts after warmup
let total_processed = metrics.get_path("total", 0);
if total_processed > 1000 {  // Only alert after warmup
  let span_rate = span.metrics.get_path("errors", 0) / span.size;
  if span_rate > 0.10 { eprint("⚠️  Error rate exceeds 10%"); }
}
```

**When to use each:**

- `span.metrics`: "What changed in this window?"
- `metrics`: "What's the big picture?"

### Signal Handling

If you press Ctrl+C during span processing, Kelora waits for the current `--span-close` hook to finish before exiting (press Ctrl+C twice to force quit). This prevents incomplete span reports.

## Next Steps

Now that you understand span aggregation, continue your journey:

- **[Configuration and Reusability](configuration-and-reusability.md)** - Save span pipelines as reusable aliases
- **[How-To: Roll Up Logs with Span Windows](../how-to/span-aggregation-cookbook.md)** - More advanced span patterns
- **[Concepts: Pipeline Model](../concepts/pipeline-model.md)** - Deep dive into span processing architecture

**Related guides:**

- [Metrics and Tracking](metrics-and-tracking.md) - Understanding `track_*()` functions
- [Working with Time](working-with-time.md) - Timestamp parsing and filtering
- [Script Variables: State](../reference/script-variables.md#state) - Complex cross-event state tracking
- [How-To: Build a Service Health Snapshot](../how-to/monitor-application-health.md) - Real-world monitoring patterns
