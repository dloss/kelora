#!/usr/bin/env bash
# Measure the allocation-free grouping change: wall clock (min of N runs with the
# plain release binary) plus allocs/line (single run with the bench-alloc binary).
#
# Usage: measure_grouping.sh <plain_bin> <alloc_bin> <label>
set -euo pipefail

PLAIN="${1:?plain binary}"
ALLOC="${2:?bench-alloc binary}"
LABEL="${3:?label}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORPUS="$HERE/corpus"
RUNS="${RUNS:-5}"

LF="$CORPUS/logfmt_1m_100pct.log"

# Warm page cache.
cat "$LF" > /dev/null 2>&1 || true

# scenario <id> <args...>: time the plain binary RUNS times, keep the min; then
# one alloc run for allocs/line.
scenario() {
  local id="$1"; shift
  local best=""
  for _ in $(seq "$RUNS"); do
    local start end elapsed
    start=$(date +%s.%N)
    "$PLAIN" "$@" > /dev/null 2>&1
    end=$(date +%s.%N)
    elapsed=$(echo "$end - $start" | bc)
    if [[ -z "$best" ]] || (( $(echo "$elapsed < $best" | bc -l) )); then
      best="$elapsed"
    fi
  done
  local allocline
  allocline=$("$ALLOC" "$@" 2>&1 >/dev/null | grep '^bench-alloc:' || true)
  printf '%-10s %-8s  wall_min=%ss  %s\n' "$LABEL" "$id" "$best" "$allocline"
}

echo "== $LABEL (RUNS=$RUNS, corpus=logfmt_1m_100pct) =="
scenario "A-drain"  -f logfmt "$LF" --drain -k msg
scenario "B-stats"  -f logfmt "$LF" --stats
scenario "C-passth" -f logfmt "$LF"
