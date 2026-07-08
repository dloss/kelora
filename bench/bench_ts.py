#!/usr/bin/env python3
"""Wall-clock A/B benchmark for the timestamp fast-path pre-filter (spec §5).

Each scenario is run over the 1M-line UNSORTED corpora (so a --until early-exit
cannot help — the measured win is the pre-filter's alone). Every command is run
RUNS times per binary after WARMUP warmups; we report median and min (min is the
most noise-robust on a shared box). Pure stdlib so it runs where hyperfine is
absent.

Usage:
  python3 bench/bench_ts.py [baseline_bin] [candidate_bin] [corpus_dir]

Defaults: bench/bin/kelora-baseline, bench/bin/kelora-candidate, bench/corpus
Generate the corpus first:
  python3 bench/gen_ts_corpus.py --lines 1000000 --outdir bench/corpus

Scenarios (spec §5):
  A  --since <90% mark>, logfmt        target win (≥20%, expect 2-4x)
  B  --since <90% mark>, json          target win (≥20%)
  C  --since <0% mark>   (all match)   pure overhead (≤3%)
  D  no time filter                    gate off, unchanged
  E  --levels error --since <90%>      composition win
"""
import subprocess, time, sys, statistics, os

BASE = sys.argv[1] if len(sys.argv) > 1 else "bench/bin/kelora-baseline"
CAND = sys.argv[2] if len(sys.argv) > 2 else "bench/bin/kelora-candidate"
CORP = sys.argv[3] if len(sys.argv) > 3 else "bench/corpus"
RUNS, WARMUP = 9, 2

SINCE90 = "2026-01-01T21:36:00Z"
SINCE0 = "2026-01-01T00:00:00Z"

LF = f"{CORP}/ts_logfmt_1m.log"
J = f"{CORP}/ts_json_1m.log"


def warm(path):
    with open(path, "rb") as f:
        while f.read(1 << 20):
            pass


def timeit(binpath, args):
    with open(os.devnull, "wb") as fo:
        t = time.monotonic()
        subprocess.run([binpath] + args, stdout=fo, stderr=subprocess.DEVNULL)
        return time.monotonic() - t


def bench(label, args):
    for _ in range(WARMUP):
        timeit(CAND, args)
        timeit(BASE, args)
    b = sorted(timeit(BASE, args) for _ in range(RUNS))
    c = sorted(timeit(CAND, args) for _ in range(RUNS))
    bmed, cmed = statistics.median(b), statistics.median(c)
    impr = (bmed - cmed) / bmed * 100
    speedup = bmed / cmed if cmed else float("inf")
    print(f"{label:28s} base_med={bmed*1000:8.1f}ms cand_med={cmed*1000:8.1f}ms "
          f"base_min={b[0]*1000:7.1f} cand_min={c[0]*1000:7.1f} "
          f"improvement={impr:+6.1f}% ({speedup:.2f}x)")


def main():
    for p in (LF, J):
        if not os.path.exists(p):
            print(f"missing corpus {p}; run gen_ts_corpus.py first", file=sys.stderr)
            sys.exit(1)
        warm(p)

    print("== Scenario A — --since 90% mark, logfmt (target win) ==")
    bench("A-logfmt-seq", ["-f", "logfmt", LF, "--since", SINCE90])
    bench("A-logfmt-parallel", ["-f", "logfmt", LF, "--since", SINCE90, "--parallel"])
    print("== Scenario B — --since 90% mark, json (target win) ==")
    bench("B-json-seq", ["-f", "json", J, "--since", SINCE90])
    bench("B-json-parallel", ["-f", "json", J, "--since", SINCE90, "--parallel"])
    print("== Scenario C — --since 0% mark, everything matches (pure overhead ≤3%) ==")
    bench("C-logfmt-seq", ["-f", "logfmt", LF, "--since", SINCE0])
    bench("C-json-seq", ["-f", "json", J, "--since", SINCE0])
    print("== Scenario D — no time filter (gate off, unchanged) ==")
    bench("D-logfmt-seq", ["-f", "logfmt", LF])
    bench("D-json-seq", ["-f", "json", J])
    print("== Scenario E — --levels error --since 90% (composition) ==")
    bench("E-logfmt-seq", ["-f", "logfmt", LF, "--levels", "error", "--since", SINCE90])
    bench("E-json-seq", ["-f", "json", J, "--levels", "error", "--since", SINCE90])


if __name__ == "__main__":
    main()
