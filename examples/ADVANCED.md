# Advanced Kelora Examples

Purpose-built pipelines that highlight Kelora's scripting, metrics, and windowing features. Start with the [homepage examples](../docs/index.md) or [Quickstart](../docs/quickstart.md) if you want shorter commands, then come back here when you need heavier automation.

---

## 1. Normalize bespoke deployment logs
Turn ad-hoc delimited text into structured events, extract latency numbers, and keep only the slow or broken requests.

### Sample log (`examples/release_pipe.log`)
```
2024-07-17 12:10:02|WARN|checkout|req-92|retrying payment status=SOFT_FAIL duration=890ms
2024-07-17 12:10:06|ERROR|checkout|req-93|final failure status=HARD_FAIL duration=1480ms cause=db_deadlock
```

### Command
```bash
kelora -f 'cols:ts level service request_id *message' examples/release_pipe.log \
  --cols-sep '|' \
  --normalize-ts \
  --exec 'e.latency_ms = e.message.extract_re("(\\d+)ms", 1).to_int()' \
  --filter 'e.level == "ERROR" || e.latency_ms > 800' \
  --keys ts,service,request_id,latency_ms,message \
  -F logfmt
```

### Why it matters
Combines the column parser (`-f 'cols:…'`) with regex extraction (`text.extract_re`) and timestamp normalization (`--normalize-ts`) to tidy bespoke deployment logs in one pass.

---

## 2. Decode Kubernetes audit tokens safely
Investigate slow or failing auth requests by decoding JWT payloads, inferring roles, and masking sensitive IPs and tokens.

### Sample log (`examples/k8s_security.jsonl`)
```json
{"timestamp":"2024-07-17T12:12:05Z","level":"WARN","pod":"auth-api-7d6d8c5bcb-lvk8x","latency_ms":1820,"token":"eyJhbGciOi...","ip":"203.0.113.45","event":"login_timeout"}
{"timestamp":"2024-07-17T12:12:09Z","level":"ERROR","pod":"auth-api-7d6d8c5bcb-lvk8x","latency_ms":2400,"token":"eyJhbGciOi...","ip":"203.0.113.45","event":"login_fail"}
```

### Command
```bash
kelora -j examples/k8s_security.jsonl \
  --filter 'e.level == "WARN" || e.level == "ERROR" || e.latency_ms > 1500' \
  --exec 'if e.has_field("token") {
            let jwt = e.token.parse_jwt();
            e.role = jwt.get_path("claims.role", "guest");
            e.token = e.token.slice(":8") + "…";
          }' \
  --exec 'if e.has_field("ip") { e.ip = e.ip.mask_ip(2) }' \
  --keys timestamp,pod,role,latency_ms,event,ip \
  -F json
```

### Why it matters
Shows how `text.parse_jwt`, `get_path`, and `mask_ip` work together for security triage and redaction without leaving the CLI.

---

## 3. Fan out batch payloads and count failures
Explode nested arrays into individual events with `emit_each`, then track totals and uniques for settlement failures.

### Sample log (`examples/batch_settlements.jsonl`)
```json
{"batch_id":"2024-07-17-A","user":"acme","attempts":[{"card":"amex","status":"OK","amount":12900},{"card":"visa","status":"DECLINED","reason":"insufficient"}]}
{"batch_id":"2024-07-17-B","user":"globex","attempts":[{"card":"mc","status":"DECLINED","reason":"fraud"},{"card":"mc","status":"DECLINED","reason":"fraud"}]}
```

### Command
```bash
kelora -j examples/batch_settlements.jsonl --metrics \
  --exec 'if e.has_field("attempts") {
            emit_each(e.attempts, #{batch_id: e.batch_id, user: e.user});
            e = ();
          }' \
  --filter 'e.status == "DECLINED"' \
  --exec 'track_count("declined_total"); track_unique("cards", e.card)' \
  --keys batch_id,user,card,reason \
  -F csv
```

### Why it matters
Demonstrates fan-out plus metrics tracking (`track_count`, `track_unique`) to turn log batches into structured analytics quickly.

---

## 4. Five-minute latency SLO rollups
Use time-based spans to emit per-window slow-request percentages with no extra tooling.

### Sample log (`examples/web_latency.jsonl`)
```json
{"timestamp":"2024-07-17T12:00:01Z","latency_ms":320,"path":"/checkout"}
{"timestamp":"2024-07-17T12:04:12Z","latency_ms":1800,"path":"/checkout"}
```

### Command
```bash
kelora -j examples/web_latency.jsonl --metrics \
  --exec 'track_count("total"); if e.latency_ms > 1200 { track_count("slow") }' \
  --span 5m \
  --span-close '
    let total = span.metrics.get_path("total", 0);
    let slow = span.metrics.get_path("slow", 0);
    if total > 0 {
      let pct = slow * 100 / total;
      print("window=" + span.id + " slow_pct=" + pct.to_string() +
            " slow=" + slow.to_string() + " total=" + total.to_string());
    }
  ' \
  -F none
```

### Why it matters
Highlights span windows (`--span`, `--span-close`) and per-span `span.metrics` for SLO rollups without databases.

---

## 5. Streaming deploy guardrail with sliding windows
Watch a live deploy (`tail -f`), maintain a sliding window, and append alerts to disk when bursts hit.

### Sample log (`examples/deploy_tail.jsonl`)
```json
{"timestamp":"2024-07-17T12:20:00Z","service":"api","level":"ERROR","message":"timeout","latency_ms":2100}
{"timestamp":"2024-07-17T12:20:02Z","service":"api","level":"ERROR","message":"timeout","latency_ms":2150}
```

### Command
```bash
tail -f examples/deploy_tail.jsonl | kelora -j --window 25 --metrics --allow-fs-writes -qq \
  --exec '
    let recent = window.pluck("level");
    let err = recent.filter(|lvl| lvl == "ERROR").len();
    if err >= 3 {
      append_file("/tmp/deploy-alerts.log",
        now_utc().to_string() + " burst errors (" + err.to_string() + "/25)\n");
      track_count("burst");
    }
  ' \
  --end '
    let bursts = metrics.get_path("burst", 0);
    if bursts > 0 { eprint("deploy blocked; bursts=" + bursts.to_string()); exit(1); }
  '
```

### Why it matters
Combines sliding windows (`--window` + `window.pluck()`), quiet mode, metrics, and file output (`append_file`) to build a lightweight deploy guard without external alerting systems.

---

## 6. Incident timeline across mixed formats
Fuse syslog headers with embedded logfmt payloads, keep stacktraces intact, and print only the context that matters during an outage.

### Sample log (`examples/incident_story.log`)
```
2024-06-19T12:31:52Z kube-controller host=cp-1 level=INFO action=rollout ns=payments detail="Scaling replicas"
2024-06-19T12:32:07Z kubelet[worker-2] level=ERROR Back-off restarting failed container
2024-06-19T12:32:08Z kubelet[worker-2] level=ERROR user=system:node detail="CrashLoopBackOff" pod=checkout-v2
```

### Command
```bash
kelora -f line examples/incident_story.log \
  --multiline timestamp \
  --exec '
    let parts = e.line.split(" ");
    if parts.len() >= 3 {
      let logfmt_part = parts.slice("2:").join(" ");
      if logfmt_part.contains("=") {
        e += logfmt_part.parse_logfmt();
        e.timestamp = parts[0];
        e.process = parts[1];
      }
    }
  ' \
  --filter '["ERROR", "CRITICAL"].contains(e.level) || e.action == "rollout"' \
  --before-context 1 --after-context 1 \
  --keys timestamp,process,level,ns,pod,action
```

### Why it matters
Demonstrates format fusion (`parse_logfmt()` inside syslog), context controls, and richer filtering logic than simple grep during live incident response.

---

## 7. Latency bucket dashboard with tracked metrics
Assign latency buckets, count statuses, and print a JSON stream plus a metrics trailer that can be scraped or persisted.

### Sample log (`examples/payments_latency.jsonl`)
```json
{"timestamp":"2024-07-01T09:00:00Z","order_id":"O-18","status":"ok","duration_ms":140,"region":"us-east-1"}
{"timestamp":"2024-07-01T09:00:03Z","order_id":"O-19","status":"ok","duration_ms":460,"region":"us-east-1"}
{"timestamp":"2024-07-01T09:00:04Z","order_id":"O-20","status":"error","duration_ms":1480,"region":"us-west-2"}
```

### Command
```bash
kelora -j examples/payments_latency.jsonl \
  --exec '
    let bucket = if e.duration_ms < 200 {
      "<200ms"
    } else if e.duration_ms < 500 {
      "200-500ms"
    } else {
      "slow";
    };
    e.latency_bucket = bucket;
    track_bucket("latency", bucket);
    track_count("status_" + e.status.to_string());
  ' \
  --metrics \
  --keys timestamp,order_id,status,region,latency_bucket,duration_ms \
  -F json
```

### Why it matters
Pairs per-event enrichment with `track_bucket()`/`track_count()` so you get both detailed rows and aggregated insight from a single run.

---

## 8. Fan out nested audit findings with `emit_each`
Turn arrays of security findings into flat events, filter by severity, and keep analyst-friendly fields only.

### Sample log (`examples/audit_findings.jsonl`)
```json
{"timestamp":"2024-05-01T09:12:00Z","user":"alice","findings":[{"kind":"mfa_disabled","severity":60},{"kind":"stale_token","severity":30}]}
{"timestamp":"2024-05-01T09:13:10Z","user":"bob","findings":[{"kind":"risky_device","severity":80}]}
```

### Command
```bash
kelora -j examples/audit_findings.jsonl \
  --exec '
    let rows = [];
    if e.has_field("findings") {
      for finding in e.findings {
        rows.push(#{timestamp: e.timestamp, user: e.user, kind: finding.kind, severity: finding.severity});
      }
      emit_each(rows);
      e = ();
    }
  ' \
  --filter 'e.severity >= 50' \
  --keys timestamp,user,kind,severity \
  -F json
```

### Why it matters
Showcases Rhai loops plus `emit_each()` to flatten nested JSON, then prunes to the high-severity results analysts actually need.

---

## 9. Sliding-window credential guardrail
Maintain a rolling view of auth responses and raise structured alerts when failure bursts spike beyond a threshold.

### Sample log (`examples/auth_burst.jsonl`)
```json
{"timestamp":"2024-06-01T10:00:01Z","user":"alice","ip":"198.51.100.10","status":200}
{"timestamp":"2024-06-01T10:00:02Z","user":"alice","ip":"198.51.100.10","status":401}
{"timestamp":"2024-06-01T10:00:03Z","user":"alice","ip":"198.51.100.10","status":401}
```

### Command
```bash
kelora -j examples/auth_burst.jsonl \
  --window 20 \
  --exec '
    let recent = window.pluck("status");
    let failures = recent.reduce(|acc, code| acc + if code >= 500 { 1 } else { 0 }, 0);
    if failures >= 5 {
      e.alert = failures.to_string() + " failures in last " + recent.len().to_string() + " events";
    } else {
      e = ();
    }
  ' \
  --keys timestamp,user,ip,alert \
  -F logfmt
```

### Why it matters
Highlights `--window` + `window.pluck()` for contextual analytics where simple counts miss short, sharp spikes.

---

## 10. Span-based SLO scoreboard
Use fixed five-minute spans, tracked metrics, and `--span-close` hooks to emit per-window summaries for dashboards.

### Sample log (`examples/uptime_windows.jsonl`)
```json
{"timestamp":"2024-07-01T00:00:10Z","status":200,"service":"billing"}
{"timestamp":"2024-07-01T00:02:11Z","status":503,"service":"billing"}
{"timestamp":"2024-07-01T00:04:05Z","status":200,"service":"billing"}
{"timestamp":"2024-07-01T00:06:01Z","status":500,"service":"billing"}
```

### Command
```bash
kelora -j examples/uptime_windows.jsonl \
  --span 5m \
  --exec '
    track_count("events");
    if e.status >= 500 { track_count("failures"); }
  ' \
  --span-close '
    let total = span.metrics.get_path("events", 0);
    let failed = span.metrics.get_path("failures", 0);
    let rate = if total == 0 { 0.0 } else { failed * 100.0 / total };
    emit_each([#{window: span.id, total: total, failures: failed, error_pct: rate}]);
  ' \
  -F json
```

### Why it matters
Demonstrates span hooks, safe nested access via `get_path`, and `emit_each` to broadcast per-window rollups without leaving the streaming pipeline.
