# Level pre-filter benchmark & differential harness

Measurement and correctness tooling for the raw-line level pre-filter (see
`src/pipeline/prefilter.rs`). Everything here is run ad-hoc during development;
none of it runs in CI.

## Layout

| Path | Purpose |
|------|---------|
| `gen_corpus.py` | Deterministic (seeded) corpus generator. |
| `run.sh` | Benchmark harness — baseline vs candidate, one `hyperfine` call per scenario. |
| `diff_check.sh` | Differential correctness harness — asserts byte-identical stdout/stderr/exit across a flag matrix. |
| `corpus/` | Generated corpora (git-ignored; regenerate with `gen_corpus.py`). |
| `bin/` | Compiled binaries: `kelora-baseline` (frozen pre-change build) + candidate copies (git-ignored). |
| `results/` | Curated result write-ups (`*.md`). Raw `hyperfine` exports land in `results/raw*/` (git-ignored). |

## Prerequisites

- `python3` (corpus generator, stdlib only)
- [`hyperfine`](https://github.com/sharkdp/hyperfine) (`apt install hyperfine`)
- Two `kelora` release binaries to compare.

## Workflow

```bash
# 1. Generate corpora (≈1 min; writes ~2.5 GB into bench/corpus/).
python3 bench/gen_corpus.py --lines 1000000

# 2. Build & snapshot the baseline (pre-change) binary.
git stash            # or check out the base commit
cargo build --release
cp target/release/kelora bench/bin/kelora-baseline

# 3. Build the candidate (with the pre-filter).
git stash pop
cargo build --release

# 4. Correctness: must exit 0 (byte-identical across the matrix).
BASELINE=bench/bin/kelora-baseline CANDIDATE=target/release/kelora bench/diff_check.sh

# 5. Performance: baseline vs candidate side by side.
BASELINE=bench/bin/kelora-baseline CANDIDATE=target/release/kelora bench/run.sh
```

## Corpora

`gen_corpus.py` writes matched logfmt and JSON-lines files whose content is
identical line-for-line (same records, different encoding), plus a small syslog
corpus whose level lives only in the `<priority>` number:

- `logfmt_{1pct,20pct,100pct}.log`, `json_{1pct,20pct,100pct}.log`
- `logfmt_1m.log` / `json_1m.log` — aliases of the 1% variant (scenario A target)
- `syslog_20pct.log` — priority-encoded levels (exercises the parser gate)

The error-level fraction (1% / 20% / 100%) controls pre-filter selectivity. The
100% variant is the pre-filter's worst case: every line is a "maybe", so the
scan is pure overhead.

## Scenarios (`run.sh`)

| ID | Command shape | Measures |
|----|---------------|----------|
| A | `--levels error` on 1%-error corpus, `-F json` | target win |
| B | `--levels error` on 20%-error corpus | moderate selectivity |
| C | `--levels error` on 100%-error corpus | pre-filter pure overhead |
| D | no level filter, pass-through | gate off — must be unchanged |
| E | `--levels error -C 2` | gate off (context) — must be unchanged |
| F | `--levels error --parallel` on 1%-error corpus | parallel path win |
