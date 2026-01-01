# Incident Response Playbooks

Real-world scenarios for using Kelora during production incidents. Each playbook provides copy-paste commands for quick triage and deep analysis.

## Quick Reference

| Incident Type | First Command | Jump To |
|--------------|---------------|---------|
| API latency spike | `kelora app.jsonl --filter 'e.response_time_ms > 500' --exec 'track_stats("latency", e.response_time_ms); track_top("endpoint", e.endpoint, 10)' -m` | [Playbook 1](#1-api-latency-spike) |
| Error rate spike | `kelora app.jsonl --filter 'e.level == "ERROR"' --drain -k message` | [Playbook 2](#2-error-rate-spike) |
| Auth failures | `kelora auth.log --filter 'e.line.contains("failed")' --exec 'track_top("ip", e.client_ip, 20); track_count(e.username)' -m` | [Playbook 3](#3-authentication-failures) |
| Database slow queries | `kelora db.jsonl --filter 'e.query_time_ms > 1000' --exec 'track_stats("query_time", e.query_time_ms); track_top("query_type", e.query, 10)' -m` | [Playbook 4](#4-database-performance-degradation) |
| Resource exhaustion | `kelora app.log --filter 'e.line.contains("pool") || e.line.contains("exhausted")' -e 'e.absorb_kv("line")' -J` | [Playbook 5](#5-resource-exhaustion) |
| Deployment correlation | `kelora app.jsonl --since "2025-01-20T14:00:00Z" --until "2025-01-20T15:00:00Z" -l error,warn --stats` | [Playbook 6](#6-deployment-correlation) |
| Rate limit abuse | `kelora api.jsonl --filter 'e.status == 429' --exec 'track_top("user", e.user_id, 50); track_bucket("hour", e.timestamp.substring(0,13))' -m` | [Playbook 7](#7-rate-limit-investigation) |
| Trace request across services | `kelora *.jsonl --filter 'e.request_id == "abc123"' --normalize-ts -k timestamp,service,message,status` | [Playbook 8](#8-distributed-trace-analysis) |

---

## 1. API Latency Spike

**Scenario**: Monitoring alerts show P95 latency jumped from 100ms to 2000ms. You need to find which endpoints are slow and when it started.

### Reconnaissance (10 seconds)

Get the lay of the land first:

```bash
kelora api.jsonl --stats
```

**What to look for:**
- Time span - Does log window cover the incident?
- Event count - Do you have enough data?
- Keys seen - What fields are available for analysis?

### Quick Triage (30 seconds)

Get immediate stats on response times:

```bash
kelora api.jsonl --exec 'track_stats("response", e.response_time_ms)' --metrics
```

**What to look for:**
- `response_p95`, `response_p99` - Are they above SLA?
- `response_max` - Is there a ceiling (timeouts)?
- `response_count` - How many requests in this window?

### Find Problem Endpoints (1 minute)

```bash
kelora api.jsonl \
  --filter 'e.response_time_ms > 500' \
  --exec 'track_stats("slow", e.response_time_ms); track_top("endpoint", e.endpoint, 10)' \
  --metrics
```

**What to look for:**
- `endpoint` top list shows which routes are slowest
- If one endpoint dominates, it's a specific service issue
- If many endpoints affected, check infrastructure

### Time-Based Analysis (2 minutes)

When did the latency spike start?

```bash
kelora api.jsonl \
  --filter 'e.response_time_ms > 500' \
  --exec 'e.bucket = to_datetime(e.timestamp).round_to("5m").to_iso()' \
  --exec 'track_bucket("time", e.bucket)' \
  --metrics
```

**What to look for:**
- Buckets show time distribution of slow requests
- Sharp increase = deployment or configuration change
- Gradual increase = resource exhaustion or load

### Visualize Latency Distribution (1 minute)

Use tailmap to see percentile-based visualization over time:

```bash
kelora api.jsonl -F tailmap --keys response_time_ms
```

**Reading tailmap output:**
- `_` = below p90 (fast)
- `1` = p90-p95 (slow)
- `2` = p95-p99 (slower)
- `3` = above p99 (slowest)

Look for vertical bars of `2` and `3` to pinpoint when latency spiked.

### Deep Dive: Correlate with Status Codes

Are slow requests failing?

```bash
kelora api.jsonl \
  --filter 'e.response_time_ms > 500' \
  --exec 'track_top("status", e.status.to_string(), 10); track_top("endpoint_status", e.endpoint + " [" + e.status + "]", 15)' \
  --metrics
```

**Example using sample data:**

```bash
kelora examples/api_latency_incident.jsonl \
  --filter 'e.response_time_ms > 100' \
  --exec 'track_stats("slow", e.response_time_ms); track_top("endpoint", e.endpoint, 5); track_bucket("5min", e.timestamp.substring(0, 15))' \
  --metrics
```

---

## 2. Error Rate Spike

**Scenario**: Error monitoring shows 500 errors/minute (normally 5/minute). Find what's breaking.

### Quick Triage: Error Templates (30 seconds)

Use drain to find common error patterns:

```bash
kelora app.jsonl --filter 'e.level == "ERROR"' --drain -k message
```

**What to look for:**
- Template #1 with high count = root cause candidate
- Many different templates = cascading failures
- New templates = recently introduced bug

### Find Error Hotspots (1 minute)

```bash
kelora app.jsonl \
  --filter 'e.level == "ERROR"' \
  --exec 'track_count(e.error_type); track_top("service", e.service, 10); track_bucket("minute", e.timestamp.substring(0, 16))' \
  --metrics
```

### Extract Stack Traces (2 minutes)

Get actual error details for top issues:

```bash
kelora app.jsonl \
  --filter 'e.level == "ERROR" && e.get_path("stack_trace") != ()' \
  -k timestamp,service,message,stack_trace \
  -n 10
```

### Correlate with Users/Endpoints (3 minutes)

Are errors isolated to specific users or widespread?

```bash
kelora app.jsonl \
  --filter 'e.level == "ERROR"' \
  --exec 'track_unique("users", e.user_id); track_top("endpoint", e.endpoint, 10); track_top("error_msg", e.message, 5)' \
  --metrics
```

**What to look for:**
- `users` count - 1 user = isolated issue, 1000s = systemic
- Top endpoints - Which API routes are failing?
- Error messages - Same error repeated = single root cause

**Example using sample data:**

```bash
kelora examples/api_errors.jsonl \
  --filter 'e.level == "ERROR"' \
  --drain -k error
```

---

## 3. Authentication Failures

**Scenario**: Auth service logs show spike in failed login attempts. Is this a brute force attack or service issue?

### Quick Triage: Failed Login Patterns (30 seconds)

```bash
kelora auth.log \
  --filter 'e.line.contains("failed") || e.line.contains("invalid")' \
  --exec 'e.absorb_kv("line")' \
  --exec 'track_top("ip", e.get_path("ip", "unknown"), 20); track_count(e.get_path("user", "unknown"))' \
  --metrics=short
```

**Note:** Using `--metrics=short` for compact output during time-sensitive incidents.

**What to look for:**
- Top IPs with hundreds of attempts = brute force
- Single IP with specific user = targeted attack
- Many IPs, many users = service outage

### Detect Brute Force (1 minute)

Find IPs with suspicious attempt counts:

```bash
kelora auth.log \
  --filter 'e.line.contains("failed")' \
  --exec 'e.absorb_kv("line")' \
  --exec 'track_top("ip_user", e.get_path("ip", "") + " -> " + e.get_path("user", ""), 30)' \
  --metrics
```

### Time-Based Attack Analysis (2 minutes)

When did the attack start, and is it ongoing?

```bash
kelora auth.log \
  --filter 'e.line.contains("failed")' \
  --exec 'track_bucket("hour", e.timestamp.substring(0, 13)); track_count("total_failures")' \
  --metrics
```

### Extract Attacker IPs for Blocking (1 minute)

Get unique IPs with >10 failed attempts:

```bash
kelora auth.log \
  --filter 'e.line.contains("failed")' \
  --exec 'e.absorb_kv("line")' \
  --exec 'track_count(e.get_path("ip", ""))' \
  --metrics | grep -E '^\s+[0-9]+\.[0-9]+' | awk '{if ($2 > 10) print $1}'
```

### Identify Attack Sources (2 minutes)

Classify IPs as internal vs external threats:

```bash
kelora auth.log \
  --filter 'e.line.contains("failed")' \
  --exec 'e.absorb_kv("line")' \
  --exec '
    let ip = e.get_path("ip", "");
    e.ip_type = if ip.is_private_ip() { "internal" } else { "external" };
    track_top("attacks", ip + " [" + e.ip_type + "]", 30)
  ' \
  --metrics
```

**What to look for:**
- External IPs = Internet-based attack, add to firewall
- Internal IPs = Compromised account or insider threat
- Mix of both = Potentially botnet or distributed attack

**Example using sample data:**

```bash
kelora examples/auth_burst.jsonl \
  --filter 'e.status == 401' \
  --exec 'track_top("ip", e.ip, 20); track_count(e.user)' \
  --metrics
```

---

## 4. Database Performance Degradation

**Scenario**: Database dashboard shows query latency increased 10x. Find slow queries.

### Quick Triage: Query Stats (30 seconds)

```bash
kelora db.jsonl \
  --exec 'track_stats("query_time", e.query_time_ms)' \
  --metrics
```

### Find Slowest Queries (1 minute)

```bash
kelora db.jsonl \
  --filter 'e.query_time_ms > 1000' \
  --exec 'track_top("query", e.query, 10); track_stats("slow", e.query_time_ms)' \
  --metrics
```

**What to look for:**
- Specific query repeated = missing index or N+1 query
- Many different queries = database overload

### Detect N+1 Query Problems (2 minutes)

Find queries executed hundreds of times:

```bash
kelora db.jsonl \
  --exec 'track_count(e.query)' \
  --metrics | sort -k2 -n -r | head -20
```

### Correlate with Application Endpoints (3 minutes)

Which API endpoints trigger slow queries?

```bash
kelora db.jsonl \
  --filter 'e.query_time_ms > 1000' \
  --exec 'track_top("endpoint_query", e.endpoint + " :: " + e.query.substring(0, 50), 15)' \
  --metrics
```

**Example using sample data:**

```bash
kelora examples/database_queries.jsonl \
  --filter 'e.query_time_ms > 100' \
  --exec 'track_stats("slow", e.query_time_ms); track_top("query_type", e.query_type, 10)' \
  --metrics
```

---

## 5. Resource Exhaustion

**Scenario**: Logs mention "pool exhausted", "too many connections", or "out of memory". Find the bottleneck.

### Quick Triage: Find Resource Errors (30 seconds)

```bash
kelora app.log \
  --filter 'e.line.contains("pool") || e.line.contains("exhausted") || e.line.contains("memory")' \
  --exec 'e.absorb_kv("line")' \
  --drain -k msg
```

### Extract Resource Metrics (1 minute)

If logs contain key=value metrics, extract them:

```bash
kelora app.log \
  --filter 'e.line.contains("pool") || e.line.contains("connections")' \
  --exec 'e.absorb_kv("line")' \
  --exec 'track_max("max_used", e.get_path("used", 0)); track_max("max_waiting", e.get_path("waiting", 0))' \
  -J
```

### Timeline Analysis (2 minutes)

When did resource exhaustion start?

```bash
kelora app.log \
  --filter 'e.line.contains("pool") || e.line.contains("exhausted")' \
  --exec 'track_bucket("minute", e.timestamp.substring(0, 16))' \
  --metrics
```

**Example using sample data:**

```bash
kelora examples/quickstart.log \
  -f 'cols:ts(3) level *msg' \
  --filter 'e.msg.contains("Pool exhausted")' \
  -J
```

The `absorb_kv()` function works best when logs contain actual `key=value` pairs. For other formats, filter to find relevant lines and extract manually.

---

## 6. Deployment Correlation

**Scenario**: Deployment finished at 14:00. Did it cause increased errors?

### Compare Pre/Post Deployment (1 minute)

**Before deployment:**
```bash
kelora app.jsonl --until "2025-01-20T14:00:00Z" -l error,warn --stats
```

**After deployment:**
```bash
kelora app.jsonl --since "2025-01-20T14:00:00Z" -l error,warn --stats
```

**What to look for:**
- Compare error counts and event counts
- New error types in "After" = regression
- Significant count increase = deployment issue

### Find New Error Types (2 minutes)

Errors that appeared post-deployment:

```bash
# Get post-deployment errors
kelora app.jsonl \
  --since "2025-01-20T14:00:00Z" \
  --filter 'e.level == "ERROR"' \
  --drain -k message
```

Compare template IDs to pre-deployment run. New templates = new bugs.

### Visualize Error Timeline (1 minute)

See exact moment errors spiked:

```bash
kelora app.jsonl \
  --since "2025-01-20T13:30:00Z" \
  --until "2025-01-20T14:30:00Z" \
  --filter 'e.level == "ERROR"' \
  --exec 'track_bucket("5min", e.timestamp.substring(0, 15))' \
  --metrics
```

---

## 7. Rate Limit Investigation

**Scenario**: Rate limiting triggered on API. Is this legitimate traffic or abuse?

### Quick Triage: Who's Getting Rate Limited? (30 seconds)

```bash
kelora api.jsonl \
  --filter 'e.status == 429' \
  --exec 'track_top("user", e.user_id, 50); track_top("ip", e.client_ip, 30)' \
  --metrics
```

**What to look for:**
- Single user/IP dominating = likely abuse or misconfigured client
- Many different users = limits too aggressive or legitimate traffic spike

### Analyze Request Patterns (1 minute)

What endpoints are being rate limited?

```bash
kelora api.jsonl \
  --filter 'e.status == 429' \
  --exec 'track_top("endpoint", e.endpoint, 10); track_top("user_agent", e.user_agent, 20)' \
  --metrics
```

### Detect Scrapers/Bots (2 minutes)

Find suspicious user agents or request patterns:

```bash
kelora api.jsonl \
  --filter 'e.status == 429' \
  --exec 'track_top("ua_ip", e.user_agent + " [" + e.client_ip + "]", 20)' \
  --metrics
```

### Time-Based Rate Limit Analysis (2 minutes)

Are rate limits being hit constantly or during specific times?

```bash
kelora api.jsonl \
  --filter 'e.status == 429' \
  --exec 'track_bucket("hour", e.timestamp.substring(0, 13)); track_count("total_429s")' \
  --metrics
```

---

## 8. Distributed Trace Analysis

**Scenario**: Request ID `abc-123` failed. Trace it across all services.

### Trace Single Request (30 seconds)

```bash
kelora *.jsonl \
  --filter 'e.request_id == "abc-123"' \
  --normalize-ts \
  -k timestamp,service,level,message,status,duration_ms
```

### Trace Multiple Related Requests (1 minute)

Find all requests from a specific user session:

```bash
kelora *.jsonl \
  --filter 'e.session_id == "xyz-789"' \
  --normalize-ts \
  -k timestamp,service,request_id,endpoint,status \
  --output-format csv
```

### Identify Service Delays (2 minutes)

Which service in the chain was slowest?

```bash
kelora *.jsonl \
  --filter 'e.request_id == "abc-123"' \
  --exec 'track_top("service_duration", e.service + ": " + e.get_path("duration_ms", 0).to_string() + "ms", 10)' \
  --metrics
```

### Extract Full Request Context (2 minutes)

Get complete request lifecycle with all fields:

```bash
kelora *.jsonl \
  --filter 'e.request_id == "abc-123"' \
  --normalize-ts \
  -F inspect
```

---

## Advanced Techniques

### Combining Multiple Playbooks

Incident often requires multiple angles:

```bash
# Latency + Error correlation
kelora api.jsonl \
  --filter 'e.response_time_ms > 500 || e.status >= 500' \
  --exec 'track_stats("latency", e.response_time_ms); track_top("status", e.status.to_string(), 10); track_top("endpoint", e.endpoint, 10)' \
  --metrics
```

### Streaming Analysis During Active Incident

Monitor live logs as incident unfolds:

```bash
# Real-time error tracking with periodic updates (every 10 events)
tail -f /var/log/app.log | kelora -j \
  --filter 'e.level == "ERROR"' \
  --exec 'track_count("errors"); track_top("msg", e.message, 5)' \
  --span 10 \
  --span-close 'eprint(span.metrics.to_json())' \
  -q
```

**For Kubernetes pods:**
```bash
kubectl logs -f deployment/api-server | kelora -j -l error,warn \
  --exec 'track_stats("latency", e.response_time_ms)' \
  --span 1m \
  --span-close 'eprint(span.metrics.to_json())' \
  -q
```

**For Docker containers:**
```bash
docker logs -f container_name 2>&1 | kelora -f line \
  --filter 'e.line.matches("ERROR|FATAL")' \
  --exec 'e.absorb_kv("line")' \
  -J
```

### Export for Further Analysis

Save filtered data for deeper investigation:

```bash
# Export events to JSON
kelora app.jsonl \
  --since "2025-01-20T14:00:00Z" \
  --filter 'e.level == "ERROR"' \
  -F json > incident-errors.jsonl

# Export metrics to JSON file (while suppressing terminal output)
kelora app.jsonl \
  --exec 'track_stats("latency", e.response_time_ms); track_top("endpoint", e.endpoint, 20)' \
  --metrics-file incident-metrics.json \
  --silent

# Export to CSV for spreadsheet analysis
kelora api.jsonl \
  --filter 'e.status >= 500' \
  -k timestamp,endpoint,status,user_id,error_message \
  -F csv > server-errors.csv
```

Then analyze with other tools, load into a database, or share with team.

### Parallel Processing for Large Archives

Speed up analysis on multi-GB files:

```bash
kelora huge-archive.log.gz --parallel \
  --filter 'e.level == "ERROR"' \
  --exec 'track_stats("response", e.response_time_ms)' \
  --metrics
```

### Sampling for Extremely Large Datasets

When logs are too big to process quickly, use deterministic sampling:

```bash
# Analyze 10% of requests (consistent sampling by request_id)
kelora huge-logs.jsonl \
  --filter 'e.request_id.bucket() % 10 == 0' \
  --exec 'track_stats("latency", e.response_time_ms)' \
  --metrics

# Sample every 100th event (fast counter-based)
kelora huge-logs.jsonl \
  --filter 'sample_every(100)' \
  --exec 'track_top("endpoint", e.endpoint, 20)' \
  --metrics
```

**When to use each:**
- `bucket() % N` - Deterministic by field value (same IDs always included)
- `sample_every(N)` - Fast counter-based (1 in N events)

---

## Playbook Cheat Sheet

Copy these to your incident runbook:

```bash
# 1. RECONNAISSANCE - Always start here
kelora app.jsonl --stats

# 2. ERROR PATTERN DISCOVERY
kelora app.jsonl -l error --drain -k message

# 3. TOP OFFENDERS
kelora app.jsonl -e 'track_top("key", e.field, 20)' -m

# 4. TIME DISTRIBUTION (proper datetime bucketing)
kelora app.jsonl -e 'e.bucket = to_datetime(e.timestamp).round_to("5m").to_iso()' \
  -e 'track_bucket("time", e.bucket)' -m

# 5. COMPREHENSIVE STATS
kelora app.jsonl -e 'track_stats("metric", e.value)' -m

# 6. VISUALIZE LATENCY OVER TIME
kelora api.jsonl -F tailmap --keys response_time_ms

# 7. EXPORT METRICS TO FILE
kelora app.jsonl -e 'track_stats("latency", e.ms)' \
  --metrics-file incident.json --silent

# 8. SAFE FIELD ACCESS
kelora app.jsonl --filter 'e.get_path("nested.field", 0) > 100'

# 9. IP CLASSIFICATION
kelora auth.log -e 'e.ip_type = if e.ip.is_private_ip() { "internal" } else { "external" }'

# 10. SAMPLING LARGE LOGS
kelora huge.log --filter 'sample_every(100)' -e 'track_count("total")' -m

# 11. LIVE KUBERNETES MONITORING
kubectl logs -f deploy/app | kelora -j -l error,warn -q \
  -e 'track_stats("latency", e.response_time_ms)' \
  --span 1m --span-close 'eprint(span.metrics.to_json())'

# 12. EXTRACT SUBSET WITH TIME RANGE
kelora app.jsonl --since 2h --until now -l error -J > recent-errors.jsonl
```

---

## Tips for Effective Incident Response

1. **Always start with `--stats`**: Reconnaissance first - understand time span, field availability, and data volume
2. **Use `--drain` early**: Quickly understand log patterns without reading every line
3. **Visualize with tailmap**: `kelora api.jsonl -F tailmap --keys response_time_ms` shows latency distribution at a glance
4. **Safe field access**: Use `e.get_path("field.path", default)` instead of direct access to avoid missing field errors
5. **Proper time bucketing**: Use `to_datetime().round_to("5m")` instead of `substring()` for robust time analysis
6. **Filter progressively**: Start broad, narrow down with additional `--filter` flags
7. **Track everything in one pass**: Combine multiple `track_*()` calls for efficiency
8. **Export for collaboration**: Use `--metrics-file incident.json --silent` to save metrics without terminal clutter
9. **Classify IPs**: Use `is_private_ip()` to distinguish internal vs external threats
10. **Sample huge datasets**: Use `sample_every(100)` or `bucket() % 10 == 0` for multi-TB logs
11. **Use compact output**: `--metrics=short` during time-sensitive incidents
12. **Live streaming**: Kubernetes: `kubectl logs -f` | Docker: `docker logs -f` | Files: `tail -f`
13. **Leverage time ranges**: `--since 2h` and `--until now` for recent events
14. **Parallel for speed**: Add `--parallel` when processing GB+ files (metrics still work!)
15. **Document your queries**: Save working commands to your incident runbook

---

## Next Steps

- [Power-User Techniques](power-user-techniques.md) - Advanced Rhai functions and patterns
- [Build Streaming Alerts](build-streaming-alerts.md) - Automate incident detection
- [Debug Issues](debug-issues.md) - Troubleshooting Kelora itself
- [Monitor Application Health](monitor-application-health.md) - Proactive monitoring strategies
