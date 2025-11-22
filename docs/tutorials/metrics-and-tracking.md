# Metrics and Tracking

Turn raw log streams into actionable numbers. This tutorial walks through Kelora's
metrics pipeline, from basic counters to custom summaries that you can export or
feed into dashboards.

## What You'll Learn

- Track counts, sums, buckets, and unique values with Rhai helpers
- Combine `--metrics`, `--stats`, `--begin`, and `--end` for structured reports
- Use sliding windows and percentiles for latency analysis
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
      -F none \
      -e 'track_count(e.service)' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -F none \
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
      -F none \
      -e 'track_count(e.service)' \
      -m --stats
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -F none \
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
      -F none \
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
      -F none \
      -e 'if e.has("duration_ms") { track_sum("total_duration", e.duration_ms); track_count("duration_count"); track_min("min_duration", e.duration_ms); track_max("max_duration", e.duration_ms) }' \
      --metrics
    ```

**Available aggregation functions:**

- `track_sum(key, value)` - Accumulates totals (throughput, volume)
- `track_min(key, value)` - Tracks minimum value seen
- `track_max(key, value)` - Tracks maximum value seen
- `track_count(key)` - Counts occurrences of key
- `track_inc(key, amount)` - Increment counter by amount (not shown above)

**Note:** There's no `track_avg()` function. Calculate averages in `--end` stage:
```rhai
--end 'let avg = metrics.total_duration / metrics.duration_count; print("Average: " + avg)'
```

## Step 2.5 – Histograms with track_bucket()

Build histograms by grouping values into buckets—perfect for latency distributions.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'if e.has("duration_ms") {
              let bucket = (e.duration_ms / 1000) * 1000;
              track_bucket("latency_histogram", bucket)
          }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -F none \
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

## Step 3 – Unique Values and Cardinality

`track_unique()` stores distinct values for a key—handy for unique user counts or
cardinality analysis.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_unique("services", e.service)' \
      -e 'if e.level == "ERROR" { track_unique("error_messages", e.message) }' \
      --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_unique("services", e.service)' \
      -e 'if e.level == "ERROR" { track_unique("error_messages", e.message) }' \
      --metrics
    ```

Use `metrics["services"].len()` later to compute the number of distinct members.

### Viewing Full Metrics

When `track_unique()` collects many items, `-m` shows only the first 5 with a
hint. Use `-mm` for the complete list or `--metrics-json` for JSON format:

=== "Full table with -mm"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_unique("services", e.service)' \
      -mm
    ```

=== "JSON format with --metrics-json"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_unique("services", e.service)' \
      --metrics-json
    ```

The `-mm` flag shows all items in table format, while `--metrics-json` outputs
structured JSON to stderr. Both are mutually exclusive—pick the format that
matches your workflow. You can also combine `-m` with `--metrics-file` to get
both table output and a JSON file.

## Step 4 – Sliding Windows and Percentiles

Enable the window buffer to examine recent events. The example below tracks a
five-event moving average and P95 latency for CPU metrics.

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

The special `window` variable becomes available once you pass `--window`. Use
`window.pluck_as_nums("FIELD")` for numeric arrays and `window.pluck("FIELD")`
for raw values.

## Step 5 – Custom Reports with `--end`

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
      -F none \
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
      -F none \
      -e 'track_count(e.service)' \
      -e 'track_count(e.level)' \
      -m \
      -I metrics_summary.rhai \
      --end 'summarize_metrics()'

    rm metrics_summary.rhai
    ```

The automatically printed `--metrics` block remains, while `--end` gives you a
clean text summary that you can redirect or feed into alerts.

## Step 6 – Persist Metrics to Disk

Use `--metrics-file` to serialize the metrics map as JSON for other tools.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_count(e.service)' \
      -m \
      --metrics-file metrics.json

    cat metrics.json
    rm metrics.json
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      -F none \
      -e 'track_count(e.service)' \
      -m \
      --metrics-file metrics.json

    cat metrics.json
    rm metrics.json
    ```

The JSON structure mirrors the in-memory map, so you can load it with `jq`, a
dashboard agent, or any scripting language.

## Step 7 – Streaming Scoreboards

Kelora keeps metrics up to date even when tailing files or processing archives.
This command watches a gzipped access log and surfaces top status classes.

=== "Command"

    ```bash
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let klass = ((e.status / 100) * 100).to_string(); track_count(klass)' \
      -m -F none \
      -n 0
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let klass = ((e.status / 100) * 100).to_string(); track_count(klass)' \
      -m -F none \
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
      -F none -n 0
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
      -F none -n 0
    ```

`track_bucket(key, bucket_value)` keeps nested counters so you can emit a
human-readable histogram once processing finishes.

## Troubleshooting

- **No metrics printed**: Ensure you pass `-m` (or `--metrics`) or consume `metrics`
  within an `--end` script. Tracking functions alone do not emit output.

- **Truncated arrays**: If `-m` shows only the first 5 items with a hint, use `-mm`
  for full table output, `--metrics-json` for JSON format, or `--metrics-file` to
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
| `track_min(key, value)` | Minimum value | `track_min("fastest", e.duration)` |
| `track_max(key, value)` | Maximum value | `track_max("slowest", e.duration)` |
| `track_bucket(key, bucket)` | Histogram buckets | `track_bucket("status", (e.status/100)*100)` |
| `track_unique(key, value)` | Unique values | `track_unique("users", e.user_id)` |

**Note:** Calculate averages using `sum / count` in the `--end` stage.

## Summary

You've learned:

- ✅ Track counts with `track_count()` and increment with `track_inc()`
- ✅ Aggregate numbers with `track_sum()`, `track_min()`, `track_max()`
- ✅ Calculate averages in `--end` stage from sum and count
- ✅ Build histograms with `track_bucket()`
- ✅ Count unique values with `track_unique()`
- ✅ View metrics with `-m`, `-mm`, and `--metrics-json`
- ✅ Persist metrics with `--metrics-file`
- ✅ Generate custom reports in `--end` stage
- ✅ Use sliding windows for percentile analysis

## Next Steps

Now that you can track and aggregate data, continue to:

- **[Begin and End Stages](begin-end-stages.md)** - Use `--begin` and `--end` for advanced workflows
- **[Advanced Scripting](advanced-scripting.md)** - Advanced transformation patterns
- **[Configuration and Reusability](configuration-and-reusability.md)** - Save common patterns as aliases

**Related guides:**

- [Concepts: Scripting Stages](../concepts/scripting-stages.md) - Deep dive into stage execution
- [Function Reference](../reference/functions.md#tracking-functions) - Complete function signatures
- [How-To: Build a Service Health Snapshot](../how-to/monitor-application-health.md) - Real-world examples
