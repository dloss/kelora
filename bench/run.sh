#!/usr/bin/env bash
# Benchmark harness for the level pre-filter.
#
# Benchmarks a baseline binary and a candidate binary side by side in a single
# hyperfine invocation per scenario, so the comparison is direct. Corpora live
# on local disk; hyperfine's --warmup handles page-cache warming. Each command
# redirects stdout to /dev/null so the measurement includes formatting and write
# cost without flooding the terminal.
#
# Usage:
#   BASELINE=bench/bin/kelora-baseline CANDIDATE=target/release/kelora \
#       bench/run.sh [OUTDIR]
#
# Results (JSON + markdown) are written under OUTDIR (default: bench/results/raw).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CORPUS="$HERE/corpus"
BASELINE="${BASELINE:-$HERE/bin/kelora-baseline}"
CANDIDATE="${CANDIDATE:-$HERE/../target/release/kelora}"
OUTDIR="${1:-$HERE/results/raw}"
mkdir -p "$OUTDIR"

WARMUP="${WARMUP:-3}"
MINRUNS="${MINRUNS:-10}"

LF1="$CORPUS/logfmt_1pct.log";     J1="$CORPUS/json_1pct.log"
LF20="$CORPUS/logfmt_20pct.log";   J20="$CORPUS/json_20pct.log"
LF100="$CORPUS/logfmt_100pct.log"; J100="$CORPUS/json_100pct.log"

# bench <id> <shared kelora args (both binaries)>
# stdout is discarded inside the command string so it is part of what hyperfine
# times.
bench() {
  local id="$1"; local args="$2"
  echo "=== scenario $id ==="
  hyperfine \
    --warmup "$WARMUP" --min-runs "$MINRUNS" \
    --command-name "baseline: $id"  "$BASELINE $args >/dev/null" \
    --command-name "candidate: $id" "$CANDIDATE $args >/dev/null" \
    --export-json "$OUTDIR/$id.json" \
    --export-markdown "$OUTDIR/$id.md"
}

# Scenario A: --levels error on 1%-error corpus, -F json output (target win).
bench "A-logfmt" "-f logfmt --levels error -F json $LF1"
bench "A-json"   "-f json --levels error -F json $J1"

# Scenario B: moderate selectivity (20% error).
bench "B-logfmt" "-f logfmt --levels error $LF20"
bench "B-json"   "-f json --levels error $J20"

# Scenario C: 100% error — pre-filter pure overhead (everything is a "maybe").
bench "C-logfmt" "-f logfmt --levels error $LF100"
bench "C-json"   "-f json --levels error $J100"

# Scenario D: no level filter, pass-through (gate off; must be unchanged).
bench "D-logfmt" "-f logfmt $LF20"
bench "D-json"   "-f json $J20"

# Scenario E: --levels error + -C 2 (gate off via context; must be unchanged).
bench "E-logfmt" "-f logfmt --levels error -C 2 $LF20"
bench "E-json"   "-f json --levels error -C 2 $J20"

# Scenario F: --levels error --parallel on 1%-error corpus (parallel path win).
bench "F-logfmt" "-f logfmt --levels error --parallel $LF1"
bench "F-json"   "-f json --levels error --parallel $J1"

echo
echo "Raw results in $OUTDIR"
