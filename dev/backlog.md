# Kelora Backlog

Combined list of new features and technical debt items. All features here are parallel-safe and fit the streaming pipeline architecture.

See also: [v2 behavior notes](v2-behavior-notes.md) for candidate breaking
changes around resiliency, parser strictness, config validation, and scored
auto-format detection.

See also: [named formats](named-formats.md) for the built-in named log-format
catalogue (state, limitations, and roadmap — auto-detect split, scored
detection, user-loadable format files).

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
track_freq("bucket", bucket_time.to_string())
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

## Considered and Rejected (Klogg / Logana Reviews)

Reviewed [klogg](https://github.com/variar/klogg) and
[logana](https://github.com/pauloremoli/logana) for features Kelora might
borrow. After a fresh-eye stress test, **none survived**. Recording the
rejections here so the same ideas don't get re-suggested without new
information.

GUI affordances (tabs, marks, annotations, scratchpad, vim navigation, mouse,
session persistence, remappable keybindings, context overview) are skipped on
principle — they don't map to a streaming CLI tool.

| Idea | Source | Why rejected |
|---|---|---|
| Input encoding auto-detection (UTF-16/CP1251/…) | klogg | Speculative future user; auto-detection is unreliable on small samples and can corrupt data silently; modern Windows logs export as UTF-8. A manual `--encoding` flag could be added if a real user asks. |
| Context lines `-A`/`-B`/`-C` | klogg | **Already supported.** |
| Native `--follow` mode | klogg | `tail -F` already handles rotation; PowerShell `Get-Content -Wait` covers Windows. The win over `tail -F \| kelora` is small and the watcher bug surface (NFS, symlinks, truncation races, multi-file) is large. |
| Configurable highlighter framework | klogg | Kelora's primary output is machine-readable; interactive coloring is a secondary use case. Level coloring already exists; broader highlighting is overreach for a framework with diminishing returns. `bat`/`grcat`/`lnav` cover the niche. |
| Boolean quick-filter `-g`/`--grep` | klogg | Contradicts Kelora's "use grep for simple text search" positioning; adds a third filter mechanism; it's a dead-end DSL that can't reference fields or compose with Rhai. |
| Named filter/pipeline profiles | klogg | Shell aliases, shell functions, and `Justfile` recipes already solve this with transferable skills. Adding a profile schema duplicates the existing `.kelora.ini` defaults and creates precedence/composition slopes. |
| Expanded archive formats (bz2/zip/tar.\*) | logana | bz2 is largely obsolete in 2026 (gzip and zstd dominate; xz already supported). zip and tar are multi-file containers with real design questions, not single-stream decoders. Wait for a request. |
| OTLP/JSON parser | logana | Without a gRPC/HTTP receiver, only catches the file-capture/debug audience, not live OTel pipelines. The audience that has OTLP/JSON files usually also has the Collector itself with OTTL. Revisit only if Kelora makes OTel a strategic priority. |
| journalctl convenience parser | logana | `journalctl -o json \| kelora -j` already works; the only thing a built-in adds is field renaming, which is a one-line `--exec`. Sets a bad precedent (`-f kubectl`, `-f aws-cli`, …). |
| Markdown output format `-F md` | logana | CSV/TSV already covers the "paste into a doc" workflow (GitHub/Notion/Docs auto-render). Markdown tables degrade past ~6 columns. Real ticket workflows use fenced code blocks of raw lines, not tables. |
| MCP server mode | logana | Major architectural shift (long-running process, session/handle abstraction). Marginal benefit over Bash-invoked Kelora for agentic harnesses that already have shell access. Should be a deliberate strategic call, not a backlog drift item. |

Useful meta-observation: Kelora's existing design space is more complete than
a feature-comparison-with-other-tools exercise suggests. The remaining gaps
relative to klogg/logana are either already covered, GUI-only, architecturally
incompatible with the streaming CLI model, or features looking for users.

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
