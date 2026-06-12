# Format Reference

Quick reference for all formats supported by Kelora.

## Input Formats

Specify input format with `-f, --input-format <format>`.

### Overview

| Format | Description |
|--------|----------|
| `auto` | Auto-detect from first non-empty line (default) |
| `json` | Application logs, structured data (shorthand: `-j`) |
| `line` | Unstructured logs, raw text |
| `logfmt` | Heroku-style logs, simple structured logs |
| `csv` / `tsv` | Spreadsheet data, exports |
| `syslog` | System logs, network devices |
| `combined` | Apache/Nginx web server access logs |
| `cef` | ArcSight Common Event Format, SIEM data |
| `cols:<spec>` | Custom column-based logs |
| `regex:<pattern>` | Custom regex parsing with named groups and type annotations |
| `<fmt1>,<fmt2>[,…]` | Cascade mode — try parsers in order, first success wins (e.g. `json,line`) |

### JSON Format

**Syntax:** `-f json` or `-j`

**Description:** JSON Lines format (one object per line). Nested structures preserved.

**Input Example:**
```json
{"timestamp": "2024-01-15T10:30:00Z", "level": "ERROR", "service": "api", "message": "Connection failed"}
```

**Output Fields:** All JSON fields become event fields with original names and types.

**Notes:**

- Use `-M json` for multi-line JSON objects
- Preserves field types (strings, numbers, booleans, null)
- Supports nested objects and arrays

### Line Format

**Syntax:** `-f line`

**Description:** Plain text, one line per event.

**Output Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `line` | String | Complete line content |

**Notes:**

- Auto-detect falls back to line when it can't identify a structured format
- Empty lines are skipped
- Useful for unstructured logs or custom parsing with `--exec`

### Logfmt Format

**Syntax:** `-f logfmt`

**Description:** Heroku-style key-value pairs.

**Input Example:**
```
timestamp=2024-01-15T10:30:00Z level=ERROR service=api message="Connection failed"
```

**Output Fields:** All key-value pairs become top-level fields.

**Notes:**

- Supports quoted values: `key="value with spaces"`
- Keys must be alphanumeric (with underscores/hyphens)

### CSV / TSV Formats

**Syntax:**

- `-f csv` - Comma-separated with header
- `-f tsv` - Tab-separated with header
- `-f csvnh` - CSV without header
- `-f tsvnh` - TSV without header

**Output Fields:**

- **With header:** Field names from header row
- **Without header:** `c1`, `c2`, `c3`, etc.

**Type Annotations:**

Specify field types for automatic conversion:
```bash
kelora -f 'csv status:int bytes:int response_time:float' access.csv
```

Supported types: `int`, `float`, `bool`

**Notes:**

- Quoted fields supported: `"value, with, commas"`
- Escaped quotes: `"value with ""quotes"""`
- Ragged rows are preserved, not dropped: columns beyond the header (or beyond
  the first row in header-less mode) are kept under positional names (`c5`,
  `c6`, ... counted from 1), and rows with fewer columns leave the trailing
  fields absent. Both cases are counted and reported as a hint on stderr.
- `--strict` rejects ragged rows as parse errors instead.

### Syslog Format

**Syntax:** `-f syslog`

**Description:** RFC5424 and RFC3164 syslog messages. Auto-detects format.

**Input Examples:**

RFC5424:
```
<165>1 2024-01-15T10:30:00.000Z myhost myapp 1234 ID47 - Connection failed
```

RFC3164:
```
<34>Jan 15 10:30:00 myhost myapp[1234]: Connection failed
```

**Output Fields:**

| Field | Type | RFC5424 | RFC3164 | Description |
|-------|------|---------|---------|-------------|
| `pri` | Integer | ✓ | ✓* | Priority value (facility * 8 + severity) |
| `facility` | Integer | ✓ | ✓* | Syslog facility code |
| `severity` | Integer | ✓ | ✓* | Severity level (0-7) |
| `level` | String | ✓ | ✓* | Log level (EMERG, ALERT, CRIT, ERROR, WARN, NOTICE, INFO, DEBUG) |
| `ts` | String | ✓ | ✓ | Parsed timestamp |
| `host` | String | ✓ | ✓ | Source hostname |
| `prog` | String | ✓ | ✓ | Application/program name |
| `pid` | Integer/String | ✓ | ✓ | Process ID (parsed as integer if numeric) |
| `msgid` | String | ✓ | - | Message ID |
| `version` | Integer | ✓ | - | Syslog protocol version |
| `msg` | String | ✓ | ✓ | Log message |

*RFC3164: Only present if priority prefix `<NNN>` is included

**Notes:**

- Severity levels: 0=emerg, 1=alert, 2=crit, 3=err, 4=warn, 5=notice, 6=info, 7=debug

### Combined Log Format

**Syntax:** `-f combined`

**Description:** Apache/Nginx web server logs. Auto-handles three variants:

- Apache Common Log Format (CLF)
- Apache Combined Log Format
- Nginx Combined with request_time

**Input Examples:**

Common:
```
192.168.1.1 - user [15/Jan/2024:10:30:00 +0000] "GET /index.html HTTP/1.0" 200 1234
```

Combined:
```
192.168.1.1 - user [15/Jan/2024:10:30:00 +0000] "GET /api/data HTTP/1.1" 200 1234 "http://example.com/" "Mozilla/5.0"
```

Nginx with request_time:
```
192.168.1.1 - - [15/Jan/2024:10:30:00 +0000] "GET /api/data HTTP/1.1" 200 1234 "-" "curl/7.68.0" "0.123"
```

**Output Fields:**

| Field | Type | Common | Combined | Nginx | Description |
|-------|------|--------|----------|-------|-------------|
| `ip` | String | ✓ | ✓ | ✓ | Client IP address |
| `identity` | String | ✓ | ✓ | ✓ | RFC 1413 identity (omit if `-`) |
| `user` | String | ✓ | ✓ | ✓ | HTTP auth username (omit if `-`) |
| `ts` | String | ✓ | ✓ | ✓ | Request timestamp |
| `request` | String | ✓ | ✓ | ✓ | Full HTTP request line |
| `method` | String | ✓ | ✓ | ✓ | HTTP method (auto-extracted) |
| `path` | String | ✓ | ✓ | ✓ | Request path (auto-extracted) |
| `protocol` | String | ✓ | ✓ | ✓ | HTTP protocol (auto-extracted) |
| `status` | Integer | ✓ | ✓ | ✓ | HTTP status code |
| `bytes` | Integer | ✓ | ✓ | ✓ | Response size (omit if `-`, keep if `0`) |
| `referer` | String | - | ✓ | ✓ | HTTP referer (omit if `-`) |
| `user_agent` | String | - | ✓ | ✓ | HTTP user agent (omit if `-`) |
| `request_time` | Float | - | - | ✓ | Request time in seconds (omit if `-`) |

**Notes:**

- Parser auto-detects variant per line
- Fields with `-` values omitted (except `bytes` includes `0`)

### CEF Format

**Syntax:** `-f cef`

**Description:** ArcSight Common Event Format for security logs.

**Input Example:**
```
CEF:0|Security|threatmanager|1.0|100|worm successfully stopped|10|src=10.0.0.1 dst=2.1.2.2 spt=1232
```

**Output Fields:**

**Syslog prefix (optional):**

| Field | Type | Description |
|-------|------|-------------|
| `ts` | String | Timestamp from syslog prefix |
| `host` | String | Hostname from syslog prefix |

**CEF header:**

| Field | Type | Description |
|-------|------|-------------|
| `cefver` | String | CEF format version |
| `vendor` | String | Device vendor name |
| `product` | String | Device product name |
| `version` | String | Device version |
| `eventid` | String | Event signature ID |
| `event` | String | Event name/classification |
| `severity` | String | Event severity (0-10) |

**Extensions:** All extension key=value pairs become top-level fields with automatic type conversion (integers, floats, booleans)

### Column Format

**Syntax:** `-f 'cols:<spec>'`

**Description:** Custom column-based parsing with whitespace (or custom separator) splitting.

**Separator:** Use `--cols-sep <separator>` for custom separators (default: whitespace)

**Specification Syntax:**

- `field` - Consume one column
- `field(N)` - Consume N columns and join
- `-` or `-(N)` - Skip one or N columns
- `*field` - Capture remaining columns (must be last)
- `field:type` - Apply type annotation (`int`, `float`, `bool`, `string`)

**Examples:**

Simple fields:
```bash
# Input: ERROR api "Connection failed"
kelora -f 'cols:level service *msg' app.log
```

Multi-token timestamp:
```bash
# Input: 2024-01-15 10:30:00 INFO Connection failed
kelora -f 'cols:ts(2) level *msg' app.log --ts-field ts
```

Custom separator:
```bash
# Input: name|age|city
kelora -f 'cols:name age:int city' --cols-sep '|' data.txt
```

**Output Fields:** Field names from specification with applied type conversions.

**Notes:**

- `*field` must be the final token

### Regex Format

**Syntax:** `-f 'regex:<pattern>'`

**Description:** Parse logs using regular expressions with named capture groups and optional type annotations.

**Pattern Syntax:**

- `(?P<name>pattern)` - Named capture group (field stored as string)
- `(?P<name:type>pattern)` - Named capture group with type annotation

**Supported Types:** `int`, `float`, `bool` (lowercase only)

**Examples:**

Simple extraction:
```bash
# Input: 404 Not found
kelora -f 'regex:(?P<code:int>\d+) (?P<msg>.*)' app.log
```

Structured logs:
```bash
# Input: 2025-01-15T10:00:00Z [ERROR] Database connection failed
kelora -f 'regex:^(?P<ts>\S+) \[(?P<level>\w+)\] (?P<msg>.+)$' app.log
```

Apache-style logs with typed fields:
```bash
# Input: 192.168.1.1 - - [15/Jan/2025:10:00:00 +0000] "GET /api/users HTTP/1.1" 200 1234
kelora -f 'regex:^(?P<ip>\S+) - - \[(?P<timestamp>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+) HTTP/[\d.]+" (?P<status:int>\d+) (?P<bytes:int>\d+)$' access.log
```

**Output Fields:** Field names from capture groups with applied type conversions.

**Behavior:**

- **Full-line matching:** Pattern implicitly anchored with `^...$`
- **Empty captures:** Skipped (not stored as fields)
- **Non-matching lines:**
    - Default (lenient): Returns error, line skipped, processing continues
    - With `--strict`: Returns error, processing halts
- **Type conversion failures** (e.g., `"abc"` for `:int`):
    - Default (lenient): Automatically falls back to storing as string
    - With `--strict`: Returns error, processing halts

**Reserved Field Names:**

The following names cannot be used: `original_line`, `parsed_ts`, `fields`

**Limitations:**

- Nested named capture groups are not supported
- Type annotations must be lowercase (`:int`, not `:INT`)

**Notes:**

- Use raw strings in shell to avoid escaping issues: `-f 'regex:...'`
- Combine with `--ts-field` to specify which field contains the timestamp
- Non-capturing groups `(?:...)` are supported

### Auto-Detection

**Syntax:** `-f auto`

**Description:** Automatically detect format from first non-empty line.

**Detection Order:**

1. JSON (starts with `{`)
2. Syslog (starts with `<NNN>`)
3. CEF (starts with `CEF:`)
4. Combined (matches Apache/Nginx pattern)
5. Logfmt (contains `key=value` pairs)
6. CSV (contains commas with consistent pattern)
7. Line (fallback)

**Notes:**

- Detects once, applies to all lines
- Not suitable for mixed-format files — use **cascade mode** instead

### Auto-Detection Per File

**Syntax:** `-f auto-per-file`

**Description:** Automatically detect format from the first non-empty line of
each input file, then apply that parser to the rest of the file.

**Good fit:** Batch runs where each file is internally consistent, but
different files use different formats.

**Example Usage:**
```bash
# JSON app logs and logfmt worker logs in one invocation
kelora -f auto-per-file -J logs/api/*.log logs/workers/*.log

# Aggregate error events across mixed file formats without splitting first
kelora -f auto-per-file --levels error,warn logs/**/* -J
```

**Notes:**

- Detects once per file
- Uses the same first-non-empty-line semantics as `-f auto`
- For mixed formats within a single file, use **cascade mode** instead
- Not supported with `--parallel` or `--merge-sorted`

### Cascade Mode

**Syntax:** `-f <fmt1>,<fmt2>[,…]` (comma-separated list of simple formats)

**Description:** Try each parser in order on every line; the first one that
succeeds handles the event. Designed for the common "noisy JSON" case —
structured logs with plain-text noise (stack traces, panics, startup banners)
interspersed — without requiring users to split the stream with `grep` first.

**Input Example:**
```
{"level":"info","msg":"hello"}
Server starting on port 8080
{"level":"error","msg":"connection refused"}
java.lang.NullPointerException
```

**Example Usage:**
```bash
# Noisy JSON with plain-text fallback
kelora -f json,line app.log

# Three-way cascade
kelora -f json,logfmt,line mixed.log

# Segment downstream by how each event was parsed
kelora -f json,line app.log --filter 'e._format == "line"'

# See per-format breakdown
kelora -f json,line app.log --stats
```

**The `_format` field:** Every event emitted in cascade mode gets a
`_format` field naming the winning parser (e.g. `"json"`, `"line"`). This
field is **only** added in cascade mode — single-format runs are unchanged.
Filter or group by it in Rhai, or inspect it in the output to debug
classification.

**Diagnostic counts:** With `--stats`, cascade mode adds a per-format
breakdown so silent misclassification surfaces immediately:

```
Cascade formats: json=9812, line=23
```

**Allowed in a comma list:** `json`, `line`, `raw`, `logfmt`, `syslog`,
`cef`, `combined`.

**Not allowed in a comma list** (rejected at CLI parse time):

- `auto` — meaningless inside a cascade list; list the formats explicitly
- `csv`, `tsv`, `csvnh`, `tsvnh` — schema-based; headers/types can't safely
  change mid-stream
- `cols:<spec>`, `regex:<pattern>` — a regex pattern may itself contain
  commas, so commas can't safely delimit them. Use **repeated `-f`** instead.

**Cascades with `cols:`/`regex:` — use repeated `-f`.** Pass one `-f` per
format and they are tried in order, exactly like a comma list, but each spec
is taken whole so `cols:`/`regex:` work as members:

```bash
# JSON lines plus a 'timestamp LEVEL message' app log in one file
kelora -f json -f 'cols:ts(2) level *msg' app.log

# Selective regex first, raw line as the catch-all for anything else
kelora -f json -f 'regex:(?P<ts>\S+ \S+) (?P<level>\w+) (?P<msg>.*)' -f line app.log
```

A comma list and repeated `-f` can be combined; comma-list members are
flattened into the cascade in order.

**Catch-alls go last.** `line`, `raw`, and `cols:` match essentially every
line (in resilient mode `cols:` fills missing fields with `()` rather than
failing), so anything listed after them would never run — Kelora rejects that
ordering. `regex:` is selective: it declines non-matching lines, so it may sit
earlier in the cascade and fall through to a later catch-all (as in the
example above, where stack-trace lines that don't match the regex are kept by
`line`).

**Ordering matters.** The first parser that returns `Ok` wins, so list
high-confidence formats first and use `line` as the terminal fallback.
Liberal grammars (like `logfmt`, which accepts any `key=value` substring)
should come *after* stricter ones to avoid swallowing events that a later
parser would handle correctly.

**Multiline:** Multiline chunking is format-aware and runs *before* parsing,
so it follows the first listed format's strategy. Per-chunk
reclassification is intentionally not supported.

**Notes:**

- Adds ~5–10% overhead per line vs. a single parser (one extra parse
  attempt on fall-through)
- Safe to combine with `--filter`, `--select`, `--stats`, `--metrics`, and
  Rhai scripting
- Works in both sequential and parallel modes

## Output Formats

Specify output format with `-F, --output-format <format>`.

| Format | Description |
|--------|-------------|
| `default` | Key-value format with colors |
| `json` | JSON lines (one object per line) |
| `logfmt` | Key-value pairs (logfmt format) |
| `inspect` | Debug format with type information |
| `levelmap` | Events grouped by log level |
| `keymap` | Shows first character of specified field (requires `--keys` with exactly one field) |
| `tailmap` | Visualizes numeric field distributions with percentile thresholds (requires `--keys` with exactly one numeric field) |
| `csv` | CSV with header row |
| `tsv` | Tab-separated values with header row |
| `csvnh` | CSV without header |
| `tsvnh` | TSV without header |

Use `-q/--quiet` to suppress output (implied by `--stats` and `--metrics`).

**Levelmap Visual Example:**

![Levelmap output format showing compact log visualization](../screenshots/levelmap.gif)

The `levelmap` format provides a compact visual representation of logs, showing timestamps and level indicators in a condensed format ideal for quick scanning.

**Keymap Format:**

The `keymap` format works similarly to `levelmap` but displays the first character of any specified field instead of being limited to log levels. This is useful for visualizing patterns in custom fields like HTTP methods, status codes, user types, etc.

- Requires `--keys` (or `-k`) with exactly one field name
- Shows the first character of the field value (converted to string for non-string fields)
- Displays `.` for empty or missing field values
- Groups events by timestamp like `levelmap`
- Not compatible with `--parallel` mode

**Tailmap Format:**

The `tailmap` format visualizes numeric field distributions over time using tail-focused percentile thresholds (p90, p95, p99). This is ideal for performance monitoring, latency analysis, and identifying outliers.

- Requires `--keys` (or `-k`) with exactly one numeric field name
- Uses symbols: `_` (below p90), `1` (p90-p95), `2` (p95-p99), `3` (above p99), `.` (missing)
- Shows a summary with field statistics and percentile thresholds
- Groups events by timestamp for timeline visualization
- Not compatible with `--parallel` mode

**Use cases:**
- API response time analysis
- Database query performance monitoring
- Request latency tracking
- Any time-series numeric data where tail latencies matter

**Examples:**
```bash
kelora -j app.log -F json                      # Output as JSON
kelora -j app.log -F csv --keys ts,level,msg   # Output as CSV
kelora -F keymap -k method access.log          # Show HTTP method patterns
kelora -F keymap --keys status api.log         # Show status field patterns
kelora -F tailmap -k response_time api.log     # Visualize response time distribution
kelora -F tailmap --keys query_time_ms db.log  # Show database query performance
kelora -j app.log --stats                      # Only stats
```

## See Also

- [CLI Reference](cli-reference.md) - Complete flag documentation including timestamp parsing, multiline strategies, and prefix extraction
- [Quickstart](../quickstart.md) - Format examples with annotated output
- [Parsing Custom Formats Tutorial](../tutorials/parsing-custom-formats.md) - Step-by-step guide
- [Prepare CSV Exports for Analytics](../how-to/process-csv-data.md) - CSV-specific tips
