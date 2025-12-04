# Verbose Redesign Spec (-v / -vv / -vvv)

## Goals
- Make `-v`-levels show useful pipeline progress, not just errors.
- Keep output bounded/safe for terminals and `--parallel`.
- Keep fast path cheap when verbose is off.

## Current State (for reference)
- `-v`..`-vvv` only affect error verbosity plus Rhai debug tracer (can be very chatty).
- No per-stage visibility or progress indicators during long runs.

## Proposed Semantics
- `-v` (progress + errors):
  - Startup plan (once): input format (resolved), multiline strategy, timestamp hints, formatter, keys/core, take/head/context, span config, parallel/threads, stage count summary.
  - Immediate error lines as today.
  - Progress blocks: first block after whichever comes later: 5s or 5000 processed events since start; subsequent blocks keyed off last tick using the same rule. Skip if nothing processed. Counts: lines read, events emitted, errors by type (top 3, keyed by `error_type` suffix used in tracking), spans open/closed, throughput estimate (cumulative evts/sec).
- `-vv` (sampled flow):
  - Includes `-v`.
  - Stage map printed once with stable IDs matching runtime order (including implicit/auto stages; IDs start at 1 and are reused across modes): `#1 filter <expr trimmed>`, `#2 exec <script trimmed>`, etc.
  - Sampled event trace: first `K` events and every `M`th thereafter (defaults below). For each sampled event: `event <id> file:line stage_outcomes=` list of `filter#n=pass/fail`, `exec#m=ok`, `drop by filter#k` etc.; formatter preview (inspect one-liner); context marker (before/after/match) if active; span marker if active. Exec deltas obey truncation caps (see below) when listing added/removed keys.
  - Drop reasons: when an event is skipped, log the first responsible stage ID (sampled only). Formatter failures count as that stage.
  - Periodic block includes small per-stage aggregates (top 3 dropping/error stages), cumulative since start.
- `-vvv` (deep debug):
  - Includes `-vv`.
  - Existing Rhai ExecutionTracer output (scope/AST/step) but gated by sample budget (each tracer line counts toward global cap).
  - Parse errors show full offending line and line stats (current behavior), span lifecycle traces sampled, per-stage timing summary at end (cumulative ms and count, sorted by time).

## Defaults / Caps (tunable constants)
- Sampling: `K=5`, `M=100` for `-vv`; `K=3`, `M=50` for `-vvv`.
- Global trace cap per process: `MAX_TRACE_LINES=200` across sampled traces plus Rhai tracer; notice counts toward cap. Per-worker stderr buffer bounded to `min(500, MAX_TRACE_LINES)`; workers drop locally once their buffer is full and the coordinator prints a single “trace cap reached” notice on first drop.
- Truncation: script/expr previews 80 chars; key diffs 8 keys (applies to exec deltas and diffs); formatter preview 120 chars; UTF-8 safe with `...` suffix. No full payloads before `-vvv`.
- Progress cadence: see above (5s or 5000 events, whichever is later).
- Stop emitting traces after `--take`/`--head` exhausted; suppress further progress ticks once the take/head budget is consumed and the pipeline drains.

## Parallel Mode Handling
- Stage map and startup plan emitted by coordinator only.
- Event IDs assigned by coordinator, global monotonic, attached before workers run stages; parallel traces include worker tag (e.g., `w2:e1043`) but sampling uses the global ID for determinism.
- Per-event traces: workers decide sampling deterministically from the global event ID; send trace lines through the existing ordered stderr capture with the bounded buffer noted above. Once a buffer is full, workers drop locally; coordinator prints a single “trace cap reached” notice on first drop.
- Per-stage aggregates: use thread-local tracking with keys like `__kelora_stage_drop_<id>` / `__kelora_stage_error_<id>` and `__op_* = count` so reducer sums lock-free.
- Ordering: traces may interleave between workers; document “best-effort order”. Errors stay ordered as today.

## Data to Collect (hot-path constraints)
- Event identity: global monotonic ID; add worker tag only when in parallel. Include `file:line`.
- Per-stage status for the current event: enum pass/fail/exec_ok/drop_reason. Avoid cloning whole events; capture only filename:line, event id, and formatted preview (inspect) on sampled events.
- Exec deltas: when sampled, diff key set before/after exec; list added/removed keys only.
- Context/span markers from existing metadata.
- Counters: lines read, events emitted, errors by type, spans open/closed, per-stage drop/error counts (via tracking).

## Output Rules / Suppression
- Honor `--silent`/`-m`/`-s`/`--no-events`/`-q` → suppress traces/progress; still allow fatal errors.
- Honor `--no-diagnostics` → suppress periodic progress blocks only (traces/errors still print).
- Honor `--no-emoji`/`NO_EMOJI` in prefixes for all verbose lines.

## Failure / Backpressure Strategy
- If trace buffer is full or cap exceeded, drop further traces; emit one notice (counts toward cap). Never block workers.
- Progress blocks are best-effort; if reducer snapshot unavailable, skip the tick.

## Testing Notes
- Cover sequential vs parallel runs to ensure caps prevent runaway output.
- Validate drop reasons and stage IDs align with startup map.
- Ensure silent/-m/-s produce no traces.
- Check long filenames/basename conflicts still format correctly in traces.

## Open Questions (to decide when implementing)
- None. Sampling and cap constants stay compiled-in (no new flags); adjust only via code changes.
