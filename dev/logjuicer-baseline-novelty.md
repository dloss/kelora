# Baseline Novelty Detection — Design Note (logjuicer-inspired)

Status: **proposal / not started — concept UX-validated by simulation.**
Captures whether and how to borrow
[logjuicer](https://github.com/logjuicer/logjuicer)'s core idea — *highlight
the lines in a target log that don't appear in a known-good baseline* — without
importing its machinery wholesale. Written after stress-testing the idea
against the real `src/drain.rs` and simulating the end-to-end UX with the
existing `drain_template()` Rhai function (see "Template ID stability").

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

## Template ID stability — TESTED end-to-end on realistic logs

This section has flip-flopped; here is the empirically-settled truth. Two
claims were tested against the real binary, simulating `--novel-vs` with the
existing `drain_template()` Rhai function (the exact primitive the feature
would use) on a 200-line baseline vs a 203-line target (same normal traffic +
3 injected anomalies: a NullPointerException, a DB-pool-exhausted error, an
OutOfMemoryError).

**Finding 1 — IDs are NOT corpus-stable for realistic lines.** An interim draft
claimed `template(line) ≈ grok_mask(line)`, a pure per-line function, based on
toy 4–5 token tests. That was wrong: those tests never triggered Drain's
leaf-level similarity-merging. Realistic multi-token lines *do* trigger it, and
the resulting wildcard placement is **order- and corpus-dependent**. Concretely,
identical normal traffic produced disjoint template sets in two independent
runs:

```
BASELINE run:   ... msg="GET <path> <num> in <duration>     (GET generalized)
                ... msg="PUT /login <num> in <duration>     (PUT kept literal)
TARGET run:     ... msg="PUT <path> <num> in <duration>     (PUT generalized)
                ... msg="GET /health <num> in <duration>    (GET kept literal)
```

→ **zero ID overlap** between the two runs, even for lines of the same shape.
(For the same input fed twice, batch `--drain=id` and per-event
`drain_template()` *do* agree — the instability is across *different* corpora,
not across code paths.)

**Finding 2 — the naive "saved ID set" design is UNUSABLE.** Building a baseline
ID set and testing target IDs against it flagged **203 of 203** target events as
novel — every normal line a false positive, the 3 real anomalies buried. This
kills the "model = a text file of IDs / diff two `--drain=id` outputs" design.

**Finding 3 — the shared-tree design WORKS, and works well.** Feeding baseline
then target through *one* drain state (shared tree), and flagging target lines
whose template ID was not seen during the baseline phase, surfaced **exactly 3
of 203** — precisely the injected anomalies, zero false positives, zero misses.
This is the design the very first draft proposed (and an interim draft wrongly
discarded). It is the design to build.

**Consequences for the design:**

- A baseline "model" is **not** a portable set of IDs. Correct novelty needs the
  baseline *ingested into the same tree* that classifies the target. The simple
  implementation re-ingests the baseline file on every run (cheap). A truly
  portable precomputed model needs the serialized `DrainTree` (`drain-rs`
  0.3.0 serde support **unverified** — validate before promising it).
- **Freeze risk to validate in implementation:** as target lines merge into an
  existing cluster, that cluster's template string can generalize further and
  its hash/ID can drift, which could make a known line look novel. It did not
  bite in this test, but the robust implementation should *freeze* the tree
  after the baseline phase and do **read-only matching** for the target (no new
  clusters, no further generalization). `drain-rs`'s `add_log_line` mutates;
  whether a read-only match path exists upstream is an open question.

**Caveats that still hold:**

- IDs shift if `default_filter_patterns()` or drain config changes. The `v1:`
  prefix versions only the *hashing*; any persisted model must additionally
  record drain config + a filter-set version and refuse mismatched loads.
- Novelty quality tracks **grok masking coverage**: a variable token grok does
  not mask (a bare-word ID, an unmasked username) stays literal, so each value
  is its own template — inflating the baseline set and causing false positives.
  Mitigate with ignore patterns / extended filters, not by trusting the default.

## Proposed CLI surface

Primary, flag-driven for the main use case:

```
kelora --novel-vs <baseline-file> [target...]
```

- Mechanism (shared-tree, two-phase — see the tested findings above): in a Begin
  pre-pass, ingest `<baseline-file>` into the drain tree and record the set of
  template IDs produced; **freeze** the tree; then stream targets through the
  *same* tree (read-only match) and keep only events whose template ID was not
  in the baseline set. Validated to surface exactly the anomalies with no false
  positives. The naive "diff two independent `--drain=id` runs" does **not**
  work (100% false positives) — do not implement that.
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
- Reuse existing drain tuning (`depth`, `max_children`, `similarity`). Both the
  grok filter set *and* the similarity threshold matter: masking decides which
  tokens are variable, and similarity-merging (which **is** active on realistic
  lines) decides how aggressively lines collapse into shared templates.

Optional frequency mode (binary membership is all-or-nothing — a template at
0.001% in the baseline reads as "seen" even if it's 60% of the target):
`--novel-vs-ratio <N>` flags templates whose target frequency is ≥ N× their
baseline frequency, catching rate-shift anomalies that pure set-membership
misses. Defer unless usage shows membership alone is too coarse.

## Integration points

- `src/drain.rs`: needs a real extension, not a one-liner. The baseline and
  target **must share one tree** (independent runs give 100% false positives).
  Add: (a) a way to record the set of template IDs seen during a "baseline
  phase", and (b) a *frozen / read-only match* mode so target lines are
  classified against the baseline clusters without mutating them (no new
  clusters, no further generalization that would drift a cluster's ID). Today's
  `drain_template()` always mutates; the read-only path is the core new work and
  may need an upstream `drain-rs` capability. `generate_template_id` stays as-is.
- `src/pipeline/stages.rs`: today `DrainStage` (line ~1112) is summary-only and
  passes events through unchanged. Novelty needs a *classifying* stage holding
  the baseline ID set that `Skip`s events whose (read-only-matched) template ID
  is present. Baseline ingestion is a Begin-phase pre-pass into the shared tree,
  then per-event classification streams. Sequential-only, consistent with
  `--drain` (`drain` state is `thread_local`).
- `src/cli.rs`: add the flags near the existing `--drain` block (~line 818),
  with the same "data-only mode" semantics (hush hints, still surface warnings).

## Limitations / forcing functions (be honest in docs)

- **Requires a shared, frozen tree — not a portable ID file.** The biggest
  constraint (tested): baseline and target must be clustered together. Re-
  ingesting the baseline each run is the practical answer; a portable model
  needs `DrainTree` serialization (unverified).
- **Binary membership misses rate shifts.** A template at 0.001% in baseline but
  60% in target reads as "seen." Membership is all-or-nothing where logjuicer's
  cosine is continuous. Optional ratio mode is the mitigation.
- **Novelty quality = masking coverage.** Unmasked variable tokens (bare-word
  IDs, usernames) stay literal, so each value is its own template — inflating
  the baseline set and causing false positives. Let users extend filters / add
  ignore patterns; don't trust the default masking blindly.
- **Single field only.** Novelty in structured fields (a new `error_code`)
  isn't caught unless folded into the drained text. Same scope as `--drain`.
- **Incomplete baseline → false positives** (rare-but-normal lines). Mitigated,
  not solved, by `--novel-extra-baseline` and `--novel-ignore`.
- **No semantic understanding** — same ceiling logjuicer hits. Statistical
  novelty, not root-cause reasoning.
- **Template strings shift if grok filters / drain config change.** Any
  persisted model must record them and refuse mismatched loads.

## Alternatives considered

- *Diff two independent `--drain=id` outputs* (saved-ID-set model). **Rejected —
  tested, 203/203 false positives.** Independent corpora generalize the same
  lines into different templates, so their ID sets don't overlap. The appeal of
  a portable, dependency-free model file is real, but it doesn't work.
- *Shared-tree, two-phase, baseline frozen + read-only target match.*
  **Selected — tested, surfaced exactly the anomalies with zero false
  positives.** Costs a baseline re-ingest per run and some real work in
  `drain.rs`, but it's the design that actually works.
- *Port logjuicer's cosine-similarity engine.* Rejected for now — different
  philosophy from Kelora's deterministic streaming model, adds vector-math
  dependency and a "training" concept that sits awkwardly in a Unix-pipe tool.
  Template membership gets ~80% of the value, deterministic and explainable.
- *Pure Rhai-only solution* (no flag). Awkward as the primary path — sharing one
  frozen tree across a baseline pre-pass and per-event classification is hard to
  express in script. Keep the flag primary; a Rhai predicate can come later.

## Phasing

The signal quality is already validated by the simulation above (shared-tree:
exactly the 3 anomalies, no false positives). Remaining work is the
implementation, not proving the concept.

1. **Build the shared-tree core:** extend `src/drain.rs` with a baseline-phase
   ID set + a frozen, read-only match mode (the real work; may need an upstream
   `drain-rs` capability — spike that first). Wire a classifying stage and the
   `--novel-vs` flag.
2. **Productionize:** `--novel-ignore`, `--novel-extra-baseline`, sensible
   defaults for which field is drained, docs, tests (clustering has good
   coverage to model after).
3. **Maybe:** portable `--novel-model <file>` *iff* `DrainTree` serde works
   (else stop at re-ingestion); `--novel-vs-ratio` frequency mode if membership
   proves too coarse; a Rhai predicate for annotate-don't-drop workflows.

## Should we? — yes, and it's usable

Validated end-to-end on a realistic before/after pair: the shared-tree design
surfaced exactly the injected anomalies with zero false positives, which is the
UX that makes this worth shipping. The naive saved-ID-set shortcut is **not**
usable (100% false positives) and must be avoided. Build the focused,
Kelora-idiomatic subset (shared-tree template-novelty on the existing grok+Drain
pipeline, output-as-events); hold the line against the cosine-similarity ML
engine and the HTML report. Kelora should borrow logjuicer's *question*, not
become logjuicer.
