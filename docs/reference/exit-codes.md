# Exit Codes

Complete reference for Kelora's exit codes and their meanings.

## The model in one line

> **Kelora exits non-zero when it couldn't do the job you asked — not because the data was messy.**

The model turns on **gates vs. transforms**:

- **Gates — parse and filter — must work.** If a gate never once succeeds (no
  line parses, or a filter errors on *every* event and so never selects
  anything), the output is empty or meaningless — a broken command — so the run
  exits `1`. A gate erroring on only *some* records is recovered (exit `0`).
- **Transforms — exec — are best-effort.** A failing `--exec` rolls back to the
  original event and emits it, so exec errors are reported but **never fail the
  run on their own**, even when they hit every event.

Structural failures (a named input that won't open) and `--assert` violations
fail in any mode. `--strict` escalates: any single parse/filter/exec error fails
immediately, and `--assert` adds explicit data-quality gates.

This is independent of output flags: the signal is computed in the always-on
tracker, so `--metrics`, `--drain`, `-q`, and `--no-diagnostics` all preserve
the exit code.

## Standard Exit Codes

Kelora uses standard Unix exit codes to indicate success or failure:

| Code | Name | Meaning | Cause |
|------|------|---------|-------|
| `0` | Success | The run did its job | Clean processing, or *recovered* errors (some lines skipped, exec transforms rolled back) |
| `1` | General Error | The run couldn't do the job | A gate that never succeeded (every line fails to parse, or a filter errors on every event), an `--assert` violation, a file that couldn't be opened, or any strict-mode error |
| `2` | Usage Error | Invalid command-line usage | Invalid flags, incompatible options, configuration errors |

## Signal Exit Codes

When Kelora is interrupted by signals:

| Code | Signal | Meaning |
|------|--------|---------|
| `130` | SIGINT | Interrupted by Ctrl+C |
| `134` | SIGABRT | Internal thread panic (a bug — please report) |
| `141` | SIGPIPE | Broken pipe (normal in Unix pipelines) |
| `143` | SIGTERM | Terminated by system or user |

Exit code `134` only appears on an unexpected internal panic in one of Kelora's
processing threads (reader, worker, or sink). The release binary is built with
`panic = "abort"`, so such a panic aborts the process immediately rather than
unwinding. These are always bugs — the same conditions previously terminated the
run with exit `1`/`101`; only the reported code changed. Please report any
occurrence.

## Exit Code 0: Success

Indicates **the run did its job**. The work you asked for happened, even if some
individual records were skipped or rolled back along the way.

**Important:** Exit `0` is not "zero errors" — it's "the job got done". Filtering
events is not an error (filtering everything still exits `0`). *Recovered*
per-record errors — a few unparseable lines among good ones, a `--filter`/`--exec`
that errors on *some* events — are reported as diagnostics but keep exit `0`,
because the stage still succeeded on other records. To fail on *any* such error,
use `--strict`; to fail on explicit data-quality rules, use `--assert`.

```bash
# Returns 0 - filtering is not an error
kelora -j app.log --levels critical
echo $?
0

# Returns 0 - some lines failed to parse, but others succeeded (recovered)
printf '{"ok":1}\nNOT JSON\n{"ok":2}\n' | kelora -j
echo $?
0

# Returns 1 - NO line parsed: the format is wrong, so the run couldn't do its job
printf 'plain one\nplain two\n' | kelora -j
echo $?
1
```

## Exit Code 1: The run couldn't do the job

Indicates the run failed to do what was asked. Common causes:

| Cause | Meaning | Example |
|-------|---------|---------|
| **Parse gate failed** | *Every* line failed to parse — the format is wrong or the input is unusable | `kelora -j` on plain-text logs |
| **Filter gate failed** | A `--filter` errored on *every* event, so it never selected anything | `--filter 'status >= 500'` (missing `e.`) |
| **Assertion failures** | `--assert` expressions evaluated to false (an explicit data-quality gate) | Missing required fields |
| **File I/O failures** | A named input file failed to open or decompress | Permission denied, file not found |
| **Strict-mode errors** | *Any* parse/filter/exec error while `--strict` was enabled | Missing field access, type errors |

Parse and filter are *gates*: a gate that errored on **some** records is
recovered (exit `0`); the same gate erroring on **every** record means it never
once worked, which is a broken command (exit `1`). `--exec` is **not** a gate —
it's a best-effort transform that rolls back on error and never fails the run on
its own (use `--strict`/`--assert`).

### Resilient Mode (Default)

Gate errors are recorded but processing continues. They affect the exit code
only when the gate never succeeds; exec errors never affect it:

```bash
# Recovered: a field-name typo in an EXEC errors on every event, but exec rolls
# back and emits the original events -> exit 0 (use --strict to fail)
kelora -q -j app.log --exec 'e.x = e.valeu.to_int()'   # typo, but best-effort
echo $?
0

# Broken gate: a field-name typo in a FILTER errors on every event, so it never
# selected anything -> exit 1, even without --strict
kelora -q -j app.log --filter 'status >= 500'   # should be e.status
echo $?
1
```

For automation, use `--strict` to fail on the *first* parse/filter/exec error,
and `--assert` to fail on explicit data-quality requirements.

### Strict Mode

Processing aborts immediately on first error with `--strict`:

```bash
kelora -j --strict app.log
# ... aborts on first parse/filter/exec/file error ...
echo $?
1  # Processing aborted

# With multiple files, strict mode aborts on first file failure
kelora -j --strict good.log missing.log another.log
⚠️ Failed to open file 'missing.log': No such file or directory
echo $?
1  # Aborted immediately, another.log not processed
```

### Examples

```bash
# Parse error
echo '{invalid json}' | kelora -j
# Strict-mode filter error
kelora -j --strict app.log --filter 'e.missing_field.to_int()'
# Strict-mode exec error
kelora -j --strict app.log --exec 'e.result = e.invalid.to_int()'
# Assertion failure
kelora -j app.log --assert 'e.has("user_id")'
# File I/O error (some files failed)
kelora -j good.log missing.log
# All return exit code 1
```

## Exit Code 2: Usage Errors

Indicates **command-line usage errors** before processing begins:

| Error Type | Example |
|------------|---------|
| Invalid flags | `kelora --invalid-flag app.log` |
| Missing arguments | `kelora --filter` (no expression provided) |
| Incompatible flags | `kelora -I helper.rhai --filter 'e.level == "ERROR"'` |
| Invalid configuration | `kelora --config-file invalid.ini app.log` |
| No input provided | `kelora` (no files + stdin is TTY) |

**Note:** File I/O failures (unable to open files) are **processing errors** (exit 1), not usage errors, regardless of how many files fail. This is consistent with standard Unix tools like `cat`, `tail`, and `head`.

```bash
# File I/O failures are always exit 1
kelora -j nonexistent.log
⚠️ Failed to open file 'nonexistent.log': No such file or directory
echo $?
1  # Processing error, not usage error

# Even if all files fail
kelora -j missing1.log missing2.log
⚠️ Failed to open file 'missing1.log': No such file or directory
⚠️ Failed to open file 'missing2.log': No such file or directory
echo $?
1  # Still processing error (like parse errors)

# Usage error (invalid filter-stage include file)
kelora -I helper.rhai --filter 'is_error(e.level)' app.log
⚠️ --include file 'helper.rhai' cannot contain statements when used with --filter; only function definitions are allowed
echo $?
2  # Usage error
```

## Mode Interactions

### Processing Modes

| Mode | Behavior | Exit Code 1 Timing | Use Case |
|------|----------|-------------------|----------|
| **Resilient** (default) | Continue on recovered runtime errors | At end only for unrecovered failures | Production, exploratory analysis |
| **Strict** (`--strict`) | Abort on first error | Immediately on first error | Validation, fail-fast |
| **Parallel** (`--parallel`) | Same as sequential | Same as non-parallel | Performance (exit code behavior unchanged) |

### Quiet Modes

Exit codes are preserved under all quiet/silent combinations. Use the new toggles to control output without changing exit semantics:

- `-q/--quiet`: suppress events only.
- `--no-diagnostics`: suppress diagnostics and summaries (fatal line still emitted).
- `--silent`: suppress pipeline terminal emitters (events/diagnostics/stats/terminal metrics); script output still allowed; metrics files still write.
- `-m/--metrics` / `-s/--stats`: data-only modes that already suppress events (no need for `-q`) and script output while emitting the selected channel.

## Using Exit Codes in Scripts

### Comprehensive Example

```bash
#!/bin/bash
# Production log processing script

# Check for usage errors first
if ! kelora -j --strict app.log > /dev/null 2>&1; then
    exit_code=$?
    case $exit_code in
        1)
            echo "✗ Parse errors detected, check log format"
            kelora -j --verbose app.log 2>&1 | grep "error:" | head -5
            exit 1
            ;;
        2)
            echo "✗ Usage error, check command syntax"
            exit 2
            ;;
        130)
            echo "⚠️  Interrupted by user"
            exit 130
            ;;
        141)
            # SIGPIPE is normal in pipelines
            exit 0
            ;;
        *)
            echo "✗ Unknown error (exit code: $exit_code)"
            exit $exit_code
            ;;
    esac
fi

# Process validated logs
if kelora -j app.log --levels error,critical -F json > errors.json; then
    echo "✓ No errors found"
else
    echo "✗ Processing failed"
    exit 1
fi
```

### Common Patterns

```bash
# Basic validation
kelora --silent -j --strict app.log && echo "✓ Valid" || echo "✗ Invalid"

# Ignore SIGPIPE in pipelines
kelora -j large.log | head -n 10 || [ $? -eq 141 ]

# CI/CD validation
kelora -j --strict logs/*.json > /dev/null || {
    echo "✗ Log validation failed"
    exit 1
}

# Check for critical events
if ! kelora -q -j app.log --levels error,critical; then
    echo "✗ Found critical errors"
    exit 1
fi

# Validation loop
for file in logs/*.json; do
    kelora -j --strict "$file" > /dev/null 2>&1 || {
        echo "✗ Invalid: $file"
        exit 1
    }
done
```

### Makefile Integration

```makefile
.PHONY: check-logs
check-logs:
	@kelora -j logs/*.json --levels error || \
		(echo "Errors found in logs" && exit 1)

.PHONY: validate-logs
validate-logs:
	@kelora -j --strict logs/*.json > /dev/null || \
		(echo "Log validation failed" && exit 1)
```

## Troubleshooting

### Exit Code 1 When Expecting 0

**Problem:** Getting exit code `1` but don't see errors.

**Solution:** Use `--verbose` to see all errors, or check stats:

```bash
kelora -j --verbose app.log
kelora -j --stats app.log
```

### Exit Code 141 (SIGPIPE)

**This is normal** in pipelines when downstream commands close early:

```bash
kelora -j large.log | head -n 10
# Exit code 141 is expected and normal

# Ignore SIGPIPE if needed
kelora -j large.log | head -n 10 || [ $? -eq 141 ]
```

### Exit Code 2 with Valid Syntax

**Check:**

- File exists and is readable: `ls -l app.log`
- Permissions are correct: `stat app.log`
- Configuration file is valid
- Try ignoring config: `kelora --ignore-config -j app.log`

### Different Exit Codes in CI vs Local

**Possible causes:**

- Different file permissions/paths
- Different configuration files
- Environment variables

**Solution:** Test with same conditions:
```bash
kelora --ignore-config -j app.log  # Ignore config files
```

## See Also

- [Error Handling](../concepts/error-handling.md) - Error handling modes and strategies
- [CLI Reference](cli-reference.md) - Complete flag documentation
- [Quiet/Silent Controls](../concepts/error-handling.md#quietsilent-controls) - Suppressing output for automation
