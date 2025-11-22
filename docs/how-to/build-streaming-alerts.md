# Design Streaming Alerts

Create lightweight monitors that watch live log streams, trigger when conditions are met, and hand off alerts to your paging or messaging tools.

## When This Helps
- You need a fast safety net during an incident or rollout.
- Existing monitoring is delayed, but logs stream in real time.
- Teams want deterministic checks (exit codes, console output) embedded in automation.

## Before You Start
- Ensure Kelora is installed or run `cargo run --quiet --release --` from the repository root.
- Pick a log source that updates continuously (`tail -f`, `journalctl -f`, container logs). Examples below use `examples/simple_json.jsonl`.
- Keep alert logic simple: Kelora excels at boolean conditions, counters, and short windows. For complex routing, hand the output to an external service.

## Step 1: Choose an Ingestion Pattern
Pipe a continuous log stream into Kelora. For multi-file input, append `--parallel` once you validate the basic pipeline.

```bash
tail -f /var/log/app.log | kelora -j \
  --since "5 minutes ago"
```

Notes:

- `tail -F` survives log rotation.
- Use `--from-stdin` only when you want to suppress format detection banner (Kelora infers format when possible, but explicit `-j`/`-f` is safer).

## Step 2: Detect the Condition
Combine level filters with Rhai expressions to describe the alert.

```bash
tail -f /var/log/app.log | kelora -j \
  -l error,critical \
  --filter 'e.service == "payments" || e.message.contains("timeout")' \
  -e 'eprint(`${e.level}: ${e.service} ${e.message}`)'
```

- Use `-l warn,error,critical` when log levels are reliable and you want broad coverage.
- Reuse helpers like `e.message.contains()` or `e.get_path()` for structured payloads.
- Keep `eprint()` output short; it routes to stderr and works well with `-q`/`--silent` (see next step).

## Step 3: Control Noise
Quiet modes and counters help you avoid pager fatigue.

!!! note
    Need alerts to come from `print`/`eprint`? Use `-q --no-diagnostics` to suppress events and diagnostics while keeping script output. `--silent` suppresses script output too.

```bash
tail -f /var/log/app.log | kelora -j -q \
  -e 'track_count("total")' \
  -e 'track_count("level|" + e.level)' \
  -m \
  --end '
    let total = metrics.get_path("total", 0);
    let errors = metrics.get_path("level|ERROR", 0);
    if total > 0 && errors * 100 / total > 5 {
      eprint(`ALERT: error rate ${errors}/${total}`);
      exit(1);
    }
  '
```

-- `-q` suppresses events so only alerts are printed; add `--silent` to suppress diagnostics too.

- Combine `--window N` with `window.pluck()` to examine rolling slices when bursts matter more than totals.
- Call `exit(1)` to propagate failure into CI or cron jobs; Kelora exits with 0 otherwise.

## Step 4: Decide How to Emit Alerts
Direct alerts to stderr for on-call use, files for dashboards, or downstream commands.

```bash
tail -f /var/log/app.log | kelora -j --allow-fs-writes -q \
  -l critical \
  -e 'append_file("/tmp/critical.log", `${e.timestamp} ${e.service} ${e.message}\n`)'
```

- Use `append_file()` sparingly; it requires `--allow-fs-writes` and should point to writable locations.
- To integrate with webhooks, pipe Kelora output into `while read line; do curl … "$line"; done`.
- Respect `--no-emoji` when the receiver cannot handle Unicode.

## Step 5: Wrap the Monitor
Embed the pipeline in a script so that your automation platform can restart it or notify the right channel.

```bash
#!/usr/bin/env bash
set -euo pipefail

if ! tail -f /var/log/app.log | kelora -j --silent -l critical; then
  printf 'Critical log seen at %s\n' "$(date -Is)" | mail -s "PROD critical" ops@example.com
fi
```

- `--silent` suppresses terminal output; only exit status (and metrics files) remains.
- Add `systemd` or supervisor configuration around this script to handle restarts.

## Variations
- **Service-specific watcher**  
  ```bash
  tail -f /var/log/app.log | kelora -j -q \
    --filter 'e.service == "search"' \
    -l error \
    -e 'eprint("search error: " + e.message)'
  ```

- **Spike detection**  
  ```bash
  tail -f /var/log/app.log | kelora -j --window 50 -q \
    -l error \
    -e 'let recent = window_events();' \
    -e 'if recent.len() >= 10 { eprint("ALERT: error spike (" + recent.len().to_string() + " / 50)") }'
  ```

- **Access log latency guard**  
  ```bash
  tail -f /var/log/nginx/access.log | kelora -f combined -q \
    --filter 'e.get_path("request_time", "0").to_float() > 1.5' \
    -e 'eprint(`SLOW: ${e.method} ${e.path} ${e.request_time}s`)'
  ```

## Validate Before Paging People
- Reproduce the alert with sample data (`cat fixtures.log | kelora …`) to make sure the condition triggers as expected.
- Monitor `--stats` output occasionally; parse failures or zero processed events usually indicate path or format mistakes.
- Document the exit code contract (0 = no alert, non-zero = alert) so downstream tooling handles it correctly.

## See Also
- [Triage Production Errors](find-errors-in-logs.md) for deeper investigation once the alert fires.
- [Build a Service Health Snapshot](monitor-application-health.md) to keep longer-term metrics.
- [Process Archives at Scale](batch-process-archives.md) when you want to backfill alert logic on historical data.
