# Begin and End Stages

Learn how to use Kelora's four scripting stages to build complete data processing pipelines with initialization, transformation, filtering, and summarization.

## What You'll Learn

- Understand the pipeline lifecycle: `--begin` → (filter/exec)* → `--end`
- Initialize shared state with the `conf` map
- Load lookup data from files
- Enrich events using lookup tables
- Multiple filters and execs in sequence
- Summarize results in the `--end` stage
- Access metrics from `track_*()` functions

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Basic CLI usage
- [Introduction to Rhai Scripting](intro-to-rhai.md) - Basic scripting knowledge
- **Time:** ~20 minutes

## Sample Data

This tutorial uses:

- `examples/simple_json.jsonl` - Application logs
- `examples/service_metadata.json` - Service lookup data

---

## Understanding the Pipeline Lifecycle

Kelora processes events through four distinct stages:

```
┌─────────┐     ┌──────────────────────────┐     ┌────────┐
│ --begin │ ──→ │ Per-Event Processing     │ ──→ │ --end  │
└─────────┘     │ (--filter, --exec) × N   │     └────────┘
                └──────────────────────────┘
```

**Execution order:**

1. **`--begin`** - Runs **once** before processing any events
2. **Per-event stages** - Run for **each** event in CLI order:
   - `--filter` - Keep or discard events
   - `--exec` - Transform or track events
   - Can have multiple of each, intermixed
3. **`--end`** - Runs **once** after all events processed

---

## Step 1: The --begin Stage

Use `--begin` to initialize state before processing events.

### Simple Initialization

Set up a flag that all events can read:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.environment = "production"' \
        --exec 'e.env = conf.environment' \
        -k service,env,message \
        --take 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.environment = "production"' \
        --exec 'e.env = conf.environment' \
        -k service,env,message \
        --take 3
    ```

**Key insight:** The `conf` map is written in `--begin` and readable in `--exec` and `--filter` stages.

---

## Step 2: Loading Lookup Data

Use helper functions in `--begin` to load external data.

### Available Helper Functions

| Function | Purpose | Returns |
|----------|---------|---------|
| `read_lines(path)` | Read file as lines | Array of strings |
| `read_file(path)` | Read entire file | Single string |

**Note:** To load JSON, use `read_file()` and parse it with `.parse_json()`, or define data inline.

### Load Lookup Data Inline

Let's create service metadata and enrich events:

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{
                    api: #{team: "backend", owner: "alice@example.com", criticality: "high"},
                    database: #{team: "data", owner: "bob@example.com", criticality: "critical"},
                    auth: #{team: "security", owner: "charlie@example.com", criticality: "critical"},
                    cache: #{team: "infra", owner: "diana@example.com", criticality: "medium"},
                    scheduler: #{team: "ops", owner: "evan@example.com", criticality: "low"}
                }' \
        --exec 'if conf.services.contains(e.service) {
                    let meta = conf.services[e.service];
                    e.team = meta.team;
                    e.owner = meta.owner;
                    e.criticality = meta.criticality
                }' \
        -k service,team,owner,criticality,message \
        --take 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{api: #{team: "backend", owner: "alice@example.com", criticality: "high"}, database: #{team: "data", owner: "bob@example.com", criticality: "critical"}, auth: #{team: "security", owner: "charlie@example.com", criticality: "critical"}, cache: #{team: "infra", owner: "diana@example.com", criticality: "medium"}, scheduler: #{team: "ops", owner: "evan@example.com", criticality: "low"}}' \
        --exec 'if conf.services.contains(e.service) { let meta = conf.services[e.service]; e.team = meta.team; e.owner = meta.owner; e.criticality = meta.criticality }' \
        -k service,team,owner,criticality,message \
        --take 5
    ```

**What happened:**

1. `--begin` created an inline map with service metadata in `conf.services`
2. `--exec` looked up each event's service in the loaded data
3. Added team, owner, and criticality fields to each event

---

## Step 3: Load Array Data with read_lines()

Load a list of values from a text file.

First, let's create a blocked IPs file:

=== "Command"

    ```bash
    echo "192.168.1.100
    10.0.0.50
    172.16.0.99" > /tmp/blocked_ips.txt

    echo '{"ip":"192.168.1.100","user":"admin"}
    {"ip":"192.168.1.200","user":"alice"}
    {"ip":"10.0.0.50","user":"bob"}
    {"ip":"192.168.1.201","user":"charlie"}' | \
        kelora -j \
        --begin 'conf.blocked = read_lines("/tmp/blocked_ips.txt")' \
        --exec 'e.is_blocked = e.ip in conf.blocked' \
        --filter 'e.is_blocked' \
        -k ip,user,is_blocked
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    echo "192.168.1.100
    10.0.0.50
    172.16.0.99" > /tmp/blocked_ips.txt

    echo '{"ip":"192.168.1.100","user":"admin"}
    {"ip":"192.168.1.200","user":"alice"}
    {"ip":"10.0.0.50","user":"bob"}
    {"ip":"192.168.1.201","user":"charlie"}' | \
        kelora -j \
        --begin 'conf.blocked = read_lines("/tmp/blocked_ips.txt")' \
        --exec 'e.is_blocked = e.ip in conf.blocked' \
        --filter 'e.is_blocked' \
        -k ip,user,is_blocked
    ```

**Pattern:** Use `read_lines()` for simple lists, `in` operator to check membership.

---

## Step 4: Multi-Stage Pipelines

Chain multiple `--exec` and `--filter` stages to build complex logic.

### Progressive Filtering

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{
                    api: #{criticality: "high"},
                    database: #{criticality: "critical"},
                    auth: #{criticality: "critical"},
                    cache: #{criticality: "medium"}
                }' \
        --exec 'if conf.services.contains(e.service) {
                    e.criticality = conf.services[e.service].criticality
                } else {
                    e.criticality = "unknown"
                }' \
        --filter 'e.criticality == "critical" || e.criticality == "high"' \
        --exec 'e.is_error = e.level == "ERROR" || e.level == "CRITICAL"' \
        --filter 'e.is_error' \
        -k service,criticality,level,message
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{api: #{criticality: "high"}, database: #{criticality: "critical"}, auth: #{criticality: "critical"}, cache: #{criticality: "medium"}}' \
        --exec 'if conf.services.contains(e.service) { e.criticality = conf.services[e.service].criticality } else { e.criticality = "unknown" }' \
        --filter 'e.criticality == "critical" || e.criticality == "high"' \
        --exec 'e.is_error = e.level == "ERROR" || e.level == "CRITICAL"' \
        --filter 'e.is_error' \
        -k service,criticality,level,message
    ```

**Pipeline flow:**

1. Load metadata in `--begin`
2. Enrich with criticality (`--exec`)
3. Keep only critical/high services (`--filter`)
4. Add error flag (`--exec`)
5. Keep only errors (`--filter`)
6. Output result

---

## Step 5: The --end Stage

Use `--end` to run code **once** after all events are processed.

### Simple Summary

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --begin 'print("Starting analysis...")' \
        --exec 'track_count(e.service)' \
        --end 'print("Processed " + metrics.keys().len() + " unique services")' \
        -F none \
        --metrics
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --begin 'print("Starting analysis...")' \
        --exec 'track_count(e.service)' \
        --end 'print("Processed " + metrics.keys().len() + " unique services")' \
        -F none \
        --metrics
    ```

**Available in `--end`:**

- `metrics` - Map populated by `track_*()` functions
- `conf` - The configuration map (read-only)

---

## Step 6: Accessing Metrics in --end

Build custom reports using tracked metrics.

### Generate Custom Report

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --exec 'track_count(e.level)' \
        --exec 'track_count(e.service)' \
        --end 'print("=== Log Summary ===");
               print("Total levels: " + metrics.keys().filter(|k| k != "service").len());
               print("Total services: " + metrics.keys().filter(|k| k == "api" || k == "database" || k == "auth" || k == "cache" || k == "scheduler").len())' \
        -F none
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --exec 'track_count(e.level)' \
        --exec 'track_count(e.service)' \
        --end 'print("=== Log Summary ==="); print("Levels tracked: " + metrics.keys().len()); for key in metrics.keys() { print("  " + key + ": " + metrics[key]) }' \
        -F none
    ```

**Pattern:** Use `--end` to generate reports, send notifications, or write summary files.

---

## Step 7: Real-World Example - Alert on Critical Errors

Combine all stages for a production-ready alert pipeline.

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{
                    database: #{criticality: "critical", owner: "bob@example.com"},
                    auth: #{criticality: "critical", owner: "charlie@example.com"}
                 };
                 conf.alert_threshold = 1' \
        --exec 'if conf.services.contains(e.service) {
                    e.criticality = conf.services[e.service].criticality;
                    e.owner = conf.services[e.service].owner
                }' \
        --filter 'e.criticality == "critical" && (e.level == "ERROR" || e.level == "CRITICAL")' \
        --exec 'track_count(e.owner)' \
        --end 'print("=== Alert Summary ===");
               for owner in metrics.keys() {
                   let count = metrics[owner];
                   if count >= conf.alert_threshold {
                       print("ALERT: " + owner + " has " + count + " critical error(s)")
                   } else {
                       print(owner + ": " + count + " error(s)")
                   }
               }' \
        -F none
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        --begin 'conf.services = #{database: #{criticality: "critical", owner: "bob@example.com"}, auth: #{criticality: "critical", owner: "charlie@example.com"}}; conf.alert_threshold = 1' \
        --exec 'if conf.services.contains(e.service) { e.criticality = conf.services[e.service].criticality; e.owner = conf.services[e.service].owner }' \
        --filter 'e.criticality == "critical" && (e.level == "ERROR" || e.level == "CRITICAL")' \
        --exec 'track_count(e.owner)' \
        --end 'print("=== Alert Summary ==="); for owner in metrics.keys() { let count = metrics[owner]; if count >= conf.alert_threshold { print("ALERT: " + owner + " has " + count + " critical error(s)") } else { print(owner + ": " + count + " error(s)") } }' \
        -F none
    ```

**Complete pipeline:**

1. `--begin`: Load service metadata and set threshold
2. `--exec`: Enrich events with criticality and owner
3. `--filter`: Keep only critical errors
4. `--exec`: Track error count per owner
5. `--end`: Generate alert report based on threshold

---

## Step 8: When to Use Each Stage

### Use --begin when you need to:
- ✅ Load lookup tables (JSON, CSV, text files)
- ✅ Initialize configuration values
- ✅ Read reference data that doesn't change per event
- ✅ Set up shared state for all events

### Use --exec when you need to:
- ✅ Transform event fields
- ✅ Add computed fields
- ✅ Track metrics with `track_*()`
- ✅ Enrich events with lookup data
- ✅ Modify existing fields

### Use --filter when you need to:
- ✅ Keep events matching criteria
- ✅ Discard irrelevant events
- ✅ Narrow down data progressively
- ✅ Sample events (every Nth event)

### Use --end when you need to:
- ✅ Generate summary reports
- ✅ Access final metrics
- ✅ Print completion messages
- ✅ Send notifications or alerts
- ✅ Write summary files

---

## Important Rules

### 1. The conf Map

```
--begin:          conf is READ-WRITE
--exec, --filter: conf is READ-ONLY
--end:            conf is READ-ONLY
```

You can **only modify** `conf` in `--begin`. After that, it's frozen.

### 2. Pipeline Order

Scripts execute in the **exact order** you specify on the command line:

```bash
# Order matters!
kelora -j app.log \
    --exec 'e.x = 1' \      # Step 1: x = 1
    --exec 'e.y = e.x + 1'  # Step 2: y = 2 (reads x)
    --filter 'e.y > 1'      # Step 3: keep if y > 1
```

### 3. Metrics Availability

- `track_*()` functions can be called in `--exec` stages
- The `metrics` map is **only available** in `--end`
- Metrics accumulate across all events

### 4. Variable Scope Between Stages

**Each `--exec` stage runs in isolation.** Local variables declared with `let` do NOT persist across stages:

```bash
# ❌ WRONG - ctx is lost in the second stage
kelora -j app.log \
    -e 'let ctx = e.user_id' \
    -e 'e.context = ctx'  # ERROR: ctx doesn't exist here!

# ✅ CORRECT - Use semicolons within one stage
kelora -j app.log \
    -e 'let ctx = e.user_id; e.context = ctx'
```

**What DOES persist between stages:**

- ✅ **Event fields** - Modifications to `e.field` carry forward
- ✅ **`conf` map** - Initialized in `--begin`, read-only afterward
- ✅ **`metrics` map** - Populated by `track_*()`, accessed in `--end`
- ✅ **`window` array** - When `--window` is enabled

**What DOES NOT persist:**

- ❌ **Local variables** (`let x = ...`) - Scoped to the stage only
- ❌ **Function definitions** - Unless loaded via `--include`

**When to use multiple stages vs semicolons:**

Use **semicolons within one `-e`** when you need shared local variables:
```bash
kelora -j app.log \
    -e 'let total = e.count + e.bonus; e.result = total * 2'
```

Use **multiple `-e` stages** for stage snapshotting (see below).

### 5. Resilient Mode and Stage Snapshotting

In resilient mode (`--resilient`), Kelora creates a **snapshot after each successful stage**. If a later stage fails, the event **rolls back to the last successful snapshot**:

```bash
# With --resilient, events that fail in later stages
# are restored to their last successful state
kelora -j app.log --resilient \
    -e 'e.step1 = "done"' \           # Snapshot 1
    -e 'e.step2 = risky_operation(e)' \ # Might fail
    -e 'e.step3 = "done"'             # If step2 fails, event has step1 but not step2
```

**Why use multiple stages:**

- ✅ **Error isolation** - Failures don't corrupt earlier transformations
- ✅ **Progressive validation** - Each stage is a checkpoint
- ✅ **Debugging** - Easier to identify which stage failed

**Example - Risky enrichment with fallback:**
```bash
kelora -j app.log --resilient \
    -e 'e.safe_field = "processed"' \      # Always succeeds
    -e 'e.risky = parse_json(e.raw_data)' \ # Might fail for some events
    -e 'track_count(e.risky.status)' \     # Only runs if risky succeeded
    -k safe_field,risky
```

Events that fail the risky parse still have `safe_field` and are included in output (unless filtered out).

**Trade-off:**

- **Multiple stages**: Better error handling, snapshotting, but variables don't persist
- **Single stage with semicolons**: Shared variables, but all-or-nothing execution

---

## Common Patterns

### Pattern 1: Lookup Table Enrichment

```bash
kelora -j app.log \
    --begin 'conf.lookup = #{key1: "value1", key2: "value2"}' \
    --exec 'e.extra = conf.lookup.get(e.key, "unknown")'
```

### Pattern 2: Progressive Filtering

```bash
kelora -j app.log \
    --filter 'e.service == "api"' \      # Narrow to API
    --exec 'e.slow = e.duration > 1000' \  # Compute flag
    --filter 'e.slow' \                    # Keep slow ones
    --exec 'track_count(e.endpoint)'       # Track them
```

### Pattern 3: Summary Report

```bash
kelora -j app.log \
    --exec 'track_count(e.status)' \
    --end 'print("Total requests: " + metrics.values().sum())' \
    -F none --metrics
```

### Pattern 4: Conditional Enrichment

```bash
kelora -j app.log \
    --begin 'conf.vip_users = read_lines("vip.txt")' \
    --exec 'e.vip = e.user_id in conf.vip_users' \
    --filter 'e.vip' \
    --exec 'e.priority = "high"'
```

---

## Practice Exercises

### Exercise 1: Load and Count by Team

Load service metadata and count events by team:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/simple_json.jsonl \
    --begin 'conf.services = #{
                api: #{team: "backend"},
                database: #{team: "data"},
                auth: #{team: "security"}
            }' \
    --exec 'if conf.services.contains(e.service) {
                e.team = conf.services[e.service].team
            }' \
    --exec 'track_count(e.team)' \
    -F none --metrics
```
</details>

### Exercise 2: Filter by Criticality Threshold

Keep only events from critical services:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/simple_json.jsonl \
    --begin 'conf.critical_services = ["database", "auth"]' \
    --filter 'e.service in conf.critical_services' \
    -k service,level,message
```
</details>

### Exercise 3: Report Error Rate

Calculate and report the error rate:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/simple_json.jsonl \
    --exec 'track_count("total");
            if e.level == "ERROR" { track_count("errors") }' \
    --end 'let total = metrics.get("total", 0);
           let errors = metrics.get("errors", 0);
           let rate = if total > 0 { (errors.to_float() / total.to_float() * 100.0) } else { 0.0 };
           print("Error rate: " + rate + "%")' \
    -F none
```
</details>

---

## Debugging Tips

### Check What's in conf

```bash
kelora -j app.log \
    --begin 'conf.test = "hello"' \
    --exec 'print("conf keys: " + conf.keys())' \
    --take 1
```

### Inspect Metrics in --end

```bash
kelora -j app.log \
    --exec 'track_count(e.service)' \
    --end 'print("metrics keys: " + metrics.keys()); print("metrics: " + metrics)' \
    -F none
```

### Use --verbose for Errors

```bash
kelora -j app.log \
    --begin 'conf.bad = bad_function()' \
    --verbose
```

---

## Summary

You've learned:

- ✅ The four pipeline stages: `--begin` → (filter/exec)* → `--end`
- ✅ Initialize state with `--begin` and the `conf` map
- ✅ Load data with `read_lines()` and `read_file()`, or define inline
- ✅ Enrich events using lookup tables
- ✅ Build multi-stage pipelines with progressive filtering
- ✅ Generate reports in `--end` using metrics
- ✅ Understand when to use each stage
- ✅ Important rules: `conf` is read-only after `--begin`, order matters

## Next Steps

Now that you understand the complete pipeline lifecycle, continue to:

- **[Metrics and Tracking](metrics-and-tracking.md)** - Deep dive into `track_*()` functions
- **[Advanced Scripting](advanced-scripting.md)** - Advanced transformation patterns
- **[Working with Time](working-with-time.md)** - Time-based filtering and operations

**Related guides:**

- [How-To: Build a Service Health Snapshot](../how-to/monitor-application-health.md)
- [How-To: Design Streaming Alerts](../how-to/build-streaming-alerts.md)
- [Concepts: Scripting Stages](../concepts/scripting-stages.md) - Deep technical details
