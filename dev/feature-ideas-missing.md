# Missing Feature Ideas for Kelora

This document contains features that fit Kelora's design principles and architecture but haven't been implemented yet. All suggestions are:

- ✅ Simple to implement (30-200 lines each)
- ✅ Fit the streaming pipeline architecture
- ✅ **Work in parallel mode** (Kelora's key performance advantage)
- ✅ Actually missing (not currently possible without verbose workarounds)
- ✅ Broadly useful for common log analysis patterns
- ✅ Follow Kelora's existing conventions

**Note:** Sequential-only features have been removed from this list. Features requiring global state or event ordering defeat Kelora's parallel processing advantage and are better handled by post-processing tools.

## Datetime & Time-Based Features

### 1. Time Bucketing/Truncation for Timestamps (`dt.floor_to()` / `dt.ceil_to()`)

**What:** Truncate timestamps down or up to a time interval boundary for easy grouping and aggregation.

**Why useful:** For grouping, floor/ceil is more predictable than rounding (bucket boundaries are stable). `round_to()` already exists; this adds the missing "bucket edge" helpers.

**Example:**
```rhai
// Truncate to hour boundary for grouping
e.ts = to_datetime(e.timestamp).floor_to("1h")

// Align to 5-minute bucket edges
bucket_time = to_datetime(e.ts).floor_to("5m")
track_count(bucket_time.to_string())
```

**Implementation:** Simple datetime manipulation using existing Duration types. ~60 lines in `datetime.rs`.


---

## Sampling & Filtering

### 4. Probabilistic Sampling (`sample_prob()`)

**What:** Sample events with a probability `p` (0.0-1.0).

**Why useful:** `bucket()` is deterministic and `sample_every()` is counter-based. A probabilistic helper makes "sample ~10%" one line without manual `rand()` checks.

**Example:**
```rhai
// Keep ~1% of events
if !sample_prob(0.01) { skip() }

// 10% sampling for metrics
if sample_prob(0.10) {
    track_count("sampled_errors")
}
```

**Implementation:** Wrapper around `rand()`. ~20 lines in `rhai_functions/random.rs`.

---

## String & Parsing Functions

### 2. Smart Text Truncation (`truncate_words()`)

**What:** Truncate strings but preserve word boundaries - avoid cutting mid-word for cleaner output.

**Why useful:** When limiting message length for display or storage, cutting mid-word looks broken. This keeps it readable.

**Example:**
```rhai
// Truncate long messages cleanly
e.summary = e.message.truncate_words(50)  // Max 50 chars, break at word boundary

// Preserve formatting
e.preview = e.description.truncate_words(100, "...")

// Compare:
// Bad:  "The quick brown fox jum..."
// Good: "The quick brown fox..."
```

**Implementation:** String splitting with UTF-8 awareness, ~40 lines in `strings.rs`.

---

## Array & Collection Functions

### 3. Array Set Operations (`intersect()`, `difference()`, `union()`)

**What:** Set operations on arrays - find common elements, differences, combinations.

**Why useful:** Common when comparing tags, roles, permissions, or features between events. Currently requires verbose filtering.

**Example:**
```rhai
// Find common tags between events
common_tags = current_tags.intersect(previous_tags)

// Find newly added permissions
new_perms = after.permissions.difference(before.permissions)

// Combine feature flags
all_features = baseline_features.union(experimental_features)

// Alert on tag changes
added = e.tags.difference(baseline_tags)
if added.len() > 0 {
    e.tags_added = added
}
```

**Implementation:** HashSet operations, ~50 lines in `arrays.rs`.

---

## Integration & Output Features

### 4. Output Format Templates (`--template`)

**What:** Built-in templates for common log aggregation formats (Loki streams, GELF, etc.) - converts Kelora JSON to destination-specific formats.

**Why useful:** Integration with centralized logging systems currently requires piping through jq or manual field mapping. Templates eliminate this friction and showcase Kelora's format conversion strengths.

**Example:**
```bash
# Convert to Loki stream format with labels
kelora -j app.jsonl --template loki:labels=service,level > loki.json

# Output GELF format for Graylog
kelora -j app.jsonl --template gelf:host=myserver > gelf.json

# VictoriaLogs (already compatible, but explicit)
kelora -j app.jsonl --template victorialogs > vl.jsonl

# Custom templates via file
kelora -j app.jsonl --template @my-format.json > output.json
```

**Implementation:** Template system in output module, ~150 lines. Ships with built-in templates for Loki, GELF, Elastic Common Schema, OpenTelemetry logs.

---

### 5. Nested JSON Path Extraction (`emit_each_path()`)

**What:** Extract and unwrap nested arrays in one operation, then fan them out as events.

**Why useful:** Querying log aggregators returns nested JSON structures. Currently requires verbose loops with `emit_each()`. A JSONPath-style extractor would be cleaner.

**Example:**
```rhai
// Extract Loki query results - currently verbose:
for stream in e.get_path("data.result", []) {
  for value in stream.values { emit_each([value[1]]) }
}

// With emit_each_path():
emit_each_path("data.result[].values[].[1]")  // JSONPath-style

// Extract Graylog messages - currently:
emit_each(e.get_path("messages", []))

// With emit_each_path():
emit_each_path("messages[]")

// Elasticsearch hits
emit_each_path("hits.hits[]._source")
```

**Implementation:** JSONPath-lite parser + nested extraction, ~120 lines in `maps.rs` or `emit.rs`. Subset of JSONPath (arrays, wildcards, no filters).

---

## Summary

All features in this document are **parallel-safe** and align with Kelora's streaming architecture.

### Priority 1: High Impact, Simple Implementation
1. **Time Bucketing (`dt.floor_to()` / `dt.ceil_to()`)** - Essential for time-based grouping (~60 lines)
2. **Probabilistic Sampling (`sample_prob()`)** - Completes sampling toolkit (~20 lines)
3. **Output Format Templates (`--template`)** - Eliminates jq for integrations (~150 lines)

### Priority 2: Useful Additions
4. **Array Set Operations** - Collection utilities (~50 lines)
5. **Nested JSON Extraction (`emit_each_path()`)** - Simplifies query response processing (~120 lines)
6. **Smart Truncation (`truncate_words()`)** - Clean text handling (~40 lines)

## Implementation Notes

All features in this document:
- Work in **parallel mode** (Kelora's key performance advantage)
- Use existing infrastructure (datetime wrappers, string utilities, etc.)
- Follow Kelora's conventions (method-style calls, error handling)
- Require no external dependencies
- Fit within the streaming pipeline model
- Are stateless transformations or output operations

### Why Sequential-Only Features Were Removed

Features requiring global state, event ordering, or consecutive events defeat Kelora's parallel processing advantage. Examples of **removed** sequential-only features:
- `track_seen()` - Deduplication requires global state lookup
- `track_sequence()` - Sequences are inherently serial
- `track_correlation()` - Paired events may hit different workers
- `track_burst()` - Time-window tracking needs ordering
- `track_rate()` - Requires event ordering by time
- `track_delta()` - Requires consecutive events
- `track_moving_avg()` - Needs ordered sliding window
- `track_first()` / `track_last()` - Can't merge results across workers

Users needing these patterns should either:
- Use post-processing tools better suited for stateful operations
- Accept `--no-parallel` performance trade-off for small datasets
