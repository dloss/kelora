#!/usr/bin/env bash
# Differential correctness check for the timestamp fast-path pre-filter.
#
# Runs the BASELINE and CANDIDATE binaries on identical input across a matrix of
# flag combinations and asserts that stdout, stderr, and exit code are all
# byte-identical. Exits non-zero if ANY case differs.
#
# Usage:
#   bench/diff_ts_check.sh <baseline_bin> <candidate_bin>
set -uo pipefail

BASELINE="${1:?usage: diff_ts_check.sh <baseline_bin> <candidate_bin>}"
CANDIDATE="${2:?usage: diff_ts_check.sh <baseline_bin> <candidate_bin>}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FAILURES=0
CASES=0

SINCE90="2026-01-01T21:36:00Z"   # ~10% kept
SINCE50="2026-01-01T12:00:00Z"   # ~50% kept
UNTIL50="2026-01-01T12:00:00Z"
SINCE0="2026-01-01T00:00:00Z"    # all kept

# --- build the small differential corpora -----------------------------------
python3 "$HERE/gen_ts_corpus.py" --lines 5000 --outdir "$WORK/corpus" >/dev/null
LF="$WORK/corpus/ts_logfmt_5000.log"
J="$WORK/corpus/ts_json_5000.log"

# Cascade / no-timestamp / malformed / mixed-shape corpus. Deliberately mixes:
#  - flat objects with a string ts (fast-path eligible),
#  - objects with NO ts field (resilient skip under --since),
#  - objects with a NESTED ts only (top-level absent -> no ts),
#  - objects with a NUMERIC ts (fast path bails, full parse handles),
#  - objects where a later key repeats ts,
#  - logfmt-shaped lines (for the json,logfmt cascade).
MIXED="$WORK/mixed.log"
python3 - "$MIXED" <<'PY'
import json, sys, random
rng = random.Random(99)
def iso(s):
    hh, rem = divmod(s, 3600); mm, ss = divmod(rem, 60)
    return f"2026-01-01T{hh:02d}:{mm:02d}:{ss:02d}Z"
with open(sys.argv[1], "w") as f:
    for i in range(4000):
        s = rng.randrange(86400)
        k = i % 8
        if k == 0:
            f.write(json.dumps({"ts": iso(s), "level": "info", "seq": i}) + "\n")
        elif k == 1:
            f.write(json.dumps({"level": "warn", "seq": i, "msg": "no ts here"}) + "\n")
        elif k == 2:
            f.write(json.dumps({"meta": {"ts": iso(s)}, "level": "info", "seq": i}) + "\n")
        elif k == 3:
            f.write(json.dumps({"ts": s + 1735689600, "level": "info", "seq": i}) + "\n")
        elif k == 4:
            f.write(json.dumps({"time": iso(s), "level": "debug", "seq": i}) + "\n")
        elif k == 5:
            f.write(f"ts={iso(s)} level=info seq={i} msg=logfmt-line\n")
        elif k == 6:
            f.write(json.dumps({"ts": iso(s), "ts_dup_note": "x", "level": "info", "seq": i}) + "\n")
        else:
            f.write(json.dumps({"ts": iso(s), "level": "error", "nested": {"a": [1, 2, 3]}, "seq": i}) + "\n")
PY

# Malformed timestamps + broken lines. Broken JSON lines are placed so both
# binaries reject them identically; where a line is well-formed but its ts is
# unparseable, both yield no ts and behave identically.
MALFORMED="$WORK/malformed.log"
python3 - "$MALFORMED" <<'PY'
import json, sys, random
rng = random.Random(7)
def iso(s):
    hh, rem = divmod(s, 3600); mm, ss = divmod(rem, 60)
    return f"2026-01-01T{hh:02d}:{mm:02d}:{ss:02d}Z"
with open(sys.argv[1], "w") as f:
    for i in range(3000):
        s = rng.randrange(86400)
        k = i % 5
        if k == 0:
            f.write(json.dumps({"ts": "not-a-timestamp", "level": "info", "seq": i}) + "\n")
        elif k == 1:
            f.write("{ broken json not closed \n")
        elif k == 2:
            f.write(json.dumps({"ts": "", "level": "info", "seq": i}) + "\n")
        else:
            f.write(json.dumps({"ts": iso(s), "level": "info", "seq": i}) + "\n")
PY

# Timezone-bearing and naive timestamps.
TZMIX="$WORK/tzmix.log"
python3 - "$TZMIX" <<'PY'
import json, sys
rows = [
    {"ts": "2026-01-01T21:00:00+02:00", "seq": 0},   # 19:00Z
    {"ts": "2026-01-01T21:00:00Z", "seq": 1},
    {"ts": "2026-01-01 21:00:00", "seq": 2},         # naive
    {"ts": "2026-01-01T23:59:59-05:00", "seq": 3},   # next-day-ish Z
    {"ts": "2026-01-01T21:36:00Z", "seq": 4},        # exactly the since bound
]
with open(sys.argv[1], "w") as f:
    for r in rows:
        f.write(json.dumps(r) + "\n")
PY

# A pure-JSON mixed corpus (no logfmt-shaped lines) for the --parallel case:
# under --parallel the *sample* of parse errors surfaced on stderr is inherently
# non-deterministic (worker scheduling decides which few surface first — this is
# true of the baseline binary too), so the parallel corpus is kept parse-error
# free to make stderr deterministic while still exercising the fast path over
# no-ts / nested-ts / numeric-ts / dup-ts shapes.
MIXEDJSON="$WORK/mixed_json.log"
python3 - "$MIXEDJSON" <<'PY'
import json, sys, random
rng = random.Random(101)
def iso(s):
    hh, rem = divmod(s, 3600); mm, ss = divmod(rem, 60)
    return f"2026-01-01T{hh:02d}:{mm:02d}:{ss:02d}Z"
with open(sys.argv[1], "w") as f:
    for i in range(4000):
        s = rng.randrange(86400); k = i % 7
        if k == 0:   f.write(json.dumps({"ts": iso(s), "level": "info", "seq": i}) + "\n")
        elif k == 1: f.write(json.dumps({"level": "warn", "seq": i}) + "\n")
        elif k == 2: f.write(json.dumps({"meta": {"ts": iso(s)}, "seq": i}) + "\n")
        elif k == 3: f.write(json.dumps({"ts": s + 1735689600, "seq": i}) + "\n")
        elif k == 4: f.write(json.dumps({"time": iso(s), "seq": i}) + "\n")
        elif k == 5: f.write(json.dumps({"ts": iso(s), "ts_note": "x", "seq": i}) + "\n")
        else:        f.write(json.dumps({"ts": iso(s), "nested": {"a": [1, 2]}, "seq": i}) + "\n")
PY

# --- comparison harness ------------------------------------------------------
# Strip the non-deterministic Throughput line, and rewrite each binary's own
# basename to a placeholder so usage/error strings that embed argv[0] compare
# equal regardless of which binary produced them.
norm() {
  sed -e '/^Throughput: /d' \
      -e "s|$(basename "$BASELINE")|BIN|g" \
      -e "s|$(basename "$CANDIDATE")|BIN|g" "$1"
}

check() {
  local name="$1"; shift
  [[ "$1" == "--" ]] && shift
  CASES=$((CASES + 1))
  local bo="$WORK/b.out" be="$WORK/b.err" co="$WORK/c.out" ce="$WORK/c.err"
  "$BASELINE" "$@" >"$bo.raw" 2>"$be.raw"; local brc=$?
  "$CANDIDATE" "$@" >"$co.raw" 2>"$ce.raw"; local crc=$?
  norm "$bo.raw" >"$bo"; norm "$be.raw" >"$be"
  norm "$co.raw" >"$co"; norm "$ce.raw" >"$ce"
  local ok=1
  if [[ $brc -ne $crc ]]; then ok=0; echo "  [exit] $name: baseline=$brc candidate=$crc"; fi
  if ! cmp -s "$bo" "$co"; then ok=0; echo "  [stdout] $name differs"; diff <(head -5 "$bo") <(head -5 "$co") | head -12; fi
  if ! cmp -s "$be" "$ce"; then ok=0; echo "  [stderr] $name differs"; diff "$be" "$ce" | head -12; fi
  if [[ $ok -eq 1 ]]; then echo "  ok: $name"; else FAILURES=$((FAILURES + 1)); echo "  FAIL: $name"; fi
}

echo "== Pre-filter ACTIVE (json/logfmt, --since/--until, resilient) =="
check "A-logfmt-since90"  -- -f logfmt "$LF" --since "$SINCE90"
check "A-json-since90"    -- -f json   "$J"  --since "$SINCE90"
check "since90-json-out"  -- -f json   "$J"  --since "$SINCE90" -F json
check "since50-logfmt"    -- -f logfmt "$LF" --since "$SINCE50"
check "until50-json"      -- -f json   "$J"  --until "$UNTIL50"
check "since-until-band"  -- -f json   "$J"  --since "$SINCE50" --until "$SINCE90"
check "since0-all-json"   -- -f json   "$J"  --since "$SINCE0"
check "parallel-json"     -- -f json   "$J"  --since "$SINCE90" --parallel
check "parallel-logfmt"   -- -f logfmt "$LF" --since "$SINCE90" --parallel
check "combined-lvl-since" -- -f json  "$J"  --levels error --since "$SINCE90"

echo "== Mixed shapes (no-ts, nested-ts, numeric-ts, dup-ts) =="
check "mixed-since90"     -- -f json "$MIXED" --since "$SINCE90"
check "mixed-since50"     -- -f json "$MIXED" --since "$SINCE50"
check "mixed-until50"     -- -f json "$MIXED" --until "$UNTIL50"
check "mixed-cascade"     -- -f json,logfmt "$MIXED" --since "$SINCE90"
check "mixed-parallel"    -- -f json "$MIXEDJSON" --since "$SINCE90" --parallel
check "mixed-parallel-seq" -- -f json "$MIXEDJSON" --since "$SINCE90"

echo "== Timezone-bearing and naive timestamps =="
check "tz-since90"        -- -f json "$TZMIX" --since "$SINCE90"
check "tz-until"          -- -f json "$TZMIX" --until "$UNTIL50"
check "tz-utc-tzarg"      -- -f json "$TZMIX" --since "$SINCE90" --input-tz UTC

echo "== Gate-off (behavior identical by inertness) =="
check "strict-since90"    -- -f json "$J" --since "$SINCE90" --strict
check "strict-malformed"  -- -f json "$MALFORMED" --since "$SINCE90" --strict
check "ts-field-override" -- -f json "$J" --since "$SINCE90" --ts-field ts
check "context-since"     -- -f json "$J" --since "$SINCE90" -C 2
check "stats-since"       -- -f json "$J" --since "$SINCE90" --stats
check "no-filter"         -- -f json "$J"
check "malformed-active"  -- -f json "$MALFORMED" --since "$SINCE90"
check "malformed-stats"   -- -f json "$MALFORMED" --since "$SINCE90" --stats

echo
echo "Ran $CASES cases; $FAILURES failed."
[[ $FAILURES -eq 0 ]] || exit 1
