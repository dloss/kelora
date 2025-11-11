# Glossary

Quick reference for Kelora terminology. Terms are organized alphabetically with cross-references to detailed documentation.

---

## A

### Auto-detection
The ability to automatically identify input format by examining file content rather than filename. Activated with `-f auto`. See [Format Reference](reference/formats.md).

---

## B

### Batch
A group of log lines processed together in parallel mode. Default batch size is 1000 lines. See `--batch-size` in [CLI Reference](reference/cli-reference.md).

### Brief Mode
Display mode that shows only field values without field names. Activated with `-b`. See [Basics Tutorial](tutorials/basics.md#brief-mode-b---values-only).

---

## C

### Context Lines
Log lines shown before and/or after a matching event to provide surrounding context. Configured with `-A` (after), `-B` (before), or `-C` (both). Similar to grep's context flags. See [Processing Architecture](concepts/pipeline-model.md#context-lines).

### Core Fields
The essential fields displayed with `-c`: timestamp, level, and message. See [Basics Tutorial](tutorials/basics.md#core-fields-c---essentials-only).

---

## E

### Event
A structured data object (map/dictionary) representing a single log entry after parsing. Each event contains fields that can be accessed in Rhai scripts via the `e` variable. Example: after parsing `{"level": "ERROR", "message": "timeout"}`, you can access `e.level` and `e.message`.

**Key points:**
- Created by parsing raw log lines
- Accessible via `e` in filters and transforms
- Fields accessed with dot notation: `e.field_name`
- Nested fields: `e.user.name`

See [Events and Fields](concepts/events-and-fields.md).

### Event Boundary
The point where one log entry ends and another begins. Important for multiline logs where stack traces or wrapped messages span multiple lines. See [Multiline Strategies](concepts/multiline-strategies.md).

### Exec Stage
A transformation stage where Rhai scripts modify events. Specified with `--exec` or `-e`. Scripts can add, modify, or remove fields. Example: `-e 'e.duration_s = e.duration_ms / 1000'`. See [Scripting Stages](concepts/scripting-stages.md).

---

## F

### Field
A key-value pair within an event. Fields can contain strings, numbers, booleans, nulls, nested objects, or arrays. Access fields using `e.field_name` in scripts.

**Examples:**
- `e.timestamp` - String field
- `e.status` - Number field
- `e.user.id` - Nested field
- `e.tags` - Array field

See [Events and Fields](concepts/events-and-fields.md).

### Filter Stage
A stage that keeps or skips events based on a boolean expression. Specified with `--filter`. Events where the expression evaluates to `true` are kept; `false` means skip. Example: `--filter 'e.status >= 500'`. See [Scripting Stages](concepts/scripting-stages.md).

### Format
The structure and syntax of input log data. Common formats: JSON, logfmt, syslog, CSV, Apache/Nginx combined format. Specified with `-f` or `--input-format`. See [Format Reference](reference/formats.md).

---

## L

### Level
The severity or importance of a log event. Common levels: DEBUG, INFO, WARN, ERROR, CRITICAL. Can be filtered with `-l` (include) or `-L` (exclude). Case-insensitive. See [Basics Tutorial](tutorials/basics.md#level-filtering-l-l).

### Line-Level Processing
Operations performed on raw string lines before parsing into events. Includes line filtering (`--ignore-lines`, `--keep-lines`), line skipping (`--skip-lines`), and multiline aggregation. See [Processing Architecture](concepts/pipeline-model.md#layer-2-line-level-processing).

---

## M

### Metadata
Contextual information about log processing available in the `meta` variable. Includes:
- `meta.filename` - Current input file
- `meta.line_number` - Line number in file
- `meta.span_id` - Current span identifier (if using spans)

See [Script Variables](reference/script-variables.md).

### Metrics
User-defined counters and aggregations tracked with `track_*()` functions. Displayed with `--metrics` or saved with `--metrics-file`. Includes counts, sums, unique values, and buckets. See [Metrics and Tracking Tutorial](tutorials/metrics-and-tracking.md).

### Multiline
A strategy for combining multiple consecutive raw lines into a single event before parsing. Used for logs with stack traces, wrapped messages, or multi-line JSON. Specified with `-M`. See [Multiline Strategies](concepts/multiline-strategies.md).

**Common strategies:**
- `timestamp` - Lines starting with timestamps begin new events
- `indent` - Indented lines continue previous event
- `regex` - Custom patterns define boundaries
- `all` - Entire input as one event

---

## P

### Parallel Mode
Processing mode where log lines are batched and processed concurrently across multiple CPU cores. Activated with `--parallel`. Trades some features (spans, cross-event context) for higher throughput. See [Processing Architecture](concepts/pipeline-model.md#parallel-processing-model).

### Parser
The component that converts raw text into structured events. Each format has its own parser: JSON parser, logfmt parser, syslog parser, etc. See [Format Reference](reference/formats.md).

### Pipeline
The sequence of stages through which events flow: Input → Parse → Filter/Transform → Output. User-controlled stages (filter, exec, levels) run in CLI order. See [Processing Architecture](concepts/pipeline-model.md).

---

## R

### Resilient Mode
Default error handling mode where parsing errors and script failures are logged but don't stop processing. Failed events are skipped and processing continues. Opposite of strict mode. See [Error Handling](concepts/error-handling.md).

### Rhai
The embedded scripting language used for filters and transforms. Rust-based with JavaScript-like syntax. Provides 150+ built-in functions for log analysis. See [Introduction to Rhai Tutorial](tutorials/intro-to-rhai.md) and [Rhai Cheatsheet](reference/rhai-cheatsheet.md).

---

## S

### Span
A group of consecutive events treated as a unit for aggregation. Spans close after N events (count-based) or after a time window (time-based). Configured with `--span` and `--span-close`. See [Span Aggregation Tutorial](tutorials/span-aggregation.md).

**Examples:**
- `--span 100` - Spans of 100 events each
- `--span 5m` - 5-minute time windows
- `--span 1h` - 1-hour time windows

### Stage
A single processing step in the pipeline. User-controlled stages include:
- `--filter` - Boolean filter
- `--exec` / `-e` - Transform script
- `--levels` / `-l` - Include log levels
- `--exclude-levels` / `-L` - Exclude log levels

Stages run in the order specified on the command line. See [Scripting Stages](concepts/scripting-stages.md).

### Statistics
Auto-collected processing metrics displayed with `--stats`. Includes events parsed, filtered, output; discovered levels and field names; errors; time span. Different from user-defined metrics. See [Processing Architecture](concepts/pipeline-model.md#internal-statistics-stats).

### Streaming
Processing mode where events are read, processed, and output one at a time without buffering the entire file in memory. Default mode (sequential). Enables real-time analysis of live logs. See [Performance Model](concepts/performance-model.md).

### Strict Mode
Error handling mode where any parsing or script error immediately aborts processing with exit code 1. Activated with `--strict`. Opposite of resilient mode. See [Error Handling](concepts/error-handling.md).

---

## T

### Timestamp
A field containing the date and time when a log event occurred. Kelora auto-detects common field names: `timestamp`, `ts`, `time`, `@timestamp`. Used for time-based filtering with `--since` and `--until`. See [Working with Time Tutorial](tutorials/working-with-time.md).

### Tracking
The process of accumulating metrics across events using `track_*()` functions:
- `track_count(key)` - Count occurrences
- `track_sum(key, value)` - Sum values
- `track_unique(key, value)` - Collect unique values
- `track_min/max(key, value)` - Track extremes

See [Metrics and Tracking Tutorial](tutorials/metrics-and-tracking.md).

### Transform
A modification applied to an event, typically in an `--exec` stage. Can add new fields, modify existing fields, or remove fields. Example: `--exec 'e.status_class = e.status / 100'`.

---

## W

### Window
A sliding window of recent events accessible in scripts via the `window` array. Configured with `--window N` to keep the last N events in memory. Useful for contextual analysis. Example: `--window 10 --exec 'e.recent_errors = window.filter(|x| x.level == "ERROR").len()'`. See [Advanced Scripting Tutorial](tutorials/advanced-scripting.md).

---

## See Also

- [Basics Tutorial](tutorials/basics.md) - Learn fundamental concepts through examples
- [Events and Fields](concepts/events-and-fields.md) - Deep dive on event structure
- [Processing Architecture](concepts/pipeline-model.md) - Understanding the pipeline
- [Scripting Stages](concepts/scripting-stages.md) - Filter and transform details
- [Function Reference](reference/functions.md) - All 150+ built-in functions
- [CLI Reference](reference/cli-reference.md) - Complete command-line reference
