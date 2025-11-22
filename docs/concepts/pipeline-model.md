# Processing Architecture

Understanding how Kelora processes logs through its multi-layer architecture.

## Overview

Kelora's processing model consists of three distinct layers operating on different data types:

1. **Input Layer** - File/stdin handling and decompression
2. **Line-Level Processing** - Raw string filtering and event boundary detection
3. **Event-Level Processing** - Structured data transformation and output

This layered architecture enables efficient streaming with low memory usage while supporting both sequential and parallel processing modes.

---

## Layer 1: Input Layer

### Input Sources

**Stdin Mode:**

- Activated when no files specified or file is `"-"`
- Background thread reads from stdin via channel
- Supports one stdin source (error if `"-"` appears multiple times)
- Useful for piping: `tail -f app.log | kelora -j`

**File Mode:**

- Processes one or more files sequentially
- Tracks current filename for context
- Supports `--file-order` for processing sequence:
  - `cli` (default) - Process in CLI argument order
  - `name` - Sort alphabetically
  - `mtime` - Sort by modification time (oldest first)

**Examples:**
```bash
# Stdin mode
tail -f app.log | kelora -j

# File mode with ordering
kelora *.log --file-order mtime

# Mixed stdin and files
kelora file1.log - file2.log  # stdin in middle
```

### Automatic Decompression

Kelora automatically detects and decompresses compressed input using **magic bytes detection** (not file extensions):

**Supported Formats:**

- **Gzip** - Magic bytes `1F 8B 08` (`.gz` files or gzipped stdin)
- **Zstd** - Magic bytes `28 B5 2F FD` (`.zst` files or zstd stdin)
- **Plain** - No magic bytes, passthrough

**Behavior:**

- Transparent decompression before any processing
- Works on both files and stdin
- ZIP files explicitly rejected with error message
- Decompression happens in Input Layer

**Examples:**
```bash
kelora app.log.gz                    # Auto-detected gzip
kelora app.log.zst --parallel        # Auto-detected zstd
gzip -c app.log | kelora -j          # Gzipped stdin
```

### Reader Threading

**Sequential Mode:**

- Spawns background reader thread
- Sends lines via bounded channel (1024 line buffer)
- Main thread processes lines one at a time
- Supports multiline timeout flush (default: 200ms)

**Parallel Mode:**

- Reader batches lines (default: 1000 lines, 200ms timeout)
- Worker pool processes batches concurrently
- No cross-batch state (impacts multiline, spans)

---

## Layer 2: Line-Level Processing

Operations on raw string lines **before** parsing into events.

### Line Skipping (`--skip-lines`)

Skip first N lines from input (useful for CSV headers, preambles).

```bash
kelora data.csv --skip-lines 1
```

### Line Filtering (`--ignore-lines`, `--keep-lines`)

Regex-based filtering on raw lines before parsing:

- `--ignore-lines <REGEX>` - Skip lines matching pattern
- `--keep-lines <REGEX>` - Keep only lines matching pattern

**Resilient mode:** Skip non-matching lines, continue processing
**Strict mode:** Abort on regex error

```bash
# Ignore health checks before parsing
kelora access.log --ignore-lines 'health-check'

# Keep only lines starting with timestamp
kelora app.log --keep-lines '^\d{4}-\d{2}-\d{2}'
```

### Section Selection

Extract specific sections from logs based on start/end markers:

**Flags:**

- `--section-after <REGEX>` - Begin section (exclude marker line)
- `--section-from <REGEX>` - Begin section (include marker line)
- `--section-through <REGEX>` - End section (include marker line)
- `--section-before <REGEX>` - End section (exclude marker line)
- `--max-sections <N>` - Limit number of sections

**State Machine:**
```
NotStarted → (match start) → InSection → (match end) → BetweenSections → ...
```

**Example:**
```bash
# Extract sections between markers
kelora system.log \
    --section-from '=== Test Started ===' \
    --section-through '=== Test Completed ==='
```

### Event Aggregation (Multiline)

Detects event boundaries to combine multiple lines into single events **before parsing**.

**Four Strategies:**

**1. Timestamp Strategy** (auto-detect timestamp headers)
```bash
kelora app.log -M timestamp
kelora app.log -M 'timestamp:format=%Y-%m-%d %H-%M-%S'
```
Detects lines starting with timestamps as new events. Continuation lines (stack traces, wrapped messages) are appended to current event.

**2. Indent Strategy** (whitespace continuation)
```bash
kelora app.log -M indent
```
Lines starting with whitespace are continuations of previous event.

**3. Regex Strategy** (custom patterns)
```bash
kelora app.log -M 'regex:match=^\['
kelora app.log -M 'regex:match=^\[:end=^\['
```
Define custom start/end patterns for event boundaries using `match=` (required) and `end=` (optional) segments within the `-M` argument.

**4. All Strategy** (entire input as one event)
```bash
kelora config.json -M all
```
Buffers entire input as single event (use for structured files).

*Note:* The current CLI treats `:` as an option separator inside the `-M` value. For regex patterns, encode literal colons (for example `\x3A`). Timestamp hints that require `:` currently need pre-normalised input or a regex-based strategy.

**Multiline Timeout:**

- Sequential mode: Flush incomplete events after timeout (default: 200ms)
- Parallel mode: Flush at batch boundaries (no timeout)

**Critical:** Multiline creates event boundaries before parsing. Each complete event string is then parsed into structured data.

---

## Layer 3: Event-Level Processing

Operations on parsed events (maps/objects).

### Parsing

Convert complete event strings into structured maps:

```
Event string → Parser → Event map (e.field accessible)
```

Parsers: `json`, `logfmt`, `syslog`, `combined`, `csv`, `tsv`, `cols`, etc.

### Script Stages (Pipeline Core)

**User-controlled stages execute exactly where you place them on the CLI:**

- `--filter <EXPR>` – Boolean filter (true = keep, false = skip)
- `--levels/-l <LIST>` – Include log levels (case-insensitive, repeatable)
- `--exclude-levels/-L <LIST>` – Exclude log levels (case-insensitive, repeatable)
- `--exec <SCRIPT>` – Transform/process event
- `--exec-file <PATH>` – Execute script from file (alias: `-E`)

You can mix and repeat these flags; each stage sees the output of the previous one.

**Example:**
```bash
kelora -j app.log \
    --levels error,critical \        # Stage 1: Level filter
    --filter 'e.status >= 400' \     # Stage 2: Filter
    --exec 'e.alert = true' \        # Stage 3: Exec (only 4xx/5xx errors)
    --exclude-levels debug \         # Stage 4: Remove any downgraded events
    --exec 'track_count(e.path)'     # Stage 5: Exec (track surviving paths)
```

Each stage processes the output of the previous stage sequentially.

### Complete Stage Ordering

**User-controlled stages** (run in the order you specify them on the CLI):

1. `--filter`, `--levels`, `--exclude-levels`, `--exec`, `--exec-file`

**Fixed-position filters** (always run after user-controlled stages, regardless of CLI order):

2. **Timestamp filtering** – `--since`, `--until`
3. **Key filtering** – `--keys`, `--exclude-keys`

Place `--levels` before heavy transforms to prune work early, or add another `--levels` after a script if you synthesise a level field there.

### Span Processing

Groups events into spans for aggregation:

**Count-based Spans:**
```bash
kelora -j app.log --span 100 \
    --span-close 'print("Span complete: " + meta.span_id)'
```
Closes span every N events that pass filters.

**Time-based Spans:**
```bash
kelora -j app.log --span 5m \
    --span-close 'track_sum("requests", span.size)'
```
Closes span on aligned time windows (5m, 1h, 30s, etc.).

**Span Processing Flow:**

1. Event passes through filters/execs
2. Span processor assigns `span_id` and `SpanStatus`
3. Event processed with span context
4. When span closes → `--span-close` hook executes
5. Hook has access to `meta.span_id`, `meta.span_start`, `meta.span_end`, `metrics`

**Constraints:**

- Spans force sequential mode (incompatible with `--parallel`)
- Span state maintained across events

### Begin and End Stages

**`--begin`:** Execute once before processing any events
**`--end`:** Execute once after all events processed

```bash
kelora -j app.log \
    --begin 'print("Starting analysis")' \
    --exec 'track_count(e.service)' \
    --end 'print("Services seen: " + metrics.len())' \
    --metrics
```

In parallel mode:

- `--begin` runs sequentially before worker pool starts
- `--end` runs sequentially after workers complete (with merged metrics)

### Context Lines

Show surrounding lines around matches:

- `--before-context N` / `-B N` - Show N lines before match
- `--after-context N` / `-A N` - Show N lines after match
- `--context N` / `-C N` - Show N lines before and after

Requires active filtering (--filter, --levels, --since, etc.).

```bash
kelora -j app.log \
    --filter 'e.level == "ERROR"' \
    --before-context 2 \
    --after-context 2
```

### Output Stage

Format and emit events:

- Apply `--keys` field selection
- Convert timestamps (--normalize-ts, --show-ts-local, --show-ts-utc)
- Format output (--output-format: default, json, csv, etc.)
- Apply `--take` limit
- Write to stdout or files

```bash
kelora -j app.log \
    --keys timestamp,service,message \
    -F json \
    --take 100
```

---

## Parallel Processing Model

Kelora's `--parallel` mode is **batch-parallel**, not stage-parallel.

### Architecture

```
Sequential:  Line → Line filters → Multiline → Parse → Script stages → Output
             (one at a time)

Parallel:    Batch of lines → Worker pool
             Each worker: Line filters → Multiline → Parse → Script stages
             Results → Ordering buffer → Output
```

Where:

- **Line filters** = `--skip-lines`, `--ignore-lines`, `--section-start`, etc.
- **Multiline** = Event boundary detection (aggregates multiple lines into events)
- **Script stages** = `--filter` and `--exec` in CLI order

### How It Works

1. **Reader thread** batches lines (default: 1000 lines, 200ms timeout)
2. **Worker pool** processes batches independently (default: CPU count workers)
3. Each worker has its own Pipeline instance
4. **Results merged** with ordering preservation (default) or unordered (`--unordered`)
5. **Stats/metrics merged** from all workers

**Configuration:**
```bash
kelora -j large.log \
    --parallel \
    --threads 8 \
    --batch-size 2000 \
    --batch-timeout 500
```

### Constraints and Tradeoffs

**Incompatible Features:**

- **Spans** - Cannot maintain span state across batches (forces sequential)
- **Cross-event context** - Each batch processed independently

**Multiline Behavior:**

- Multiline chunking happens **per-batch**
- Event boundaries may not span batch boundaries
- Consider larger batch sizes for multiline workloads

**Ordering:**

- Default: Preserve input order (adds overhead)
- `--unordered`: Trade ordering for maximum throughput

**Best For:**

- Large files with independent events
- CPU-bound transformations (regex, hashing, calculations)
- High-throughput batch processing

**Not Ideal For:**

- Real-time streaming (use sequential)
- Cross-event analysis (use spans in sequential mode)
- Small files (overhead exceeds benefit)

---

## Metrics and Statistics

Kelora maintains two tracking systems:

### User Metrics (`--metrics`)

Populated by Rhai functions in --exec scripts:

```bash
kelora -j app.log \
    --exec 'track_count(e.service)' \
    --exec 'track_sum("total_bytes", e.bytes)' \
    --exec 'track_unique("users", e.user_id)' \
    --metrics
```

**Available Functions:**

- `track_count(key)` - Increment counter
- `track_sum(key, value)` - Sum values
- `track_min(key, value)` - Track minimum value
- `track_max(key, value)` - Track maximum value
- `track_unique(key, value)` - Collect unique values
- `track_bucket(key, bucket)` - Track values in buckets

**Access in --end stage:**
```bash
kelora -j app.log \
    --exec 'track_count(e.service)' \
    --end 'print("Total services: " + metrics.len())' \
    --metrics
```

**Output:**

- Printed to stderr with `--metrics`
- Written to JSON file with `--metrics-file metrics.json`

### Internal Statistics (`--stats`)

Auto-collected counters:

- `events_created` - Parsed events
- `events_output` - Output events
- `events_filtered` - Filtered events
- `discovered_levels` - Log levels seen
- `discovered_keys` - Field names seen
- Parse errors, filter errors, etc.

```bash
kelora -j app.log --stats
```

### Parallel Metrics Merging

In parallel mode:

- Each worker maintains local tracking state
- GlobalTracker merges worker states after processing:
  - Counters: summed
  - Unique sets: unioned
  - Averages: recomputed from sums and counts
- Merged metrics available in `--end` stage

---

## Error Handling

### Resilient Mode (Default)

- **Parse errors:** Skip line, continue processing
- **Filter errors:** Treat as `false`, skip event
- **Transform errors:** Return original event unchanged
- **Summary:** Show error count at end

```bash
kelora -j app.log --verbose  # Show errors as they occur
```

### Strict Mode (`--strict`)

- **Any error:** Abort immediately with exit code 1
- **No summary:** Program exits on first error

```bash
kelora -j app.log --strict
```

### Verbosity Levels

- `-v` / `--verbose` - Show detailed errors (level 1)
- `-vv` - More verbose (level 2)
- `-vvv` - Maximum verbosity (level 3)

### Quiet/Output Modes

- `-q` / `--no-events` - Suppress events
- `--no-diagnostics` - Suppress diagnostics (fatal line still emitted)
- `--silent` - Suppress terminal output (events, diagnostics, stats, terminal metrics, script output); one fatal line on errors; metrics files still write

---

## Complete Data Flow

```
┌─────────────────────────────────────────┐
│  Layer 1: Input                         │
├─────────────────────────────────────────┤
│  • Stdin or Files (--file-order)        │
│  • Automatic decompression (gzip/zstd)  │
│  • Reader thread spawning               │
└──────────────┬──────────────────────────┘
               │ Raw lines
┌──────────────▼──────────────────────────┐
│  Layer 2: Line-Level Processing         │
├─────────────────────────────────────────┤
│  • --skip-lines (skip first N)          │
│  • --section-start/through (sections)   │
│  • --ignore-lines/--keep-lines (regex)  │
│  • Multiline chunker (event boundaries) │
└──────────────┬──────────────────────────┘
               │ Complete event strings
┌──────────────▼──────────────────────────┐
│  Layer 3: Event-Level Processing        │
├─────────────────────────────────────────┤
│  • Parser → Event map                   │
│  • Span preparation (assign span_id)    │
│  • Script stages (--filter/--exec)      │
│    - User stages in CLI order           │
│    - Timestamp filtering (--since)      │
│    - Level filtering (--levels)         │
│    - Key filtering (--keys)             │
│  • Span close hooks (--span-close)      │
│  • Output formatting                    │
└──────────────┬──────────────────────────┘
               │ Formatted output
               ▼
           stdout/files
```

**Parallel Mode Differences:**
```
Line batching (1000 lines) → Worker pool
Each worker independently:

  - Line-level processing
  - Event-level processing
Results → Ordering buffer → Merged output
Metrics → GlobalTracker → Merged stats
```

---

## Performance Characteristics

### Streaming

- **Low memory usage** - Events processed and discarded
- **Real-time capable** - Works with `tail -f` and live streams
- **No lookahead** - Cannot access future events (except with `--window`)

### Sequential vs Parallel

**Sequential (default):**

- Events processed in order
- Lower memory usage
- Predictable output order
- Supports spans and cross-event state
- Best for streaming and interactive use

**Parallel (`--parallel`):**

- Events processed in batches across cores
- Higher throughput for CPU-bound work
- Higher memory usage (batching + worker pools)
- Limited cross-event features
- Best for batch processing large files

### Optimization Tips

**Early filtering:**
```bash
# Good: Cheap filters first
kelora -j app.log \
    --levels error \
    --filter 'e.message.matches(r"expensive.*regex")'

# Less efficient: Expensive filter on all events
kelora -j app.log \
    --filter 'e.message.matches(r"expensive.*regex")' \
    --levels error
```

**Use --keys to reduce output processing:**
```bash
kelora -j app.log --keys timestamp,message -F json
```

**Parallel for CPU-bound transformations:**
```bash
kelora -j large.log \
    --parallel \
    --exec 'e.hash = e.content.hash("sha256")' \
    --batch-size 1000
```

**Use --take for quick exploration:**
```bash
kelora -j large.log --take 100
```

---

## See Also

- [Events and Fields](events-and-fields.md) - How events are structured
- [Scripting Stages](scripting-stages.md) - Writing --filter and --exec scripts
- [Error Handling](error-handling.md) - Resilient vs strict modes
