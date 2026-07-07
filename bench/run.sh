#!/usr/bin/env bash
# Benchmark the raw-line level pre-filter with hyperfine.
#
# Runs each scenario as a single hyperfine invocation that compares the BASELINE
# binary against the CANDIDATE binary on identical input, so the two are measured
# back-to-back under the same page-cache/CPU conditions. Exports both JSON and
# Markdown per scenario.
#
# Usage:
#   bench/run.sh <baseline_bin> <candidate_bin> [outdir]
#
# Corpora are expected under bench/corpus/ (generate with gen_corpus.py). If they
# are missing, this script generates the 1M-line set first.
set -euo pipefail

BASELINE="${1:?usage: run.sh <baseline_bin> <candidate_bin> [outdir]}"
CANDIDATE="${2:?usage: run.sh <baseline_bin> <candidate_bin> [outdir]}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTDIR="${3:-$HERE/results/raw}"
CORPUS="$HERE/corpus"
TAG="1m"

mkdir -p "$OUTDIR"

if [[ ! -f "$CORPUS/logfmt_${TAG}_1pct.log" ]]; then
  echo "corpus missing; generating 1M-line set (this takes a minute)…" >&2
  python3 "$HERE/gen_corpus.py" --lines 1000000 --outdir "$CORPUS"
fi

# Warm the page cache for every corpus file once up front; hyperfine --warmup
# also handles this per-command, but this keeps the first scenario honest.
cat "$CORPUS"/*.log > /dev/null 2>&1 || true

WARMUP=3
MINRUNS=10

# run_scenario <id> <description> <arg-string-after-binary>
# The arg string is applied identically to both binaries. {bin} is substituted
# by hyperfine's parameter list.
run_scenario() {
  local id="$1"; shift
  local desc="$1"; shift
  local args="$1"; shift
  echo "== Scenario $id ($desc) ==" >&2
  hyperfine \
    --warmup "$WARMUP" --min-runs "$MINRUNS" \
    --command-name "baseline: $id" "$BASELINE $args" \
    --command-name "candidate: $id" "$CANDIDATE $args" \
    --export-json "$OUTDIR/${id}.json" \
    --export-markdown "$OUTDIR/${id}.md"
}

LF1="$CORPUS/logfmt_${TAG}_1pct.log"
LF20="$CORPUS/logfmt_${TAG}_20pct.log"
LF100="$CORPUS/logfmt_${TAG}_100pct.log"
J1="$CORPUS/json_${TAG}_1pct.log"
J20="$CORPUS/json_${TAG}_20pct.log"
J100="$CORPUS/json_${TAG}_100pct.log"

# Scenario A — target win: sparse matches, JSON output rendering.
run_scenario "A-logfmt" "levels error, 1% corpus, -F json"   "-f logfmt $LF1 --levels error -F json"
run_scenario "A-json"   "levels error, 1% corpus, -F json"   "-f json   $J1  --levels error -F json"
# Scenario B — moderate selectivity.
run_scenario "B-logfmt" "levels error, 20% corpus"           "-f logfmt $LF20 --levels error"
run_scenario "B-json"   "levels error, 20% corpus"           "-f json   $J20  --levels error"
# Scenario C — pre-filter pure overhead (everything matches).
run_scenario "C-logfmt" "levels error, 100% corpus"          "-f logfmt $LF100 --levels error"
run_scenario "C-json"   "levels error, 100% corpus"          "-f json   $J100  --levels error"
# Scenario D — pass-through, no level filter (gate off; must be unchanged).
run_scenario "D-logfmt" "pass-through, no filter"            "-f logfmt $LF1"
run_scenario "D-json"   "pass-through, no filter"            "-f json   $J1"
# Scenario E — context option forces the gate off (must be unchanged).
run_scenario "E-logfmt" "levels error + -C 2 (gate off)"    "-f logfmt $LF1 --levels error -C 2"
run_scenario "E-json"   "levels error + -C 2 (gate off)"    "-f json   $J1  --levels error -C 2"
# Scenario F — parallel path win.
run_scenario "F-logfmt" "levels error, 1% corpus, --parallel" "-f logfmt $LF1 --levels error --parallel"
run_scenario "F-json"   "levels error, 1% corpus, --parallel" "-f json   $J1  --levels error --parallel"

echo "Raw hyperfine exports written to $OUTDIR" >&2
