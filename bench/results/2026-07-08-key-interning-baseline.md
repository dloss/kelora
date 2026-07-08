# Field-name small-strings — baseline (pre-change)

Baseline for the field-name small-strings / parser-allocation-hygiene spec.
Measured on `claude/field-name-small-strings-o5vd0j` at the commit *before*
Phase 1. Corpus: `bench/corpus/{logfmt,json}_1m_1pct.log`, 1,000,000 lines each
(generated with `bench/gen_corpus.py --lines 1000000`).

- Wall clock: best-of-N of the release binary (`cargo build --release`), warm
  page cache. No `hyperfine` in this environment; timings via a best-of-N
  `date +%s.%N` wrapper. Phase deltas are re-measured baseline-vs-candidate
  back-to-back, so these absolute numbers are orientation only.
- Allocs: `--features bench-alloc` release binary. The counting allocator tallies
  `alloc`+`realloc`+`alloc_zeroed`. `lines_read` is only populated under `-s`, so
  the harness prints `lines=0` in pass-through mode; allocs/line below is
  `total_allocs / 1_000_000` (corpus is exactly 1M lines).

## Pass-through parse + format, 1M lines, sequential

| Scenario | Command | Wall (s) | Total allocs | allocs/line |
|----------|---------|---------:|-------------:|------------:|
| A  logfmt → logfmt | `-f logfmt LF` | 7.336 | 158,233,054 | 158.23 |
| A' logfmt → json   | `-f logfmt LF -F json` | 8.050 | 160,225,819 | 160.23 |
| B  json → json     | `-f json J -F json` | 8.389 | 140,006,816 | 140.01 |
| B' json → logfmt   | `-f json J -F logfmt` | 7.157 | 138,006,805 | 138.01 |

`LF = bench/corpus/logfmt_1m_1pct.log`, `J = bench/corpus/json_1m_1pct.log`.

## Targets (from spec §6)

- Scenario A: phase 1 ≥8% wall; phases 1+2 ≥20% wall.
- Scenario B: ≥8% combined wall.
- allocs/line: phase 1 ≥30% fewer; combined ≥60% fewer (scenarios A+B).
