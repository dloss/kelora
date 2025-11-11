# Troubleshooting Guide

Quick solutions for common issues and systematic debugging workflows.

## Quick Debugging Workflow

When something isn't working, follow this process:

1. **Check field names and types** - Use `-F inspect` to see actual field structure
2. **Enable error output** - Use `--verbose` to see detailed error messages
3. **Check statistics** - Use `--stats` to see how many events were filtered/errored
4. **Test with small sample** - Use `--take 10` for fast iteration
5. **Use strict mode** - Use `--strict` to fail-fast on errors during debugging

**Example debugging session:**

```bash
# 1. What fields are actually present?
kelora -j app.log -F inspect --take 3

# 2. Why is my filter not matching?
kelora -j app.log --filter 'e.status >= 500' --verbose --stats

# 3. Test quickly with a small sample
kelora -j app.log --filter 'e.status >= 500' --take 10
```

---

## Common Issues

### "Field not found" or Empty Output

**Problem:** No output or errors like "field 'status' not found".

**Possible causes:**

1. **Wrong format specified** - Default is `-f line` (plain text), not JSON
2. **Field doesn't exist** - Field name is misspelled or doesn't exist in all events
3. **All events filtered out** - Your filters are too restrictive

**Solutions:**

```bash
# Check if format is correct
kelora -j app.log -F inspect --take 3        # For JSON
kelora -f logfmt app.log -F inspect --take 3 # For logfmt

# See what fields are actually present
kelora -j app.log -F inspect | head -20

# Check if field exists before using it
kelora -j app.log --filter 'e.has("status") && e.status >= 500'

# See statistics to understand what's happening
kelora -j app.log --filter 'e.status >= 500' --stats
```

**Common mistakes:**

- Using `-j` when logs are not JSON format
- Accessing nested fields incorrectly: use `e.user.id`, not `e["user.id"]`
- Filtering before creating computed fields (see Stage Order below)

---

### Filters Not Working

**Problem:** Filter expressions don't match expected events.

**Possible causes:**

1. **Stage ordering** - Filtering on fields created by `--exec` that comes later
2. **Type mismatch** - Comparing string "500" with number 500
3. **Missing fields** - Field doesn't exist in some events
4. **Logic errors** - Boolean logic isn't what you think

**Solutions:**

```bash
# Check field types with inspect
kelora -j app.log -F inspect --take 5

# Safe field access with existence check
kelora -j app.log --filter 'e.has("status") && e.status >= 500'

# Handle missing fields with defaults
kelora -j app.log --filter 'e.get("status", 0) >= 500'

# Debug with verbose output
kelora -j app.log --filter 'e.status >= 500' --verbose

# See how many events match
kelora -j app.log --filter 'e.status >= 500' --stats
```

**Stage order matters:**

```bash
# ❌ WRONG: Filter runs before exec creates the field
kelora -j app.log --filter 'e.duration_s > 1' \
    -e 'e.duration_s = e.duration_ms / 1000'

# ✅ RIGHT: Exec creates field first, then filter uses it
kelora -j app.log \
    -e 'e.duration_s = e.duration_ms / 1000' \
    --filter 'e.duration_s > 1'
```

---

### Type Comparison Errors

**Problem:** Errors like "cannot compare string with number" or unexpected filter behavior.

**Cause:** Fields have different types than expected. Common with string numbers like `"500"` vs integer `500`.

**Solutions:**

```bash
# Check actual types
kelora -j app.log -F inspect --take 5

# Convert strings to numbers safely
kelora -j app.log --filter 'e.status.to_int_or(0) >= 500'

# Convert to string for comparison
kelora -j app.log --filter 'e.status.to_string() == "500"'

# Handle nulls and missing values
kelora -j app.log \
    --filter 'e.has("status") && e.status.to_int_or(0) >= 500'
```

**Common type conversions:**

| Function | Purpose | Safe Default |
|----------|---------|--------------|
| `.to_int_or(0)` | String → Integer | Returns 0 on error |
| `.to_float_or(0.0)` | String → Float | Returns 0.0 on error |
| `.to_string()` | Any → String | Always succeeds |
| `.get("field", default)` | Field access with fallback | Returns default if missing |

---

### Empty Output Despite Having Data

**Problem:** Kelora processes logs but outputs nothing.

**Possible causes:**

1. **All events filtered out** - Level filtering, time ranges, or filters too restrictive
2. **Quiet mode enabled** - Using `-qq` suppresses event output
3. **Wrong output format** - Using `-F none` by mistake
4. **Timestamp filtering** - Events outside `--since`/`--until` range

**Solutions:**

```bash
# Check statistics to see what happened
kelora -j app.log --stats

# Remove all filtering temporarily
kelora -j app.log --take 10

# Check if level filtering is too restrictive
kelora -j app.log --levels error --stats  # See how many events have ERROR level

# Verify output format
kelora -j app.log -F pretty --take 10

# Check timestamp range
kelora -j app.log --since '1 hour ago' --stats
```

---

### Parse Errors

**Problem:** Errors like "failed to parse JSON" or "invalid format".

**Possible causes:**

1. **Wrong format specified** - File isn't actually JSON/logfmt/etc
2. **Malformed data** - Invalid syntax in log lines
3. **Mixed formats** - File contains multiple formats
4. **Multiline events** - JSON/messages span multiple lines

**Solutions:**

```bash
# Try auto-detection
kelora -f auto app.log --take 10

# Check raw file content
head -20 app.log

# See parse errors with verbose
kelora -j app.log --verbose --stats

# Use strict mode to fail on first error
kelora -j --strict app.log 2>&1 | head -20

# For multiline JSON, use multiline mode
kelora -j -M start app.log  # Multiline starting with {

# For mixed formats, use line filtering
kelora -j app.log --keep-lines '^\{'  # Only lines starting with {
```

**Check format detection:**

```bash
# Test different formats
kelora -f json app.log --take 5 -F inspect
kelora -f logfmt app.log --take 5 -F inspect
kelora -f line app.log --take 5 -F inspect
```

---

### Performance Issues

**Problem:** Kelora is slow or uses too much memory.

**Solutions:**

```bash
# Use parallel mode for large files
kelora -j --parallel large.log

# Adjust batch size for parallel processing
kelora -j --parallel --batch-size 10000 large.log

# Filter early in pipeline to reduce work
kelora -j app.log \
    --levels error \           # Filter early
    -e 'e.heavy_transform()'   # Then do expensive work

# Use line-level filtering before parsing
kelora -j app.log --keep-lines 'ERROR|CRITICAL'

# Process compressed files directly
kelora -j app.log.gz  # Automatic decompression

# Limit output for testing
kelora -j large.log --take 100 --stats
```

**Performance tips:**

- Put cheap filters before expensive transforms
- Use `--parallel` for files > 100MB
- Filter at line level with `--keep-lines` when possible
- Use `--take` to limit output during development

See [Performance Model](concepts/performance-model.md) for details.

---

### Script Errors in --exec

**Problem:** Runtime errors in Rhai scripts with `--exec` or `-e`.

**Common causes:**

1. **Accessing missing fields** - Use `.has()` or `.get()` to check first
2. **Type errors** - Operating on wrong types
3. **Null values** - Field exists but is null
4. **Syntax errors** - Invalid Rhai syntax

**Solutions:**

```bash
# Check field existence before access
kelora -j app.log -e '
    if e.has("duration_ms") {
        e.duration_s = e.duration_ms / 1000
    }
'

# Use safe accessors with defaults
kelora -j app.log -e 'e.duration_s = e.get("duration_ms", 0) / 1000'

# Handle nulls explicitly
kelora -j app.log -e '
    if e.value != () {
        e.processed = e.value * 2
    }
'

# Test with strict mode to catch errors early
kelora -j --strict app.log -e 'e.result = e.value.to_int()'

# Use verbose to see which line failed
kelora -j app.log --verbose -e 'e.result = e.value.to_int()'
```

---

### Stage Order Confusion

**Problem:** Computed fields aren't available where expected.

**Key principle:** Stages run **in the order you specify them** on the command line.

**Examples:**

```bash
# ❌ WRONG: Filtering before field exists
kelora -j app.log \
    --filter 'e.duration_s > 1' \
    -e 'e.duration_s = e.duration_ms / 1000'
# Error: e.duration_s doesn't exist yet!

# ✅ RIGHT: Create field, then filter
kelora -j app.log \
    -e 'e.duration_s = e.duration_ms / 1000' \
    --filter 'e.duration_s > 1'

# ❌ WRONG: Level filtering before level is set
kelora -j app.log \
    --levels error \
    -e 'e.level = "ERROR"'
# Event filtered out before level is set!

# ✅ RIGHT: Set level, then filter
kelora -j app.log \
    -e 'if !e.has("level") { e.level = "ERROR" }' \
    --levels error
```

**Rule of thumb:** Put creation before filtering, filtering before transformation.

---

### Configuration Issues

**Problem:** Unexpected behavior due to configuration files.

**Solutions:**

```bash
# Ignore config files for testing
kelora --ignore-config -j app.log

# Check which config files are being used
kelora -j app.log --verbose 2>&1 | grep -i config

# Check config file syntax
cat ~/.config/kelora/kelora.ini

# Test with explicit config
kelora --config-file ./test.ini -j app.log
```

**Config file locations (in precedence order):**

1. `./.kelora.ini` (project directory)
2. `~/.config/kelora/kelora.ini` (user config)
3. System defaults

See [Configuration System](concepts/configuration-system.md).

---

## Debugging Tools Reference

### `-F inspect` - See Field Structure

Shows field names, types, and values for debugging:

```bash
kelora -j app.log -F inspect --take 3
```

**Output example:**
```
🔹 {"timestamp": String("2024-01-15T10:00:00Z"), "level": String("ERROR"), "status": Integer(500)}
```

**Use when:**
- Need to see actual field types
- Unsure what fields are present
- Debugging type comparison errors

---

### `--verbose` - Detailed Error Output

Shows detailed error messages for parse, filter, and exec errors:

```bash
kelora -j app.log --verbose
```

**Use multiple times for more detail:**
- `-v` - Show errors with context
- `-vv` - Show detailed stack traces
- `-vvv` - Show all debug information

**Use when:**
- Filters aren't working
- Parse errors occurring
- Scripts failing

---

### `--stats` - Processing Summary

Shows statistics at end of processing:

```bash
kelora -j app.log --stats
```

**Output example:**
```
🔹 Processed 1000 lines, 850 events output, 150 filtered, 0 errors
🔹 Time range: 2024-01-15T10:00:00Z to 2024-01-15T11:00:00Z
🔹 Levels: ERROR(150), WARN(300), INFO(550)
```

**Use when:**
- Need to understand filtering results
- Checking error counts
- Verifying time ranges
- Understanding level distribution

---

### `--take N` - Limit Output

Process only first N events (after filtering):

```bash
kelora -j app.log --take 10
```

**Use when:**
- Testing filters quickly
- Debugging with large files
- Iterating on scripts
- Checking output format

**Tip:** Combine with `--stats` to see total counts while limiting output:

```bash
kelora -j large.log --filter 'e.level == "ERROR"' --take 10 --stats
```

---

### `--strict` - Fail-Fast Mode

Abort immediately on first error:

```bash
kelora -j --strict app.log
```

**Use when:**
- Debugging parse errors
- Validating log format
- Testing scripts
- CI/CD validation

**Difference from default:**
- **Default (resilient):** Continue on errors, show count at end
- **Strict:** Abort immediately on first error

---

### `--keep-lines` / `--ignore-lines` - Pre-filter

Filter at line level before parsing (faster than post-parse filtering):

```bash
# Only process lines containing ERROR
kelora -j app.log --keep-lines 'ERROR'

# Skip debug lines
kelora -j app.log --ignore-lines 'DEBUG|TRACE'
```

**Use when:**
- Processing very large files
- Most lines can be skipped early
- Performance is critical

---

## Exit Codes for Automation

Kelora uses standard exit codes:

| Code | Meaning | Common Cause |
|------|---------|--------------|
| `0` | Success | No errors occurred |
| `1` | Processing errors | Parse errors, filter errors, exec errors |
| `2` | Usage errors | Invalid flags, missing files, bad config |
| `130` | Interrupted | Ctrl+C (SIGINT) |
| `141` | Broken pipe | Normal in pipelines with `head` |

**Check exit code in scripts:**

```bash
if ! kelora -j app.log --strict > /dev/null 2>&1; then
    echo "❌ Validation failed (exit code: $?)"
    exit 1
fi
```

See [Exit Codes Reference](reference/exit-codes.md) for complete documentation.

---

## Common Error Messages

### "Failed to parse JSON on line N"

**Cause:** Invalid JSON syntax on that line.

**Solutions:**
- Check the line: `sed -n 'Np' app.log` (where N is the line number)
- Use `--verbose` to see the problematic line
- Use `-f auto` to try auto-detection
- Use multiline mode if JSON spans multiple lines: `-M start`

---

### "Cannot access field 'X' of non-object value"

**Cause:** Trying to access field on a value that isn't an object (e.g., null, array, primitive).

**Solutions:**
- Check field existence: `e.has("field")`
- Use inspect to see structure: `-F inspect`
- Handle nulls: `if e.field != () { ... }`

---

### "Type mismatch: cannot compare String with Integer"

**Cause:** Comparing values of different types (e.g., `"500" >= 500`).

**Solutions:**
- Convert types: `e.status.to_int_or(0) >= 500`
- Check types with inspect: `-F inspect`
- Use string comparison: `e.status == "500"`

---

### "Field 'X' not found"

**Cause:** Accessing a field that doesn't exist in the event.

**Solutions:**
- Check field name spelling
- Verify field exists: `e.has("field")`
- Use safe accessor: `e.get("field", default)`
- Check actual field names: `-F inspect`

---

### "No events matched filter"

**Cause:** All events filtered out by your filters.

**Solutions:**
- Check filter logic
- Use `--stats` to see filtering breakdown
- Remove filters temporarily to see all events
- Check if level filtering is too restrictive

---

### "Failed to open file"

**Cause:** File doesn't exist, no permission, or path is wrong.

**Solutions:**
- Check file exists: `ls -l file.log`
- Check permissions: `stat file.log`
- Use absolute path
- Check if file is actually stdin: use `-` for stdin

---

## Getting More Help

### Built-in Help

```bash
kelora --help              # Complete CLI reference
kelora --help-examples     # Common usage patterns
kelora --help-rhai         # Rhai scripting guide
kelora --help-functions    # All built-in functions
kelora --help-time         # Timestamp format reference
kelora --help-multiline    # Multiline strategies
```

### Documentation

- **[Quickstart](quickstart.md)** - Get started in 5 minutes
- **[Basics Tutorial](tutorials/basics.md)** - Learn fundamental concepts
- **[Intro to Rhai](tutorials/intro-to-rhai.md)** - Learn scripting
- **[Glossary](glossary.md)** - Terminology reference
- **[Error Handling](concepts/error-handling.md)** - Error modes in detail
- **[Exit Codes](reference/exit-codes.md)** - Exit code reference

### Tips for Asking Questions

When asking for help, include:

1. **Command you ran** - Full command with all flags
2. **Sample input** - A few lines of log data
3. **Expected output** - What you expected to see
4. **Actual output** - What actually happened
5. **Error messages** - Complete error output with `--verbose`
6. **Field structure** - Output of `-F inspect --take 3`

**Example:**

```
Command:
  kelora -j app.log --filter 'e.status >= 500'

Sample input:
  {"timestamp": "2024-01-15T10:00:00Z", "status": "500"}

Error:
  Type mismatch: cannot compare String with Integer

Field structure (kelora -j app.log -F inspect --take 1):
  {"status": String("500")}

Expected: Filter events with status >= 500
Actual: Error on type comparison
```

---

## Quick Reference

**Start here when debugging:**

1. `kelora -j app.log -F inspect --take 5` - See field structure
2. `kelora -j app.log --verbose --stats` - See errors and statistics
3. `kelora -j app.log --take 10` - Test with small sample
4. `kelora -j --strict app.log` - Fail-fast mode for debugging

**Common fixes:**

- Field not found → Use `e.has("field")` or `e.get("field", default)`
- Type error → Use `.to_int_or(0)` or `.to_string()`
- Empty output → Check with `--stats`
- Parse error → Check format with `-f auto` or use `--verbose`
- Stage order → Create fields before filtering on them
- Slow processing → Use `--parallel` for large files
