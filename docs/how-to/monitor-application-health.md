# Build a Service Health Snapshot

Create a concise operational report from JSON service logs, tracking errors, latency, and resource pressure without leaving the terminal.

## When to Reach for This
- You need a quick health summary after a deployment or incident.
- Product teams request a daily/weekly status report built from application logs.
- You are validating that a suspected fix reduced error rates or latency.

## Before You Start
- Example commands use `examples/simple_json.jsonl`. Replace it with your application logs.
- Ensure key fields exist in the payload (e.g., `service`, `level`, `duration_ms`, `memory_percent`, `status`).
- Rhai metric helpers (`track_count`, `track_sum`, `track_max`, etc.) power the summary; see [Tutorial: Metrics and Tracking](../tutorials/metrics-and-tracking.md) if you need a refresher.

## Step 1: Build a Baseline Scoreboard
Count events by service and severity to frame the rest of the investigation.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'track_count(e.service)' \
  -e 'track_count("level_" + e.level)' \
  --metrics \
  --stats
```

- `--metrics` prints the tracked values once input completes.
- `--stats` reports parse errors and throughput, giving you confidence in the numbers.

## Step 2: Add Performance Signals
Track latency and resource metrics with averages and extremes per service.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'let latency = e.get_path("duration_ms");
      track_sum("latency_total_ms|" + e.service, latency);
      track_max("latency_p99|" + e.service, latency);
      if latency != () {
        track_count("latency_samples|" + e.service);
      }' \
  -e 'track_max("memory_peak|" + e.service, e.get_path("memory_percent"))' \
  --metrics
```

Guidance:

- `e.get_path()` returns unit `()` when a field is missing; check for `()` to avoid polluting metrics.
- Combine totals and sample counts to compute averages (e.g., divide `latency_total_ms|SERVICE` by `latency_samples|SERVICE`) in an `--end` block or downstream tooling.
- Track additional business KPIs (orders, sign-ups) with `track_sum()` or `track_count()` as needed.

## Step 3: Flag Error Hotspots
Separate healthy traffic from incidents and identify recurring failure modes.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  -e 'let code = e.get_path("error.code", "unknown");' \
  -e 'track_count("errors|" + e.service)' \
  -e 'track_count("error_code|" + code)' \
  -k timestamp,service,message \
  --metrics
```

- Running the same pipeline without `-k` is useful when you only need counts.
- Combine multiple passes (baseline + errors) to build an overall picture or wrap the logic in a tiny shell script.

## Step 4: Generate a Shareable Summary
Create a compact report for status updates or documentation.

```bash
kelora -j examples/simple_json.jsonl \
  -e 'track_count(e.service)' \
  -e 'let latency = e.get_path("duration_ms");
      track_sum("latency_total_ms|" + e.service, latency);
      track_max("latency_p99|" + e.service, latency);
      if latency != () {
        track_count("latency_samples|" + e.service);
      }' \
  -e 'track_max("memory_peak|" + e.service, e.get_path("memory_percent"))' \
  -m \
  --end '
    print("=== Service Snapshot ===");
    let totals = #{};
    let samples = #{};
    let p99 = #{};
    let memory = #{};

    for key in metrics.keys() {
      let name = key.to_string();
      let value = metrics[name];

      if name.contains("|") {
        let parts = name.split("|");
        if parts.len() == 2 {
          let kind = parts[0];
          let service = parts[1];
          if kind == "latency_total_ms" {
            totals[service] = value;
          } else if kind == "latency_samples" {
            samples[service] = value;
          } else if kind == "latency_p99" {
            p99[service] = value;
          } else if kind == "memory_peak" {
            memory[service] = value;
          }
        }
      } else {
        print(name + ": " + value.to_string());
      }
    }

    for service in totals.keys() {
      let total = totals[service];
      let sample_count = if samples.contains(service) { samples[service] } else { 0 };
      if sample_count != 0 {
        let avg = total / sample_count;
        print("latency_avg_" + service + ": " + avg.to_string());
      }
      if p99.contains(service) {
        print("latency_p99_" + service + ": " + p99[service].to_string());
      }
      if memory.contains(service) {
        print("memory_peak_" + service + ": " + memory[service].to_string());
      }
    }
  '
```

- `-m` (or `--metrics`) keeps the metrics map available in the `--end` block.
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
    -e 'track_count(e.level)' \
    -e 'let latency = e.get_path("duration_ms");
        track_sum("latency_total_ms|" + e.service, latency);
        if latency != () {
          track_count("latency_samples|" + e.service);
        }' \
    --metrics
  ```

- **Time-boxed reports**  
  ```bash
  kelora -j app.log \
    --since "2 hours ago" \
    -e 'e.window = e.timestamp.format("%Y-%m-%d %H:00")' \
    -e 'track_count(e.window)' \
    --metrics
  ```

- **Live monitoring**  
  ```bash
  tail -f /var/log/app.log | kelora -j -q \
    -l error \
    -e 'track_count(e.service)' \
    -e 'eprint("ALERT: error in " + e.service)'
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
