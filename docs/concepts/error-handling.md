# Error Handling

Understanding Kelora's error handling modes and how to diagnose issues.

## Processing Modes

Kelora offers two error handling modes:

| Mode | Behavior | Use Case |
|------|----------|----------|
| **Resilient (default)** | Skip errors, continue processing | Production log analysis, exploratory work |
| **Strict (`--strict`)** | Fail-fast on errors | Data validation, CI/CD pipelines |

## Resilient Mode

### Overview

In resilient mode (default), Kelora continues processing even when errors occur:

- **Parse errors**: Skip unparseable lines, continue with next line
- **Filter errors**: Treat as `false`, skip event
- **Transform errors**: Return original event unchanged (atomic rollback)
- **Summary**: Show recovered runtime errors as warnings at end
- **Exit code**: `0` while the run still did its job — even with skipped lines or
  rolled-back transforms. It becomes `1` only when a *gate* never once succeeded
  (a `--filter` that errors on every event, or an input where no line parses), or
  on a structural / `--assert` failure. Transform (`--exec`) errors are
  best-effort and never fail the run on their own. See
  [Exit codes: the model](#exit-codes-the-model).

### When to Use

- Analyzing messy production logs
- Exploratory data analysis
- Real-time log streaming
- Mixed format log files

### Example Behavior

```bash
kelora -j app.log --exec 'e.result = e.value.to_int() * 2'
```

**Input:**
```json
{"value": "123"}
{"value": "invalid"}
{"value": "456"}
```

**Output:**
```
result=246
(skipped - error converting "invalid")
result=912
```

**Summary:**
```
🔹 Processed 3 lines, 2 events output, 1 error
```

### Error Recording

Errors are recorded but don't stop processing:

```bash
kelora -j app.log --filter 'e.timestamp.to_unix() > 1000000'
```

If `e.timestamp` is missing or invalid:

- Filter evaluates to `false`
- Event is skipped
- Error is recorded
- Processing continues
- Exit code stays `0` **as long as the filter succeeded on at least one event**.
  If the filter errors on *every* event (e.g. a field-name typo like `status`
  instead of `e.status`), it never matched anything — that is a broken command,
  not data noise, so the run exits `1`. See
  [Exit codes: the model](#exit-codes-the-model).

## Strict Mode

### Overview

In strict mode (`--strict`), Kelora fails immediately on the first error:

- **Parse errors**: Show error, abort immediately
- **Filter errors**: Show error, abort immediately
- **Transform errors**: Show error, abort immediately
- **Exit code**: Non-zero on any error

### When to Use

- Data validation pipelines
- CI/CD quality gates
- Critical processing where partial results aren't acceptable
- Debugging log parsing issues

### Example Behavior

```bash
kelora -j --strict app.log --exec 'e.result = e.value.to_int() * 2'
```

**Input:**
```json
{"value": "123"}
{"value": "invalid"}
{"value": "456"}
```

**Output:**
```
result=246
⚠️  kelora: line 2: exec error - cannot convert 'invalid' to integer
```

**Exit code:** `1`

Processing stops at the first error. Line 3 is never processed.

### Enabling Strict Mode

```bash
kelora -j --strict app.log
```

## Error Types

### Parse Errors

Occur when input lines can't be parsed in the specified format.

**JSON parse error:**
```bash
kelora -j app.log
```

**Input:**
```
{"valid": "json"}
{invalid json}
{"more": "valid"}
```

**Resilient behavior:**

- Line 1: Parsed successfully
- Line 2: Skipped (parse error recorded)
- Line 3: Parsed successfully

**Strict behavior:**

- Line 1: Parsed successfully
- Line 2: Error shown, processing aborts
- Line 3: Never processed

### Filter Errors

Occur when `--filter` expressions fail during evaluation.

**Example:**
```bash
kelora -j --filter 'e.timestamp.to_unix() > 1000000' app.log
```

If `e.timestamp` is missing:

**Resilient behavior:**

- Filter evaluates to `false`
- Event is skipped
- Error recorded

**Strict behavior:**

- Error shown immediately
- Processing aborts

### Transform Errors

Occur when `--exec` scripts fail during execution.

**Example:**
```bash
kelora -j --exec 'e.result = e.value.to_int()' app.log
```

If `e.value` is not a valid integer:

**Resilient behavior:**

- Transformation rolled back (atomic)
- Original event returned unchanged
- Error recorded
- **Exit code stays `0`** — exec is best-effort enrichment. Even an `--exec`
  that errors on *every* event is recovered (the original events still flow), so
  it never fails the run on its own. Use `--strict` (fail on the first error) or
  `--assert` (explicit gate) when a transform must succeed.

**Strict behavior:**

- Error shown immediately
- Processing aborts

## Verbose Error Reporting

### Default Error Reporting

By default, errors are collected and summarized at the end:

```bash
kelora -j app.log --exec 'e.result = e.value.to_int()'
```

**Summary:**
```
🔹 Processed 100 lines, 95 events output, 5 errors
```

### Verbose Mode (`--verbose`)

Show each error immediately as it occurs:

```bash
kelora -j --verbose app.log --exec 'e.result = e.value.to_int()'
```

**Output:**
```
⚠️  kelora: line 5: exec error - cannot convert 'abc' to integer
result=123
⚠️  kelora: line 12: exec error - cannot convert 'def' to integer
result=456
⚠️  kelora: line 23: exec error - field 'value' not found
result=789
```

**Summary:**
```
🔹 Processed 100 lines, 95 events output, 5 errors
```

### Multiple Verbosity Levels

```bash
-v      # Show errors immediately
-vv     # Show errors with more context
-vvv    # Show errors with full details
```

### Verbose with Strict

Combine for immediate errors and fail-fast:

```bash
kelora -j --strict --verbose app.log
```

Errors are shown immediately, then processing aborts.

## Quiet/Silent Controls

Use the new orthogonal toggles to control output for automation:

| Flag | Effect |
|------|--------|
| `-q` / `--quiet` | Suppress events (formatter output) |
| `--no-diagnostics` | Suppress diagnostics/summaries (fatal line still emitted) |
| `--silent` | Suppress pipeline terminal output (events/diagnostics/stats/terminal metrics); script output allowed unless combined with `--no-script-output` or data-only modes; emit one fatal line on errors; metrics files still write |
| `--no-script-output` | Suppress Rhai `print`/`eprint` (implied by data-only modes) |
| `-s` / `--stats=FORMAT` | Show stats only (implies `-q/--quiet`; also suppresses script output; diagnostics stay on). Format: table, json |
| `-m` / `--metrics=FORMAT` | Show metrics only (implies `-q/--quiet`; suppresses diagnostics except fatal line, stats, script output). Format: short, full (default), json |
| `--with-stats` | Show stats alongside events (rare case) |
| `--with-metrics` | Show metrics alongside events (rare case) |

Examples:

```bash
kelora -q -j app.log --with-stats                    # No events; stats emit
kelora --silent -j app.log && echo "Clean" || echo "Has errors"
kelora -m --metrics-file metrics.json app.log        # Metrics only
kelora --metrics=json app.log                        # Metrics in JSON format
```

## Exit codes: the model

One rule captures Kelora's exit-code behavior:

> **Kelora exits non-zero when it couldn't do the job you asked — not because the data was messy.**

That splits cleanly into three tiers:

| Tier | Exit | What it means | Examples |
|------|------|---------------|----------|
| **Recovered** | `0` | The run did its job. Individual records may have been skipped, rolled back, or left un-enriched, and they're reported as diagnostics. | A few unparseable lines among good ones; an `--exec` that errors on some (or even all) events |
| **Couldn't do the job** | `1` | A *gate* never once succeeded, a forbidden operation, or a structural / explicit-gate failure. | A `--filter` that errors on **every** event; an input where **no** line parses; mutating `conf` outside `--begin`; a named input that can't be opened; an `--assert` violation |
| **Invalid usage** | `2` | Bad command line — caught before processing. | Unknown flag, incompatible options, invalid config |

The key distinction is **gates vs. transforms**:

- **Gates — parse and filter — must work.** If a gate never once succeeds, the
  output is empty or meaningless: no line parsed, or a filter that errored on
  every event never actually selected anything (the dangerous case where "show
  me the errors" returns nothing and looks like success). That's a broken
  command, so it exits `1`. A gate that errors on only *some* records is data
  noise and is recovered.
- **Transforms — exec — are best-effort.** A failing `--exec` rolls back to the
  event as it was before that stage and emits it anyway, so the output stays
  valid even when the transform errors on every event. Exec errors are reported
  but **never fail the run on their own**. Use `--strict` to fail on the first
  error, or `--assert` for an explicit data-quality gate.

This holds regardless of output flags — the signal is computed independently of
`--stats`/`--no-diagnostics` collection — so `--metrics`, `--drain`, `-q`, and
`--no-diagnostics` all preserve the exit code.

### Scenario reference

| Scenario | Default | `--strict` |
|----------|:-------:|:----------:|
| Clean run | `0` | `0` |
| Filter legitimately matches nothing | `0` | `0` |
| Some lines fail to parse, others succeed | `0` | `1` (aborts on first) |
| **Every** line fails to parse (wrong format) | `1` | `1` |
| `--filter` errors on **every** event (typo, type bug) | `1` | `1` |
| `--exec` errors on some events (heterogeneous logs) | `0` | `1` (aborts on first) |
| `--exec` errors on **every** event (best-effort transform) | `0` | `1` |
| Broken `--exec` behind a selective `--filter` | `0` | `1` |
| Mutating `conf` outside `--begin` (forbidden) | `1` | `1` |
| Named input file can't be opened | `1` | `1` |
| `--assert` violation | `1` | `1` |

### Full code table

| Code | Meaning |
|------|---------|
| `0` | The run did its job (possibly with recovered parse/exec errors) |
| `1` | A gate (parse/filter) never succeeded, a structural failure, or an `--assert`/strict failure |
| `2` | Invalid usage (CLI errors, incompatible flags) |
| `130` | Interrupted (Ctrl+C) |
| `134` | Internal thread panic (a bug — please report) |
| `141` | Broken pipe (normal in Unix pipelines) |
| `143` | Terminated (SIGTERM) |

### Using Exit Codes

**In shell scripts:**
```bash
if kelora -q -j app.log; then
    echo "✓ Ran successfully (messy records, if any, were recovered)"
else
    echo "✗ Could not complete: a stage failed, or input was unusable"
    exit 1
fi
```

Note that exit `0` means "the job got done", not "zero errors": a run with a
few recovered parse errors still succeeds. To fail on *any* imperfection, add
`--strict`; to fail on explicit data-quality rules, use `--assert`.

**In CI/CD:**
```bash
kelora --silent --strict app.log || exit 1
```

**With automation:**
```bash
kelora --silent app.log; echo "Exit code: $?"
```

## Atomic Transformations

### How It Works

In resilient mode, `--exec` scripts execute **atomically**:

```bash
kelora -j --exec 'e.a = 1; e.b = e.value.to_int(); e.c = 3' app.log
```

If `e.value.to_int()` fails:

- Changes to `e.a` are **rolled back**
- `e.b` is never set
- `e.c` is never set
- **Original event** is returned unchanged

### Why Atomic?

Prevents partial transformations from corrupting data:

**Without atomicity:**
```json
// Input
{"value": "invalid"}

// Broken output (partial transformation)
{"value": "invalid", "a": 1}  // Missing b and c!
```

**With atomicity:**
```json
// Input
{"value": "invalid"}

// Output (unchanged)
{"value": "invalid"}  // Clean original event
```

### Multiple --exec Scripts

Each `--exec` script is atomic independently:

```bash
kelora -j \
    --exec 'e.a = e.x.to_int()' \
    --exec 'e.b = e.y.to_int()' \
    app.log
```

If first `--exec` fails:

- First transformation rolled back
- Second `--exec` **still runs** on original event

If second `--exec` fails:

- First transformation **preserved** (it succeeded)
- Second transformation rolled back

## Common Error Scenarios

### Missing Fields

**Problem:**
```bash
kelora -j --filter 'e.timestamp > "2024-01-01"' app.log
```

Some events missing `timestamp` field.

**Solution:** Use safe access:
```bash
kelora -j --filter 'e.has_path("timestamp") && e.timestamp > "2024-01-01"' app.log
```

### Type Mismatches

**Problem:**
```bash
kelora -j --exec 'e.result = e.value * 2' app.log
```

`e.value` is a string, not a number.

**Solution:** Use type conversion with defaults:
```bash
kelora -j --exec 'e.result = e.value.to_int_or(0) * 2' app.log
```

### Invalid Timestamps

**Problem:**
```bash
kelora -j --filter 'e.timestamp.to_unix() > 1000000' app.log
```

`e.timestamp` is not a valid timestamp.

**Solution:** Use safe access:
```bash
kelora -j --filter 'e.has_path("timestamp") && e.timestamp.to_unix() > 1000000' app.log
```

### Array Index Out of Bounds

**Problem:**
```bash
kelora -j --exec 'e.first = e.items[0]' app.log
```

`e.items` is empty or missing.

**Solution:** Check array length:
```bash
kelora -j --exec 'if e.has_path("items") && e.items.len() > 0 { e.first = e.items[0] }' app.log
```

### Division by Zero

**Problem:**
```bash
kelora -j --exec 'e.ratio = e.success / e.total' app.log
```

`e.total` is zero.

**Solution:** Add guard:
```bash
kelora -j --exec 'if e.total > 0 { e.ratio = e.success / e.total } else { e.ratio = 0.0 }' app.log
```

## Debugging Strategies

### Use Verbose Mode

See errors as they happen:

```bash
kelora -j --verbose app.log --exec 'e.result = e.value.to_int()'
```

### Enable Strict Mode

Find first error quickly:

```bash
kelora -j --strict app.log
```

### Inspect Problematic Lines

Use `--take` to limit processing:

```bash
kelora -j --strict --take 100 app.log
```

Process only first 100 lines to find issues faster.

### Check Field Existence

Verify fields exist before accessing:

```bash
kelora -j --exec 'if !e.has_path("value") { eprint("Line missing value: " + e) }' app.log
```

### Use Type Checking

Verify field types before operations:

```bash
kelora -j --exec 'if type_of(e.value) != "i64" { eprint("Value is not integer: " + e.value) }' app.log
```

### Validate Input Format

Test parsing with strict mode:

```bash
kelora -j --strict -s app.log
```

No output, but exits with error if parsing fails.

## Error Messages

### Parse Error Format

```
⚠️  kelora: line 42: parse error - invalid JSON at position 15
```

- `line 42`: Line number in input
- `parse error`: Error category
- Details: Specific error message

### Filter Error Format

```
⚠️  kelora: line 42: filter error - field 'timestamp' not found
```

### Exec Error Format

```
⚠️  kelora: line 42: exec error - cannot convert 'abc' to integer
```

### Enhanced Error Summaries

With `--verbose`, get example errors:

```
🔹 Processed 1000 lines, 950 events output, 50 errors

Error examples:
  line 42: exec error - cannot convert 'abc' to integer
  line 103: exec error - field 'value' not found
  line 287: filter error - timestamp is null
```

## Best Practices

### Use Resilient Mode for Production

Production logs are messy - resilient mode handles gracefully:

```bash
kelora -j app.log --levels error --keys timestamp,message
```

### Use Strict Mode for Validation

Validate data quality in pipelines:

```bash
kelora -j --strict app.log > /dev/null && echo "✓ Valid"
```

### Combine Quiet and Exit Codes

For automation, use exit codes:

```bash
kelora --silent app.log
if [ $? -eq 0 ]; then
    echo "No errors"
else
    echo "Has errors"
fi
```

### Add Defensive Checks

Use safe field access patterns:

```bash
kelora -j --exec 'e.result = e.get_path("nested.value", 0) * 2' app.log
```

### Log Errors to File

Capture errors for later analysis:

```bash
kelora -j --verbose app.log 2> errors.log
```

### Use Stats for Summary

Get error counts without verbose output:

```bash
kelora -j --stats app.log
```

Shows error count in summary.

## Parallel Processing

### Error Handling in Parallel Mode

When using `--parallel`, error handling works the same:

```bash
kelora -j --parallel app.log
```

- Errors still recorded per event
- Summary shows total errors across all threads
- Exit code reflects any errors from any thread

### Verbose with Parallel

Verbose errors are shown immediately, but may be interleaved:

```bash
kelora -j --parallel --verbose app.log
```

Errors from different threads may appear out of order.

### Strict with Parallel

First error from any thread aborts all processing:

```bash
kelora -j --parallel --strict app.log
```

## See Also

- [Pipeline Model](pipeline-model.md) - How error handling fits into processing stages
- [Scripting Stages](scripting-stages.md) - Error handling in --begin/--exec/--end
- [CLI Reference](../reference/cli-reference.md) - All error handling flags
- [Exit Codes Reference](../reference/exit-codes.md) - Complete exit code documentation
