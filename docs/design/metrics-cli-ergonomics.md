# Design: zero-script CLI sugar for common metrics

Status: proposal (exploration captured for review)

## Problem

Defining even the most common aggregation in kelora requires writing a Rhai
expression. The metrics flags (`-m`, `--metrics-file`, `--with-metrics`) only
control *output*; the *intent* always lives in a quoted script:

```bash
kelora -j app.jsonl -e 'track_freq("level", e.level)' -m       # count by level
kelora -j app.jsonl -e 'track_stats("rt", e.duration_ms)' -m   # latency stats
```

For the casual "just tally this field / summarize this number" user that is a
real barrier — they reach for `jq | sort | uniq -c` instead. Compare the
`agrind '* | json | count by service'` example already referenced in our
own docs (`docs/how-to/integrate-external-tools.md`).

## Goal

Make the highest-frequency aggregations expressible as plain CLI flags, with
**no change to the Rhai API and no change to the metrics internals**. The flags
are pure front-end sugar that synthesize the equivalent `track_*` call.

## Scope decisions (and what was rejected)

Two alternatives were explored first and dropped:

- **A grouping primitive** (`group_by(key, || { ... })` to give numeric
  aggregates the nested-table treatment `track_freq` enjoys). Rejected:
  conceptual load. It wins only for multi-metric / multi-dimensional grouping,
  is *more* verbose than the existing `"avg_" + e.svc` idiom for the single
  case, and forces a non-trivial formatter refactor (finalization of
  avg/percentile representations would have to become recursive, plus a
  percentile-suffix-vs-group-key naming collision). See "Rejected alternatives".

- **Extending `--discover FIELD`** to show a value distribution. Rejected:
  semantically wrong. `--discover` answers a *structural* question (what fields
  exist, types, cardinality, examples). A frequency table answers an
  *aggregation* question (how one field's values are distributed). Overloading
  one flag with both meanings increases conceptual load. The cardinality/examples
  in `--discover` are shape hints, not a distribution.

### Flags to add

| Flag | Expands to | Operation | Verdict |
|------|------------|-----------|---------|
| `--count FIELD` (repeatable) | `track_freq("FIELD", e.FIELD)` | frequency ("count by") | ship — most common op that benefits from sugar |
| `--describe FIELD` (repeatable) | `track_stats("FIELD", e.FIELD)` | numeric summary (count/min/max/avg/p50/p95/p99) | ship — highest friction×value per flag |
| `--top FIELD[:N]` (repeatable) | `track_top("FIELD", e.FIELD, N)` | top-N most frequent | optional fast-follow |

Prioritisation rationale: sugar value is *frequency × friction*, not raw
frequency. Plain counting ("how many events / how many match") is the most
common *question* but the *worst* sugar candidate — it is already nearly free
(`--filter` + `wc`, or processing stats), so it gets no flag. Frequency tables
are the most common operation that actually benefits (and dominate the docs
corpus). Numeric stats are domain-concentrated but collapse the most
hand-written Rhai, so they earn the second slot.

### Naming

- `--stats` / `-s` is already taken (processing statistics) and its value slot
  is the format enum, so it cannot be reused for field stats.
- `--describe` is free and is the better name regardless: it is the
  pandas `df.describe()` / R `summary()` idiom, maps 1:1 onto `track_stats`
  output, and takes a field by construction so it is unambiguous.
- `--count`, `--top`, `-D` and the other names above are free (verified against
  `src/cli.rs`).

## Semantics

### Pipeline stage — FINAL (decided)

kelora processing is an ordered list of script stages (`ScriptStageType`,
`src/pipeline/builders.rs`) that run in **command-line order** (`-l`,
`--filter`, `-e` interleave by position). The sugar flags are non-positional,
so they have no natural slot. Rule:

> The synthesized tracker runs as the **last per-event stage**, after all
> `--filter`, `-l`, and `-e` stages — compiled to the same thing an `ExecStage`
> running the equivalent `track_*` call would be, appended at the tail.

Consequences:

- Counts/summaries reflect **post-pipeline** events — the same vantage as
  `--discover-final`, deliberately *not* raw input. Sees fields created/renamed
  by an earlier `-e`; sees only events that survived filtering / were not
  dropped; reflects the value after the last `-e` touched it.

  ```bash
  kelora -j app.jsonl --filter 'e.status>=500' --count url
  # tallies url ONLY among surviving 500s — tracker runs after the filter
  ```

- Corner: if a later `-e` rewrites or deletes the field, the sugar sees the
  rewritten value. Same trade `--discover-final` already makes; acceptable.

- Always-last is chosen over CLI-position interleaving: simpler, predictable,
  matches the "result of my pipeline" mental model. Interleaving would
  reintroduce positional subtlety for marginal benefit.

### Output implication

- The flags imply `-m` (full table), which already implies `-q`.
- An explicit `--metrics=short` / `--metrics-file` still overrides the format.
- `--no-metrics` must still win (do not force output on if explicitly disabled).
- Multiple flags accumulate into one pass and one metrics table.

### Field-path mapping

`--count FIELD` → `e.FIELD`. Rule for the value:

- bare identifier (`level`) → `e.level`
- dotted path (`user.id`) → nested access
- non-identifier / special chars (`weird-field`) → bracket index `e["weird-field"]`

Default metric name and output label = the field path string.

### Inherited for free

- **Parallel:** runs per worker, metrics merge at end (`freq`/`sum`/`stats` all
  have merge strategies). No extra logic.
- **`--metrics-file`:** works unchanged.
- **Spans (`--span`):** rolls into `span.metrics` automatically — but with the
  existing limitation: `--count` (additive) is span-clean, while `--describe`'s
  non-additive parts (min/max/percentiles) are dropped per-span. Document this.

### Numeric coercion

`--describe` on a non-numeric/missing field skips via the existing unit-skip
path (counted under `--diagnostics`). No new behavior.

## Implementation sketch

- **CLI:** add flags in `src/cli.rs` under the "Metrics and Stats" help heading;
  repeatable (`Vec<String>`); `--top` parses an optional `:N` suffix.
- **Wiring:** in `src/pipeline/builders.rs`, after assembling the user stage
  list, append one synthesized exec-equivalent stage per sugar flag, in flag
  order. Turn on metrics output unless `--no-metrics` / an explicit format is set.
- **Field path → Rhai accessor:** small helper shared by all three flags.
- **Docs:** `--help` text; a short `docs/` note framing these as "shorthand for
  the common case; drop to `-e`/`track_*` for anything custom". (No new Rhai
  functions, so `docs.rs` is untouched.)
- **Tests:** flag → expected metrics output; composition with `--filter`;
  post-`-e` vantage; repeatable flags; `--no-metrics` override; parallel parity;
  span caveat for `--describe`.

## Tradeoffs

- **Two ways to do it.** These duplicate `track_*`; docs must frame them
  explicitly as sugar for the common case. Well-trodden pattern (shortcut flag +
  full scripting), but a teaching/maintenance cost.
- **Flags are API and conceptual load too** — hence the cap at two (maybe three)
  verbs and the explicit refusal of a `--metric 'count:level'` mini-language,
  which would reintroduce a sublanguage.
- **No grouping / cross-tab.** Single-field by design; "count by A and B" stays
  in Rhai. Keeps the flags honest and simple.

## Rejected alternatives (for the record)

1. Grouping primitive (`group_by`) — conceptual load; payoff conditional;
   formatter refactor + percentile/group naming collision.
2. Positional group argument on numeric trackers — inconsistent (collides with
   the existing optional `[percentiles]` arg on `track_percentiles`/`track_stats`)
   and multiplies the concept across the family.
3. `_if` conditional variants — pure API bloat (+6 names); `if cond { ... }`
   already reads fine.
4. Extending `--discover FIELD` — semantically conflates structure with
   aggregation.
5. Reusing/overloading `--stats` — taken (processing stats), value slot is the
   format enum, and same semantic conflation.

## Open question

Scope: ship `--count` + `--describe` together, or start with one (`--count`,
the safest universal pick) and add the rest once the pattern proves out.
`--top` is an optional fast-follow either way.
