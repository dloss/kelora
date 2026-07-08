#!/usr/bin/env python3
"""Wall-clock A/B benchmark for projection pushdown into parsers.

Projection is a build-time decision inside the pipeline, so the cleanest A/B is
the SAME binary with pushdown on vs off. `KELORA_NO_PROJECTION=1` forces the
safety gate closed (Projection::All), reproducing pre-projection behavior
exactly, so any delta is the pushdown itself — no separate baseline build, no
skew from unrelated commits.

Each scenario feeds a corpus file as an argument and discards stdout. We run
RUNS times per side after WARMUP warmups and report median and min (min is the
most noise-robust on a shared box).

Usage:
  python3 bench/run_projection.py [candidate_bin] [corpus_dir]

Defaults: target/release/kelora, bench/corpus
Generate the corpus first:
  python3 bench/gen_corpus.py --profile wide  --lines 1000000 --fracs 0.20
  python3 bench/gen_corpus.py --profile narrow --lines 1000000 --fracs 0.20
"""
import os
import statistics
import subprocess
import sys
import time

CAND = sys.argv[1] if len(sys.argv) > 1 else "target/release/kelora"
CORP = sys.argv[2] if len(sys.argv) > 2 else "bench/corpus"
RUNS, WARMUP = 9, 2


def warm(path):
    with open(path, "rb") as f:
        while f.read(1 << 20):
            pass


def timeit(args, env):
    with open(os.devnull, "wb") as fo:
        t = time.monotonic()
        subprocess.run(
            [CAND] + args,
            stdout=fo,
            stderr=subprocess.DEVNULL,
            env=env,
        )
        return time.monotonic() - t


def bench(label, args, infile):
    warm(infile)
    on_env = dict(os.environ)
    on_env.pop("KELORA_NO_PROJECTION", None)
    off_env = dict(os.environ, KELORA_NO_PROJECTION="1")

    for _ in range(WARMUP):
        timeit(args, on_env)
        timeit(args, off_env)

    on = sorted(timeit(args, on_env) for _ in range(RUNS))
    off = sorted(timeit(args, off_env) for _ in range(RUNS))

    def med(v):
        return statistics.median(v)

    speedup = (med(off) - med(on)) / med(off) * 100.0
    print(
        f"{label:40s}  off(all)={med(off)*1e3:7.1f}ms  "
        f"on(proj)={med(on)*1e3:7.1f}ms  min_on={on[0]*1e3:7.1f}ms  "
        f"win={speedup:+5.1f}%"
    )
    return speedup


def main():
    wj = os.path.join(CORP, "wide_json_1m_20pct.log")
    wl = os.path.join(CORP, "wide_logfmt_1m_20pct.log")
    nj = os.path.join(CORP, "json_1m_20pct.log")
    nl = os.path.join(CORP, "logfmt_1m_20pct.log")

    missing = [p for p in (wj, wl) if not os.path.exists(p)]
    if missing:
        sys.exit(f"missing corpus: {missing}\nSee the module docstring to generate it.")

    print("== Projection pushdown A/B (off = KELORA_NO_PROJECTION=1) ==")
    bench("A wide json  -k _ts,level,msg", ["-f", "json", wj, "-k", "_ts,level,msg"], wj)
    bench("A' wide json --levels error -k", ["-f", "json", wj, "--levels", "error", "-k", "_ts,level,msg"], wj)
    bench("B wide logfmt -k _ts,level,msg", ["-f", "logfmt", wl, "-k", "_ts,level,msg"], wl)
    bench("B' wide logfmt --levels error -k", ["-f", "logfmt", wl, "--levels", "error", "-k", "_ts,level,msg"], wl)
    if os.path.exists(nj):
        bench("C narrow json -k ts,level,msg", ["-f", "json", nj, "-k", "ts,level,msg"], nj)
    if os.path.exists(nl):
        bench("C narrow logfmt -k ts,level,msg", ["-f", "logfmt", nl, "-k", "ts,level,msg"], nl)
    bench("D wide json (no -k, gate off)", ["-f", "json", wj], wj)
    bench("E wide json -k msg --exec (gate off)", ["-f", "json", wj, "-k", "msg", "--exec", "e.n = 1"], wj)


if __name__ == "__main__":
    main()
