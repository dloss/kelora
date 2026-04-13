# Kelora Backlog

Combined list of new features and technical debt items. All features here are parallel-safe and fit the streaming pipeline architecture.

---

## New Rhai Functions

### Time Bucketing (`dt.floor_to()`)

**What:** Truncate timestamps *down* to a time interval boundary. Companion to `ceil_to()` (already implemented).

**Why useful:** For grouping, floor/ceil is more predictable than rounding (bucket boundaries are stable). `round_to()` and `ceil_to()` already exist; this adds the missing "floor" helper.

**Example:**
```rhai
// Truncate to hour boundary for grouping
e.ts = to_datetime(e.timestamp).floor_to("1h")

// Align to 5-minute bucket edges
bucket_time = to_datetime(e.ts).floor_to("5m")
track_count(bucket_time.to_string())
```

**Implementation:** ~60 lines in `src/rhai_functions/datetime.rs` alongside `ceil_to`.

---

### Smart Text Truncation (`truncate_words()`)

**What:** Truncate strings at word boundaries — avoids cutting mid-word for cleaner output.

**Example:**
```rhai
e.summary = e.message.truncate_words(50)       // Max 50 chars, break at word boundary
e.preview = e.description.truncate_words(100, "...")

// Bad:  "The quick brown fox jum..."
// Good: "The quick brown fox..."
```

**Implementation:** ~40 lines in `src/rhai_functions/strings.rs`. UTF-8 aware.

---

### Array Set Operations (`intersect()`, `difference()`, `union()`)

**What:** Set operations on arrays — find common elements, differences, combinations.

**Why useful:** Common when comparing tags, roles, permissions, or features between events.

**Example:**
```rhai
common_tags = current_tags.intersect(previous_tags)
new_perms   = after.permissions.difference(before.permissions)
all_features = baseline.union(experimental)

added = e.tags.difference(baseline_tags)
if added.len() > 0 { e.tags_added = added }
```

**Implementation:** HashSet operations, ~50 lines in `src/rhai_functions/arrays.rs`.

---

### Nested JSON Path Extraction (`emit_each_path()`)

**What:** Extract and fan out nested arrays as events using a JSONPath-lite syntax.

**Why useful:** Querying log aggregators (Loki, Elasticsearch, Graylog) returns nested structures. Currently requires verbose loops with `emit_each()`.

**Example:**
```rhai
// Currently verbose:
for stream in e.get_path("data.result", []) {
    for value in stream.values { emit_each([value[1]]) }
}

// With emit_each_path():
emit_each_path("data.result[].values[].[1]")

// Elasticsearch hits
emit_each_path("hits.hits[]._source")
```

**Implementation:** JSONPath-lite parser + nested extraction, ~120 lines in `src/rhai_functions/maps.rs` or `emit.rs`. Subset of JSONPath (arrays, wildcards, no filters).

---

## Output Features

### Output Format Templates (`--template`)

**What:** Built-in output templates for common log destinations (Loki, GELF, Elastic Common Schema, OpenTelemetry).

**Why useful:** Integration with centralised logging systems currently requires piping through `jq`. Templates eliminate this friction.

**Example:**
```bash
kelora -j app.jsonl --template loki:labels=service,level > loki.json
kelora -j app.jsonl --template gelf:host=myserver > gelf.json
kelora -j app.jsonl --template ecs > ecs.jsonl
```

**Implementation:** Template system in output module, ~150 lines. Ships with built-in templates; custom templates via `--template @my-format.json`.

---

## Klogg-Inspired Features

Reviewed [klogg](https://github.com/variar/klogg) (GUI log viewer) for ideas that
fit Kelora's CLI / streaming / Rhai architecture without breaking it. Items
below are additive and do not require breaking changes. Skipped: SIMD/MT search
(already covered), 2.1B-line support (streaming handles it), GUI affordances
(scratchpad, tabs, favorites menu, context overview), "search within results"
(already covered by chained `--filter`).

### 1. Input Encoding Auto-Detection

**What:** Detect non-UTF-8 input encodings (UTF-16, CP1251, Latin-1, Shift-JIS,
GBK, …) and transcode to UTF-8 at the input stage. Add a `--encoding <name>`
override for cases where detection is wrong or undesired.

**Why useful:** Real-world Windows event logs, legacy syslog dumps, and CJK
logs are commonly non-UTF-8. Kelora currently can't handle them. Klogg uses
`uchardet` for this; the Rust equivalent is `chardetng` (used by Firefox).

**Scope:** Input stage only. No impact on parsers, Rhai, or output.

**Implementation:** Wrap the input reader in an encoding-detecting decoder
(BOM sniff → `chardetng` sample → fall back to UTF-8). Per-file detection for
multi-file mode. ~150 lines plus a dependency.

---

### 2. Context Lines for Filter Matches (`-A` / `-B` / `-C`)

**What:** When `--filter` matches an event, also emit N preceding and/or
following events. Standard `grep -A/-B/-C` semantics.

**Why useful:** Klogg's whole UI is about seeing matches in context. On the
CLI, `grep -C 3` is the universal idiom. Especially valuable for chasing
errors that need surrounding stack frames or request-flow context.

**Scope:** Processing stage. Maintain a small ring buffer of recent events
plus a "lines remaining to emit" counter. Streaming-safe and bounded memory.

**Implementation:** ~120 lines wrapping the filter stage. Interaction with
parallel mode needs thought (probably sequential-only initially).

---

### 3. Native Follow Mode (`--follow` / `-F`)

**What:** Built-in `tail -f`-equivalent on a file path, with log rotation
handling (track inode, reopen on rename/truncate).

**Why useful:** Kelora documents `tail -f app.log | kelora …` but a built-in
is better UX, handles rotation correctly, and works on Windows where `tail -f`
isn't standard. Klogg's "watches for file changes on disk and reloads it"
covers the same need.

**Scope:** Input stage. The streaming pipeline already supports unbounded
input, so this is purely a smarter file reader.

**Implementation:** `notify` crate or polling-based watcher with rotation
detection. Mutually exclusive with `--merge-sorted` and `--parallel`. ~200
lines plus a dependency.

---

### 4. Highlighter Sets (Configurable Terminal Colorization)

**What:** Named highlighter rules in `.kelora.ini` that colorize output by
regex match or field/value. E.g. red bold for `level=ERROR`, dim grey for
health-check paths, yellow for slow `duration_ms > 1000`.

**Why useful:** Klogg's customizable highlighter sets are one of its
most-praised productivity features. Kelora's `default` output format already
emits emoji prefixes; structured colorization is the natural next step for
interactive log inspection.

**Scope:** Output stage only. Disabled when not a TTY or when `--no-color`
is set.

**Example config:**
```ini
[highlighter.errors]
match = 'level == "ERROR"'
style = "red,bold"

[highlighter.slow]
match = 'duration_ms > 1000'
style = "yellow"

[highlighter.healthcheck]
regex = '/healthz|/readyz'
style = "dim"
```

**Implementation:** ~200 lines in `src/output/`. Reuses Rhai for the `match`
expressions; regex variant for raw text matching.

---

### 5. Boolean Quick-Filter Shortcut (`-g` / `--grep`)

**What:** A grep-style shortcut filter using boolean regex syntax:
`-g "pattern1 AND pattern2 NOT pattern3"`. Compiles to an equivalent Rhai
filter under the hood.

**Why useful:** Klogg's signature filter syntax. Kelora's `--filter` (Rhai) is
strictly more powerful but verbose for the 80% case of "lines containing X
and Y but not Z". Easy on-ramp for klogg/grep users.

**Scope:** Pure CLI sugar — desugars into existing `--filter` machinery.
Optional; doesn't change semantics.

**Implementation:** Small parser for `AND`/`OR`/`NOT`/`(`/`)` over regex
literals, ~80 lines.

---

### 6. Named Filter/Pipeline Profiles

**What:** `[profile.<name>]` sections in `.kelora.ini` bundling `format`,
`levels`, `filter`, `exec`, `keys`, `output-format`, etc. Invoked via
`--profile <name>`.

**Why useful:** Klogg has favorites and saved searches. Lets teams share
common pipelines (e.g. `--profile nginx-errors`, `--profile k8s-audit`)
without long shell aliases.

**Scope:** Config stage only. Profile values merge with CLI flags using
the existing precedence rules (CLI > profile > config defaults).

**Implementation:** Extend the existing `.kelora.ini` loader, ~150 lines.

---

### Recommended Priority

1. **Encoding auto-detection** — biggest real-world capability gap
2. **Context lines (`-A`/`-B`/`-C`)** — standard CLI idiom, currently missing
3. **Native `--follow`** — better than piping `tail -f`, fixes Windows UX
4. **Highlighter sets** — quality-of-life for interactive use
5. **`-g` boolean quick-filter sugar** — optional on-ramp
6. **Named profiles** — optional team workflow improvement

---

## Technical Debt

### 1. Monolithic Files (Serious)

- `src/rhai_functions/strings.rs`: **6,550 lines** (3× recommended max)
- `src/parallel.rs`: 4,011 lines
- `src/engine.rs`: 3,203 lines
- `src/main.rs`: 3,109 lines

**Recommendation:** Split `strings.rs` into submodules (basic, search, transform, parsing, format).

**Effort:** 8–12h

---

### 2. Too Many Parameters (Serious)

Remaining high-parameter functions:
- `src/parallel.rs:1642` — `file_aware_batcher_thread` (13 params)
- `src/parallel.rs:2602, 2811` — 8+ params each
- `src/main.rs:1120` — 12+ params
- `src/rhai_functions/tracking.rs:159` — 9+ params (has justifying comment)

**Fix:** Create context/config structs following patterns established for `handle_file_aware_line` and `batcher_thread`.

**Effort:** 4–6h

---

### 3. Excessive Cloning (Serious)

- Total: 1,094 `.clone()` calls across codebase
- `strings.rs`: 189 | `parallel.rs`: 143 | `tracking.rs`: 106

**Fix:** Profile with `cargo flamegraph`, then optimise hot paths.

**Effort:** 8–16h (depends on profiling results)

---

### 4. Global Mutable State (Moderate)

- `src/rhai_functions/file_ops.rs:38–52` — 5 global statics make testing difficult
- `CSV_FORMATTER_HEADER_REGISTRY` in `src/output/formatters.rs:1667`

**Fix:** Dependency injection instead of globals.

**Effort:** 6–10h

---

### 5. Missing Integration Tests (Moderate)

Not covered:
- Mutex poisoning recovery
- Thread panic scenarios
- OOM handling
- Non-UTF-8 paths

**Effort:** 8–12h

---

## Priority Order

**Phase 1 — Maintainability:**
1. Reduce remaining function parameters (4–6h)
2. Split large files, starting with `strings.rs` (8–12h)

**Phase 2 — Performance:**
3. Profile and optimise clones (8–16h)

**Phase 3 — Quality:**
4. Global state refactoring (6–10h)
5. Integration tests (8–12h)

**Total technical debt:** ~34–56h
