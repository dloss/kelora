#!/usr/bin/env bash
# Differential correctness harness for the level pre-filter.
#
# Runs a baseline binary (pre-filter absent) and a candidate binary (pre-filter
# present) over an identical matrix of commands and asserts that stdout, stderr
# and exit code are byte-identical for every case. Exits non-zero on the first
# divergence.
#
# Usage:
#   BASELINE=bench/bin/kelora-baseline CANDIDATE=target/release/kelora \
#       bench/diff_check.sh
#
# Defaults assume those two paths.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
CORPUS="$HERE/corpus"
BASELINE="${BASELINE:-$HERE/bin/kelora-baseline}"
CANDIDATE="${CANDIDATE:-$HERE/../target/release/kelora}"

if [[ ! -x "$BASELINE" ]]; then echo "missing baseline binary: $BASELINE" >&2; exit 3; fi
if [[ ! -x "$CANDIDATE" ]]; then echo "missing candidate binary: $CANDIDATE" >&2; exit 3; fi

LOGFMT="$CORPUS/logfmt_20pct.log"
JSON="$CORPUS/json_20pct.log"
LOGFMT1="$CORPUS/logfmt_1pct.log"
JSON1="$CORPUS/json_1pct.log"
SYSLOG="$CORPUS/syslog_20pct.log"
for f in "$LOGFMT" "$JSON" "$LOGFMT1" "$JSON1" "$SYSLOG"; do
  if [[ ! -f "$f" ]]; then echo "missing corpus: $f (run gen_corpus.py)" >&2; exit 3; fi
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Pretty-printed JSON records spanning multiple physical lines. With
# `-M regex:match=^{` each record is reassembled before parsing, so the
# pre-filter (which runs post-assembly) must scan the whole record — a level
# token on a continuation line must not be lost. Two error records here have
# their level on a continuation line; two non-error records must be dropped.
ML="$TMP/multiline.json"
cat > "$ML" <<'EOF'
{
  "level": "error",
  "msg": "boom one"
}
{
  "level": "info",
  "msg": "ok"
}
{
  "level": "ERROR",
  "msg": "boom two"
}
{
  "level": "warn",
  "msg": "hmm"
}
EOF

fail=0
n=0

# run_case "<label>" <input-file> -- <kelora args...>
run_case() {
  local label="$1"; shift
  local input="$1"; shift
  [[ "$1" == "--" ]] && shift
  n=$((n + 1))

  "$BASELINE" "$@" < "$input" > "$TMP/b.out" 2> "$TMP/b.err"; local bc=$?
  "$CANDIDATE" "$@" < "$input" > "$TMP/c.out" 2> "$TMP/c.err"; local cc=$?

  # The --stats "Throughput:" line reports wall-clock time and lines/s, which is
  # inherently non-deterministic (it differs run-to-run and between binaries
  # even when nothing else changes). Mask it before comparison so the stats gate
  # is judged on its deterministic counters (events created/output/filtered),
  # not on timing noise.
  local f
  for f in "$TMP/b.out" "$TMP/c.out" "$TMP/b.err" "$TMP/c.err"; do
    sed -i 's/^Throughput:.*/Throughput: <masked>/' "$f"
  done

  local bad=0
  if ! cmp -s "$TMP/b.out" "$TMP/c.out"; then
    echo "DIFF [stdout] $label" >&2
    diff <(head -c 4000 "$TMP/b.out") <(head -c 4000 "$TMP/c.out") | head -20 >&2
    bad=1
  fi
  if ! cmp -s "$TMP/b.err" "$TMP/c.err"; then
    echo "DIFF [stderr] $label" >&2
    diff "$TMP/b.err" "$TMP/c.err" | head -20 >&2
    bad=1
  fi
  if [[ "$bc" != "$cc" ]]; then
    echo "DIFF [exit] $label: baseline=$bc candidate=$cc" >&2
    bad=1
  fi
  if [[ "$bad" == 0 ]]; then
    echo "ok   $label (exit $bc)"
  else
    fail=1
  fi
}

# --- Core scenarios (gate ON expected for logfmt/json + --levels) ------------
run_case "A logfmt --levels error -F json"  "$LOGFMT1" -- -f logfmt --levels error -F json
run_case "A json   --levels error -F json"  "$JSON1"   -- -f json   --levels error -F json
run_case "B logfmt --levels error"          "$LOGFMT"  -- -f logfmt --levels error
run_case "B json   --levels error"          "$JSON"    -- -f json   --levels error
run_case "multi-level logfmt error,warn"    "$LOGFMT"  -- -f logfmt --levels error,warn
run_case "mixed-case levels ERROR"          "$LOGFMT"  -- -f logfmt --levels ERROR

# --- Gate-off scenarios (must be unchanged) ----------------------------------
run_case "D logfmt pass-through"            "$LOGFMT"  -- -f logfmt
run_case "E logfmt --levels error -C 2"     "$LOGFMT"  -- -f logfmt --levels error -C 2
run_case "exclude-levels alone logfmt"      "$LOGFMT"  -- -f logfmt --exclude-levels error
run_case "syslog --levels err (priority)"   "$SYSLOG"  -- -f syslog --levels err
run_case "syslog --levels error"            "$SYSLOG"  -- -f syslog --levels error

# --- Interaction with other stages -------------------------------------------
run_case "levels + --stats"                 "$LOGFMT"  -- -f logfmt --levels error --stats
run_case "levels + --filter after"          "$LOGFMT"  -- -f logfmt --levels error --filter 'e.status >= 500'
run_case "levels + --keys projection"       "$LOGFMT"  -- -f logfmt --levels error --keys ts,level,msg
run_case "levels + include+exclude"         "$LOGFMT"  -- -f logfmt --levels error,warn --exclude-levels warn
run_case "levels error json + drain"        "$JSON"    -- -f json   --levels error --keys msg --drain
run_case "levels error --parallel"          "$LOGFMT1" -- -f logfmt --levels error --parallel
run_case "levels error json --parallel"     "$JSON1"   -- -f json   --levels error --parallel
run_case "levels error -F logfmt output"    "$JSON"    -- -f json   --levels error -F logfmt

# --- Multiline (scan must run after record assembly) -------------------------
run_case "multiline json --levels error"    "$ML" -- -f json -M 'regex:match=^\{' --levels error
run_case "multiline json --levels error par" "$ML" -- -f json -M 'regex:match=^\{' --levels error --parallel
run_case "multiline json pass-through"       "$ML" -- -f json -M 'regex:match=^\{'

echo
if [[ "$fail" == 0 ]]; then
  echo "PASS: $n cases byte-identical"
  exit 0
else
  echo "FAIL: divergence detected across $n cases" >&2
  exit 1
fi
