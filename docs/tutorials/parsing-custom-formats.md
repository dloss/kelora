# Parsing Custom Formats

Learn how to parse custom log formats using Kelora's column specification syntax.

## What You'll Learn

By the end of this tutorial, you'll be able to:

- Parse whitespace- and separator-delimited custom logs
- Use column specifications with names, multi-token captures, and skips
- Apply supported type annotations for automatic conversion
- Handle timestamp fields and greedy captures
- Combine custom parsing with filtering and transformation

## Prerequisites

- Completed the [Quickstart](../quickstart.md)
- Basic understanding of log formats
- Familiarity with command-line operations

## Overview

Many applications generate custom log formats that don't match standard formats like JSON or syslog. Kelora's `cols:<spec>` format lets you define custom column specifications to parse these logs.

**Time:** ~15 minutes

## Step 1: Understanding Column Specifications

The basic syntax for column parsing is:

```
-f 'cols:field1 field2 *rest'
```

Each token in the specification describes how many delimited columns to consume and which field name to assign.

**Token types:**

- `field` - Consume a single column into `field`
- `field(N)` - Consume `N` columns (joined with spaces or the provided separator)
- `-` / `-(N)` - Skip one or `N` columns entirely
- `*field` - Capture every remaining column into `field` (must be last)
- `field:type` - Apply a type annotation (`int`, `float`, `bool`, `string`) after extraction

Combine these building blocks to describe almost any column-based log format.

### Example Log Format

Let's start with a simple application log:

```
ERROR api Connection failed
INFO  db  Query executed
WARN  api Slow response time
```

Each line has three whitespace-delimited fields: level, service, message.

## Step 2: Basic Whitespace-Delimited Parsing

For whitespace-delimited fields, just list the field names:

=== "Command"

    ```bash
    kelora -f 'cols:ts(2) level *msg' examples/cols_fixed.log -n 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f 'cols:ts(2) level *msg' examples/cols_fixed.log -n 3
    ```

**How it works:**

- `level` - First whitespace-delimited column
- `service` - Second column
- `message` - Third column

## Step 3: Combining Multiple Columns

Sometimes a field spans more than one column. Use the `(N)` suffix to join multiple tokens.

```bash
echo "2024-01-15 10:30:00 INFO Connection failed" | \
    kelora -f 'cols:timestamp(2) level *message'
```

**How it works:**

- `timestamp(2)` - Consumes the first two columns (`2024-01-15` and `10:30:00`)
- `level` - Third column
- `*message` - Everything else in the line

## Step 4: Adding Type Annotations

Convert fields to specific types using annotations:

```bash
echo "200 1234 0.123
404 5678 0.456
500 9012 0.789" | \
    kelora -f 'cols:status:int bytes:int response_time:float'
```

**Supported types:**

- `int` - Integer (`i64`) conversion
- `float` - Floating-point (`f64`) conversion
- `bool` - Boolean conversion (`true/false`, `yes/no`, `1/0`)
- `string` - Explicitly keep as string (useful when mixing annotations)

After type conversion, you can use numeric operations immediately:

```bash
echo "200 1234 0.123" | \
    kelora -f 'cols:status:int bytes:int response_time:float' \
    --filter 'e.status >= 400'
```

## Step 5: Handling Timestamps

Kelora automatically looks for common timestamp field names such as `timestamp`, `ts`, or `time`. You can also point it at a specific field with `--ts-field` and describe the format with `--ts-format` when needed.

```bash
echo "2024-01-15T10:30:00Z ERROR Connection failed" | \
    kelora -f 'cols:timestamp level *message' --ts-field timestamp
```

Kelora will parse the `timestamp` field so that `--since`, `--until`, and timestamp-aware formatting work. If your timestamps do not include a timezone, provide `--ts-format` and optionally `--input-tz`:

```bash
kelora -f 'cols:timestamp(2) level *message' \
    --ts-field timestamp \
    --ts-format '%Y-%m-%d %H:%M:%S' \
    --input-tz 'UTC'
```

## Step 6: Custom Separators

For non-whitespace separators, use `--cols-sep`:

=== "Command"

    ```bash
    # Multi-character separator with prefix extraction
    kelora --extract-prefix node --prefix-sep ' >>> ' \
        -f 'cols:timestamp *message' \
        examples/prefix_custom.log -n 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    # Multi-character separator with prefix extraction
    kelora --extract-prefix node --prefix-sep ' >>> ' \
        -f 'cols:timestamp *message' \
        examples/prefix_custom.log -n 5
    ```

Works with any separator string:

```bash
# Comma-separated
kelora -f 'cols:level service message' --cols-sep ','

# Tab-separated
kelora -f 'cols:level service message' --cols-sep $'\t'

# Multi-character separator
kelora -f 'cols:level service message' --cols-sep ' :: '
```

## Step 7: Real-World Example - Application Logs

Let's parse a custom application log format:

**Log format:**
```
[2024-01-15 10:30:00] ERROR api Connection failed to database
[2024-01-15 10:30:01] INFO  db  Query executed successfully
[2024-01-15 10:30:02] WARN  api Slow response: 2500ms
```

**Column specification:**
```bash
kelora -f 'cols:raw_ts level service *message' app.log
```

But the timestamp is wrapped in brackets. We need to clean it up.

### Using Regex in Exec

```bash
kelora -f 'cols:raw_ts level service *message' app.log \
    -e 'e.timestamp = e.raw_ts.extract_re(r"\[(.*?)\]", 1)' \
    -e 'e.raw_ts = ()' \
    --ts-field timestamp \
    -k timestamp,level,service,message
```

**What this does:**

1. Parse `raw_ts` as the first field (including brackets)
2. Extract the timestamp using regex
3. Remove the temporary `raw_ts` field
4. Output cleaned fields

## Step 8: Combining with Transformations

Parse a custom format and add computed fields:

```bash
cat app.log | \
    kelora -f 'cols:timestamp level service *message' \
    --ts-field timestamp \
    -e 'if e.message.contains("ms") { e.duration = e.message.extract_re(r"(\d+)ms", 1).to_int() }' \
    --filter 'e.has_path("duration") && e.duration > 1000' \
    -k timestamp,service,duration,message
```

**Pipeline:**

1. Parse custom format
2. Extract duration from the message if present
3. Filter for slow requests (>1000ms)
4. Output relevant fields

## Step 9: Working with Mixed Columns

Some logs have consistent positions but variable content:

```
ERROR  2024-01-15 Connection failed
INFO   2024-01-15 Query OK
WARN   2024-01-15 High memory usage
```

Use whitespace delimiters for flexibility:

```bash
kelora -f 'cols:level date message' app.log
```

If you need to ignore specific columns, use skip tokens and multi-column captures:

```bash
kelora -f 'cols:level - message(2)' app.log
```

## Step 10: Complete Example

Let's parse a complex custom format with everything we've learned:

**Input log (custom_app.log):**
```
[2024-01-15 10:30:00] 200 api    1234 0.123 Success
[2024-01-15 10:30:01] 404 web    5678 0.456 Not found
[2024-01-15 10:30:02] 500 api    9012 0.789 Server error
```

**Parse specification:**
```bash
kelora -f 'cols:raw_ts status:int service bytes:int latency:float *message' custom_app.log \
    -e 'e.timestamp = e.raw_ts.extract_re(r"\[(.*?)\]", 1)' \
    -e 'e.raw_ts = ()' \
    --ts-field timestamp \
    -e 'e.is_error = e.status >= 400' \
    -e 'e.is_slow = e.latency > 0.5' \
    --filter 'e.is_error || e.is_slow' \
    -k timestamp,status,service,message
```

**What happens:**

1. Parse fields with type annotations
2. Extract the timestamp from brackets
3. Mark HTTP errors (`>= 400`) and slow responses
4. Filter for problematic events
5. Output cleaned fields

## Common Patterns

### Pattern 1: Log Level + Message

```bash
kelora -f 'cols:level message' app.log -l error,warn
```

### Pattern 2: Timestamp + Level + Service + Message

```bash
kelora -f 'cols:timestamp level service *message' app.log \
    --ts-field timestamp \
    --filter 'e.level == "ERROR"'
```

### Pattern 3: Type Conversion with Greedy Capture

```bash
kelora -f 'cols:status:int bytes:int duration:float *path' access.log \
    --filter 'e.status >= 500' \
    -e 'track_avg("latency", e.duration)' \
    -m
```

### Pattern 4: Extract and Transform

```bash
kelora -f 'cols:timestamp level *data' app.log \
    --ts-field timestamp \
    -e 'e.values = e.data.split(",")' \
    -e 'e.count = e.values.len()' \
    -k timestamp,level,count
```

## Tips and Best Practices

### Use Whitespace Delimiters When Possible

Simpler and more flexible:

```bash
# Good - whitespace delimited
kelora -f 'cols:level service message' app.log

# Skip columns rather than relying on alignment
kelora -f 'cols:level - *message' app.log
```

### Use Type Annotations Early

Convert types during parsing, not in exec:

```bash
# Good - parse as int
kelora -f 'cols:status:int bytes:int' --filter 'e.status >= 400'

# Less efficient - convert in exec
kelora -f 'cols:status bytes' -e 'e.status = e.status.to_int()' --filter 'e.status >= 400'
```

### Name Fields Descriptively

```bash
# Good
kelora -f 'cols:timestamp level service *message'

# Less clear
kelora -f 'cols:col1 col2 col3 col4'
```

### Use Greedy Capture for Messages

Always use `*` for the last field if it's a free-form message:

```bash
kelora -f 'cols:level service *message'
```

### Combine with Prefix Extraction

For Docker Compose-style logs:

```bash
docker compose logs | \
    kelora --extract-prefix container \
           -f 'cols:timestamp level *message' \
           --ts-field timestamp
```

### More Recipes to Practice

CSV with type annotations:

=== "Command"

    ```bash
    kelora -f "csv status:int bytes:int duration_ms:int" examples/simple_csv.csv
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f "csv status:int bytes:int duration_ms:int" examples/simple_csv.csv
    ```

TSV with type annotations:

=== "Command"

    ```bash
    kelora -f "tsv: user_id:int success:bool" examples/simple_tsv.tsv
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f "tsv: user_id:int success:bool" examples/simple_tsv.tsv
    ```

Columns format with multi-token timestamp:

=== "Command"

    ```bash
    kelora -f "cols:ts(2) level *msg:string" examples/cols_fixed.log
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f "cols:ts(2) level *msg:string" examples/cols_fixed.log
    ```

Strict mode for catching malformed rows:

=== "Command"

    ```bash
    kelora -f "csv status:int" --strict examples/errors_csv_ragged.csv
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f "csv status:int" --strict examples/errors_csv_ragged.csv
    ```

Inline type annotations and strict mode are perfect for catching malformed rows during ingestion before they reach downstream systems.

## Troubleshooting

### Fields Not Parsing Correctly

**Problem:** Fields are misaligned or missing.

**Solution:** Check separators and column counts:
```bash
# Debug by outputting all fields
kelora -f 'cols:field1 field2 *field3' app.log -n 3

# Try different separator
kelora -f 'cols:field1 field2 field3' --cols-sep '|' app.log -n 3
```

### Timestamp Not Recognized

**Problem:** Timestamp field not working with `--since`.

**Solution:** Tell Kelora which field holds the timestamp and provide the format if needed:
```bash
kelora -f 'cols:timestamp level *message' app.log \
    --ts-field timestamp \
    --ts-format '%Y-%m-%d %H:%M:%S'
```

### Type Conversion Failures

**Problem:** Integer/float conversions failing.

**Solution:** Use `get_path` with defaults in resilient mode:
```bash
kelora -f 'cols:status bytes' -e 'e.status_int = e.status.to_int_or(0)'
```

### Greedy Field Capturing Too Much

**Problem:** Last field includes separators.

**Solution:** Be specific with earlier fields:
```bash
kelora -f 'cols:level service message(2) *remainder'
```

## Next Steps

Now that you understand custom format parsing, explore:

- **[Working with Time](working-with-time.md)** - Advanced timestamp handling
- **[Format Reference](../reference/formats.md)** - Complete format documentation
- **[Advanced Scripting](advanced-scripting.md)** - Advanced event transformation
