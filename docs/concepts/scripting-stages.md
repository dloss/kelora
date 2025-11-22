# Scripting Stages

Deep dive into Kelora's Rhai scripting stages: `--begin`, `--filter`, `--exec`, and `--end`.

## Overview

Kelora provides four scripting stages for transforming log data with Rhai scripts:

```
--begin  →  [Process Events]  →  --end
              ↓
           --filter + --exec (per event, in CLI order)
```

| Stage | Runs | Purpose | Access |
|-------|------|---------|--------|
| `--begin` | Once before processing | Initialize state, load data | `conf` map, file helpers |
| `--filter` | Once per event | Select events to keep/skip | `e` (event), `conf`, `meta` |
| `--exec` | Once per event | Transform events | `e` (event), `conf`, `meta`, tracking |
| `--end` | Once after processing | Summarize, report | `metrics`, `conf` |

**Note:** `--filter` and `--exec` can be specified multiple times and execute in exact CLI order.

## Begin Stage

### Purpose

The `--begin` stage runs **once** before any events are processed. Use it to:

- Initialize lookup tables
- Load reference data from files
- Set up shared configuration
- Prepare the `conf` map for use in other stages

### The `conf` Map

The global `conf` map is **read-write** in `--begin` and **read-only** in later stages.

```bash
kelora -j \
    --begin 'conf.valid_users = ["alice", "bob", "charlie"]' \
    --exec 'e.is_valid = e.user in conf.valid_users' \
    app.log
```

### Available Helpers

Special functions available **only** in `--begin`:

#### `read_lines(path)`

Read file as array of strings (one per line, UTF-8).

```bash
kelora -j \
    --begin 'conf.blocked_ips = read_lines("blocked.txt")' \
    --exec 'if e.ip in conf.blocked_ips { e = () }' \
    app.log
```

#### `read_file(path)`

Read entire file as single string (UTF-8).

```bash
kelora -j \
    --begin 'conf.template = read_file("template.txt")' \
    --end 'print(conf.template.replace("{count}", metrics["total"].to_string()))' \
    app.log
```

**Note:** To parse JSON files, use `read_file(path)` and then call `.parse_json()` on the result, or define data structures inline using Rhai's map syntax `#{ key: value }`.

### Examples

#### Load Lookup Table

```bash
kelora -j \
    --begin 'conf.services = #{api: "API Gateway", db: "Database", cache: "Redis"}' \
    --exec 'e.service_name = conf.services.get(e.service, e.service)' \
    app.log
```

#### Load IP Geolocation Data

```bash
kelora -j \
    --begin 'conf.ip_to_country = #{
                "192.168.1.1": "US",
                "10.0.0.1": "UK",
                "172.16.0.1": "DE"
            }' \
    --exec 'e.country = conf.ip_to_country.get(e.ip, "unknown")' \
    app.log
```

#### Initialize Counters

```bash
kelora -j \
    --begin 'conf.start_time = now()' \
    --end 'let duration = now() - conf.start_time; print("Processed in " + duration + "s")' \
    app.log
```

#### Load Configuration

```bash
kelora -j \
    --begin 'conf.threshold = 1000; conf.alert_email = "ops@company.com"' \
    --exec 'if e.duration_ms > conf.threshold { eprint("⚠️  Slow request: " + e.path) }' \
    app.log
```

## Filter Stage

### Purpose

The `--filter` stage runs **once per event** to decide whether to keep or skip it. Use it to:

- Select events matching specific criteria
- Remove unwanted events (debug logs, health checks, etc.)
- Combine multiple filter conditions in sequence
- Control which events reach later `--exec` stages

### Boolean Expressions

Filters must return `true` (keep event) or `false` (skip event):

```bash
kelora -j \
    --filter 'e.level == "ERROR"' \
    app.log
```

**Behavior:**

- Returns `true` → Event passes to next stage
- Returns `false` → Event is skipped (removed from pipeline)
- Error in resilient mode → Treated as `false`, event skipped
- Error in strict mode → Processing aborts

### Access to Event Data

Filters have access to:

- `e` - The current event (read-only)
- `conf` - Configuration from `--begin` (read-only)
- `meta` - Event metadata (line, line_num, filename)

```bash
kelora -j \
    --begin 'conf.min_duration = 1000' \
    --filter 'e.duration_ms > conf.min_duration' \
    app.log
```

### Multiple Filters

Multiple `--filter` flags create an **AND** condition - events must pass all filters:

```bash
kelora -j \
    --filter 'e.level == "ERROR"' \
    --filter 'e.service == "api"' \
    --filter 'e.duration_ms > 1000' \
    app.log
```

Only events that are ERROR **AND** from api service **AND** slow will pass.

### Common Patterns

#### Basic Field Matching

```bash
kelora -j --filter 'e.status >= 400' app.log
kelora -j --filter 'e.user == "admin"' app.log
kelora -j --filter 'e.service in ["api", "db"]' app.log
```

#### String Operations

```bash
kelora -j --filter 'e.message.contains("timeout")' app.log
kelora -j --filter 'e.path.starts_with("/api/")' app.log
kelora -j --filter 'e.level.to_upper() == "ERROR"' app.log
```

#### Regex Matching

```bash
kelora -j --filter 'e.message.has_matches(r"\d{3}-\d{3}-\d{4}")' app.log
kelora -j --filter 'e.ip.has_matches(r"^192\.168\.")' app.log
```

#### Existence Checks

```bash
kelora -j --filter 'e.contains("error")' app.log       # Has 'error' field
kelora -j --filter '"error" in e' app.log              # Same as above
kelora -j --filter 'e.contains("user_id")' app.log     # Has 'user_id' field
```

#### Complex Conditions

```bash
kelora -j \
    --filter '(e.status >= 500) || (e.status >= 400 && e.duration_ms > 5000)' \
    app.log
```

#### Using conf for Dynamic Filtering

```bash
kelora -j \
    --begin 'conf.blocked_ips = ["192.168.1.100", "10.0.0.50"]' \
    --filter '!(e.ip in conf.blocked_ips)' \
    app.log
```

#### Filtering with Metadata

```bash
# Only process events from specific file
kelora -j *.log --filter 'meta.filename == "production.log"'

# Skip first 100 lines
kelora -j app.log --filter 'meta.line_num > 100'
```

### Filter vs --levels

For simple level filtering, `--levels` is more efficient than `--filter`:

**Prefer:**
```bash
kelora -j app.log --levels error,warn
```

**Over:**
```bash
kelora -j app.log --filter 'e.level in ["ERROR", "WARN"]'
```

However, `--filter` provides more flexibility for complex conditions.

Level flags behave like script stages: you can place `--levels` or `--exclude-levels` anywhere in the CLI sequence, and Kelora runs them immediately at that point. Repeat the flag when you want different level rules before and after a transformation (for example, derive a level in `--exec`, then add another `--levels` to act on the new field).

### Filter Output

Filters don't modify events - they only decide pass/skip:

```bash
# This works - filter just checks level
kelora -j --filter 'e.level == "ERROR"' app.log

# This does nothing - assignment in filter has no effect
kelora -j --filter 'e.level = "ERROR"' app.log  # Wrong!
```

To modify events, use `--exec` instead.

## Exec Stage

### Purpose

The `--exec` stage runs **once per event**. Use it to:

- Transform event fields
- Add computed fields
- Filter events (via `e = ()`)
- Track metrics
- Emit multiple events from arrays

### The Event Variable

The current event is available as `e`. Modifications to `e` persist through subsequent `--exec` scripts.

```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000' \
    --exec 'e.is_slow = e.duration_s > 1.0' \
    app.log
```

### Multiple Exec Scripts

Multiple `--exec` scripts run in order. Each sees changes from previous scripts.

```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000' \
    --exec 'track_avg("duration", e.duration_s)' \
    --exec 'if e.duration_s > 5.0 { e.alert = true }' \
    app.log
```

**Execution order:**

1. Convert `duration_ms` to `duration_s`
2. Track average duration
3. Add `alert` field for slow requests

### Variable Scope Between Exec Stages

**Important:** Each `--exec` stage has its own isolated scope. Local variables declared with `let` do **NOT** persist between stages:

```bash
# ❌ WRONG - ctx doesn't exist in the second stage
kelora -j \
    --exec 'let ctx = e.user_id' \
    --exec 'e.context = ctx' \  # ERROR: ctx undefined!
    app.log
```

**What persists between stages:**

- ✅ **Event fields** (`e.field = value`) - Modifications carry forward
- ✅ **`conf` map** - Initialized in `--begin`, read-only in exec stages
- ✅ **`metrics` map** - Populated by `track_*()` functions
- ✅ **`window` array** - When using `--window`

**What does NOT persist:**

- ❌ **Local variables** (`let x = ...`) - Scoped to the stage only
- ❌ **Function definitions** - Unless loaded via `--include`

**Solution - Use semicolons for shared variables:**

When you need local variables to persist across operations, use semicolons within a single `--exec`:

```bash
# ✅ CORRECT - Both operations in one stage
kelora -j \
    --exec 'let ctx = e.user_id; e.context = ctx' \
    app.log
```

**When to use multiple stages vs semicolons:**

Use **multiple `-e` stages** when you want:

- Stage-level error isolation (see resilient mode below)
- Logical separation of transformation steps
- Progressive validation checkpoints

Use **semicolons within one `-e`** when you need:

- Shared local variables across operations
- All-or-nothing execution (no partial results)

### Intermixing --filter and --exec

`--filter` and `--exec` are both script stages and execute in **exact CLI order**:

```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000' \    # Stage 1: Transform all events
    --filter 'e.duration_s > 1.0' \                    # Stage 2: Keep only slow events
    --exec 'track_count("slow")' \                     # Stage 3: Track (slow only)
    --exec 'e.alert = true' \                          # Stage 4: Add field (slow only)
    app.log
```

**What happens:**

1. First `--exec` adds `duration_s` field to all events
2. `--filter` removes events under 1.0s
3. Second `--exec` only processes slow events (tracks count)
4. Third `--exec` only processes slow events (adds alert field)

Later stages only see events that passed earlier filters. This allows precise control over which events are transformed or tracked.

### Atomic Execution

In resilient mode (default), exec scripts execute **atomically**:

- If an error occurs, changes are rolled back
- Original event is returned unchanged
- Processing continues with next event

```bash
kelora -j \
    --exec 'e.result = e.value.to_int() * 2' \
    app.log
```

If `e.value` is not a valid integer:

- Error is recorded
- Event passes through unchanged
- No partial modifications

In strict mode (`--strict`), errors abort immediately.

### Resilient Mode and Stage Snapshotting

Resilient mode creates **snapshots after each successful stage**. If a later stage fails, the event **reverts to the last successful snapshot**:

```bash
kelora -j --resilient \
    --exec 'e.step1 = "processed"' \        # Snapshot 1
    --exec 'e.step2 = risky_parse(e.raw)' \ # Might fail
    --exec 'e.step3 = "complete"' \         # Snapshot 3 (if step2 succeeds)
    app.log
```

**Behavior on error:**

- If `step2` fails for an event, it **keeps `step1` but not `step2`**
- The event continues through the pipeline with fields from the last successful stage
- Later stages see the rolled-back event

**Why use multiple stages for error handling:**

```bash
# Multiple stages - Graceful degradation
kelora -j --resilient \
    --exec 'e.safe = "always_set"' \        # Always succeeds
    --exec 'e.parsed = parse_json(e.raw)' \ # Might fail for some events
    --exec 'track_count(e.parsed.type)' \   # Only runs if parsing succeeded
    -k safe,parsed
# Events that fail parsing still have 'safe' field and appear in output

# Single stage - All-or-nothing
kelora -j --resilient \
    --exec 'e.safe = "always_set"; e.parsed = parse_json(e.raw); track_count(e.parsed.type)' \
    -k safe,parsed
# If parsing fails, the ENTIRE exec fails - no 'safe' field, event unchanged
```

**Design pattern - Progressive risk:**

Place risky operations in later stages so earlier transformations survive:

```bash
kelora -j --resilient \
    --exec 'e.normalized = e.status.to_string()' \  # Safe normalization
    --exec 'e.enriched = lookup_external(e.id)' \   # Risky external call
    --exec 'e.computed = e.enriched.calculate()' \  # Depends on risky data
    app.log
```

Events that fail external lookup still get `normalized` field.

**Trade-off:**

- **Multiple stages**: Better error isolation, partial success, but local variables don't persist
- **Single stage with semicolons**: Shared variables, but all-or-nothing execution

### Common Patterns

#### Transform Fields

```bash
kelora -j \
    --exec 'e.level = e.level.to_upper()' \
    --exec 'e.message = e.message.trim()' \
    app.log
```

#### Add Computed Fields

```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000' \
    --exec 'e.timestamp_unix = e.timestamp.to_unix()' \
    app.log
```

#### Conditional Field Creation

```bash
kelora -j \
    --exec 'if e.status >= 500 { e.severity = "critical" } else if e.status >= 400 { e.severity = "error" }' \
    app.log
```

#### Remove Events

```bash
kelora -j \
    --exec 'if e.level == "DEBUG" { e = () }' \
    app.log
```

#### Track Metrics

```bash
kelora -j \
    --exec 'track_count(e.service)' \
    --exec 'track_avg("response_time", e.duration_ms)' \
    --metrics \
    app.log
```

#### Fan-Out Arrays

```bash
kelora -j \
    --exec 'emit_each(e.items)' \
    app.log
```

Each array element becomes a separate event.

### Access to conf

The `conf` map from `--begin` is **read-only** in `--exec`:

```bash
kelora -j \
    --begin 'conf.multiplier = 2.5' \
    --exec 'e.adjusted = e.value * conf.multiplier' \
    app.log
```

### Access to meta

The `meta` variable provides event metadata in `--exec` and `--filter`:

```bash
kelora -j \
    --exec 'e.source = meta.filename' \
    server1.log server2.log
```

**Available metadata attributes:**

- `meta.line` - Original raw line from input (always available)
- `meta.line_num` - Line number, 1-based (available when processing files)
- `meta.filename` - Source filename (available with multiple files or explicit file arguments)

**Multi-file tracking example:**

```bash
kelora -j logs/*.log --metrics \
    --exec 'if e.level == "ERROR" { track_count(meta.filename) }' \
    --end 'for file in metrics.keys() { print(file + ": " + metrics[file] + " errors") }'
```

**Debugging with line numbers:**

```bash
kelora -j --filter 'e.status >= 500' \
    --exec 'eprint("⚠️  Server error at " + meta.filename + ":" + meta.line_num)' \
    app.log
```

**Re-parsing with raw line:**

```bash
kelora -j \
    --exec 'if e.message.contains("CUSTOM:") { e.custom = meta.line.after("CUSTOM:").parse_json() }' \
    app.log
```

## End Stage

### Purpose

The `--end` stage runs **once** after all events are processed. Use it to:

- Summarize metrics
- Generate reports
- Print final statistics
- Export aggregated data

### The metrics Map

The global `metrics` map contains all tracked data from `track_*()` functions:

```bash
kelora -j \
    --exec 'track_count(e.service)' \
    --end 'for key in metrics.keys() { print(key + ": " + metrics[key]) }' \
    app.log
```

### Available Data

In `--end`, you have access to:

- `metrics` - All tracked metrics (counts, sums, averages, etc.)
- `conf` - Read-only configuration from `--begin`
- Standard Rhai functions (print, file helpers if `--allow-fs-writes`)

### Examples

#### Print Summary Statistics

```bash
kelora -j \
    --exec 'track_count("total"); if e.level == "ERROR" { track_count("errors") }' \
    --end 'let error_rate = metrics.errors / metrics.total * 100; print("Error rate: " + error_rate + "%")' \
    app.log
```

#### Generate Report

```bash
kelora -j \
    --exec 'track_count(e.service)' \
    --end 'print("=== Service Report ==="); for svc in metrics.keys() { print(svc + ": " + metrics[svc] + " requests") }' \
    app.log
```

#### Export Metrics to File

```bash
kelora -j --allow-fs-writes \
    --exec 'track_count(e.service)' \
    --end 'append_file("report.txt", "Total services: " + metrics.len().to_string())' \
    app.log
```

#### Calculate Percentages

```bash
kelora -j \
    --exec 'track_count("total"); track_count(e.level)' \
    --end 'for level in ["INFO", "WARN", "ERROR"] { let pct = metrics.get(level, 0) / metrics.total * 100; print(level + ": " + pct + "%") }' \
    app.log
```

## Stage Interaction

### Data Flow Between Stages

```
--begin:  Initialize conf map
    ↓
    conf (read-only)
    ↓
--exec:   Process events, track metrics
    ↓
    metrics + conf (both read-only)
    ↓
--end:    Summarize and report
```

### Complete Example

```bash
kelora -j \
    --begin 'conf.threshold = 1000; conf.start = now()' \
    --exec 'if e.duration_ms > conf.threshold { track_count("slow") }' \
    --exec 'track_count("total")' \
    --end 'let elapsed = now() - conf.start; print("Processed " + metrics.total + " events in " + elapsed + "s"); print("Slow requests: " + metrics.get("slow", 0))' \
    app.log
```

**Flow:**

1. `--begin`: Set threshold to 1000ms, record start time
2. `--exec` (per event): Track slow requests, track total
3. `--end`: Calculate elapsed time, print summary

## Using Exec Files

### `-E, --exec-file`

Load Rhai script from file for the exec stage:

**transform.rhai:**
```rhai
// Convert duration to seconds
e.duration_s = e.duration_ms / 1000;

// Add severity based on status
if e.status >= 500 {
    e.severity = "critical";
} else if e.status >= 400 {
    e.severity = "error";
} else {
    e.severity = "ok";
}

// Track metrics
track_count(e.severity);
track_avg("response_time", e.duration_s);
```

**Usage:**
```bash
kelora -j -E transform.rhai --metrics app.log
```

### `-I, --include`

Include Rhai library files before script stages:

**helpers.rhai:**
```rhai
fn classify_status(status) {
    if status >= 500 {
        "server_error"
    } else if status >= 400 {
        "client_error"
    } else if status >= 300 {
        "redirect"
    } else if status >= 200 {
        "success"
    } else {
        "other"
    }
}
```

**Usage:**
```bash
kelora -j \
    -I helpers.rhai \
    --exec 'e.status_class = classify_status(e.status)' \
    app.log
```

## Best Practices

### Use --begin for Initialization

**Good:**
```bash
kelora -j \
    --begin 'conf.lookup = read_json("data.json")' \
    --exec 'e.name = conf.lookup.get(e.id, "unknown")' \
    app.log
```

**Bad:**
```bash
kelora -j \
    --exec 'let lookup = read_json("data.json"); e.name = lookup.get(e.id, "unknown")' \
    app.log
```

The bad example reads the file **once per event** (slow and wasteful).

### Keep --exec Scripts Simple

Break complex logic into multiple `--exec` scripts:

**Good:**
```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000' \
    --exec 'e.is_slow = e.duration_s > 1.0' \
    --exec 'if e.is_slow { track_count("slow_requests") }' \
    app.log
```

**Bad:**
```bash
kelora -j \
    --exec 'e.duration_s = e.duration_ms / 1000; e.is_slow = e.duration_s > 1.0; if e.is_slow { track_count("slow_requests") }' \
    app.log
```

The good example is easier to read and debug.

### Use --end for Summaries

**Good:**
```bash
kelora -j \
    --exec 'track_count(e.service)' \
    --end 'print("Total services: " + metrics.len())' \
    app.log
```

**Bad:**
```bash
kelora -j \
    --exec 'track_count(e.service); print("Processing...")' \
    app.log
```

The bad example prints on every event (noisy).

### Leverage File Helpers

For complex logic, use `-E` and `-I`:

```bash
kelora -j -I helpers.rhai -E transform.rhai --metrics app.log
```

This keeps command lines clean and logic maintainable.

## Performance Considerations

### Begin Stage Overhead

The `--begin` stage runs once, so file I/O here is acceptable:

```bash
kelora -j \
    --begin 'conf.large_dataset = read_json("10mb.json")' \
    --exec 'e.enriched = conf.large_dataset.get(e.id, #{})' \
    app.log
```

### Exec Stage Optimization

The `--exec` stage runs per event. Avoid expensive operations:

**Slow:**
```bash
kelora -j \
    --exec 'let lookup = read_json("data.json"); e.name = lookup.get(e.id, "unknown")' \
    app.log
```

**Fast:**
```bash
kelora -j \
    --begin 'conf.lookup = read_json("data.json")' \
    --exec 'e.name = conf.lookup.get(e.id, "unknown")' \
    app.log
```

### End Stage Overhead

The `--end` stage runs once, so complex calculations are fine:

```bash
kelora -j \
    --exec 'track_count(e.service)' \
    --end 'let sorted = metrics.keys().sort(); for key in sorted { print(key + ": " + metrics[key]) }' \
    app.log
```

## Parallel Processing

When using `--parallel`, scripting stages behave differently:

### Begin and End

`--begin` and `--end` run **once** (not parallelized):

```bash
kelora -j --parallel \
    --begin 'conf.start = now()' \
    --exec 'track_count(e.service)' \
    --end 'print("Duration: " + (now() - conf.start))' \
    app.log
```

### Exec Stage

`--exec` runs in parallel across worker threads:

- Each thread has its own copy of `conf` (read-only)
- Tracking functions aggregate across threads
- Event modifications are isolated per thread

```bash
kelora -j --parallel \
    --exec 'e.duration_s = e.duration_ms / 1000' \
    --exec 'track_count(e.service)' \
    app.log
```

### Thread Safety

Kelora handles thread safety automatically:

- `conf` is cloned per thread (immutable)
- `metrics` uses thread-safe aggregation
- Event modifications are isolated

You don't need to worry about race conditions in scripts.

## Troubleshooting

### conf is Read-Only in --exec

**Problem:**
```bash
kelora -j --exec 'conf.value = 42' app.log
# Error: conf is read-only in exec stage
```

**Solution:** Initialize in `--begin`:
```bash
kelora -j --begin 'conf.value = 42' --exec 'e.result = conf.value * 2' app.log
```

### metrics Not Available in --exec

**Problem:**
```bash
kelora -j --exec 'print(metrics["total"])' app.log
# Error: metrics not available in exec stage
```

**Solution:** Use `--end`:
```bash
kelora -j --exec 'track_count("total")' --end 'print(metrics["total"])' app.log
```

### File Helpers Not Working

**Problem:**
```bash
kelora -j --exec 'append_file("out.txt", e.message)' app.log
# Error: filesystem writes not allowed
```

**Solution:** Add `--allow-fs-writes`:
```bash
kelora -j --allow-fs-writes --exec 'append_file("out.txt", e.message)' app.log
```

### Script Stage Ordering

**Understanding:** `--filter` and `--exec` execute in **exact CLI order**, intermixed.

```bash
# Both --exec scripts run on all events
kelora -j --exec 'e.a = 1' --exec 'e.b = e.a + 1' app.log

# Filter runs between execs - second --exec only sees filtered events
kelora -j --exec 'e.a = 1' --filter 'e.level == "ERROR"' --exec 'e.b = e.a + 1' app.log
```

**What happens in the second example:**

1. First `--exec` adds field `a=1` to all events
2. `--filter` removes non-ERROR events
3. Second `--exec` adds field `b=2` to ERROR events only

This is **expected behavior** - later stages only process events that passed earlier filters. Each stage (filter or exec) processes the output of the previous stage sequentially.

## See Also

- [Processing Architecture](pipeline-model.md) - Three-layer processing model and script stage ordering
- [Events and Fields](events-and-fields.md) - Working with event data
- [Function Reference](../reference/functions.md) - All available Rhai functions
- [CLI Reference](../reference/cli-reference.md) - Complete flag documentation
