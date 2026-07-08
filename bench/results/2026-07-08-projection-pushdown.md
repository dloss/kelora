# Projection pushdown into parsers — 2026-07-08

Implements spec "Projection pushdown into parsers". A `-k time,level,msg` query
on a wide record used to parse every field into a `Dynamic` and then let
`KeyFilterStage` throw most of them away. Projection pushdown computes, at
pipeline construction, the set of top-level fields anything downstream can
observe and hands it to the parser, which skips building `Dynamic` values for
the rest. See `src/projection.rs` and `PipelineBuilder::compute_projection`.

## Method

A/B on the **same** release binary: pushdown ON vs OFF, where OFF is the
internal escape hatch `KELORA_NO_PROJECTION=1` (forces `Projection::All`,
reproducing pre-projection behavior exactly). This isolates the pushdown from
any unrelated drift — no separate baseline build.

The numbers below are the **interleaved** harness: for each scenario, ON and
OFF runs alternate (11 each, after 2 warmups per side), medians reported. On
this shared 4-core box (Linux 6.18) the naive all-ON-then-all-OFF ordering
showed a ~±8% noise floor (a gate-off control that must be 0% measured +8%);
interleaving cut that to ~±1-2%, confirmed by the D/E gate-off controls landing
at −1.0% / −0.1%.

Corpora (`bench/gen_corpus.py --profile both --lines 1000000 --fracs 0.20`,
then measured on 300k-line head subsets — the win percentage is size-stable and
the subsets keep the box responsive):
- **wide** — ~40 fields, Kubernetes-shaped, two nested objects
  (`kubernetes`, `resource`).
- **narrow** — the ~15-field level-prefilter corpus.

Harness for the committed 1M A/B is `bench/run_projection.py`.

## Results (300k lines, off = `KELORA_NO_PROJECTION=1`)

| ID | Command | off (All) | on (proj) | win |
|----|---------|-----------|-----------|-----|
| A  | wide json `-k _ts,level,msg`                | 3911.2 ms | 2715.1 ms | **+30.6%** |
| A' | wide json `--levels error -k _ts,level,msg` | 1151.8 ms |  940.6 ms | +18.3% |
| B  | wide logfmt `-k _ts,level,msg`              | 4214.5 ms | 3306.3 ms | +21.5% |
| B' | wide logfmt `--levels error -k _ts,level,msg`| 1227.2 ms | 1078.3 ms | +12.1% |
| C  | narrow json `-k ts,level,msg`               | 1732.6 ms | 1688.9 ms | +2.5% |
| C' | narrow logfmt `-k ts,level,msg`             | 1880.6 ms | 1751.6 ms | +6.9% |
| D  | narrow json (no `-k`, gate off) — control   | 2443.2 ms | 2468.0 ms | −1.0% |
| E  | wide json `-k msg --exec` (gate off) — control | 15023 ms | 15031 ms | −0.1% |

## Reading the numbers

- **A: +30.6% — clears the ≥20% kill criterion.** Skipping `Dynamic`
  construction for ~37 of 40 fields — including the two nested maps, whose
  recursive `rhai::Map`/`Array` allocation dominates the per-field cost — is the
  win. (An earlier all-ON-then-all-OFF pass measured A at +22.5%; interleaving
  removes the drift that was masking the true figure.)
- **A'/B' (combined with `--levels error`).** With the raw-line level pre-filter
  (prior spec) dropping ~80% of the 20%-error corpus *before* the parser runs,
  fewer lines reach the projected parse path, so the two optimizations stack but
  the level pre-filter has already removed most of the parse work. Still solidly
  positive (+18.3% / +12.1%).
- **B: +21.5%.** logfmt skips value-string building + numeric coercion +
  `Dynamic` for unwanted keys. It still scans every value character (v1; the
  tokenizer-level skip is a non-goal), so it lands just under the spec's ≥25%
  aspiration — that figure assumed the key-interning span parser (an explicit
  upstream dependency of this spec) which is **not yet landed**. When it is, the
  span parser removes the char-scan and B should reach/exceed 25%; re-baseline
  then.
- **C/C': +2.5% / +6.9%.** Narrow records have far fewer unneeded fields, so a
  smaller — but real and honest — win. Note C' was a **−10.3% regression** in
  the first cut: a `HashSet<String>` with the std SipHash hasher made the
  per-field `wants()` lookup cost more than logfmt saved on short values.
  Switching the projection set to `ahash` (as `FieldMap` already uses) flipped
  it to +6.9%.
- **D/E: gate off, unchanged (−1.0% / −0.1%, i.e. 0 within noise).** D has no
  `-k` (default output prints all fields); E has a Rhai `--exec` (can read
  arbitrary fields). Both resolve to `Projection::All`, byte-identical to
  baseline — pinned by the differential tests.

## Correctness

`KELORA_NO_PROJECTION=1` vs default output is **byte-identical** across the
differential matrix (sequential and parallel) on the real corpus: `-k` order,
`-k` naming an absent field (incl. the present-fields typo hint),
`--exclude-keys` alone, `--levels`+`-k`, `--since`, nested top-level `-k`, every
output format, `--stats`/`--discover` (forced `All`), cascades, and Rhai stages
(gate off). Unit coverage: `projection_tests` in `src/pipeline/builders.rs`
(including the tripwire: a mock stage without `field_demands` resolves to
`All`) and `projected_*` tests in `src/parsers/{json,logfmt}.rs`.

A subtlety worth recording: the field-name discovery that powers the `-k` typo
hint is collected whenever diagnostics are not suppressed — **not only under
`--stats`** — and it reads the *set of field names*. So the parsers keep every
field's **name**, materializing a cheap `Dynamic::UNIT` placeholder for unwanted
values rather than dropping the entry; `KeyFilterStage` removes the placeholders
before output. Level-field *values* are likewise always kept (for the
discovered-levels set). Both are why the differential is clean rather than
merely close.

## JSON tokenizer-level skip — worth a follow-up?

The v1 JSON path still fully tokenizes skipped values (serde `IgnoredAny`); it
only avoids the `Dynamic` allocation and the deep `Map`/`Array` recursion.
Scenario A shows that avoiding the allocation alone clears the bar by a wide
margin (+30.6%). A tokenizer-level skip (`RawValue` or a hand cursor over the
`{...}`) would additionally save the UTF-8/number scanning of skipped values —
most visible on records dominated by large skipped string/number values. Given
A already passes comfortably, the tokenizer skip is a reasonable **follow-up,
not a blocker** (and remains an explicit non-goal of this spec).

## Raw output

```
== batch 1 (interleaved, 11 runs/side) ==
A  wide json -k                      off= 3911.2ms on= 2715.1ms win=+30.6%
C  narrow json -k                    off= 1732.6ms on= 1688.9ms win= +2.5%
C' narrow logfmt -k                  off= 1880.6ms on= 1751.6ms win= +6.9%
D  control (no -k, must ~0)          off= 2443.2ms on= 2468.0ms win= -1.0%
== batch 2 (interleaved, 11 runs/side) ==
A' wide json --levels error -k       off= 1151.8ms on=  940.6ms win=+18.3%
B  wide logfmt -k                    off= 4214.5ms on= 3306.3ms win=+21.5%
B' wide logfmt --levels error -k     off= 1227.2ms on= 1078.3ms win=+12.1%
== E control (gate off, --exec) ==
E  wide json -k msg --exec           off=15023.0ms on=15030.7ms win= -0.1%
```
