# Script Variables Reference

Kelora exposes several built-in variables to Rhai scripts. Their availability depends on which stage is executing (per-event filters/execs, span hooks, begin/end hooks, etc.). Use this page whenever you need to know which data is in scope.

## Stage Overview

| Stage / Feature | Variables Available | Notes |
|-----------------|---------------------|-------|
| `--filter`, `--exec`, `--exec-file` | `line`, `e`, `meta`, `conf`, `state` | Per-event context. `e` and `state` are writable; the others are read-only. `state` only in sequential mode. |
| `--filter` / `--exec` with `--window` | `line`, `e`, `meta`, `conf`, `state`, `window` | Adds the sliding `window` array (current event at index `0`; read-only). `state` only in sequential mode. |
| `--begin` | `conf`, `state` | Map to seed configuration before events arrive. `conf` is read-only; `state` is writable (sequential mode only). |
| `--end` | `metrics`, `conf`, `state` | Inspect final tracker totals in `metrics` and final `state`; `conf` and `metrics` are read-only. `state` only in sequential mode. |
| `--span-close` | `span`, `metrics`, `conf` | Summarises the closed span with per-span data (`span`) and cumulative totals (`metrics`). All read-only. |

> Reading a variable that does not exist in the current stage raises a Rhai error. Variables listed here are populated with meaningful data (individual fields inside maps may still be `()`). Other globals such as `line`/`e` exist behind the scenes for compatibility but start empty in stages where they are not useful.

## Common Variables

### `line`
- Type: `String`
- Snapshot of the text the parser captured for the current record. Structured formats strip trailing newlines before storing it, `-f raw` preserves every byte (including terminators), and `--multiline` reflects the chunk assembled by the multiline stage. Present in every stage and read-only.
- You’ll also see the same text at `meta.line`; that copy travels with saved metadata (window snapshots, span hooks, cloned `meta` values) so the original record is still available later on.

### `e`
- Type: `Map`
- Event map during per-event stages. Mutating `e` inside `--exec` updates the emitted event; setting a field to `()` removes it; assigning `e = ()` clears the entire event. Writable.

### `meta`
- Type: `Map`
- Metadata derived from the pipeline and span system. Attempting to mutate `meta` fields has no effect; use event fields on `e` for custom annotations instead. Read-only.

| Key | Type | Description |
|-----|------|-------------|
| `line` | `String` | Same as the standalone `line` variable. |
| `line_num` | `Int` | 1-based line number when input comes from files. |
| `filename` | `String` | Source filename, if known. |
| `span_status` | `String` | `"included"`, `"late"`, `"unassigned"`, or `"filtered"` when spans are enabled. Missing otherwise. |
| `span_id` | `String` or `()` | Span identifier for the current event. |
| `span_start` / `span_end` | `DateTime` or `()` | Span bounds for the current event. |

### `conf`
- Type: `Map`
- Configuration produced by `--begin` and CLI options; the map is frozen so mutation attempts are ignored. Read-only.

### `state`
- Type: `StateMap` (special wrapper, not a regular `Map`)
- Mutable global map for complex state tracking across events. Available in all per-event stages (`--filter`, `--exec`, `--begin`, `--end`) **in sequential mode only**.
- **When to use**: Deduplication, storing complex objects, cross-event dependencies that `track_*()` functions cannot handle.
- **When NOT to use**: Simple counting or metrics—prefer `track_count()`, `track_sum()`, etc., which work in parallel mode too.

**Direct operations** (no conversion needed):

- Indexing: `state["key"]` for get/set, returns `()` if key doesn't exist
- Methods: `contains(key)`, `get(key)`, `set(key, value)`, `len()`, `is_empty()`, `keys()`, `values()`, `clear()`, `remove(key)`
- Operators: `+=`, `mixin(map)`, `fill_with(map)`

**For other map functions**: Convert to regular map first using `state.to_map()`, then use any map function:
```rhai
// Convert state to use functions like to_logfmt(), to_kv(), etc.
print(state.to_map().to_logfmt());
let json_str = state.to_map().to_json();
```

**Parallel mode restriction**: Accessing `state` in `--parallel` mode causes a runtime panic with a clear error message. State requires sequential processing to maintain consistency.

**Example use cases**:
```rhai
// Deduplication - track seen IDs
if !state.contains(e.request_id) {
    state[e.request_id] = true;
    // Process first occurrence
} else {
    // Skip duplicate
    e = ();
}

// Store complex nested state
if !state.contains(e.user) {
    state[e.user] = #{login_count: 0, last_seen: ()};
}
let user_data = state[e.user];
user_data.login_count += 1;
user_data.last_seen = e.timestamp;
state[e.user] = user_data;
```

## Span Hooks (`--span-close`)

### `span`
- Type: `Span` (custom Rhai type)
- Binding available only in `--span-close`. Read-only.

| Property | Type | Description |
|----------|------|-------------|
| `span.id` | `String` | Unique span identifier (`#0`, `#1`, ... for count spans; `2024-01-15T10:00:00Z/5m` for time spans). |
| `span.start` / `span.end` | `DateTime` or `()` | Span boundary timestamps (time spans only). |
| `span.size` | `Int` | Number of events that survived filters and entered the span. |
| `span.events` | `Array<Map>` | Copy of each included event, with helper fields (`line`, `line_num`, `filename`, `span_status`, `span_id`, `span_start`, `span_end`). Read-only. |
| `span.metrics` | `Map` | Per-span deltas computed from `track_*()` calls since the span opened; zero-delta keys are omitted. Read-only.

> `--span-close` runs without a "current event", so `e` and `meta` are empty. Inspect `span.events` when you need per-event details from inside the close hook.

### `metrics` vs `span.metrics`
- `span.metrics`: Only the delta accumulated while the span was open. After the hook runs, Kelora resets the baseline for the next span; mutations are discarded. Read-only.
- `metrics`: The global tracker map used across the entire run (the same map exposed to `--end`). Values persist between spans and reflect cumulative totals; update via `track_*()` calls, not direct assignment. Read-only.

## Windowed Scripts

- `window`: Present when using `--window` features. It is an array of event maps representing the sliding window, with `window[0]` being the current event followed by prior events retained by the window manager. Read-only.

## Begin / End Hooks

- `--begin`: Runs before any event is parsed. `e` and `meta` start empty; use this stage for initialization and configuration population (`conf`).
- `--end`: Receives the final `metrics` map so you can report overall totals after processing completes.
