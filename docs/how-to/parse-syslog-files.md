# Investigate Syslog Sources

Parse syslog-formatted logs (RFC 3164 / RFC 5424) to surface security events, infrastructure issues, and service-level anomalies.

## When to Use This Guide
- Auditing authentication activity (`sshd`, `sudo`, VPN) on Linux systems.
- Monitoring routers, firewalls, or load balancers that emit syslog.
- Building quick, scriptable reports without standing up a SIEM.

## Before You Start
- Examples use `examples/simple_syslog.log`. Replace it with `/var/log/syslog`, `/var/log/auth.log`, or device exports.
- Syslog encodes both severity (0–7) and facility (0–23). Keep a cheat sheet nearby for your environment.
- If timestamps appear with unusual formats, consult `kelora --help-time` and be ready to specify `--ts-format`.

## Step 1: Inspect the Stream
Confirm parsing works and note which fields are available.

```bash
kelora -f syslog examples/simple_syslog.log -n 5
```

Common fields:

- `timestamp`, `hostname`, `process`, `pid`, `message`
- `facility` (integer code) and `severity` (0 = emergency, 7 = debug)
- Some devices include structured data in `message`; plan to parse it with Rhai helpers.

## Step 2: Filter by Severity or Facility
Focus on critical events first, then expand scope.

```bash
kelora -f syslog /var/log/syslog \
  --filter 'e.severity <= 3' \
  -k timestamp,hostname,process,message
```

Helpful ranges:

- `<= 2` for emergencies/alerts/critical.
- `== 4` for warnings.
- Facility codes: `0` Kernel, `3` System daemons, `4` Auth/Security, `10` Auth (private), `16+` Local use.

## Step 3: Target Specific Services
Investigate authentication flows or infrastructure components.

```bash
kelora -f syslog /var/log/auth.log \
  --filter 'e.process == "sshd" && e.message.contains("Failed password")' \
  -e 'e.username = e.message.extract_re(r"for ([^ ]+)", 1)' \
  -k timestamp,hostname,username,message
```

- Combine multiple processes: `--filter 'e.process == "sudo" || e.process == "su"'`.
- Use `extract_re()` or `parse_kv()` to decode structured messages (firewalls, network gear).

## Step 4: Add Enrichment and Metrics
Capture per-host or per-IP trends while reviewing raw events.

```bash
kelora -f syslog /var/log/syslog \
  --filter 'e.severity <= 3' \
  -e 'let ip = e.message.extract_ip(); if ip != "" { track_count(ip) }' \
  --metrics
```

- `track_count(e.hostname)` surfaces noisy machines.
- Record severity names for reporting:
  ```bash
  -e 'e.severity_name = ["EMERG","ALERT","CRIT","ERROR","WARN","NOTICE","INFO","DEBUG"][e.severity]'
  ```

## Step 5: Export for Stakeholders
Hand off filtered events to incident responders or auditors.

```bash
kelora -f syslog /var/log/syslog \
  --since "today 00:00" \
  --filter 'e.severity <= 3' \
  -k timestamp,hostname,process,severity,message \
  -F csv > syslog-critical.csv
```

Alternatives:

- `-J` for JSON exports consumed by log analytics tools.
- Use `-q` when running inside scripts that only care about exit codes or metrics.

## Variations
- **RFC 5424 structured data**  
```bash
kelora -f syslog app-5424.log \
  -e 'e.absorb_kv("message")' \
  -k timestamp,hostname,app_id,msgid,message
```
- **Network device monitoring**  
  ```bash
  kelora -f syslog firewall.log \
    --filter 'e.facility == 4' \
    -e 'e.src_ip = e.message.extract_re(r"SRC=([^ ]+)", 1)' \
    -e 'track_count(e.src_ip)' \
    --metrics
  ```

- **Time-boxed reporting**  
  ```bash
  kelora -f syslog /var/log/syslog \
    --since "1 hour ago" \
    -e 'e.hour = to_datetime(e.timestamp).format("%Y-%m-%d %H:00")' \
    -e 'track_count(e.hour)' \
    --metrics
  ```

## Troubleshooting
- **Timestamps not recognised**: specify `--ts-format` matching the source (e.g., `%b %e %H:%M:%S` for classic syslog).
- **Facility/severity seems wrong**: some appliances offset the code; inspect `e.priority` and decode manually with `e.priority / 8` (facility) and `e.priority % 8` (severity).
- **Parsing stops**: enable `--verbose` to view problematic lines; consider `--strict` once the pipeline is stable.

## See Also
- [Analyze Web Traffic](analyze-web-traffic.md) for HTTP access logs that complement syslog-level insights.
- [Triage Production Errors](find-errors-in-logs.md) when application logs, not syslog, contain the signal.
- [Concept: Performance Model](../concepts/performance-model.md) if you need to process multi-GB syslog archives quickly.
