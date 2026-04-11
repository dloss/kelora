# Refactoring Plan

This plan covers structural improvements to the current codebase without a full
rewrite. Each item is scoped to be completable as a standalone PR. None require
architectural changes to the pipeline, the Rhai integration, or the public CLI
behaviour.

The test suite (18 K lines, 41 integration test files) is the safety net
throughout. Run `just test` before and after every item.

---

## Out of scope

These would require a full rewrite and are explicitly deferred:

- Decoupling `Event` from `rhai::Dynamic`
- Eliminating all global mutable state from stats / tracking / field-discovery
- Making stateful formatters (levelmap, keymap, tailmap) parallel-safe

---

## Items

Items are ordered by recommended execution sequence. Later items are easier or
cleaner after earlier ones land.

---

### R1 — Merge the duplicated hot-path methods in `pipeline/mod.rs`

**Problem.** `process_line` and `process_chunk_directly` are ~500 lines each and
nearly identical. The comment in `process_chunk_directly` even says: *"This is
the same logic as in process_line starting from the 'Parse stage' comment."*
Any bug fix or feature addition in the parse-to-output path currently requires
editing two copies.

**Goal.** One shared `process_chunk(chunk: String, ctx: &mut PipelineContext)
-> Result<Vec<FormattedOutput>>` private method that both call.

**Affected files.**
- `src/pipeline/mod.rs` (primary)

**Approach.**

1. Read both methods side-by-side and document every actual difference (there
   are very few — mainly that `process_line` feeds through the chunker first).
2. Extract the body that starts at "Parse stage" into `fn process_chunk(...)`.
3. `process_line`: call `self.chunker.feed_line(line)` → if `Some(chunk)`,
   delegate to `process_chunk`.
4. `process_chunk_directly`: delegate directly to `process_chunk`.
5. Delete the now-redundant body.

**Verification.** `just test` — the parallel, sequential, multiline, and span
integration tests all exercise this path.

**Risk.** Low. Pure deduplication. The compiler verifies the signatures; the
tests verify the behaviour.

---

### R2 — Factor out `apply_script_result` duplication

**Problem.** The `Emit` and `EmitMultiple` branches of `apply_script_result`
in `src/pipeline/mod.rs` are ~200 lines each of near-identical stat tracking,
span handling, limiter checks, and output logic.

**Goal.** A private `fn apply_single_event(event: Event, ...) ->
Result<Option<FormattedOutput>>` helper, with `EmitMultiple` iterating over it.

**Affected files.**
- `src/pipeline/mod.rs`

**Approach.**

1. List every real difference between the two branches (file-ops assignment for
   index 0 in the multi-event case is the main one).
2. Parameterise the helper to cover those differences.
3. Replace both branches with calls to the helper.

**Verification.** Output formatting, parallel, and span tests.

**Risk.** Low-medium. The function is complex. Diff carefully before deleting
the old code.

---

### R3 — Split `engine.rs` into a module

**Problem.** `src/engine.rs` is 3 851 lines mixing Rhai engine construction,
function registration, event↔scope conversion, filter compilation, script
execution, window handling, and a dead debug scaffolding.

**Goal.** `src/engine/` module with focused files. No logic changes.

**Affected files.**
- `src/engine.rs` → delete
- `src/engine/mod.rs` — public re-exports, `RhaiEngine` struct
- `src/engine/compiler.rs` — `CompiledExpression`, `compile_filter_*`
- `src/engine/executor.rs` — `execute_compiled_filter*`, `execute_script*`
- `src/engine/scope.rs` — event↔Rhai scope conversion helpers
- `src/engine/debug.rs` — `DebugTracker`, `DebugConfig` (see R4)

**Approach.** Move code file by file. Each move is a `cargo check`-able step.
Update `mod engine;` in `main.rs`/`lib.rs` — nothing else needs to change
because the public surface stays the same.

**Verification.** `cargo check` after each file move; `just test` at the end.

**Risk.** Very low. Pure reorganisation; the compiler enforces correctness.

---

### R4 — Remove dead `DebugTracker` code

**Depends on R3** (so the code is visible in isolation).

**Problem.** `DebugTracker` and `DebugConfig` in `engine.rs` are compiled in
but never reachable from the CLI binary. They add ~300 lines of noise and the
`#[allow(dead_code)]` annotation at the top of the file is there to suppress
the resulting warnings.

**Goal.** Delete `engine/debug.rs`. Remove the `#[allow(dead_code)]` attribute
if it was only covering this code.

**Approach.**

1. Confirm no public or crate-level callsites: `grep -r "DebugTracker\|DebugConfig" src/`.
2. Delete `engine/debug.rs`.
3. Remove its `mod debug;` declaration and any re-exports.
4. Remove the module-level `#[allow(dead_code)]` if it is now unneeded.

**Verification.** `cargo check --all-targets` must emit zero dead-code
warnings for the engine module.

**Risk.** Very low. Dead code removal.

---

### R5 — Split `rhai_functions/tracking.rs` into a module

**Problem.** `src/rhai_functions/tracking.rs` is 4 386 lines mixing: error
tracking, user-visible metrics aggregation, internal stats counters,
snapshot formatting, JSON serialisation, and parallel merge logic. It is the
largest file in the codebase.

**Goal.** `src/rhai_functions/tracking/` module. No logic changes.

**Affected files.**
- `src/rhai_functions/tracking.rs` → delete
- `src/rhai_functions/tracking/mod.rs` — public re-exports, `TrackingSnapshot`
- `src/rhai_functions/tracking/errors.rs` — `track_error`, error sample storage
- `src/rhai_functions/tracking/metrics.rs` — user-facing `track_*` functions
- `src/rhai_functions/tracking/merge.rs` — parallel merge logic (HLL, T-digest,
  counters, arrays)
- `src/rhai_functions/tracking/format.rs` — `format_metrics_output`,
  `format_metrics_json`, `extract_error_summary_from_tracking`

**Approach.** Same as R3: move code incrementally, `cargo check` each step.

**Verification.** `just test` — metrics_tracking_tests.rs and parallel_tests.rs
are the key files.

**Risk.** Very low. Pure reorganisation.

---

### R6 — Replace `lazy_static!` with `std::sync::LazyLock`

**Problem.** `lazy_static` is an external dependency used for global
initialisation. `std::sync::LazyLock` (stable since Rust 1.80) is a drop-in
replacement. `once_cell::sync::Lazy` is also present in places and can be
replaced at the same time.

**Goal.** Remove the `lazy_static` and `once_cell` dependencies (or downscope
`once_cell` to only the parts that use `OnceCell`/`OnceLock` not covered by
std).

**Affected files.** Grep for `lazy_static!` and `once_cell::sync::Lazy` — as
of the current tree this touches ~10 files in `src/`.

**Approach.**

```rust
// Before
lazy_static! { static ref FOO: Foo = Foo::new(); }
// After
static FOO: LazyLock<Foo> = LazyLock::new(|| Foo::new());
```

For `OnceLock` / `OnceCell` usage already using std equivalents, no change
needed.

**Verification.** `cargo check`, then `just test`. Also run `cargo deny` to
confirm the dependency is gone.

**Risk.** Very low. Mechanical substitution with identical semantics.

---

### R7 — Extract timestamp anchor resolution from `main.rs`

**Problem.** Lines 149–298 of `src/main.rs` are ~130 lines of since/until
anchor resolution logic (handling `since+/since-/until+/until-` prefixes and
circular-dependency detection). This logic belongs in `timestamp.rs`, which
already owns all other timestamp parsing.

**Goal.**

```rust
// src/timestamp.rs
pub fn resolve_time_range(
    since_str: Option<&str>,
    until_str: Option<&str>,
    tz: Option<&str>,
) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)>
```

`main.rs` calls this one function and handles only the error formatting.

**Affected files.**
- `src/main.rs` (remove ~130 lines)
- `src/timestamp.rs` (add ~130 lines + unit tests)

**Approach.** Copy the logic to `timestamp.rs` first, add unit tests, then
replace the inline block in `main.rs` with the new function call.

**Verification.** `timestamp_filtering_tests.rs` (1 111 lines) covers this
path exhaustively. Add a few unit tests directly on the new function for the
anchor edge cases.

**Risk.** Low. The logic moves unchanged; the tests cover the behaviour.

---

### R8 — Remove duplicate `validate_cli_args` call in `main.rs`

**Problem.** `validate_cli_args(&cli)` is called at both line 76 and line 425
of `src/main.rs`. The second call is a no-op redundancy left over from
refactoring.

**Goal.** One call, at line 76 (before config construction).

**Affected files.**
- `src/main.rs`

**Verification.** `just test`.

**Risk.** Trivial.

---

### R9 — Replace `__kelora_stats_*` magic keys in `internal_tracker`

**Depends on R5** (so the merge logic is isolated and readable).

**Problem.** Parallel workers communicate internal stats to the merger thread
via magic string keys (`"__kelora_stats_events_created"`,
`"__op___kelora_stats_events_created"`, etc.) inside a
`HashMap<String, Dynamic>`. This convention is invisible to the type system:
a typo silently produces wrong counts, and every stat requires two insertions
(the value and the `__op_` merge hint).

**Goal.** A typed `InternalStats` struct that workers return alongside their
`FormattedOutput` batches. The merger thread receives and merges typed values.

```rust
#[derive(Default)]
struct InternalStats {
    events_created: u64,
    events_output: u64,
    events_filtered: u64,
    lines_errors: u64,
    discovered_levels: Vec<String>,
    discovered_keys: Vec<String>,
    // ...
}
```

The `__kelora_stats_*` / `__op_*` keys are removed entirely from
`internal_tracker`. `internal_tracker` can then shrink to only the
user-visible tracker entries.

**Affected files.**
- `src/pipeline/mod.rs` — `PipelineContext`, `collect_discovered_*` helpers
- `src/parallel/worker.rs` — return `InternalStats` with batch result
- `src/parallel/tracker.rs` — merge `InternalStats` instead of HashMap scan
- `src/parallel/sink.rs` — aggregate merged stats
- `src/runner.rs` — assemble final `ProcessingStats` from merged `InternalStats`

**Verification.** `parallel_tests.rs` (2 138 lines) and
`metrics_tracking_tests.rs` (1 480 lines) are the primary targets.

**Risk.** Medium. The parallel path is the most complex part of the codebase.
Make this change on a dedicated branch, run the full test suite including
`just bench-quick` to confirm no performance regression.

---

### R10 — Consolidate `rhai_functions/strings/` submodules

**Problem.** The strings module is split into 8 files (mod.rs, core.rs,
slice.rs, trim.rs, regex.rs, substring.rs, output.rs, tests.rs) for what is a
single logical concern. The granularity adds coordination overhead without
adding clarity.

**Goal.** Merge into 3 files:
- `strings/mod.rs` — registration + re-exports
- `strings/ops.rs` — all non-regex string operations (merge core, slice, trim,
  substring, output)
- `strings/regex_ops.rs` — regex-backed operations
- `strings/tests.rs` — keep as-is

**Affected files.** `src/rhai_functions/strings/`

**Verification.** `text_functions_tests.rs` (687 lines).

**Risk.** Very low. Pure reorganisation.

---

## Sequence summary

| # | Item | Risk | Depends on |
|---|------|------|------------|
| R1 | Merge `process_line`/`process_chunk_directly` | Low | — |
| R2 | Factor out `apply_script_result` duplication | Low-medium | R1 (cleaner after) |
| R3 | Split `engine.rs` into module | Very low | — |
| R4 | Remove dead `DebugTracker` | Very low | R3 |
| R5 | Split `tracking.rs` into module | Very low | — |
| R6 | Replace `lazy_static` / `once_cell` with std | Very low | — |
| R7 | Extract timestamp anchor resolution | Low | — |
| R8 | Remove duplicate `validate_cli_args` call | Trivial | — |
| R9 | Replace `__kelora_stats_*` magic keys | Medium | R5 |
| R10 | Consolidate `strings/` submodules | Very low | — |

R3, R5, R6, R7, R8, R10 are fully independent and can be done in any order or
in parallel across branches. R1 should land before R2. R9 is the only
medium-risk item and should be done last, after R5, on its own branch with
extra review.

---

## Definition of done

Each item is complete when:
- `just fmt` passes with no changes
- `just lint` passes with zero warnings
- `just test` passes
- The git log for the PR contains no functional changes (only structure,
  naming, and file organisation) — or if logic did move, it is covered by new
  unit tests added in the same PR
