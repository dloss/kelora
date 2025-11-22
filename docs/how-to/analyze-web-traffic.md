# Analyze Web Traffic

Understand how HTTP traffic behaves, catch spikes in errors, and share data-backed summaries with stakeholders.

## When to Use This Guide
- Operating Nginx or Apache services that emit combined-format access logs.
- Investigating customer complaints about latency or failed requests.
- Building quick daily reports without moving data into another system.

## Before You Start
- Ensure the access logs include the fields you need. The bundled sample `examples/simple_combined.log` follows the Apache/Nginx combined format.
- `request_time` is only present when you add it to your Nginx log format. If it is missing, prefer backend application metrics or use upstream timing fields instead.
- Run commands from the repo root or update paths to point at your own logs (Kelora handles `.gz` files automatically).

## Step 1: Inspect a Sample
Confirm the log format and field names before you start filtering.

```bash
kelora -f combined examples/simple_combined.log -n 5
```

Key fields available in the combined format:

- `ip`, `timestamp`, `method`, `path`, `status`, `bytes`
- Optional `request_time` (Nginx custom field), `referer`, `user_agent`
- Use `--keys` or `-k` to display additional headers if you extended the format.

## Step 2: Highlight Errors and Hotspots
Filter for failing requests and capture the context you need for triage.

```bash
kelora -f combined examples/simple_combined.log \
  --filter 'e.status >= 500' \
  -k timestamp,ip,status,request
```

Tips:

- For client errors, use `'e.status >= 400 && e.status < 500'`.
- When the application encodes errors in the URI, add a second filter such as `e.path.contains("/api/")`.
- Use `--before-context` and `--after-context` if you need to see neighbouring requests from the same source.

## Step 3: Investigate Slow Endpoints
Track latency outliers to confirm performance complaints or detect resource exhaustion.

```bash
kelora -f combined examples/simple_combined.log \
  --filter 'e.get_path("request_time", "0").to_float() > 1.0' \
  -k timestamp,method,path,request_time,status
```

If `request_time` is not logged:

- Switch to backend service logs via [Build a Service Health Snapshot](monitor-application-health.md).
- Consider adding upstream timing variables (`$upstream_response_time`, `$request_time`) to your Nginx format so Kelora can read them directly.

## Step 4: Summarise by Status and Source
Generate quick aggregates to prioritise remediation work or include in change reviews.

```bash
kelora -f combined examples/simple_combined.log \
  -e 'track_count("status_" + e.status)' \
  -e 'track_count("method_" + e.method)' \
  -e 'track_count(e.ip)' \
  --metrics
```

- `track_count(e.ip)` highlights noisy consumers or suspicious sources.
- Use `track_bucket()` with `request_time` to build latency histograms.
- Run with `--stats` for throughput metrics and parse error counts.

## Step 5: Export a Shareable Slice
Deliver the findings to teammates or import them into downstream tools.

```bash
kelora -f combined examples/simple_combined.log \
  --filter 'e.status >= 500' \
  -k timestamp,ip,status,request,user_agent \
  -F csv > web-errors.csv
```

Alternatives:

- `-J` to produce JSON for ingestion into a SIEM.
- Add `--no-diagnostics` to suppress diagnostics if the output is piped into another script.

## Variations
- **Focus on a specific endpoint**  
  ```bash
  kelora -f combined /var/log/nginx/access.log \
    --filter 'e.path.starts_with("/api/orders")' \
    -e 'track_count(e.status)' \
    --metrics
  ```

- **Compare time windows**  
  ```bash
  kelora -f combined /var/log/nginx/access.log \
    --since "yesterday 00:00" --until "yesterday 12:00" \
    -e 'track_count("morning_" + e.status)' \
    --metrics
  ```

- **Detect suspicious behaviour**  
  ```bash
  kelora -f combined /var/log/nginx/access.log \
    --filter 'e.method == "POST" && !e.path.starts_with("/api/")' \
    -k timestamp,ip,method,path
  ```

- **Process rotated archives**  
  ```bash
  kelora -f combined /var/log/nginx/access.log*.gz \
    --parallel --unordered \
    -e 'track_count(e.status)' \
    --metrics
  ```

## Validate and Communicate
- Use `--strict` if you added new log fields and want Kelora to stop on parsing mistakes.
- Attach `--stats` output to change reports so readers can see event counts and error rates.
- Note whether `request_time` or other custom fields were available; this influences how teams interpret latency results.

## See Also
- [Process Archives at Scale](batch-process-archives.md) for large historical datasets.
- [Design Streaming Alerts](build-streaming-alerts.md) to notify on live 5xx spikes.
- [Investigate Syslog Sources](parse-syslog-files.md) for load balancer or reverse-proxy system logs.
