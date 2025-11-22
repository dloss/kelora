# Roll Up Logs with Span Windows

Batch events into fixed-size or time-based windows using `--span` so you can compute summaries without losing streaming behavior.

## When to Use Spans
- Produce per-minute or per-N-event digests during incident reviews.
- Generate lightweight dashboards straight from log streams.
- Detect bursts or trends without shipping data into another analytics system.

## Before You Start
- Examples use `examples/simple_json.jsonl` and assume a parsed timestamp (`timestamp` field). Adapt field names as needed.
- `--span` disables parallel mode; span processing requires ordered input.
- Late or timestamp-free events require special handling (see Step 4).

## Step 1: Select a Window Strategy
- **Count-based**: `--span 500` closes the window every 500 events that pass all filters.
- **Time-based**: `--span 5m` groups events by wall-clock duration (supports `s`, `m`, `h`).
- Optional `--span-close '...Rhai script...'` runs when the window closes. Use it for custom summaries or output.

## Step 2: Count-Based Example
Summarise every 200 events for quick batch metrics.

```bash
kelora -j examples/simple_json.jsonl \
  --span 200 \
  --span-close '
    let metrics = span.metrics;
    print(`${span.id},events=${span.size},errors=${metrics.get_path("level|ERROR", 0)}`);
  ' \
  --exec '
    track_count("total");
    track_count("level|" + e.level);
  '
```

- `span.id` is an incrementing identifier (`span-000001`, ...).
- `span.metrics` exposes only the deltas collected within that window.
- Because the original events are still streamed, you can attach more `--exec` or `--filter` stages before or after the span logic.

## Step 3: Time-Based Example
Roll up five-minute error summaries aligned to timestamps.

```bash
kelora -j examples/simple_json.jsonl \
  --span 5m \
  --exec '
    track_count("total");
    track_count("level|" + e.level);
  ' \
  --span-close '
    let total = span.metrics.get_path("total", 0);
    let errors = span.metrics.get_path("level|ERROR", 0);
    if total > 0 {
      let rate = (errors.to_float() / total.to_float()) * 100.0;
      print(`${span.start},total=${total},errors=${errors},error_rate=${rate}`);
    }
  '
```

- `span.start` and `span.end` use `DateTime` values from event timestamps.
- Windows are tumbling; events fall into the window that matches their timestamp.
- Ensure input is chronologically sorted to avoid unnecessary late events.

## Step 4: Handle Late or Missing Timestamps
Inspect span metadata to see how events were classified.

```bash
kelora -j examples/simple_json.jsonl \
  --span 1m \
  --exec '
    if meta.span_status == "late" {
      eprint(`Late event: ${e.timestamp}`);
    } else if meta.span_status == "unassigned" {
      eprint(`Missing timestamp on line ${meta.line_num}`);
    }
  ' \
  --span-close 'print(span.id + ": " + span.size.to_string())'
```

- `meta.span_status` is one of `active`, `late`, or `unassigned`.
- Use `--strict` if missing timestamps should abort the run instead of being skipped.

## Step 5: Export or Chain Spans
Write per-span summaries to files or feed them into downstream commands.

```bash
kelora -j examples/simple_json.jsonl \
  --span 10m \
  --exec 'track_count(e.service)' \
  --span-close '
    let path = "/tmp/span-" + span.id + ".csv";
    for (service, count) in span.metrics {
      append_file(path, `${span.start},${service},${count}\n`);
    }
  ' \
  --allow-fs-writes
```

- Remember to enable `--allow-fs-writes` when using `append_file()`.
- Alternatively, pipe Kelora output into `jq`, DuckDB, or spreadsheets for visualisation.

## Variations
- **Per-service spans**  
  ```bash
  kelora -j app.log \
    --filter 'e.service == "payments"' \
    --span 100 \
    --span-close 'print(span.id + ",payments=" + span.size.to_string())'
  ```

- **Sliding error thresholds**  
  Combine `--span` with [Design Streaming Alerts](build-streaming-alerts.md) to trigger notifications when span metrics cross a threshold.

- **Hybrid reporting**  
  Run two spans simultaneously by invoking Kelora twice: one count-based for rapid feedback, another time-based for dashboards.

## Best Practices
- Sort archives by timestamp before running time-based spans to minimise late events.
- Keep span window sizes reasonable; extremely large spans consume more memory.
- Capture `--stats` output alongside span summaries for auditing.

## See Also
- [Process Archives at Scale](batch-process-archives.md) to prepare large datasets before spanning.
- [Build a Service Health Snapshot](monitor-application-health.md) for metric examples you can embed in span logic.
- `kelora --help-time` and `kelora --help-functions` for timestamp parsing and metric helper references.
