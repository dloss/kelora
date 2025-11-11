# Why Kelora? (vs angle-grinder and others)

## The Positioning Problem

angle-grinder beats Kelora in almost every benchmark:
- Simple filtering: **1.2x faster**
- Field extraction: **13x faster**
- JSON filtering: **17x faster**
- JSON transform: **40x faster**
- Multi-stage pipeline: **32x faster**

So why would anyone use Kelora?

---

## The Honest Answer

**Choose angle-grinder when:**
- You need raw speed
- Your pipelines fit the DSL: `* | json | where | count by`
- You want a live terminal UI
- Your analysis is "search + basic aggregation"

**Choose Kelora when:**
- You need **full scripting** (loops, state, functions, rich logic)
- You want **40+ specialized functions** built-in
- You need **sliding windows** for context-aware analysis
- You want **reusable workflows** via config files
- Your pipeline has **complex multi-stage transformations**
- You're mixing **multiple formats** in one pass

---

## Concrete Differentiators

### 1. Full Programming Language (Rhai)

**angle-grinder:** DSL with operators
```
* | json | where status >= 500 | count by endpoint
```

**Kelora:** Full Rhai scripting
```rhai
// --exec script
if e.has("response_time") {
    let ms = e.response_time.to_int();
    if ms > 1000 {
        e.slow = true;
        track_percentile("latency_ms", ms);
    }
}

// Track trends
if window.len() > 10 {
    let recent_errors = window
        .filter(|x| x.level == "ERROR")
        .len();
    if recent_errors > 5 {
        emit_alert("Error burst detected");
    }
}
```

You can:
- Use conditionals, loops, local variables
- Call functions, build reusable helpers
- Maintain state across events
- Import external scripts with `--script`

### 2. Sliding Windows

**angle-grinder:** Not available

**Kelora:** Built-in windowing
```bash
kelora -j logs.jsonl --window 60 \
  --exec 'let recent = window.filter(|x| x.level == "ERROR");
          if recent.len() >= 3 {
              e.burst = true
          }'
```

Every event has access to the previous N events (or N seconds). Essential for:
- Error burst detection
- Trend analysis
- Context enrichment ("what happened before this error?")
- Moving averages, rate calculations

### 3. Specialized Functions (40+)

angle-grinder has basic operators. Kelora has domain-specific functions:

**Security/Privacy:**
- `mask_ip(octets)` - "192.168.1.1" → "192.168.0.0"
- `hash_consistent(field)` - Stable anonymization
- `parse_jwt(token)` - Extract claims without decoding

**Parsing:**
- `parse_logfmt(str)` - Extract embedded key=value pairs
- `parse_url(str)` - Break down URLs into components
- `absorb_kv(field)` - Pull key=value from message into event fields

**Time:**
- `parse_time(str, fmt, tz)` - Custom timestamp parsing
- `to_timestamp()` - Convert to Unix epoch
- Time zone conversions

**Metrics:**
- `track_count(key)` - Count occurrences
- `track_percentile(key, value)` - P50/P95/P99
- `track_hll(key, value)` - Cardinality estimation

**Transforms:**
- `get_path("a.b.c", default)` - Safe nested access
- `flatten(prefix)` - Nested JSON → flat fields
- `fan_out(array_field)` - One event → many

See `--help-functions` for all 40+.

### 4. Config Files

**angle-grinder:** Command-line only

**Kelora:** Reusable configs
```ini
# .kelora.ini
[default]
input-format = json
output-format = logfmt
no-emoji = true
levels = error,critical

[production-triage]
filter = e.environment == "prod"
exec = e.absorb_kv("msg")
keys = timestamp,service,error,user_id
```

Run with: `kelora --config production-triage app.jsonl`

Share configs with team, version control, per-project workflows.

### 5. Begin/End Stages

**angle-grinder:** Per-event processing only

**Kelora:** Setup and teardown
```bash
kelora --begin 'let errors = [];' \
       --exec 'if e.level == "ERROR" { errors.push(e.service); }' \
       --end 'print_json(errors.unique())'
```

Use cases:
- Load reference data (service maps, user lists)
- Initialize metrics
- Post-process aggregations
- Emit summary reports

### 6. Multi-Format Pipelines

**angle-grinder:** JSON, logfmt, basic text

**Kelora:** All of above plus:
- Syslog (RFC3164/RFC5424)
- CSV/TSV with type annotations
- Apache combined/common
- Custom column specs: `cols:ts level service *message`
- Docker logs with prefixes
- Kubernetes logs

Parse syslog that contains embedded logfmt:
```bash
kelora -f syslog app.log \
  --exec 'if e.msg.contains("=") {
            e += e.msg.parse_logfmt()
          }' \
  -F json
```

---

## Use Case Matrix

| Scenario | angle-grinder | Kelora | Why |
|----------|---------------|--------|-----|
| Search for "ERROR" | ✅ Best | ⚠️ Overkill | Just use grep or agrind |
| Filter + project fields | ✅ Best | ⚠️ Slower | agrind's DSL is perfect here |
| Count by field | ✅ Best | ⚠️ Slower | agrind has `count by` built-in |
| Detect error bursts | ❌ Hard | ✅ Best | Need windowing |
| Parse JWT, mask IPs | ❌ Manual | ✅ Best | Built-in functions |
| Multi-stage transform | ⚠️ Chaining | ✅ Best | Full scripting helps |
| Load external data | ❌ Not available | ✅ Best | `--begin` stage |
| Reusable workflows | ❌ Shell aliases | ✅ Best | Config files |
| Mix syslog + JSON | ⚠️ Tricky | ✅ Best | Multi-format support |
| Span aggregation | ❌ Not available | ✅ Best | `--span-by` + windows |

---

## Real-World Examples

### Example 1: API Error Context

**Problem:** When an API returns 500, I want to see what happened in the previous 5 requests.

**angle-grinder:** Not easily doable (no window access)

**Kelora:**
```bash
kelora -j api.jsonl --window 5 \
  --filter 'e.status.to_int() >= 500' \
  --exec 'e.context = window.map(|x| x.method + " " + x.endpoint)'
```

### Example 2: Anonymize Logs for Support

**Problem:** Need to share logs with vendor, must mask PII.

**angle-grinder:** Would need external tools

**Kelora:**
```bash
kelora -j prod.jsonl \
  --exec 'e.user_id = e.user_id.hash_consistent();
          e.ip = e.ip.mask_ip(2);
          if e.has("email") { e.del("email"); }' \
  -F json > sanitized.jsonl
```

### Example 3: Track P95 Latency by Endpoint

**Problem:** Want to know which endpoints are slow (P95 > 500ms).

**angle-grinder:** Can count, but not percentiles

**Kelora:**
```bash
kelora -j access.jsonl \
  --exec 'track_percentile(e.endpoint + ".latency", e.latency_ms.to_int())' \
  --metrics
```

Output shows P50/P95/P99 for each endpoint.

### Example 4: Correlate Across Services

**Problem:** Trace IDs span multiple services. Want to see full trace.

**angle-grinder:** Would need to process multiple times

**Kelora:**
```bash
kelora -j services.jsonl \
  --begin 'let traces = #{};' \
  --exec 'let tid = e.trace_id;
          if !traces.contains(tid) { traces[tid] = []; }
          traces[tid].push(e.service + ":" + e.message);' \
  --end 'for (tid, events) in traces {
           print(`Trace ${tid}: ${events.join(" -> ")}`);
         }'
```

---

## Performance Reality Check

Yes, Kelora is slower. But consider:

**Scenario:** Analyze 1M log lines

- **angle-grinder**: 3 seconds
- **Kelora**: 40 seconds (sequential), 8 seconds (parallel)

**Is 8 seconds a problem?**

- For ad-hoc exploration: Yes, use angle-grinder
- For automated reporting: No, 8s is fine
- For complex pipelines: Often faster than multiple tools + bash

**When Kelora is actually faster:**

Bash pipeline with 5 tools + glue:
```bash
zcat logs.gz | \
  jq -c 'select(.level == "ERROR")' | \
  grep "api" | \
  awk '{...}' | \
  some_script.py | \
  sort | uniq -c
```

vs Kelora:
```bash
kelora logs.gz --parallel --filter '...' --exec '...' --metrics
```

The Kelora version might be faster because:
- No process spawning overhead
- No serialization between tools
- Single-pass processing
- Parallel mode uses all cores

---

## The Bottom Line

**angle-grinder** is a fast, focused tool for common log queries.

**Kelora** is a programmable log analysis platform for complex workflows.

Choose based on:
- **Complexity** - Simple → angle-grinder, Complex → Kelora
- **Speed** - Critical → angle-grinder, Acceptable → Kelora
- **Reusability** - One-off → angle-grinder, Repeatable → Kelora
- **Features** - Basic → angle-grinder, Specialized → Kelora

Both tools are excellent. They occupy different points on the expressiveness/speed tradeoff curve.

---

## For the README

Suggested "When to use Kelora" section:

```markdown
## When to Use Kelora

Kelora fills the gap between simple tools (grep, jq, awk) and full observability platforms.

**Use Kelora when:**
- You need **40+ specialized functions** (parse_jwt, mask_ip, track_percentile)
- You need **sliding windows** for context-aware analysis
- You want **full scripting** with loops, state, and conditionals
- Your pipeline mixes **multiple log formats**
- You need **reusable workflows** via config files

**Use something else when:**
- Simple pattern matching: `grep` / `ripgrep` (50-100x faster)
- Basic JSON queries: `jq` or `angle-grinder` (5-40x faster)
- Pure CSV analytics: `qsv` / `miller` (95x faster)
- Interactive exploration: `lnav`

See [Performance Comparisons](https://kelora.dev/performance-comparisons) for
honest benchmarks and detailed guidance.
```

---

## HN Positioning

When someone asks "Why not angle-grinder?":

> "angle-grinder is excellent for speed and common queries. Kelora trades speed
> for full scripting (Rhai), 40+ specialized functions, and sliding windows.
> Think: angle-grinder for ad-hoc queries, Kelora for complex analysis pipelines."

Short version: **Different tools for different problems.**
