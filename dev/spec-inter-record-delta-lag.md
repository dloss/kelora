# Spec: Inter-Record Delta/Lag Functions

Status: Planned (v1 scope locked)  
Owner: Kelora core  
Scope: Rhai built-ins + pipeline runtime state + docs/tests

## 1) Problem Statement

Kelora has strong per-event scripting and window analytics, but common sequential time-series operations still require awkward manual scripting. Adjacent-record helpers should be first-class for core workflows.

Primary target workflows:

- Detect latency jumps (`current - previous`)
- Compute value deltas over N records
- Smooth noisy metrics in-stream (`ewma`)

## 2) Goals and Non-Goals

### Goals (v1)

1. Add ergonomic built-ins for adjacent-record analysis:
   - `prev(field)`
   - `lag(field, n)`
   - `delta(field)` / `delta(field, n)`
   - `ewma(key, value, alpha)`
2. Keep streaming behavior predictable and memory-bounded.
3. Provide resilient defaults (`()`) plus strict fail-fast variants.
4. Define deterministic lifecycle semantics and sequential-only behavior up front.
5. Ship clear docs/help examples and comprehensive tests.

### Non-Goals (v1)

1. Timestamp-derived `rate*` helpers (deferred)
2. `*_or` fallback variants (deferred)
3. SQL-style window functions (`lead`, `rank`, partitioning)
4. Parallel cross-worker adjacency stitching
5. Retroactive correction for out-of-order events

## 3) User Experience

### 3.1 Function Signatures (v1)

Resilient variants (default):

```rhai
prev(field: &str) -> Dynamic | ()
lag(field: &str, n: int) -> Dynamic | ()
delta(field: &str) -> f64 | ()
delta(field: &str, n: int) -> f64 | ()
ewma(key: &str, value: f64, alpha: f64) -> f64
```

Strict variants:

```rhai
prev_strict(field: &str) -> Dynamic
lag_strict(field: &str, n: int) -> Dynamic
delta_strict(field: &str) -> f64
delta_strict(field: &str, n: int) -> f64
ewma_strict(key: &str, value: f64, alpha: f64) -> f64
```

### 3.2 Example Usage

```bash
kelora -j app.jsonl --exec '
  e.prev_ms = prev("duration_ms");
  e.delta_ms = delta("duration_ms");
' --filter 'e.delta_ms != () && e.delta_ms > 500'
```

```bash
kelora -j metrics.jsonl --exec '
  e.latency_smooth = ewma("latency_ms", e.latency_ms, 0.2);
'
```

### 3.3 UX Rules

1. These functions do **not** require `--window`.
2. Missing history returns `()` in resilient mode.
3. Strict variants raise runtime errors with actionable messages.
4. `lag(..., n)` requires `n >= 1`.

## 4) Event Lifecycle Contract (exact integration/update point)

v1 uses **processed-event adjacency** and one deterministic update point.

For each event in sequential processing order:

1. Parse event.
2. Run per-event script stages (`begin` / `filter` / `exec` / `end` as configured).
3. During scripting, `prev/lag/delta` read only previously committed history.
4. After per-event scripting completes, commit current event fields/state for the next event.

Policy clarifications:

- Filtered-out events still advance history if they reached per-event scripting.
- History continuity is by stream order and continues across files.

Rationale (short): this model is deterministic, simple to explain, and aligns with script-stage processing rather than output-only visibility.

## 5) Semantics and Edge Cases

### 5.1 Coercion policy

**No implicit coercion in v1.**

- `delta*` accepts only native numeric values.
- Numeric strings (e.g., `"123"`) are invalid:
  - resilient: return `()`
  - strict: runtime error

### 5.2 EWMA

Formula:

`S_t = alpha * x_t + (1 - alpha) * S_{t-1}`

Rules:

- `alpha` in `(0, 1]`
- first observation initializes `S_0 = x_0`
- state is keyed by `key` across the run

### 5.3 File boundaries

Default behavior: history continues across input files in stream order.

## 6) Parallel Mode

All v1 functions are **sequential-only**.

In `--parallel`, calling these functions must raise an explicit runtime error (same style as existing sequential-only stateful helpers).

## 7) Error Handling

### 7.1 Resilient mode

- Invalid/missing value => `()`
- Insufficient history => `()`
- Invalid argument contract (`n`, `alpha`) => runtime error

### 7.2 Strict variants

Runtime errors should include:

- function name
- offending field and/or argument
- offending type/value
- fix hint

Example shape:

`delta_strict("duration_ms"): expected numeric current and lagged values; got string="abc"`

## 8) Internal Design

### 8.1 Module and registration

- Implement in `src/rhai_functions/inter_record.rs` (or `lag.rs`)
- Register in `src/rhai_functions/mod.rs`

### 8.2 Runtime storage

Thread-local state for sequential mode:

- `history[field]`: ring buffer of recent values for lag lookup
- `ewma[key]`: f64 accumulator

Complexity:

- `prev`/`delta`: O(1) typical per call
- `lag(field, n)`: O(1) lookup, O(fields * max_n_requested) memory

Safety cap:

- `lag(n)` capped at `10_000` (error above cap)

### 8.3 Mode guard

Mirror existing sequential-only guard behavior/messages (as used by `state`/other stateful helpers).

## 9) Documentation Plan

Update:

1. `src/rhai_functions/docs.rs` (`--help-functions` catalog + examples)
2. `docs/reference/functions.md` (detailed semantics)
3. `docs/reference/script-variables.md` (adjacency and lifecycle semantics)
4. `--help-functions` examples for filtered-event progression and common recipes

## 10) Test Plan

### Unit tests

1. `prev` first event => `()`
2. `prev` second event returns first value
3. `lag(n)` with insufficient history
4. `delta` int/float happy-path
5. `delta` non-numeric and numeric-string behavior
6. strict variants raise expected errors
7. `ewma` initialization and recurrence
8. alpha bounds checks
9. cap enforcement for `lag(n) > 10_000`

### Integration tests

1. CLI pipeline with `--exec` and `--filter`
2. filtered events still advancing history
3. cross-file behavior in stream order
4. parallel mode explicit error message

### Property tests (optional)

- EWMA boundedness for bounded inputs

## 11) Performance Considerations

1. Avoid unnecessary `Dynamic` cloning
2. Keep ring buffers compact and bounded
3. Benchmark with and without inter-record helpers

Target guideline:

- `<10%` additional CPU for typical `prev/delta` usage

## 12) Rollout Strategy

Phase 1 (this spec):

- `prev`, `lag`, `delta`, `ewma` (+ strict variants)
- lifecycle contract above
- sequential-only guard

Future phase:

- `rate*` once timestamp semantics are finalized and tested
- optional additional ergonomic variants if needed
