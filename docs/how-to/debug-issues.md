# Debug Issues Systematically

Learn how to diagnose and fix problems when Kelora isn't working as expected.

## Quick Debugging Workflow

When something isn't working, follow this process:

1. **Check field names and types** - Use `-F inspect` to see actual field structure
2. **Enable error output** - Use `--verbose` to see detailed error messages
3. **Check statistics** - Use `--stats` to see how many events were filtered/errored
4. **Test with small sample** - Use `--take 10` for fast iteration
5. **Use strict mode** - Use `--strict` to fail-fast on errors during debugging
6. **Validate data quality** - Use `--assert` to check for required fields or invariants

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

- `-v` - Show error messages as they occur
- `-vv` - Show error messages plus original line content for parse errors
- `-vvv` - Show error messages, lines, plus detailed line analysis (character counts, encoding issues)

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

### `--assert` - Data Validation

Validate events against boolean expressions. Events still pass through (unlike `--filter`), but violations are reported to stderr:

```bash
# Check all events have required field
kelora -j app.log --assert 'e.has("user_id")'

# Validate transformations
kelora -j data.log \
    --exec 'e.name = e.name.lower()' \
    --assert 'e.name == e.name.lower()'

# Multiple validation rules
kelora -j app.log \
    --assert 'e.has("timestamp")' \
    --assert 'e.status >= 0'
```

**Use when:**

- Validating log data quality
- Checking required fields exist
- Verifying transformations are correct
- Testing data pipelines

**Behavior:**

- Events always pass through to output
- Violations reported immediately to stderr
- Exit code 1 if any assertions fail
- Use with `--stats` to see failure counts
- Combine with `--strict` for fail-fast validation

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

## Getting More Help

### Built-in Help

```bash
kelora --help              # Complete CLI reference
kelora --help-examples     # Common usage patterns
kelora --help-rhai         # Rhai scripting guide
kelora --help-functions    # All built-in functions (add a KEYWORD to search, e.g. --help-functions ip)
kelora --help-time         # Timestamp format reference
kelora --help-multiline    # Multiline strategies
```

### Documentation

- **[Quickstart](../quickstart.md)** - Get started in 5 minutes
- **[Basics Tutorial](../tutorials/basics.md)** - Learn fundamental concepts
- **[Intro to Rhai](../tutorials/intro-to-rhai.md)** - Learn scripting
- **[Glossary](../glossary.md)** - Terminology reference
- **[Common Errors](../reference/common-errors.md)** - Error reference
- **[Error Handling](../concepts/error-handling.md)** - Error modes in detail
- **[Exit Codes](../reference/exit-codes.md)** - Exit code reference

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
5. `kelora -j app.log --assert 'e.has("field")' --stats` - Validate data quality

**Common fixes:**

- Field not found → Use `e.has("field")` or `e.get("field", default)`
- Type error → Use `.to_int_or(0)` or `.to_string()`
- Empty output → Check with `--stats`
- Parse error → Check format with `-f auto` or use `--verbose`
- Stage order → Create fields before filtering on them
- Slow processing → Use `--parallel` for large files
