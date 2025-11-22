# Introduction to Rhai Scripting

Learn how to write simple Rhai scripts to filter and transform log events. This tutorial bridges the gap between basic CLI usage and advanced scripting.

## What You'll Learn

- Understand the event object (`e`) and field access
- Write simple filter expressions with `--filter`
- Transform events with basic `-e` scripts
- Use string operations and conditionals
- Convert between types safely
- Understand why pipeline order matters
- Debug scripts with `-F inspect` and `--verbose`

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Basic CLI usage
- **Time:** ~20 minutes

## Sample Data

This tutorial uses `examples/basics.jsonl` - the same small JSON log file from the basics tutorial:

```bash exec="on" result="ansi"
cat examples/basics.jsonl
```

---

## Step 1: Understanding the Event Object

Every event in Kelora is represented as a map (dictionary) accessible via the variable `e` in your Rhai scripts.

**Accessing fields:**

```rhai
e.level          # Direct field access
e["level"]       # Bracket notation (useful for dynamic fields)
e.status         # Access any field in the event
```

Let's see the structure with `-F inspect`:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -F inspect
```

**Key insight:** Field names become properties you can access in scripts.

---

## Step 2: Simple Filter Expressions

Use `--filter` to keep only events where the expression returns `true`.

### Filter by String Equality

Keep only ERROR level events:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl --filter 'e.level == "ERROR"'
```

**What happened:** Only events where `e.level` equals `"ERROR"` are kept.

### Filter by Numeric Comparison

Keep only slow queries (duration > 1000ms):

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl --filter 'e.duration_ms > 1000'
```

**Important:** This only keeps events that **have** a `duration_ms` field. Events without it are skipped.

### Combine Conditions with Logical Operators

Find ERROR or WARN events from the database service:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    --filter 'e.level in ["ERROR", "WARN"] && e.service == "database"'
```

**Operators:**

- `==` - Equals
- `!=` - Not equals
- `>`, `>=`, `<`, `<=` - Comparison
- `&&` - AND
- `||` - OR
- `!` - NOT
- `in` - Check membership in array (e.g., `e.level in ["ERROR", "WARN"]`)

---

## Step 3: Basic Transformations with --exec

Use `--exec` (or `-e` for short) to modify events or add new fields. We'll use `-e` in all examples below.

### Add a Computed Field

Convert milliseconds to seconds:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.duration_s = e.duration_ms / 1000' \
    --filter 'e.duration_s > 1.0' \
    -k timestamp,service,duration_ms,duration_s
```

**Key insight:** `--exec` runs **before** `--filter`, so the new field is available for filtering.

### Modify Existing Fields

Normalize level to uppercase:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.level = e.level.to_upper()'
```

---

## Step 4: String Operations

Rhai provides powerful string methods for text processing.

### Check if String Contains Text

Find events with "timeout" in the message:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    --filter 'e.message.contains("timeout")'
```

### Extract Parts of Strings

Extract just the error type from messages:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    --filter 'e.level == "ERROR"' \
    -e 'e.error_type = e.message.split(" ")[0]' \
    -k timestamp,service,error_type,message
```

**Common string methods:**

- `contains(substr)` - Check if string contains text
- `starts_with(prefix)` - Check prefix
- `ends_with(suffix)` - Check suffix
- `split(sep)` - Split into array
- `to_upper()` / `to_lower()` - Change case
- `trim()` - Remove whitespace
- `len()` - String length

---

## Step 5: Conditionals and Logic

Use `if/else` to make decisions in your transforms.

### Add Severity Classification

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.severity = if e.level in ["ERROR", "CRITICAL"] { "high" } else if e.level == "WARN" { "medium" } else { "low" }' \
    -k level,severity,service,message
```

**Syntax:**
```rhai
if condition {
    // then branch
} else if another_condition {
    // else-if branch
} else {
    // else branch
}
```

---

## Step 6: Type Conversions

Fields may be strings when you need numbers (or vice versa). Convert types safely.

### Safe Conversion with Fallbacks

Use `to_int_or()` to handle conversion failures:

```bash exec="on" source="above" result="ansi"
echo '{"id":"123","status":"200","invalid":"abc"}
{"id":"456","status":"404","invalid":"xyz"}' | \
    kelora -j \
    -e 'e.id_num = e.id.to_int_or(-1);
        e.status_num = e.status.to_int_or(0);
        e.invalid_num = e.invalid.to_int_or(999)' \
    -k id,id_num,status,status_num,invalid,invalid_num
```

**Note:** Multiple statements in one `-e` are separated by semicolons and share the same scope. Use this when operations are related or when you need to share `let` variables.

**Safe conversion functions:**

- `to_int_or(fallback)` - Convert to integer or use fallback
- `to_float_or(fallback)` - Convert to float or use fallback
- `to_string()` - Convert to string (always succeeds)

---

## Step 7: Pipeline Order Matters

The order of `--filter` and `-e` flags determines execution order.

### Wrong Order: Filter Before Creating Field

This **won't work** because `duration_s` doesn't exist yet:

```bash
# WRONG - will fail
kelora -j examples/basics.jsonl \
    --filter 'e.duration_s > 1.0' \
    -e 'e.duration_s = e.duration_ms / 1000'
```

### Correct Order: Create Field Before Filtering

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.duration_s = e.duration_ms / 1000' \
    --filter 'e.duration_s > 1.0' \
    -k service,duration_s,message
```

**Rule:** Fields must exist before you filter on them. Scripts run in CLI order.

---

## Step 8: Checking if Fields Exist

Not all events have the same fields. Use `has()` to check before accessing.

### Safe Field Access

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.slow = if e.has("duration_ms") { e.duration_ms > 1000 } else { false }' \
    -k service,slow,message
```

**Pattern:**
```rhai
if e.has("field_name") {
    // Safe to access e.field_name
} else {
    // Provide default behavior
}
```

---

## Step 9: Debugging Your Scripts

When scripts don't work as expected, use these techniques.

### Use -F inspect to See Field Types

When a filter isn't working as expected, use `-F inspect` to see what fields exist and their types:

```bash exec="on" source="above" result="ansi"
echo '{"id":"42","count":"100"}
{"id":99,"count":200}' | kelora -j -F inspect
```

**Output shows:** Field name, type (string/int/etc.), and value. Notice how `id` and `count` are strings in the first event but integers in the second - this explains why `count > 50` would fail on the first event!

### Use --verbose to See Errors

When scripts fail in resilient mode, use `--verbose` to see what went wrong:

```bash exec="on" source="above" result="ansi"
echo '{"value":"not_a_number"}' | \
    kelora -j \
    -e 'e.num = e.value.to_int()' \
    --verbose
```

**Debug workflow:**

1. Use `-F inspect` to check field types
2. Use `--verbose` to see error messages
3. Use `--strict` to fail fast on first error
4. Add temporary fields to see intermediate values

---

## Step 10: Multi-Stage Pipelines

Chain multiple `-e` and `--filter` stages for complex logic.

### Progressive Refinement

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl \
    -e 'e.is_error = e.level in ["ERROR", "CRITICAL"];
        e.is_slow = e.has("duration_ms") && e.duration_ms > 1000;
        e.needs_attention = e.is_error || e.is_slow' \
    --filter 'e.needs_attention' \
    -k service,level,is_error,is_slow,message
```

**Pattern:** Build up computed fields step-by-step, then filter on the final result.

---

## Next Steps

For complete Rhai syntax reference, see the **[Rhai Cheatsheet](../reference/rhai-cheatsheet.md)**.

For all built-in functions: `kelora --help-functions`

---

## Practice Exercises

Try these on your own:

### Exercise 1: Filter by Service
Filter for events from the `database` service:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/basics.jsonl --filter 'e.service == "database"'
```
</details>

### Exercise 2: Flag High Memory Usage
Add a `high_memory` field and filter for events with memory usage above 80%:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/basics.jsonl \
    -e 'e.high_memory = e.has("memory_percent") && e.memory_percent > 80' \
    --filter 'e.high_memory' \
    -k service,memory_percent,message
```
</details>

### Exercise 3: Flag Critical Security Events
Add a `critical` field that's true for ERROR events with failed login attempts:

<details>
<summary>Solution</summary>

```bash
kelora -j examples/basics.jsonl \
    -e 'e.critical = e.level == "ERROR" && e.has("attempts")' \
    --filter 'e.critical' \
    -k service,message,attempts
```
</details>

---

## Summary

You've learned:

- Access event fields with `e.field_name`
- Filter events with `--filter` boolean expressions
- Transform events with `-e` scripts
- Use string methods like `.contains()`, `.split()`, `.to_upper()`
- Convert types safely with `to_int_or()`, `to_float_or()`
- Write conditionals with `if/else`
- Check field existence with `has()`
- Debug with `-F inspect` and `--verbose`
- Understand pipeline order (exec before filter)
- Build multi-stage pipelines

## Next Steps

Now that you understand basic Rhai scripting, continue to:

- **[Working with Time](working-with-time.md)** - Time filtering and timezone handling
- **[Metrics and Tracking](metrics-and-tracking.md)** - Aggregate data with `track_*()` functions
- **[Advanced Scripting](advanced-scripting.md)** - Advanced patterns and techniques

**Related guides:**

- [Function Reference](../reference/functions.md) - Complete function catalog
- [Rhai Cheatsheet](../reference/rhai-cheatsheet.md) - Quick syntax reference
- [How-To: Triage Production Errors](../how-to/find-errors-in-logs.md) - Practical examples
