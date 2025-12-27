# Metrics and Tracking

Turn raw log streams into actionable numbers. This tutorial walks through Kelora's
metrics pipeline, from basic counters to custom summaries that you can export or
feed into dashboards.

## What You'll Learn

- Track counts, sums, buckets, and unique values with Rhai helpers
- Combine `--metrics`, `--stats`, `--begin`, and `--end` for structured reports
- Use `track_percentiles()` for streaming P50/P95/P99 latency analysis
- Use `track_stats()` for comprehensive statistics (min, max, avg, percentiles) in one call
- Use sliding windows for moving averages and rolling calculations
- Persist metrics to disk for downstream processing

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Basic CLI usage
- [Introduction to Rhai Scripting](intro-to-rhai.md) - Rhai fundamentals
- **Time:** ~25 minutes

## Sample Data

Commands below use fixtures from the repository. If you cloned the project, the
paths resolve relative to the docs root:

- `examples/simple_json.jsonl` — mixed application logs
- `examples/window_metrics.jsonl` — high-frequency metric samples
- `examples/web_access_large.log.gz` — compressed access logs for batch jobs

All commands print real output thanks to `markdown-exec`; feel free to tweak the
expressions and rerun them locally.

## Step 1 – Quick Counts with `track_count()`

Count how many events belong to each service while suppressing event output.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      --metrics
    ```

`--metrics` prints the aggregated map when processing finishes. Use this pattern
any time you want a quick histogram after a batch run.

### Showing Stats at the Same Time

Pair `--metrics` with `--stats` when you need throughput details as well:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -m --stats
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -m --stats
    ```

`--stats` adds processing totals, time span, and field inventory without touching
your metrics map.

## Step 2 – Summaries with Sums and Averages

Kelora ships several helpers for numeric metrics. The following example treats
response sizes and latency as rolling aggregates.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_sum("total_duration", e.duration_ms);
              track_count("duration_count");
              track_min("min_duration", e.duration_ms);
              track_max("max_duration", e.duration_ms)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_sum("total_duration", e.duration_ms); track_count("duration_count"); track_min("min_duration", e.duration_ms); track_max("max_duration", e.duration_ms) }' \
      --metrics
    ```

**Available aggregation functions:**

- `track_sum(key, value)` - Accumulates totals (throughput, volume)
- `track_avg(key, value)` - Calculates averages automatically (stores sum and count internally)
- `track_min(key, value)` - Tracks minimum value seen
- `track_max(key, value)` - Tracks maximum value seen
- `track_count(key)` - Counts occurrences of key
- `track_inc(key, amount)` - Increment counter by amount (not shown above)

**Quick example of `track_avg()`:**
```rhai
# Track average response time automatically
kelora -j api_logs.jsonl -m \
  --exec 'if e.has("duration_ms") { track_avg("avg_latency", e.duration_ms) }'
```

The `track_avg()` function internally stores both sum and count, then computes the average during output. This works correctly even in parallel mode.

## Step 3 – Histograms with track_bucket()

Build histograms by grouping values into buckets—perfect for latency distributions.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              let bucket = (e.duration_ms / 1000) * 1000;
              track_bucket("latency_histogram", bucket)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { let bucket = (e.duration_ms / 1000) * 1000; track_bucket("latency_histogram", bucket) }' \
      --metrics
    ```

`track_bucket(key, bucket_value)` creates nested counters where each unique bucket
value maintains its own count. Perfect for building histograms.

**Common bucketing patterns:**

```rhai
// Round to nearest 100ms
track_bucket("latency", (duration_ms / 100) * 100)

// HTTP status code families
track_bucket("status_family", (status / 100) * 100)

// File size buckets (KB)
track_bucket("file_sizes", (bytes / 1024))

// Hour of day
track_bucket("hour_of_day", timestamp.hour())
```

## Step 4 – Top N Rankings with track_top() / track_bottom()

When you need the "top 10 errors" or "5 slowest endpoints" without tracking everything, use `track_top()` and `track_bottom()`. These functions maintain bounded, sorted lists—much more memory-efficient than `track_bucket()` for high-cardinality data.

### Frequency Rankings (Count Mode)

Track the most/least frequent items:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.level == "error" { track_top("top_errors", e.message, 5) }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.level == "error" { track_top("top_errors", e.message, 5) }' \
      --metrics
    ```

Each entry shows the item key and its occurrence count. Results are sorted by count (descending), then alphabetically.

### Value-Based Rankings (Weighted Mode)

Track items by custom values like latency, bytes, or CPU time:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_top("slowest", e.service, 3, e.duration_ms);
              track_bottom("fastest", e.service, 3, e.duration_ms)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_top("slowest", e.service, 3, e.duration_ms); track_bottom("fastest", e.service, 3, e.duration_ms) }' \
      --metrics
    ```

In weighted mode:
- `track_top()` keeps the N items with **highest** values (slowest, largest, etc.)
- `track_bottom()` keeps the N items with **lowest** values (fastest, smallest, etc.)
- For each item, the **maximum** (top) or **minimum** (bottom) value seen is retained

**When to use top/bottom vs bucket:**

| Scenario | Use This | Why |
|----------|----------|-----|
| "Top 10 error messages" | `track_top()` | Bounded memory, auto-sorted |
| "Error count by type" (low cardinality) | `track_bucket()` | Tracks all types |
| "Latency distribution 0-1000ms" | `track_bucket()` | Need full histogram |
| "10 slowest API calls" | `track_top()` | Only care about extremes |
| Millions of unique IPs | `track_top()` | Bucket would exhaust memory |

## Step 5 – Unique Values and Cardinality

`track_unique()` stores distinct values for a key—handy for unique user counts or
cardinality analysis.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_unique("services", e.service)' \
      -e 'if e.level == "ERROR" { track_unique("error_messages", e.message) }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'track_unique("services", e.service)' \
      -e 'if e.level == "ERROR" { track_unique("error_messages", e.message) }' \
      --metrics
    ```

Use `metrics["services"].len()` later to compute the number of distinct members.

### Viewing Metrics in Different Formats

By default, `-m` shows all tracked items in table format. For large collections,
you can use `--metrics=short` to see just the first 5 items, or `--metrics=json`
for structured JSON output:

=== "Full table (default)"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_unique("services", e.service)' \
      -m
    ```

=== "Abbreviated with --metrics=short"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_unique("services", e.service)' \
      --metrics=short
    ```

=== "JSON format with --metrics=json"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_unique("services", e.service)' \
      --metrics=json
    ```

The `-m` flag defaults to full table format showing all items. Use `--metrics=short`
for abbreviated output (first 5 items with a hint), or `--metrics=json` for structured
JSON to stdout. You can also combine `-m` with `--metrics-file` to get both table
output and a JSON file.

## Step 6 – Streaming Percentiles with track_percentiles()

Track percentiles across your entire dataset using the memory-efficient t-digest
algorithm. Perfect for latency analysis and SLO monitoring.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_percentiles("latency", e.duration_ms)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_percentiles("latency", e.duration_ms) }' \
      --metrics
    ```

By default, `track_percentiles()` tracks P50 (median), P95, and P99, creating auto-suffixed
metrics (`latency_p50`, `latency_p95`, `latency_p99`). This uses ~4KB per metric
regardless of event count, making it suitable for millions of events.

### Custom Percentiles

Specify custom percentiles using an array (0.0-1.0 range):

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_percentiles("latency", e.duration_ms, [0.50, 0.95, 0.999])
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_percentiles("latency", e.duration_ms, [0.50, 0.95, 0.999]) }' \
      --metrics
    ```

This creates `latency_p50` (median), `latency_p95`, and `latency_p99.9`. The
percentile accuracy is ~1-2% relative error, suitable for operational monitoring.

### Sliding Windows for Moving Averages

Use `--window` when you need moving averages or percentiles over the most recent
N events (instead of the entire stream):

=== "Command"

    ```bash
    kelora -j examples/window_metrics.jsonl \
      --filter 'e.metric == "cpu"' \
      --window 5 \
      -e $'let values = window.pluck_as_nums("value");
    if values.len() > 0 {
        let sum = values.reduce(|s, x| s + x, 0.0);
        let avg = sum / values.len();
        e.avg_last_5 = round(avg * 100.0) / 100.0;
        if values.len() >= 3 {
            e.p95_last_5 = round(values.percentile(95.0) * 100.0) / 100.0;
        }
    }' \
      -n 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/window_metrics.jsonl \
      --filter 'e.metric == "cpu"' \
      --window 5 \
      -e $'let values = window.pluck_as_nums("value");
    if values.len() > 0 {
        let sum = values.reduce(|s, x| s + x, 0.0);
        let avg = sum / values.len();
        e.avg_last_5 = round(avg * 100.0) / 100.0;
        if values.len() >= 3 {
            e.p95_last_5 = round(values.percentile(95.0) * 100.0) / 100.0;
        }
    }' \
      -n 5
    ```

The `window` variable becomes available with `--window N`. Use
`window.pluck_as_nums("FIELD")` for numeric arrays and `window.pluck("FIELD")`
for raw values.

**When to use each approach:**

| Use Case | Function | Why |
|----------|----------|-----|
| P95/P99 latency across entire dataset | `track_percentiles()` | Memory-efficient, works with millions of events |
| Moving average of last N events | `--window` + manual calc | Need sliding/rolling calculations |
| One-shot percentiles at end of stream | `--window` (unbounded) | Simple, exact percentiles on small datasets |

---

## Step 6.5 – Comprehensive Stats with track_stats()

When you need the **complete statistical picture** of a metric (min, max, avg, percentiles), use `track_stats()` as a convenience function instead of calling multiple `track_*()` functions. This is especially useful for latency analysis, API monitoring, and performance dashboards.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_stats("response_time", e.duration_ms)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_stats("response_time", e.duration_ms) }' \
      --metrics
    ```

A single call to `track_stats("response_time", e.duration_ms)` creates:

- `response_time_min` - Minimum value seen
- `response_time_max` - Maximum value seen
- `response_time_avg` - Average (stored as sum+count internally)
- `response_time_count` - Total count
- `response_time_sum` - Total sum
- `response_time_p50` - Median (50th percentile)
- `response_time_p95` - 95th percentile
- `response_time_p99` - 99th percentile

### Custom Percentiles with track_stats()

You can specify custom percentiles just like with `track_percentiles()`:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") {
              track_stats("latency", e.duration_ms, [0.50, 0.90, 0.99, 0.999])
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'if e.has("duration_ms") { track_stats("latency", e.duration_ms, [0.50, 0.90, 0.99, 0.999]) }' \
      --metrics
    ```

This creates all basic stats plus `latency_p50`, `latency_p90`, `latency_p99`, and `latency_p99.9`.

### When to Use track_stats() vs. Individual Functions

**Use `track_stats()`** when:

- You want the complete statistical picture in one call
- Analyzing latency, response time, or duration metrics
- Building dashboards that need min/max/avg/percentiles
- Prototyping or exploring data characteristics

**Use individual functions** (`track_min`, `track_max`, `track_avg`, `track_percentiles`) when:

- You only need specific statistics (saves memory)
- Fine-grained control over which metrics are tracked
- Avoiding percentile overhead (~4KB per metric)

!!! tip "Performance Note"
    `track_stats()` internally calls the same logic as individual tracking functions, so performance is similar. The main memory overhead comes from percentile tracking (~4KB per metric). If you don't need percentiles, use `track_min()`, `track_max()`, and `track_avg()` instead.

---

## Step 7 – Custom Reports with `--end`

Sometimes you need a formatted report instead of raw maps. Store a short Rhai
script and include it with `-I` so the same layout works across platforms, then
call the helper from `--end`.

=== "Command"

    ```bash
    cat <<'RHAI' > metrics_summary.rhai
    fn summarize_metrics() {
        let keys = metrics.keys();
        keys.sort();
        for key in keys {
            print(key + ": " + metrics[key].to_string());
        }
    }
    RHAI

    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -e 'track_count(e.level)' \
      -m \
      -I metrics_summary.rhai \
      --end 'summarize_metrics()'

    rm metrics_summary.rhai
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    cat <<'RHAI' > metrics_summary.rhai
    fn summarize_metrics() {
        let keys = metrics.keys();
        keys.sort();
        for key in keys {
            print(key + ": " + metrics[key].to_string());
        }
    }
    RHAI

    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -e 'track_count(e.level)' \
      -m \
      -I metrics_summary.rhai \
      --end 'summarize_metrics()'

    rm metrics_summary.rhai
    ```

The automatically printed `--metrics` block remains, while `--end` gives you a
clean text summary that you can redirect or feed into alerts.

## Step 8 – Persist Metrics to Disk

Use `--metrics-file` to serialize the metrics map as JSON for other tools.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -m \
      --metrics-file metrics.json

    cat metrics.json
    rm metrics.json
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -e 'track_count(e.service)' \
      -m \
      --metrics-file metrics.json

    cat metrics.json
    rm metrics.json
    ```

The JSON structure mirrors the in-memory map, so you can load it with `jq`, a
dashboard agent, or any scripting language.

## Step 9 – Streaming Scoreboards

Kelora keeps metrics up to date even when tailing files or processing archives.
This command watches a gzipped access log and surfaces top status classes.

=== "Command"

    ```bash
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let klass = ((e.status / 100) * 100).to_string(); track_count(klass)' \
      -m \
      -n 0
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let klass = ((e.status / 100) * 100).to_string(); track_count(klass)' \
      -m \
      -n 0
    ```

Passing `--take 0` (or omitting it) keeps processing the entire file. When you run
Kelora against a stream (`tail -f | kelora ...`), the metrics snapshot updates when
you terminate the process.

Need full histograms instead of counts? Swap in `track_bucket()`:

=== "Command"

    ```bash
    kelora -f combined examples/web_access_large.log.gz \
      -m \
      -e 'track_bucket("status_family", (e.status / 100) * 100)' \
      --end '
        let buckets = metrics.status_family.keys();
        buckets.sort();
        for bucket in buckets {
            let counts = metrics.status_family[bucket];
            print(bucket.to_string() + ": " + counts.to_string());
        }
      ' \
      -n 0
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access_large.log.gz \
      -m \
      -e 'track_bucket("status_family", (e.status / 100) * 100)' \
      --end '
        let buckets = metrics.status_family.keys();
        buckets.sort();
        for bucket in buckets {
            let counts = metrics.status_family[bucket];
            print(bucket.to_string() + ": " + counts.to_string());
        }
      ' \
      -n 0
    ```

`track_bucket(key, bucket_value)` keeps nested counters so you can emit a
human-readable histogram once processing finishes.

## Troubleshooting

- **No metrics printed**: Ensure you pass `-m` (or `--metrics`) or consume `metrics`
  within an `--end` script. Tracking functions alone do not emit output.

- **Truncated arrays**: If `-m` shows only the first 5 items with a hint, use `--metrics=full`
  for full table output, `--metrics=json` for JSON format, or `--metrics-file` to
  write JSON to disk.

- **Huge maps**: Reset counters between runs by clearing your terminal or using
  `rm metrics.json` when exporting to disk. Large cardinality sets from
  `track_unique()` are the usual culprit.

- **Operation metadata**: Kelora keeps operator hints (the `__op_*` keys)
  in the internal tracker now, so user metric maps print cleanly. If you need
  those hints for custom aggregation, read them from the internal metrics map.

- **Sliding window functions return empty arrays**: `window.pluck_as_nums("field")`
  only works after you enable `--window` and the requested field exists in the
  buffered events.

## Quick Reference: All Tracking Functions

| Function | Purpose | Example |
|----------|---------|---------|
| `track_count(key)` | Count events by key | `track_count(e.service)` |
| `track_inc(key, amount)` | Increment by amount | `track_inc("total_bytes", e.size)` |
| `track_sum(key, value)` | Sum numeric values | `track_sum("bandwidth", e.bytes)` |
| `track_avg(key, value)` | Average numeric values | `track_avg("avg_latency", e.duration)` |
| `track_min(key, value)` | Minimum value | `track_min("fastest", e.duration)` |
| `track_max(key, value)` | Maximum value | `track_max("slowest", e.duration)` |
| `track_percentiles(key, value, [percentiles])` | Streaming percentiles (P50/P95/P99 default) | `track_percentiles("latency", e.duration_ms)` |
| `track_stats(key, value, [percentiles])` | **Comprehensive stats:** min, max, avg, count, sum, percentiles | `track_stats("response_time", e.duration_ms)` |
| `track_bucket(key, bucket)` | Histogram buckets | `track_bucket("status", (e.status/100)*100)` |
| `track_top(key, item, n)` | Top N most frequent items | `track_top("errors", e.message, 10)` |
| `track_top(key, item, n, score)` | Top N by highest scores | `track_top("slowest", e.endpoint, 10, e.latency)` |
| `track_bottom(key, item, n, score)` | Bottom N by lowest scores | `track_bottom("fastest", e.endpoint, 10, e.latency)` |
| `track_unique(key, value)` | Unique values | `track_unique("users", e.user_id)` |

**Notes:**
- `track_avg()` automatically computes averages by storing sum and count internally
- `track_percentiles()` and `track_stats()` auto-suffix metrics (e.g., `latency_p95`, `latency_p99`)
- `track_stats()` is a convenience function that creates `_min`, `_max`, `_avg`, `_count`, `_sum`, and `_pXX` metrics
- Use percentiles for tail latency (P95, P99) and averages for typical behavior

## Summary

You've learned:

- ✅ Track counts with `track_count()` and increment with `track_inc()`
- ✅ Aggregate numbers with `track_sum()`, `track_avg()`, `track_min()`, `track_max()`
- ✅ Build histograms with `track_bucket()`
- ✅ Rank items with `track_top()` and `track_bottom()`
- ✅ Count unique values with `track_unique()`
- ✅ Track streaming percentiles with `track_percentiles()` for P50/P95/P99 analysis
- ✅ Get comprehensive stats with `track_stats()` (min, max, avg, count, sum, percentiles in one call)
- ✅ View metrics with `-m`, `--metrics=full`, and `--metrics=json`
- ✅ Persist metrics with `--metrics-file`
- ✅ Generate custom reports in `--end` stage
- ✅ Use sliding windows for moving averages and rolling calculations

## Next Steps

Now that you can track and aggregate data, continue to:

- **[Begin and End Stages](begin-end-stages.md)** - Use `--begin` and `--end` for advanced workflows
- **[Advanced Scripting](advanced-scripting.md)** - Advanced transformation patterns
- **[Configuration and Reusability](configuration-and-reusability.md)** - Save common patterns as aliases

**Related guides:**

- [Concepts: Scripting Stages](../concepts/scripting-stages.md) - Deep dive into stage execution
- [Function Reference](../reference/functions.md#tracking-functions) - Complete function signatures
- [How-To: Build a Service Health Snapshot](../how-to/monitor-application-health.md) - Real-world examples
