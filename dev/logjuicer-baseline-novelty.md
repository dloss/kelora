# Baseline Novelty Detection — Design Note (logjuicer-inspired)

Status: **proposal / not started.** Captures whether and how to borrow
[logjuicer](https://github.com/logjuicer/logjuicer)'s core idea — *highlight
the lines in a target log that don't appear in a known-good baseline* — without
importing its machinery wholesale. Written after stress-testing the idea
against the real `src/drain.rs` implementation.

## What logjuicer does (and what we'd actually borrow)

logjuicer compares a **target** log against a **baseline** of known-good logs:

1. Tokenize each line, stripping random/variable words (IDs, timestamps, hashes).
2. Vectorize lines via the hashing trick.
3. Nearest-neighbour by cosine similarity against the baseline corpus; lines
   with no close match are flagged as anomalies.
4. Optional persistent model; HTML report.

Its sweet spot is CI/build-failure triage ("here's a passing run, here's a
failing run — what's actually different?").

**Decision: adopt the *concept*, not the implementation.** Kelora already does
logjuicer's step 1 (tokenization) via the grok filter patterns in
`src/drain.rs` (`default_filter_patterns()` masks IPs, UUIDs, paths,
timestamps, numbers, …). Drain clustering then gives us a deterministic,
inspectable "template" per line. We build novelty detection on **template
set membership**, deliberately **skipping** the cosine-similarity/hashing-trick
ML core (fuzzy, non-reproducible, new dependency) and the HTML report (out of
scope — Kelora emits events and pipes to other tools).

## The load-bearing constraint (read this first)

Drain `template_id` = `SHA256(template_string)`, but the template *string* is an
emergent property of **the whole corpus and its ingestion order**, not a pure
function of a single line. The tree decides which token positions are variable
based on everything it has seen. The same line can therefore produce different
templates — and different IDs — in two independent runs:

```
baseline corpus (many users):  "user <*> logged in"     -> v1:aaaa…
target run (only alice seen):  "user alice logged in"   -> v1:bbbb…
```

**Consequence:** the tempting design — "run `--drain=id` on the baseline, save
the ID list, later diff the target's IDs against it" — is *fragile* and emits
phantom anomalies. It must be rejected.

**Correct design:** one shared `DrainTree`. Ingest the baseline first, freeze
which clusters are "baseline", then classify target lines against that *same*
tree. A target line is **novel** if it lands in a cluster with zero baseline
matches, or forces a brand-new cluster. This requires extending
`src/drain.rs` to track per-cluster baseline-vs-target counts (today it only
tracks a single rolling count + sample/line metadata).

Implication for "portable models": a true precomputed model file requires
serializing the `DrainTree`. `drain-rs = "0.3.0"` serde support is **unverified**
(deps not vendored at time of writing — validate before promising this). The
safe baseline (pun intended) is to **re-ingest the baseline file on every run**.
Treat a serialized model as a stretch goal gated on that validation.

## Proposed CLI surface

Primary, flag-driven for the main use case:

```
kelora --novel-vs <baseline-file> [target...]
```

- Two-phase: load + drain `<baseline-file>`, freeze, then stream targets and
  keep only events whose drained template is novel vs the baseline.
- Reuses the existing one-field constraint: requires `--keys` with exactly one
  field (same rule as `--drain`), drained via the same grok pipeline.
- Output is **normal events** (novel ones), so it composes with `-J`, `-k`,
  `--metrics`, downstream `grep`/`jq`, etc. This is the key advantage over
  logjuicer's bespoke report.

Secondary, for composability (optional, later):

- A Rhai predicate `seen_in_baseline(text) -> bool` so scripts can tag rather
  than filter (`e.novel = !seen_in_baseline(e.msg)`), enabling
  annotate-don't-drop workflows and mixing with other conditions.

Knobs (mirror logjuicer's escape hatches — these address real false-positive
sources, not nice-to-haves):

- `--novel-ignore <regex>` (repeatable): drop noisy lines from *both* corpora
  before draining. Directly borrowed from logjuicer's ignore-patterns.
- `--novel-extra-baseline <file>` (repeatable): fold additional known-good
  lines into the baseline that the primary baseline omits (logjuicer's
  "extra baseline" — the answer to "rare-but-normal" false positives).
- Reuse existing drain tuning (`depth`, `max_children`, `similarity`). **The
  similarity threshold is the dominant signal knob**; document the
  false-negative (too low) vs noise (too high) trade-off prominently.

Optional frequency mode (addresses the binary-membership blind spot, see
Limitations): `--novel-vs-ratio <N>` flags templates whose target frequency is
≥ N× their baseline frequency, catching rate-shift anomalies that pure
set-membership misses. Defer unless usage shows membership alone is too coarse.

## Integration points

- `src/drain.rs`: extend `DrainState`/`DrainResult` to track per-cluster
  baseline vs target match counts and a "frozen after baseline phase" flag.
  Keep `generate_template_id` untouched (it is contractually stable, `v1:`).
- `src/pipeline/stages.rs`: today `DrainStage` (line ~1112) is summary-only and
  passes events through unchanged. Novelty needs a *classifying* stage that can
  `Skip` non-novel events — model it after `DrainStage` but emitting/dropping
  per the membership result. Baseline ingestion is a Begin-phase pre-pass
  (batch), then per-event classification streams as normal. This is sequential-
  only, consistent with `--drain` (`drain` state is `thread_local`).
- `src/cli.rs`: add the flags near the existing `--drain` block (~line 818),
  with the same "data-only mode" semantics (hush hints, still surface warnings).

## Limitations / forcing functions (be honest in docs)

- **Binary membership misses rate shifts.** A template at 0.001% in baseline but
  60% in target reads as "seen." Membership is all-or-nothing where logjuicer's
  cosine is continuous. Optional ratio mode is the mitigation.
- **Similarity threshold is the whole ballgame.** Quality lives or dies on one
  number; surface it and document the trade-off.
- **Single field only.** Novelty in structured fields (a new `error_code`)
  isn't caught unless folded into the drained text. Same scope as `--drain`.
- **Incomplete baseline → false positives** (rare-but-normal lines). Mitigated,
  not solved, by `--novel-extra-baseline` and `--novel-ignore`.
- **Drain ordering is mildly non-deterministic.** Same data ingested in a
  different order can yield slightly different clusters. Acceptable; document it.
- **No semantic understanding** — same ceiling logjuicer hits. Statistical
  novelty, not root-cause reasoning.
- **Template strings shift if grok filters change.** Any change to
  `default_filter_patterns()` invalidates a serialized model. If we ever ship
  portable models, the model file must record the drain config + a filter-set
  version and refuse mismatched loads.

## Alternatives considered

- *Diff two `--drain=id` outputs* (naive). Rejected — see the load-bearing
  constraint; cross-corpus ID equality is not reliable.
- *Port logjuicer's cosine-similarity engine.* Rejected for now — different
  philosophy from Kelora's deterministic streaming model, adds vector-math
  dependency and a "training" concept that sits awkwardly in a Unix-pipe tool.
  Template membership gets ~80% of the value, deterministic and explainable.
- *Pure Rhai-only solution* (no flag). Rejected as the primary path — the
  two-phase baseline ingestion and shared-tree state are too awkward to express
  per-event in script; better as a stage. Keep the Rhai predicate as a
  composable secondary.

## Phasing

1. **Spike (validate the core):** extend `DrainState` for baseline/target
   counts; prototype `--novel-vs` with re-ingested baseline; eyeball signal on
   a real before/after log pair. Confirm the shared-tree approach actually
   separates novel from known lines at the default threshold.
2. **Productionize:** CLI flags, `--novel-ignore`, `--novel-extra-baseline`,
   docs (`--help-functions`/`docs.rs` if the Rhai predicate ships), tests
   (clustering already has good coverage to model after).
3. **Stretch:** validate `drain-rs` serde support; if present, add a portable
   `--novel-model <file>` with config/filter-version guarding. If absent, stop
   at re-ingestion.
4. **Maybe:** `--novel-vs-ratio` frequency mode, only if membership proves
   too coarse in practice.

## Should we?

Yes — but only the focused, Kelora-idiomatic subset above (template membership
on the existing grok+Drain pipeline, output-as-events). It's a genuine
differentiator with low architectural risk and high reuse. Hold the line
against the cosine-similarity ML engine and the HTML report. Kelora should
borrow logjuicer's *question*, not become logjuicer.
