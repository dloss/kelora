#!/usr/bin/env bash
# Differential correctness gate for the allocation-free grouping change.
# Asserts stdout + stderr + exit code are byte-identical between a baseline and a
# candidate binary across a --stats / --drain matrix on both real and adversarial
# inputs. Exits non-zero on any difference.
#
# Usage: diff_grouping.sh <baseline_bin> <candidate_bin>
set -uo pipefail

BASE="${1:?baseline}"
CAND="${2:?candidate}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORPUS="$HERE/corpus"
ADV="${ADV:-/tmp/adversarial.log}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail=0

# Truncated corpus samples keep the matrix fast while covering real data.
for pct in 1pct 20pct 100pct; do
  head -n 50000 "$CORPUS/logfmt_1m_${pct}.log" > "$TMP/lf_${pct}.log"
  head -n 50000 "$CORPUS/json_1m_${pct}.log"   > "$TMP/js_${pct}.log"
done

# Strip wall-clock-dependent lines (throughput / processing duration) that
# legitimately vary run to run; everything else (counts, orderings, samples,
# data-derived time spans) must match byte-for-byte.
norm() {
  grep -vE '^Throughput:|"duration_ms"|"lines_per_second"' "$1"
}

check() {
  local desc="$1"; shift
  "$BASE" "$@" > "$TMP/b.raw" 2> "$TMP/b.err"; local bc=$?
  "$CAND" "$@" > "$TMP/c.raw" 2> "$TMP/c.err"; local cc=$?
  norm "$TMP/b.raw" > "$TMP/b.out"; norm "$TMP/c.raw" > "$TMP/c.out"
  if ! diff -q "$TMP/b.out" "$TMP/c.out" >/dev/null; then
    echo "DIFF stdout: $desc"; diff "$TMP/b.out" "$TMP/c.out" | head -20; fail=1
  elif ! diff -q "$TMP/b.err" "$TMP/c.err" >/dev/null; then
    echo "DIFF stderr: $desc"; diff "$TMP/b.err" "$TMP/c.err" | head -20; fail=1
  elif [[ "$bc" != "$cc" ]]; then
    echo "DIFF exit ($bc vs $cc): $desc"; fail=1
  else
    echo "ok: $desc"
  fi
}

for pct in 1pct 20pct 100pct; do
  for fmt in table full id json; do
    check "drain lf $pct $fmt"  -f logfmt "$TMP/lf_${pct}.log" --drain="$fmt" -k msg
    check "drain js $pct $fmt"  -f json   "$TMP/js_${pct}.log" --drain="$fmt" -k msg
  done
  check "stats lf $pct"      -f logfmt "$TMP/lf_${pct}.log" --stats
  check "stats js $pct"      -f json   "$TMP/js_${pct}.log" --stats
  check "stats-json lf $pct" -f logfmt "$TMP/lf_${pct}.log" --stats=json
  check "passthru lf $pct"   -f logfmt "$TMP/lf_${pct}.log"
done

# Adversarial drain messages: numbers-only, empty, whitespace variants, unicode,
# very long tokens, tabs.
for fmt in table full id json; do
  check "drain adversarial $fmt" -f logfmt "$ADV" --drain="$fmt" -k msg
done
check "stats adversarial" -f logfmt "$ADV" --stats

if [[ "$fail" == 0 ]]; then echo "ALL IDENTICAL"; else echo "DIFFERENCES FOUND"; fi
exit "$fail"
