#!/usr/bin/env bash
# Differential correctness check for the level pre-filter.
#
# Runs the BASELINE and CANDIDATE binaries on identical input across a matrix of
# flag combinations and asserts that stdout, stderr, and exit code are all
# byte-identical. Exits non-zero if ANY case differs.
#
# Usage:
#   bench/diff_check.sh <baseline_bin> <candidate_bin>
set -uo pipefail

BASELINE="${1:?usage: diff_check.sh <baseline_bin> <candidate_bin>}"
CANDIDATE="${2:?usage: diff_check.sh <baseline_bin> <candidate_bin>}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FAILURES=0
CASES=0

# --- build the small differential corpora -----------------------------------
python3 "$HERE/gen_corpus.py" --lines 5000 --outdir "$WORK/corpus" >/dev/null
LF1="$WORK/corpus/logfmt_5000_1pct.log"
LF20="$WORK/corpus/logfmt_5000_20pct.log"
J1="$WORK/corpus/json_5000_1pct.log"
J20="$WORK/corpus/json_5000_20pct.log"

# Syslog corpus: level is derived from the numeric priority, NOT verbatim text,
# so the pre-filter must stay disabled here (parser gate). Some messages contain
# the word "error" to make a wrongful pre-filter observably drop records.
SYSLOG="$WORK/syslog.log"
python3 - "$SYSLOG" <<'PY'
import sys
prios = [11, 13, 14, 11, 30, 46, 85, 11]  # mix of severities incl. err(3)
msgs = ["disk error detected", "connection ok", "user login", "cache miss",
        "request served", "timeout waiting", "shard rebalanced", "fatal error"]
with open(sys.argv[1], "w") as f:
    for i in range(2000):
        p = prios[i % len(prios)]
        m = msgs[i % len(msgs)]
        f.write(f"<{p}>Jan  1 00:{(i//60)%60:02d}:{i%60:02d} host app[{100+i%50}]: {m}\n")
PY

# Multiline JSON: each record is a '{' header line, a fields line, and a '}'
# line. Reassembly must happen before the pre-filter scans, and the "error"
# token lives on a CONTINUATION line — proof the scan sees the assembled record,
# not physical lines.
MJSON="$WORK/multiline.json"
python3 - "$MJSON" <<'PY'
import json, sys
with open(sys.argv[1], "w") as f:
    for i in range(2000):
        lvl = "error" if i % 10 == 0 else "info"
        f.write("{\n")
        f.write("  " + json.dumps({"ts": f"2026-01-01T00:00:{i%60:02d}Z",
                                   "level": lvl, "seq": i,
                                   "msg": "line-continuation record"})[1:-1] + "\n")
        f.write("}\n")
PY

# Malformed input on the pre-filter's KEEP path: every line contains the token
# "error", so the pre-filter keeps all of them and parse errors on the broken
# lines are reported byte-identically. (Dropping a would-be parse-error line is a
# documented, --keep-lines-consistent behavior and is deliberately NOT asserted
# byte-identical here; the --stats gate-off case below covers malformed input
# with the pre-filter inert.)
MALFORMED="$WORK/malformed.json"
python3 - "$MALFORMED" <<'PY'
import json, sys
with open(sys.argv[1], "w") as f:
    for i in range(3000):
        if i % 7 == 0:
            f.write("this is a broken error line, not json\n")  # contains "error"
        else:
            f.write(json.dumps({"level": "error", "seq": i, "msg": "ok"}) + "\n")
PY

# --- comparison harness ------------------------------------------------------
# check <name> -- <shared args...>
# Strip the one inherently non-deterministic line from --stats output: the
# "Throughput: N lines/s in Mms" wall-clock report. Everything else in --stats
# (counts, levels, keys, time span) is deterministic and IS compared.
norm() { sed '/^Throughput: /d' "$1"; }

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
  if [[ $ok -eq 1 ]]; then
    echo "  ok: $name"
  else
    FAILURES=$((FAILURES + 1))
    echo "  FAIL: $name"
  fi
}

echo "== Scenario matrix (pre-filter ACTIVE cases: json/logfmt --levels, no context/stats) =="
# Scenarios A-F (small corpora)
check "A-logfmt" -- -f logfmt "$LF1" --levels error -F json
check "A-json"   -- -f json   "$J1"  --levels error -F json
check "B-logfmt" -- -f logfmt "$LF20" --levels error
check "B-json"   -- -f json   "$J20"  --levels error
check "C-logfmt-multi" -- -f logfmt "$LF20" --levels error,warn
check "C-json-multi"   -- -f json   "$J20"  --levels error,warn
check "D-logfmt-passthrough" -- -f logfmt "$LF1"
check "D-json-passthrough"   -- -f json   "$J1"
check "E-logfmt-context" -- -f logfmt "$LF1" --levels error -C 2
check "E-json-context"   -- -f json   "$J1"  --levels error -C 2
check "F-logfmt-parallel" -- -f logfmt "$LF1" --levels error --parallel
check "F-json-parallel"   -- -f json   "$J1"  --levels error --parallel

echo "== Gate-off matrix (behavior must be identical by inertness) =="
check "exclude-levels" -- -f json "$J20" --exclude-levels error
check "levels+exclude" -- -f json "$J20" --levels info --exclude-levels debug
check "filter-before-level" -- -f json "$J20" --filter 'e.status != 999' --levels error
check "syslog-levels" -- -f syslog "$SYSLOG" --levels error
check "syslog-levels-notice" -- -f syslog "$SYSLOG" --levels notice
check "stats" -- -f json "$J20" --levels error --stats
check "stats-passthrough" -- -f json "$J20" --stats

echo "== Interaction matrix (pre-filter active alongside other features) =="
check "filter-after-level" -- -f json "$J20" --levels error --filter 'e.seq >= 0'
check "exec-after-level" -- -f json "$J20" --levels error --exec 'e.tag = "x"'
check "keys-after-level" -- -f json "$J1" --levels error --keys level,msg
check "drain-after-level" -- -f json "$J20" --levels error --keys msg --drain
check "multiline-continuation" -- -f json "$MJSON" --multiline 'regex:match=^\{' --levels error
check "malformed-token-present" -- -f json "$MALFORMED" --levels error
check "malformed-passthrough" -- -f json "$MALFORMED" --levels error --stats

echo
echo "Ran $CASES cases; $FAILURES failed."
[[ $FAILURES -eq 0 ]] || exit 1
