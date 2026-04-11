# Spec: Bloom Filter Novelty Detection (`is_new`)

Status: Draft for implementation planning  
Owner: Kelora core  
Scope: Rhai built-ins + tracking merge integration + docs/tests

## 1) Problem Statement

Kelora can estimate cardinality with HyperLogLog and compute streaming metrics, but cannot directly answer:

> “Have I seen this value/template before?”

Two use cases exist with different requirements:

- **Best-effort novelty detection**: memory matters more than catching every new item. A Bloom filter provides bounded memory with tunable error rate.
- **Comprehensive novelty detection**: every new item must be surfaced; occasional duplicate alerts are acceptable. Requires exact membership storage up to a configurable cap.

## 2) Goals & Non-Goals

### Goals

1. Add first-seen detection primitives with explicit, distinct guarantees:
   - `is_new(value, namespace)` — exact, never misses a new item
   - `is_new_approx(value, namespace)` — approximate (Bloom), bounded memory, may miss new items
2. Keep memory bounded and configurable for both variants.
3. Support sequential mode and design for parallel mergeability.
4. Provide transparent observability of approximation behavior.
5. Make failure modes visible in the API, not just the docs.

### Non-Goals (v1)

1. Deletions (counting Bloom filters not required for v1).
2. Time-decayed filters inside core (can be layered later).
3. Cross-process shared Bloom state.

## 3) User Experience

## 3.1 Function Signatures

Two variants with distinct guarantees:

```rhai
is_new(value: Dynamic, namespace: &str) -> bool
// Exact set. Never misses a new item. Returns true on first occurrence, false thereafter.
// May re-fire on a value after the namespace cap is reached (eviction). See §4.4.

is_new_approx(value: Dynamic, namespace: &str) -> bool
// Approximate (Bloom filter). Bounded memory. May silently suppress genuinely new items.
// When true: item is DEFINITELY new. When false: item is PROBABLY seen (but may be wrong).
// See §4.4 for the full error asymmetry.
```

Naming rationale: names that try to encode the error direction in the function name
(e.g. `maybe_new`, `is_definitely_new`) misplace the uncertainty and mislead users.
The `_approx` suffix signals "check the docs before relying on this" without implying
a specific direction. The distinction between the two functions makes the choice explicit.

Advanced (optional v1.1):

```rhai
bloom_contains(value: Dynamic, namespace: &str) -> bool
bloom_add(value: Dynamic, namespace: &str) -> bool      // returns true if maybe already present
bloom_stats(namespace: &str) -> Map                      // count, m_bits, k_hashes, est_fp
bloom_clear(namespace: &str) -> ()
```

Configuration:

```bash
--bloom-default-capacity 1000000     // applies to is_new exact cap and is_new_approx
--bloom-default-fp-rate 0.01         // is_new_approx only
--bloom-max-bytes 64MB               // is_new_approx only
```

or purely script-level config (alternative):

```rhai
bloom_config("templates", #{capacity: 2_000_000, fp_rate: 0.005})
```

## 3.2 Example Usage

Use `is_new` when you must catch every new occurrence (e.g. security alerts, audit trails):

```bash
kelora --drain -k message --exec '
  let tpl = drain_template(e.message).template;
  if is_new(tpl, "tpl") {
    print("NEW_TEMPLATE " + tpl);
  }
'
```

Use `is_new_approx` when dataset is too large for exact storage and occasional misses are acceptable
(e.g. dashboards, high-cardinality stream summaries):

```bash
kelora -j app.jsonl --exec '
  // Approximate: a small fraction of genuinely new user_ids will be silently skipped.
  if is_new_approx(e.user_id, "users") { track_count("new_users") }
' --metrics
```

## 4) Semantics and Edge Cases

## 4.1 Namespace behavior

- Each namespace has an independent filter instance (exact set or Bloom depending on function used).
- First call in namespace lazily creates filter using defaults/config.

## 4.2 Type canonicalization

To avoid accidental collisions across Dynamic types:

Hash input as:

```text
type_tag || canonical_serialization(value)
```

Examples:
- int `42` ≠ string `"42"`
- map/array canonicalized via stable serialization (sorted map keys)

## 4.3 Missing/unit values

Recommendation:
- `is_new((), ns)` => `false` (no insert), to match skip-on-missing behavior of many track functions.

Alternative is strict error; default should be non-disruptive.

## 4.4 Error asymmetry

**`is_new` (exact):**
- Below cap: no errors in either direction.
- After cap/eviction: may return `true` (new) for a previously seen item that was evicted.
  This is a *duplicate alert*, not a silent miss — the user's handler fires again.

**`is_new_approx` (Bloom):**

The error direction is important and counterintuitive:

| Return value | Meaning |
|---|---|
| `true` | Item is **definitely** new — certain, no false negatives |
| `false` | Item is **probably** seen — but may be wrong (false positive) |

The failure mode is a **silent miss**: `is_new_approx` returns `false` for a genuinely
new item. The user's “new item” handler never fires. There is no warning or signal that
anything was suppressed.

This makes the Bloom variant unsuitable for use cases that must surface every new event
(security alerts, audit trails, deduplication). For those, use `is_new`.

The nominal FP rate (default 1%) applies only at or below the configured capacity.
As inserts exceed capacity, the effective FP rate rises — see §4.5.

Docs and `--help-functions` output must state the error direction explicitly, not just
note that “approximation is used.” A one-liner is sufficient:
*”Returns true only when the item is definitely new; false means probably seen but may be wrong.”*

## 4.5 Saturation behavior

**`is_new_approx` (Bloom):** as inserts exceed planned capacity, FP rate rises silently.
Silent misses become more frequent without any per-event signal.

Policy options:
1. Warn once when saturation threshold exceeded.
2. Auto-grow filter (memory tradeoff).
3. Freeze and continue with degraded quality.

Recommendation v1: warn once + continue.

**`is_new` (exact):** as inserts approach the cap, behavior is configurable:
1. Evict LRU entries — previously seen items may re-trigger (duplicate alerts, not silent misses).
2. Hard stop — refuse further inserts with a diagnostic.

Recommendation v1: evict LRU + warn once. Duplicate alerts are preferable to silent misses.

## 4.6 Parallel mode semantics

Bloom is mergeable via bitwise OR if all workers use identical filter shape and hash seeds.

Guarantee requirements:
- same `m` bits, `k` hashes, and seeds across workers
- namespace config deterministic and shared

`is_new` in parallel has nuance:
- “new” is local to worker during processing if queried pre-merge.
- global first-seen exactness is not guaranteed in-flight.

Recommendation:
- v1 parallel behavior: allow but document as **worker-local novelty during processing**.
- For global novelty, evaluate in sequential mode or in post-merge summary workflow.

Alternative: make sequential-only in v1 for simplest semantics.

## 5) Error Handling

Hard errors:
1. invalid namespace (empty string)
2. invalid config values (`fp_rate <= 0 || >= 1`, `capacity < 1`)
3. exceeding `--bloom-max-bytes` at creation time

Soft handling:
1. `value == ()` => false
2. serialization failure => false + diagnostic (or strict-mode error)

Error messages should include namespace and configured parameters.

## 6) Internal Design

## 6.1 Storage

Integrate with tracking-like state map in thread-local context.

Suggested internal keys:

```text
__bloom::<namespace>::shape
__bloom::<namespace>::bits
__op___bloom::<namespace> = "bloom_or"
```

Need a compact blob format with magic header (similar to HLL blob strategy).

## 6.2 Hashing

Use stable, deterministic hash family with explicit seed constants.

Double-hashing strategy:

`h_i(x) = h1(x) + i * h2(x)`

for i in `[0, k)`.

## 6.3 Merge support

During parallel reduction, for keys marked `bloom_or`, perform bitwise OR on bitsets.
Reject merge if shapes mismatch (hard error with clear message).

## 6.4 API registration

Add a new Rhai module (`src/rhai_functions/bloom.rs`) and register in `mod.rs`.
Update docs generator text.

## 7) Documentation Plan

Update:
1. `src/rhai_functions/docs.rs`: entries for both `is_new` and `is_new_approx`. The
   `is_new_approx` entry must lead with the error direction, not bury it: *"Returns true
   only when the item is definitely new; false means probably seen but may be wrong."*
2. `docs/reference/functions.md`: full semantics for both variants, side-by-side comparison
   of guarantees, when-to-use guidance, and examples.
3. `docs/reference/cli-reference.md`: new bloom-related CLI knobs if introduced.
4. `--help-examples`: show both variants with contrasting use cases (audit trail vs. high-cardinality summary).

## 8) Test Plan

Unit tests:
1. deterministic hashing for same value across runs
2. type-tag separation (`42` int vs `"42"` string)
3. namespace isolation
4. `is_new` returns true first call, false subsequent call (single-thread)
5. `is_new_approx` returns true first call, false subsequent call (single-thread)
6. unit value behavior
7. invalid config errors
8. saturation warning path
9. blob serialize/deserialize roundtrip
10. `is_new` never returns false for a genuinely new item below cap (completeness property)
11. `is_new` re-fires after LRU eviction (duplicate alert, not silent miss)
12. `is_new_approx` FP rate within configured bound for fixed seed + synthetic dataset

Parallel tests:
1. two worker blooms OR-merge correctness
2. merge mismatch shape -> error
3. deterministic seeds produce stable merged behavior

Statistical tests (non-flaky constraints):
- For fixed seed and synthetic dataset, measured FP within reasonable bound margin.

## 9) Performance & Memory

Expected memory formula:

`m = - (n * ln p) / (ln 2)^2` bits

`k = (m/n) * ln 2`

For `n=1,000,000`, `p=0.01`:
- `m ≈ 9.6e6` bits (~1.2MB)
- `k ≈ 7`

CPU cost per insert/query: O(k).

Set default guardrails to prevent accidental huge allocations.

## 10) Rollout Strategy

Phase 1:
- `is_new(value, namespace)` — exact, with LRU cap
- `is_new_approx(value, namespace)` — Bloom, with fixed defaults
- sequential mode only (or documented worker-local semantics if parallel enabled)

Phase 2:
- config controls + `bloom_stats`
- parallel merge support finalized and tested

Phase 3:
- optional rotating namespaces/window helpers

## 11) Security & Abuse Considerations

1. User-controlled high-cardinality values can drive CPU (k hash ops/event) and memory growth by many namespaces.
2. Mitigations:
   - max namespaces
   - max bytes total
   - optional namespace allowlist

## 12) Open Questions

1. Should both variants be sequential-only initially for strict semantic clarity?
2. CLI-level config vs Rhai `bloom_config()` vs both?
3. `is_new_approx` saturation: warn-only or auto-grow?
4. Should `is_new` / `is_new_approx` insert on query (yes, recommended), or split contains/add only?
5. Do we need namespace eviction (`bloom_clear`) in v1?
6. What is the default cap for `is_new` exact? (Suggested: 100k entries; user must opt in to larger.)
7. Should mixing `is_new` and `is_new_approx` on the same namespace be a hard error?

