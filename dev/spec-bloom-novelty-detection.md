# Spec: Bloom Filter Novelty Detection (`is_new`)

Status: Draft for implementation planning  
Owner: Kelora core  
Scope: Rhai built-ins + tracking merge integration + docs/tests

## 1) Problem Statement

Kelora can estimate cardinality with HyperLogLog and compute streaming metrics, but cannot directly answer:

> “Have I seen this value/template before?”

Exact sets can be memory-heavy at high cardinality. A Bloom filter provides bounded memory membership tests with no false negatives and tunable false-positive rate.

## 2) Goals & Non-Goals

### Goals

1. Add first-seen detection primitive:
   - `is_new(value, namespace)`
2. Keep memory bounded and configurable.
3. Support sequential mode and design for parallel mergeability.
4. Provide transparent observability of approximation behavior.

### Non-Goals (v1)

1. Deletions (counting Bloom filters not required for v1).
2. Time-decayed filters inside core (can be layered later).
3. Cross-process shared Bloom state.

## 3) User Experience

## 3.1 Function Signatures

Core:

```rhai
is_new(value: Dynamic, namespace: &str) -> bool
```

Advanced (optional v1.1):

```rhai
bloom_contains(value: Dynamic, namespace: &str) -> bool
bloom_add(value: Dynamic, namespace: &str) -> bool      // returns true if maybe already present
bloom_stats(namespace: &str) -> Map                      // count, m_bits, k_hashes, est_fp
bloom_clear(namespace: &str) -> ()
```

Configuration:

```bash
--bloom-default-capacity 1000000
--bloom-default-fp-rate 0.01
--bloom-max-bytes 64MB
```

or purely script-level config (alternative):

```rhai
bloom_config("templates", #{capacity: 2_000_000, fp_rate: 0.005})
```

## 3.2 Example Usage

```bash
kelora --drain -k message --exec '
  let tpl = drain_template(e.message).template;
  if is_new(tpl, "tpl") {
    print("NEW_TEMPLATE " + tpl);
  }
'
```

```bash
kelora -j app.jsonl --exec '
  if is_new(e.user_id, "users") { track_count("new_users") }
' --metrics
```

## 4) Semantics and Edge Cases

## 4.1 Namespace behavior

- Each namespace has an independent Bloom filter instance.
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

## 4.4 False positives

- Possible: returning “seen” for actually new item.
- Impossible: returning “new” for previously inserted item (assuming deterministic hashing).

Docs must explicitly state this.

## 4.5 Saturation behavior

As inserts exceed planned capacity, FP rate rises.

Policy options:
1. Warn once when saturation threshold exceeded.
2. Auto-grow filter (memory tradeoff).
3. Freeze and continue with degraded quality.

Recommendation v1: warn once + continue.

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
1. `src/rhai_functions/docs.rs`: function entries + warnings about FP behavior.
2. `docs/reference/functions.md`: full semantics, examples, caveats.
3. `docs/reference/cli-reference.md`: new bloom-related CLI knobs if introduced.
4. `--help-examples`: first-seen template detection example.

## 8) Test Plan

Unit tests:
1. deterministic hashing for same value across runs
2. type-tag separation (`42` int vs `"42"` string)
3. namespace isolation
4. `is_new` returns true first call, false subsequent call (single-thread)
5. unit value behavior
6. invalid config errors
7. saturation warning path
8. blob serialize/deserialize roundtrip

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
- `is_new(value, namespace)` with fixed defaults
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

1. Should `is_new` be sequential-only initially for strict semantic clarity?
2. CLI-level config vs Rhai `bloom_config()` vs both?
3. Default behavior on saturation: warn-only or auto-grow?
4. Should `is_new` insert on query (yes, recommended), or split contains/add only?
5. Do we need namespace eviction (`bloom_clear`) in v1?

