# Build a Service Health Snapshot

Create a concise operational report from JSON service logs, tracking errors, latency, and resource pressure without leaving the terminal.

## When to Reach for This
- You need a quick health summary after a deployment or incident.
- Product teams request a daily/weekly status report built from application logs.
- You are validating that a suspected fix reduced error rates or latency.

## Before You Start
- Example commands use `examples/simple_json.jsonl`. Replace it with your application logs.
- Ensure key fields exist in the payload (e.g., `service`, `level`, `duration_ms`, `memory_percent`, `status`).
- Rhai metric helpers (`track_freq`, `track_avg`, `track_percentiles`, `track_max`, etc.) power the summary; see [Tutorial: Metrics and Tracking](../tutorials/metrics-and-tracking.md) if you need a refresher.

## Step 1: Build a Baseline Scoreboard
Count events by service and severity to frame the rest of the investigation.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'track_freq("service", e.service); track_freq("level", e.level)' \
  --metrics \
  --stats
```

- `--metrics` prints the tracked values once input completes.
- `--stats` reports parse errors and throughput, giving you confidence in the numbers.

## Step 2: Add Performance Signals
Track latency and resource metrics with averages, percentiles, and extremes per service.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'let latency = e.get_path("duration_ms");
      if latency != () {
        track_avg("latency_avg|" + e.service, latency);
        track_percentiles("latency|" + e.service, latency, [0.95, 0.99]);
        track_max("latency_max|" + e.service, latency);
      }
      track_max("memory_peak|" + e.service, e.get_path("memory_percent"))' \
  --metrics
```

Guidance:

- `e.get_path()` returns unit `()` when a field is missing; check for `()` to avoid polluting metrics.
- `track_avg()` automatically computes averages without manual sum/count tracking.
- `track_percentiles()` creates auto-suffixed metrics like `latency|auth_p95` and `latency|auth_p99` for true percentile tracking.
- Track additional business KPIs (orders, sign-ups) with `track_sum()` or `track_freq()` as needed.

## Step 3: Flag Error Hotspots
Separate healthy traffic from incidents and identify recurring failure modes.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  -e 'let code = e.get_path("error.code", "unknown");
      track_freq("errors", e.service);
      track_freq("error_code", code)' \
  -k timestamp,service,message \
  --metrics
```

- Running the same pipeline without `-k` is useful when you only need counts.
- Combine multiple passes (baseline + errors) to build an overall picture or wrap the logic in a tiny shell script.

## Step 4: Generate a Shareable Summary
Create a compact report for status updates or documentation.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'track_freq("service", e.service);
      let latency = e.get_path("duration_ms");
      if latency != () {
        track_avg("latency_avg|" + e.service, latency);
        track_percentiles("latency|" + e.service, latency, [0.99]);
      }
      track_max("memory_peak|" + e.service, e.get_path("memory_percent"))' \
  -m \
  --end '
    print("=== Service Snapshot ===");
    let counts = #{};
    let avg_latency = #{};
    let p99_latency = #{};
    let memory = #{};

    for key in metrics.keys() {
      let name = key.to_string();
      let value = metrics[name];

      if name.contains("|") {
        let parts = name.split("|");
        if parts.len() == 2 {
          let kind = parts[0];
          let service = parts[1];
          if kind == "latency_avg" {
            avg_latency[service] = value;
          } else if name.ends_with("_p99") {
            // Extract service name before _p99 suffix
            let base = name.split("_p99")[0];
            let svc = base.split("|")[1];
            p99_latency[svc] = value;
          } else if kind == "memory_peak" {
            memory[service] = value;
          }
        }
      } else {
        counts[name] = value;
      }
    }

    for service in counts.keys() {
      print(service + ": " + counts[service].to_string());
      if avg_latency.contains(service) {
        print("  latency_avg: " + avg_latency[service].to_string());
      }
      if p99_latency.contains(service) {
        print("  latency_p99: " + p99_latency[service].to_string());
      }
      if memory.contains(service) {
        print("  memory_peak: " + memory[service].to_string());
      }
    }
  '
```

- `-m` (or `--metrics`) keeps the metrics map available in the `--end` block.
- The `--end` script parses auto-suffixed percentile metrics (e.g., `latency|auth_p99`) to extract service names.
- Redirect the output to a file (`> reports/service-health.txt`) or pipe it into notification tooling.

## Step 5: Export Structured Data
Share filtered events with teams that need to deep dive.

```bash
kelora -j examples/simple_json.jsonl \
  -l error \
  -e 'e.error_code = e.get_path("error.code", "unknown")' \
  -k timestamp,service,error_code,message \
  -F csv > service-errors.csv
```

- Use `-J` or `-F json` for downstream analytics pipelines.
- When privacy matters, combine with [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md).

## Variations
- **Per-service drilldown**  
  ```bash
  kelora -j app.log \
    --filter 'e.service == "payments"' \
    -e 'track_freq("level", e.level);
        let latency = e.get_path("duration_ms");
        track_sum("latency_total_ms|" + e.service, latency);
        if latency != () {
          track_freq("latency_samples", e.service);
        }' \
    --metrics
  ```

- **Time-boxed reports**  
  ```bash
  kelora -j app.log \
    --since "2 hours ago" \
    -e 'e.window = e.timestamp.format("%Y-%m-%d %H:00");
        track_freq("window", e.window)' \
    --metrics
  ```

- **Live monitoring**  
  ```bash
  tail -f /var/log/app.log | kelora -j -q \
    -l error \
    -e 'track_freq("service", e.service); eprint("ALERT: error in " + e.service)'
  ```
  Add `--no-emoji` when piping into systems that cannot render emoji.

## Validate and Iterate
- Compare counts against existing dashboards to ensure your pipeline includes every service instance.
- Make `--stats` part of the report so readers see parse failures or skipped events.
- If metrics explode after a deploy, combine this guide with [Triage Production Errors](find-errors-in-logs.md) to capture concrete samples.

## See Also
- [Tutorial: Metrics and Tracking](../tutorials/metrics-and-tracking.md) for a deeper tour of Rhai telemetry helpers.
- [Design Streaming Alerts](build-streaming-alerts.md) to turn the snapshot into a continuous monitor.
- [Process Archives at Scale](batch-process-archives.md) when historical context is required.
