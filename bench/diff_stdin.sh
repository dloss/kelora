#!/usr/bin/env bash
# Differential correctness gate for the batched (chunked) plain reader.
#
# The chunked-delivery optimization (LineMessage::Chunk) only affects the
# STDIN / single-reader parallel path (`plain_io_reader_thread`); passing a file
# argument routes through the file-aware reader, which is untouched. So this
# harness pipes every corpus in via STDIN with --parallel and asserts that
# stdout, stderr, and exit code match the baseline binary byte-for-byte.
#
# Two deterministic-noise sources are normalized before comparison:
#   * the `Throughput: ...` --stats line (wall-clock, inherently variable), and
#   * the line numbers in a sampled parse-error report, which under --parallel
#     depend on which worker reports first (already non-deterministic in the
#     baseline vs itself) — `line <N>:` is masked to `line <N>:` so the message
#     text and error count are still compared.
#
# Usage:
#   bench/diff_stdin.sh <baseline_bin> <candidate_bin>
set -uo pipefail
B="${1:?usage: diff_stdin.sh <baseline_bin> <candidate_bin>}"
C="${2:?usage: diff_stdin.sh <baseline_bin> <candidate_bin>}"
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
FAIL=0; N=0

norm() { sed -e '/^Throughput: /d' -e 's/line [0-9][0-9]*:/line <N>:/g' "$1"; }

run() { # run <name> -- <args...>   (stdin from $IN)
  local name="$1"; shift; [[ "$1" == "--" ]] && shift
  N=$((N+1))
  "$B" "$@" <"$IN" >"$W/bo.raw" 2>"$W/be.raw"; local br=$?
  "$C" "$@" <"$IN" >"$W/co.raw" 2>"$W/ce.raw"; local cr=$?
  norm "$W/bo.raw" >"$W/bo"; norm "$W/co.raw" >"$W/co"
  norm "$W/be.raw" >"$W/be"; norm "$W/ce.raw" >"$W/ce"
  local ok=1
  [[ $br -ne $cr ]] && { ok=0; echo "  [exit] $name b=$br c=$cr"; }
  cmp -s "$W/bo" "$W/co" || { ok=0; echo "  [stdout] $name differs"; }
  cmp -s "$W/be" "$W/ce" || { ok=0; echo "  [stderr] $name differs"; diff "$W/be" "$W/ce" | head -6; }
  if [[ $ok -eq 1 ]]; then echo "  ok: $name"; else FAIL=$((FAIL+1)); echo "  FAIL: $name"; fi
}

# --- corpora (deterministic) -------------------------------------------------
python3 - "$W" <<'PY'
import sys, json, gzip
w=sys.argv[1]
lvls=["info","warn","error","debug"]
with open(f"{w}/big.logfmt","w") as f:
    for i in range(500000):
        lv = "error" if i%100==0 else lvls[i%4]
        f.write(f"ts=2026-01-01T00:00:{i%60:02d}Z level={lv} seq={i} msg=event-{i} host=h{i%8} dur={i%1000}\n")
with open(f"{w}/big.json","w") as f:
    for i in range(300000):
        lv = "error" if i%100==0 else lvls[i%4]
        f.write(json.dumps({"level":lv,"seq":i,"msg":f"event-{i}","host":f"h{i%8}"})+"\n")
with open(f"{w}/big.logfmt","rb") as fi, gzip.open(f"{w}/big.logfmt.gz","wb") as fo:
    fo.write(fi.read())
# CSV with quoted embedded newlines (record spans physical lines)
with open(f"{w}/quoted.csv","w") as f:
    f.write("id,note,level\n")
    for i in range(20000):
        note = f'line-a\nline-b {i}' if i%5==0 else f'plain {i}'
        f.write(f'{i},"{note}",{"error" if i%10==0 else "info"}\n')
# input not ending in newline
with open(f"{w}/no_nl.logfmt","w") as f:
    for i in range(1000): f.write(f"a={i} level=info\n")
    f.write("a=999999 level=error")
open(f"{w}/one.logfmt","w").write("only=1 level=error\n")   # single line
open(f"{w}/empty.logfmt","w").write("")                     # empty
# a line far longer than the 256 KiB reader chunk (carry/grow), between normals
with open(f"{w}/longline.logfmt","w") as f:
    f.write("level=info seq=0 msg=start\n")
    f.write("level=error seq=1 msg=" + ("x"*400000) + "\n")
    f.write("level=info seq=2 msg=end\n")
# binary garbage / invalid UTF-8 interleaved with text, ending without newline
with open(f"{w}/binary.log","wb") as f:
    for i in range(5000):
        f.write(b"level=info seq=%d msg=" % i + bytes([0xff,0xfe,0x00,0x80,i%256]) + b" tail\n")
    f.write(b"\x00\x01\x02\x03 no newline binary tail")
# CRLF + trailing spaces (trim_end parity)
with open(f"{w}/crlf.logfmt","wb") as f:
    for i in range(3000):
        f.write(b"level=info seq=%d msg=hi   \r\n" % i)
# blank lines interspersed
with open(f"{w}/blanks.log","w") as f:
    for i in range(3000):
        f.write(f"level=info seq={i}\n")
        if i%3==0: f.write("\n")
print("corpora ready", file=sys.stderr)
PY

echo "== stdin --parallel differential matrix =="
IN="$W/big.logfmt";      run "passthrough-logfmt"  -- -f logfmt --parallel
IN="$W/big.logfmt";      run "filter-logfmt"       -- -f logfmt --parallel --filter 'e.level=="error"'
IN="$W/big.logfmt";      run "levels-logfmt"       -- -f logfmt --parallel --levels error
IN="$W/big.logfmt";      run "keep-lines"          -- -f logfmt --parallel --keep-lines 'seq=5'
IN="$W/big.logfmt";      run "ignore-lines"        -- -f logfmt --parallel --ignore-lines 'level=debug'
IN="$W/big.logfmt";      run "head"                -- -f logfmt --parallel --head 12345
IN="$W/big.logfmt";      run "skip-lines"          -- -f logfmt --parallel --skip-lines 777
IN="$W/big.logfmt";      run "json-out"            -- -f logfmt --parallel -F json
IN="$W/big.logfmt";      run "threads2"            -- -f logfmt --parallel --threads 2
IN="$W/big.logfmt";      run "threads8-filter"     -- -f logfmt --parallel --threads 8 --filter 'e.seq%7==0'
IN="$W/big.logfmt";      run "batch-size7"         -- -f logfmt --parallel --batch-size 7
IN="$W/big.logfmt";      run "stats"               -- -f logfmt --parallel --stats
IN="$W/big.json";        run "passthrough-json"    -- -f json --parallel
IN="$W/big.json";        run "filter-json"         -- -f json --parallel --filter 'e.level=="error"'
IN="$W/big.logfmt.gz";   run "gzip-passthrough"    -- -f logfmt --parallel
IN="$W/big.logfmt.gz";   run "gzip-levels"         -- -f logfmt --parallel --levels error
IN="$W/quoted.csv";      run "csv-quoted-nl"       -- -f csv --parallel
IN="$W/quoted.csv";      run "csv-quoted-filter"   -- -f csv --parallel --filter 'e.level=="error"'
IN="$W/no_nl.logfmt";    run "no-trailing-nl"      -- -f logfmt --parallel
IN="$W/one.logfmt";      run "single-line"         -- -f logfmt --parallel
IN="$W/empty.logfmt";    run "empty-input"         -- -f logfmt --parallel
IN="$W/longline.logfmt"; run "long-line"           -- -f logfmt --parallel
IN="$W/longline.logfmt"; run "long-line-maxbytes"  -- -f logfmt --parallel --max-line-bytes 1024
IN="$W/binary.log";      run "binary-garbage"      -- -f logfmt --parallel
IN="$W/binary.log";      run "binary-line"         -- -f line --parallel
IN="$W/crlf.logfmt";     run "crlf-trailing-ws"    -- -f logfmt --parallel -F logfmt
IN="$W/blanks.log";      run "blank-lines-line"    -- -f line --parallel
IN="$W/blanks.log";      run "blank-lines-logfmt"  -- -f logfmt --parallel
IN="$W/big.logfmt";      run "multiline"           -- -f logfmt --parallel --multiline 'regex:match=^ts='

echo
echo "Ran $N cases; $FAIL failed."
[[ $FAIL -eq 0 ]] || exit 1
