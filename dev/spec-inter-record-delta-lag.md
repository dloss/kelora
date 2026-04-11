# Spec: Inter-Record Delta/Lag Functions

Status: Draft for implementation planning  
Owner: Kelora core  
Scope: Rhai built-ins + pipeline runtime state + docs/tests

## 1) Problem Statement

Kelora has strong per-event scripting and window analytics, but common sequential time-series operations require awkward manual scripting. Today users can access `window` when `--window` is set, yet there is no first-class `prev/lag/delta/rate/ewma` family for adjacent-record reasoning.

This leaves major analysis workflows verbose and error-prone:
- Detect latency jumps (`current - previous`)
- Compute request-rate from counters (`delta(counter)/delta(time)`)
- Smooth noisy metrics in-stream (`ewma`)

## 2) Goals & Non-Goals

### Goals

1. Add ergonomic, predictable built-ins:
   - `prev(field)`
   - `lag(field, n)`
   - `delta(field)`
   - `delta(field, n)`
   - `rate(value_field, time_field)`
   - `ewma(key, value, alpha)`
2. Keep streaming behavior O(1) for `prev/delta/rate` and O(n) bounded by small `n` for `lag(field, n)`.
3. Provide resilient defaults (`()` for unavailable/invalid values) plus strict variants for fail-fast workflows.
4. Be explicit about parallel semantics from day one.
5. Integrate with existing docs/help style and test rigor.

### Non-Goals (v1)

1. Arbitrary SQL-style window functions (`lead`, `rank`, partition by key).
2. Stateful joins across unrelated streams.
3. Retroactive correction if out-of-order events appear.
4. Generalized user-defined rolling computations (future work).

## 3) User Experience

## 3.1 Function Signatures (v1)

Resilient variants (default):

```rhai
prev(field: &str) -> Dynamic | ()
lag(field: &str, n: int) -> Dynamic | ()
delta(field: &str) -> f64 | ()
delta(field: &str, n: int) -> f64 | ()
rate(value_field: &str, time_field: &str) -> f64 | ()
ewma(key: &str, value: f64, alpha: f64) -> f64
```

Strict variants:

```rhai
prev_strict(field: &str) -> Dynamic
lag_strict(field: &str, n: int) -> Dynamic
delta_strict(field: &str) -> f64
delta_strict(field: &str, n: int) -> f64
rate_strict(value_field: &str, time_field: &str) -> f64
ewma_strict(key: &str, value: f64, alpha: f64) -> f64
```

Optional fallback variants (if implemented in same release):

```rhai
prev_or(field: &str, fallback: Dynamic) -> Dynamic
lag_or(field: &str, n: int, fallback: Dynamic) -> Dynamic
delta_or(field: &str, fallback: f64) -> f64
rate_or(value_field: &str, time_field: &str, fallback: f64) -> f64
```

## 3.2 Example Usage

```bash
kelora -j app.jsonl --exec '
  e.prev_ms = prev("duration_ms");
  e.delta_ms = delta("duration_ms");
' --filter 'e.delta_ms != () && e.delta_ms > 500'
```

```bash
kelora -j counters.jsonl --exec '
  e.rps = rate("requests_total", "ts");
  e.rps_smooth = ewma("rps", e.rps ?? 0.0, 0.2);
'
```

## 3.3 UX Rules

1. Functions do **not** require `--window`.
2. Missing history returns `()` in resilient variants.
3. Strict variants raise runtime errors with actionable messages.
4. `lag(..., n)` requires `n >= 1`.
5. Time deltas that are `<= 0` return `()` (or error in strict mode).

## 4) Semantics and Edge Cases

## 4.1 History Unit

History is based on **post-parse event processing order** as seen by the pipeline in sequential mode.

## 4.2 Inclusion Rule

History updates after each event completes per-event script stages. If an event is filtered out by the user filter stage, it still contributes to adjacency only if we place update before filter finalization; otherwise not. Choose one policy and document clearly.

**Recommendation:** only events that reach exec stage and survive parse contribute. Filtered events still passed through scripting decisions; include them for deterministic “event-to-event” progression.

## 4.3 Type Coercion

`delta/rate` should accept:
- int/float directly
- numeric strings only in resilient mode if parsing succeeds
- otherwise `()` (or strict error)

## 4.4 Time Parsing for `rate`

`rate(value_field, time_field)` time field accepted forms:
1. `DateTimeWrapper`
2. RFC3339/string parseable by existing timestamp helper
3. numeric epoch seconds or milliseconds (heuristic optional; better explicit in v1)

Recommendation for v1: support `DateTimeWrapper` + RFC3339 string only; avoid ambiguous numeric epoch unit inference.

## 4.5 EWMA

Formula:

`S_t = alpha * x_t + (1 - alpha) * S_{t-1}`

Rules:
- `alpha` in `(0, 1]`
- first observation initializes `S_0 = x_0`
- keyed state namespace shared across run by `key`

## 4.6 File Boundaries

Default behavior: history continues across input files in stream order.
Future optional flag: `--reset-history-per-file`.

## 4.7 Parallel Mode

### v1 recommendation

Mark all functions as **sequential-only** and raise explicit runtime error in `--parallel` mode.

Rationale: cross-worker adjacency is undefined without boundary stitching.

### future

Add optional approximate per-worker semantics only with explicit opt-in (e.g., `--lag-local-worker`), not default.

## 5) Error Handling

## 5.1 Resilient Mode (default)

- Invalid/missing value => return `()`
- Insufficient history => `()`
- Invalid `n`/`alpha` => runtime error (argument contract violations should be hard errors)

## 5.2 Strict Variants

Runtime errors include:
- function name
- offending field name
- offending type/value
- fix suggestion

Example:

`delta_strict("duration_ms"): expected numeric current and lagged values; got string="abc" at current event`

## 6) Internal Design

## 6.1 New module

Create `src/rhai_functions/lag.rs` (or `inter_record.rs`) with registration in `mod.rs`.

## 6.2 Runtime storage

Thread-local per worker in sequential mode:

```text
history[field] = ring buffer of recent Dynamic values (for lag n)
last_time[field] / last_value[field] (for fast rate/delta)
ewma[key] = f64
```

Memory:
- `prev/delta/rate`: O(fields)
- `lag(n)`: O(fields * max_n_requested)

If dynamic `n` becomes large, enforce cap (e.g. 10_000) and error if exceeded.

## 6.3 Pipeline touchpoints

Need deterministic update point in event lifecycle. Preferred location: after parse success and after per-event script stage completion for current event, before moving to next event.

## 6.4 Mode guard

Mirror existing state/drain availability pattern for parallel guard and error text.

## 7) Documentation Plan

Update:
1. `src/rhai_functions/docs.rs` built-in catalog and examples.
2. `docs/reference/functions.md` detailed semantics and edge cases.
3. `docs/reference/script-variables.md` section clarifying adjacency semantics.
4. `--help-functions` examples for practical recipes.

## 8) Test Plan

Unit tests:
1. `prev` first event => `()`
2. `prev` second event returns first value
3. `lag(n)` with insufficient history
4. `delta` int, float, numeric string
5. `delta` non-numeric returns `()`
6. strict variants raise expected errors
7. `rate` with valid DateTime string sequence
8. `rate` with zero/negative dt
9. `ewma` initialization and recurrence
10. alpha bounds checks

Integration tests:
1. CLI pipeline with `--exec` and `--filter` examples.
2. Cross-file behavior in stream order.
3. Parallel mode explicit error message.

Property tests (optional):
- EWMA boundedness for bounded inputs.

## 9) Performance Considerations

1. Use small ring buffers and avoid cloning large objects where possible.
2. Cache field lookup indices if feasible.
3. Benchmark with and without lag functions to estimate overhead.

Target overhead guideline:
- `<10%` additional CPU for `prev/delta` usage at typical workloads.

## 10) Rollout Strategy

Phase 1:
- `prev`, `lag`, `delta`, `ewma` (+ strict variants)
- sequential-only guard

Phase 2:
- `rate` with robust timestamp handling
- fallback variants (`*_or`)

Phase 3:
- optional parallel semantics (only with explicit opt-in)

## 11) Open Questions

1. Should filtered events advance history?
2. Should numeric string parsing be enabled by default in resilient mode?
3. Should `rate` infer epoch units from numerics?
4. What cap should apply to `lag(n)`?
5. Should we expose reset functions (`reset_lag("field")`, `reset_ewma("key")`)?

