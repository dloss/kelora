# Level pre-filter benchmark harness

Measures the raw-line level pre-filter optimization (see the spec) with
before/after A/B benchmarking and a byte-identical differential correctness gate.

## Files

- `gen_corpus.py` — deterministic (seeded) corpus generator. Produces matched
  logfmt/JSON corpora (~15 fields/record) at 1%, 20%, and 100% error-level
  density. Re-runs are byte-for-byte reproducible.
- `run.sh` — runs the scenario suite with `hyperfine`, comparing a baseline and a
  candidate binary in one invocation per scenario. Exports JSON + Markdown.
- `diff_check.sh` — runs both binaries across a flag matrix and asserts stdout,
  stderr, and exit code are byte-identical. Exits non-zero on any difference.
- `results/` — committed result write-ups (raw hyperfine exports go under
  `results/raw/`, which is gitignored).

## Usage

```bash
# 1. Generate the 1M-line corpora (~1.2 GB, gitignored).
python3 bench/gen_corpus.py --lines 1000000 --outdir bench/corpus

# 2. Build both binaries (baseline = pre-change commit, candidate = this tree).
cargo build --release            # candidate -> target/release/kelora
#   ...check out the baseline commit and build it, or reuse bench/bin/kelora-baseline

# 3. Correctness gate (fast, small corpora built internally).
bench/diff_check.sh bench/bin/kelora-baseline target/release/kelora

# 4. Performance A/B.
bench/run.sh bench/bin/kelora-baseline target/release/kelora bench/results/raw
```

## Scenarios

| ID | Shape | Measures |
|----|-------|----------|
| A | `--levels error` on 1% corpus, `-F json` | target win |
| B | `--levels error` on 20% corpus | moderate selectivity |
| C | `--levels error` on 100% corpus | pre-filter pure overhead |
| D | no level filter, pass-through | unchanged (gate off) |
| E | `--levels error -C 2` | unchanged (context gate off) |
| F | `--levels error --parallel` on 1% corpus | parallel path win |
