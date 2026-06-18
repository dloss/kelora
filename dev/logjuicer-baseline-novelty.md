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

## Template ID stability — TESTED, and it's good news

An earlier draft of this note claimed Drain template IDs were *unstable* across
corpora (that the same line could yield different templates depending on what
else was ingested), which would have killed the simple "diff two ID lists"
design. **That claim was tested against the real binary and is false** for
kelora's configuration. Findings (debug build, default drain config):

- **Generalization comes entirely from the deterministic grok masking step**
  (`default_filter_patterns()`), which is a pure per-line function. `value is 1`,
  `value is 22`, `value is 333` all mask to `value is <num>` →
  `v1:eae236493a5d9680`, *identical across a 4-value baseline and a
  single-value target*.
- **Drain's own similarity-merge never produced a `<*>` wildcard.** Across five
  scenarios — varying early, middle, and late tokens; 12 distinct values; a
  mature 20-line cluster followed by variants — unmasked words were **never**
  generalized. Each distinct unmasked line became its own literal template.
- Therefore, in practice: `template(line) ≈ grok_mask(line)` — a pure function
  of the single line, **independent of corpus content and ingestion order**.
  `template_id = SHA256` of that is correspondingly stable.

**Consequence:** the simple design is viable. A "baseline model" is just a
**saved set of template IDs** (a plain text file) — no `DrainTree`
serialization, no dependency on `drain-rs` serde, no shared-tree two-phase
gymnastics. Build the baseline ID set once; for each target line compute its ID
and test set membership. Two independent runs compare cleanly.

**Caveats (the honest fine print):**

- This stability is a property of the *current* setup: grok masking does the
  work and Drain similarity-merging is effectively inert at the default
  `similarity = 0.4` / `depth = 4`. If a future change actually activates
  similarity-merging (or `drain-rs` changes behavior on upgrade), corpus-
  dependent `<*>` wildcards could appear and reintroduce instability. A novelty
  feature should pin/record the drain config it was built with.
- IDs also shift if `default_filter_patterns()` changes (a new mask = a new
  template string). The `v1:` prefix versions the *hashing*; a model file must
  *additionally* record the drain config + a filter-set version and refuse
  mismatched loads.
- Only as good as the masking: a variable token that grok does **not** mask
  (an unmasked username, a bare word ID) stays literal, so each distinct value
  is its own template. That inflates the baseline ID set and can cause
  false-positive novelty. This is a tuning/coverage issue (extend filters or
  add ignore patterns), not an instability.

## Proposed CLI surface

Primary, flag-driven for the main use case:

```
kelora --novel-vs <baseline-file> [target...]
```

- Mechanism (simpler than first thought — see stability finding above): in a
  Begin pre-pass, drain `<baseline-file>` and collect the **set of template
  IDs**; then stream targets and keep only events whose drained template ID is
  *not* in that set. No shared tree, no freeze step — just a `HashSet<String>`
  of IDs. Equivalently, `--novel-vs` could accept a saved ID-set file produced
  by `--drain=id`, since IDs are corpus-independent.
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
- Reuse existing drain tuning (`depth`, `max_children`, `similarity`). Note the
  **grok filter set — not the similarity threshold — is the dominant signal
  knob** here, since similarity-merging is effectively inert (see stability
  section). What gets masked determines what counts as "the same" line.

Optional frequency mode (binary membership is all-or-nothing — a template at
0.001% in the baseline reads as "seen" even if it's 60% of the target):
`--novel-vs-ratio <N>` flags templates whose target frequency is ≥ N× their
baseline frequency, catching rate-shift anomalies that pure set-membership
misses. Defer unless usage shows membership alone is too coarse.

## Integration points

- `src/drain.rs`: minimal change. `generate_template_id` is already exactly the
  primitive we need and is contractually stable (`v1:`). Add a way to compute a
  template ID for a single line against a *fresh, throwaway* drain state (so the
  baseline pre-pass and the target classification don't share/contaminate state)
  — or simply collect the baseline ID set from one drain run and the target IDs
  from another, which is sound given IDs are corpus-independent.
- `src/pipeline/stages.rs`: today `DrainStage` (line ~1112) is summary-only and
  passes events through unchanged. Novelty needs a *classifying* stage that
  holds the baseline `HashSet<String>` of IDs and `Skip`s events whose template
  ID is present. Baseline ingestion is a Begin-phase pre-pass (batch), then
  per-event classification streams as normal. Sequential-only, consistent with
  `--drain` (`drain` state is `thread_local`).
- `src/cli.rs`: add the flags near the existing `--drain` block (~line 818),
  with the same "data-only mode" semantics (hush hints, still surface warnings).

## Limitations / forcing functions (be honest in docs)

- **Binary membership misses rate shifts.** A template at 0.001% in baseline but
  60% in target reads as "seen." Membership is all-or-nothing where logjuicer's
  cosine is continuous. Optional ratio mode is the mitigation.
- **Novelty quality = masking coverage.** Because generalization is the grok
  masking step (not Drain similarity), signal quality depends on the filter
  patterns catching the variable tokens. Unmasked variable tokens (bare-word
  IDs, usernames) inflate the baseline set and cause false-positive novelty.
  Surface this; let users extend filters / add ignore patterns.
- **Single field only.** Novelty in structured fields (a new `error_code`)
  isn't caught unless folded into the drained text. Same scope as `--drain`.
- **Incomplete baseline → false positives** (rare-but-normal lines). Mitigated,
  not solved, by `--novel-extra-baseline` and `--novel-ignore`.
- **No semantic understanding** — same ceiling logjuicer hits. Statistical
  novelty, not root-cause reasoning.
- **Template strings shift if grok filters change.** Any change to
  `default_filter_patterns()` invalidates a saved ID-set model. The model file
  must record the drain config + a filter-set version and refuse mismatched
  loads.

## Alternatives considered

- *Diff two `--drain=id` outputs* (naive). **No longer rejected** — testing
  showed cross-corpus ID equality *is* reliable in kelora's config (see
  stability section). This is now a legitimate, even trivial, implementation
  path: a baseline model is just a saved ID set.
- *Shared-tree, two-phase, per-cluster baseline/target counts.* The over-
  engineered design from the first draft, motivated by a stability fear that
  didn't hold. Dropped in favor of the ID-set approach.
- *Port logjuicer's cosine-similarity engine.* Rejected for now — different
  philosophy from Kelora's deterministic streaming model, adds vector-math
  dependency and a "training" concept that sits awkwardly in a Unix-pipe tool.
  Template membership gets ~80% of the value, deterministic and explainable.
- *Pure Rhai-only solution* (no flag). Fine as a *secondary* path now that the
  primitive is just "compute ID, test set membership"; keep the flag as the
  primary ergonomic entry point.

## Phasing

1. **Spike (validate signal, not stability — stability is confirmed):**
   prototype `--novel-vs` as baseline ID-set + per-target membership; eyeball
   results on a real before/after log pair. Confirm masking coverage is good
   enough to separate novel from known lines without drowning in false
   positives.
2. **Productionize:** CLI flags, `--novel-ignore`, `--novel-extra-baseline`,
   a portable `--novel-model <file>` (saved ID set + recorded drain/filter
   version), docs (`--help-functions`/`docs.rs` if the Rhai predicate ships),
   tests (clustering already has good coverage to model after).
3. **Maybe:** `--novel-vs-ratio` frequency mode, only if membership proves
   too coarse in practice.

## Should we?

Yes — but only the focused, Kelora-idiomatic subset above (template membership
on the existing grok+Drain pipeline, output-as-events). It's a genuine
differentiator with low architectural risk and high reuse. Hold the line
against the cosine-similarity ML engine and the HTML report. Kelora should
borrow logjuicer's *question*, not become logjuicer.
