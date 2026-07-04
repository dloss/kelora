# Raw-line level pre-filter — benchmark results

**Date:** 2026-07-04
**Optimization:** `src/pipeline/prefilter.rs` — cheap case-insensitive substring
scan of the raw (post-multiline-assembly) line for the accepted `--levels`
tokens, dropping non-matching lines before the parse + `FieldMap` allocation.

## Machine

| | |
|---|---|
| CPU | Intel Xeon @ 2.80 GHz, 4 vCPU |
| Memory | 15 GiB |
| Kernel | Linux 6.18.5 (x86_64) |
| Toolchain | rustc 1.86.0, release profile |
| hyperfine | 1.18.0 (`--warmup 3`, `--min-runs 10`; C/D re-run at `--min-runs 20`) |
| Corpora | 1,000,000 lines each, on local disk (page cache warmed by hyperfine) |

## Binaries

| Binary | Commit | Notes |
|--------|--------|-------|
| baseline (`bench/bin/kelora-baseline`) | `b3072cf` (main) | pre-filter absent; frozen before any change |
| candidate (`target/release/kelora`) | branch `claude/raw-line-level-prefilter-mu0qtg` (= `b3072cf` + this change) | pre-filter present |

Both binaries were benchmarked in a single `hyperfine` invocation per scenario
for a direct comparison. Correctness was verified separately: `bench/diff_check.sh`
reports **22/22 cases byte-identical** (stdout, stderr, exit code) across the
scenario + flag matrix, including `--exclude-levels`, `-f syslog`, multiline,
`--stats`, `--drain`, `--parallel`, and a post-level `--filter` stage.

## Scenarios

| ID | Command shape | Corpus | What it measures |
|----|---------------|--------|------------------|
| A | `--levels error -F json` | 1% error, 1M | target win |
| B | `--levels error` | 20% error, 1M | moderate selectivity |
| C | `--levels error` | 100% error, 1M | pre-filter pure overhead |
| D | no level filter (pass-through) | 20% error, 1M | gate off — must be unchanged |
| E | `--levels error -C 2` | 20% error, 1M | gate off (context) — must be unchanged |
| F | `--levels error --parallel` | 1% error, 1M | parallel path win |

## Results (baseline → candidate)

| Scenario | Baseline mean [s] | Candidate mean [s] | Speedup | Δ wall-clock |
|----------|------------------:|-------------------:|--------:|-------------:|
| A · logfmt | 3.894 ± 0.252 | **0.796 ± 0.164** | 4.89× | **−79.6%** |
| A · json   | 3.972 ± 0.254 | **0.766 ± 0.203** | 5.19× | **−80.7%** |
| B · logfmt | 4.637 ± 0.246 | 2.189 ± 0.138 | 2.12× | −52.8% |
| B · json   | 4.576 ± 0.183 | 2.188 ± 0.087 | 2.09× | −52.2% |
| C · logfmt | 8.000 ± 0.209 | 7.961 ± 0.179 | 1.00× | −0.5% |
| C · json   | 7.805 ± 0.088 | 7.865 ± 0.211 | 0.99× | +0.8% |
| D · logfmt | 7.845 ± 0.177 | 7.886 ± 0.138 | 0.99× | +0.5% |
| D · json   | 8.046 ± 0.167 | 8.020 ± 0.209 | 1.00× | −0.3% |
| E · logfmt | 6.412 ± 0.050 | 6.446 ± 0.053 | 0.99× | +0.5% |
| E · json   | 6.410 ± 0.070 | 6.331 ± 0.063 | 1.01× | −1.2% |
| F · logfmt | 1.074 ± 0.018 | **0.440 ± 0.057** | 2.44× | −59.0% |
| F · json   | 1.150 ± 0.027 | **0.523 ± 0.068** | 2.20× | −54.5% |

Raw `hyperfine` JSON/markdown exports live under `bench/results/raw/` (git-ignored).

## Acceptance criteria

1. **Differential tests: zero diffs.** `bench/diff_check.sh` → 22/22 byte-identical. ✅
2. **Scenario A ≥ 25% wall-clock improvement.** Observed **−79.6% (logfmt)** and
   **−80.7% (json)** — ~5× faster, far above the 25% bar. ✅
3. **Scenarios C, D, E regression ≤ 2%.** All within ±1.2%, i.e. inside
   run-to-run noise (the two encodings of each scenario diverge in opposite
   directions). An initial 10-run pass showed D · logfmt at +2.7%; a 20-run
   re-run brought it to +0.5%, confirming the earlier figure was noise. ✅
4. **Results committed** (this file). ✅

## Interpretation

On the target workload — a selective `--levels error` filter over a log where
matching lines are a small fraction of input — the pre-filter is a decisive win:
scenario A runs ~5× faster (−80%) because 99% of lines are dropped by a
zero-allocation substring scan instead of being fully parsed into a `FieldMap`
and then discarded by `LevelFilterStage`. The benefit scales with selectivity
(scenario B, 20% error, is still ~2× faster) and carries over to the parallel
path (scenario F, ~2.2–2.4×), where the scan runs inside each worker. At the
worst case for the technique (scenario C, 100% error — every line is a "maybe"
that still parses) the extra scan is pure overhead yet stays within measurement
noise, because a matching line's level token is found near the line's start and
the scan short-circuits. The gate-off scenarios (D pass-through, E context) are
unchanged, as required: when `--levels` is absent or context is requested the
pre-filter is never constructed, so the per-run cost is a single `Option` check,
not a per-line one. Correctness is byte-identical across the full differential
matrix.
