#!/usr/bin/env python3
"""Wall-clock A/B benchmark for the batched (chunked) plain reader.

The chunked-delivery optimization only affects the STDIN / single-reader
parallel path (`plain_io_reader_thread`), so every scenario here feeds the
corpus in via STDIN (a file *argument* would route through the untouched
file-aware reader). Each command is run RUNS times per binary after WARMUP
warmups; we report the median and the min (min is the most noise-robust on a
shared box). hyperfine is preferred where available; this pure-stdlib harness
is the fallback used to produce the committed results when it is not.

Usage:
  python3 bench/bench_stdin.py [baseline_bin] [candidate_bin] [corpus_dir]

Defaults: bench/bin/kelora-baseline, target/release/kelora, bench/corpus
Generate the corpus first: python3 bench/gen_corpus.py --lines 1000000 --outdir bench/corpus
"""
import subprocess, time, sys, statistics, os

BASE = sys.argv[1] if len(sys.argv) > 1 else "bench/bin/kelora-baseline"
CAND = sys.argv[2] if len(sys.argv) > 2 else "target/release/kelora"
CORP = sys.argv[3] if len(sys.argv) > 3 else "bench/corpus"
RUNS, WARMUP = 9, 2


def warm(path):
    with open(path, "rb") as f:
        while f.read(1 << 20):
            pass


def timeit(binpath, args, infile):
    with open(infile, "rb") as fi, open(os.devnull, "wb") as fo:
        t = time.monotonic()
        subprocess.run([binpath] + args, stdin=fi, stdout=fo, stderr=subprocess.DEVNULL)
        return time.monotonic() - t


def bench(label, args, infile):
    warm(infile)
    for _ in range(WARMUP):
        timeit(CAND, args, infile)
        timeit(BASE, args, infile)
    b = sorted(timeit(BASE, args, infile) for _ in range(RUNS))
    c = sorted(timeit(CAND, args, infile) for _ in range(RUNS))
    bmed, cmed = statistics.median(b), statistics.median(c)
    impr = (bmed - cmed) / bmed * 100
    print(f"{label:26s} base_med={bmed*1000:7.1f}ms cand_med={cmed*1000:7.1f}ms "
          f"base_min={b[0]*1000:7.1f} cand_min={c[0]*1000:7.1f} improvement={impr:+5.1f}%")


LF1 = f"{CORP}/logfmt_1m_1pct.log"
J1 = f"{CORP}/json_1m_1pct.log"

print("== A: pass-through --parallel (stdin, 1M) ==")
bench("A-passthrough-logfmt", ["-f", "logfmt", "--parallel"], LF1)
bench("A-passthrough-json", ["-f", "json", "--parallel"], J1)
print("== B: --levels error --parallel (stdin, 1M, 1% error) ==")
bench("B-levels-logfmt", ["-f", "logfmt", "--parallel", "--levels", "error"], LF1)
print("== C: worker-scaling curve (stdin pass-through logfmt) ==")
for n in (1, 2, 4, 8):
    bench(f"C-threads{n}", ["-f", "logfmt", "--parallel", "--threads", str(n)], LF1)
print("== D: sequential pass-through control (untouched path) ==")
bench("D-sequential-logfmt", ["-f", "logfmt"], LF1)
