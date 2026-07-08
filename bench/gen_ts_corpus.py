#!/usr/bin/env python3
"""Deterministic corpus generator for the timestamp fast-path pre-filter benchmark.

Produces matched logfmt/JSON corpora (~15 realistic fields per record) whose
timestamps are spread UNIFORMLY AT RANDOM across a 24h window and written in
random (unsorted) order. Unsorted is deliberate: it means a `--until` early-exit
optimization cannot help, so the measured win is attributable to the pre-filter
alone (spec §2).

The logfmt and JSON variants contain byte-for-byte the *same* records (same
field values, same order), so the two formats are directly comparable.

Usage:
    python3 gen_ts_corpus.py [--lines N] [--outdir DIR]

Writes:
    <outdir>/ts_logfmt_<tag>.log
    <outdir>/ts_json_<tag>.log

The window is 2026-01-01T00:00:00Z .. 2026-01-02T00:00:00Z (86400 seconds).
Benchmark thresholds derived from that window:
    90% mark  -> --since 2026-01-01T21:36:00Z   (~10% of events kept)
    0%  mark  -> --since 2026-01-01T00:00:00Z   (all events kept)

Determinism: a fixed seed drives all randomness, so re-runs are byte-for-byte
reproducible.
"""
import argparse
import json
import os
import random

SERVICES = ["auth", "api-gateway", "billing", "search", "worker", "cache", "ingest"]
HOSTS = [f"host-{i:02d}" for i in range(1, 25)]
METHODS = ["GET", "GET", "GET", "POST", "PUT", "DELETE", "PATCH"]
PATHS = [
    "/v1/users", "/v1/orders", "/v1/search", "/health", "/v1/login",
    "/v1/products", "/metrics", "/v1/cart", "/v1/checkout", "/v1/session",
]
REGIONS = ["us-east-1", "us-west-2", "eu-west-1", "ap-southeast-1"]
LEVELS = ["info", "info", "info", "debug", "warn", "trace", "error"]
MSGS = [
    "request completed", "cache hit", "user authenticated", "job scheduled",
    "connection established", "payload validated", "record persisted",
    "index refreshed", "session created", "rate limit ok",
]

WINDOW_SECONDS = 86400  # 24h


def _iso(offset_seconds):
    # Window is a single UTC day starting 2026-01-01T00:00:00Z, so no day/month
    # rollover is needed for offsets in [0, 86400).
    hh, rem = divmod(offset_seconds, 3600)
    mm, ss = divmod(rem, 60)
    return f"2026-01-01T{hh:02d}:{mm:02d}:{ss:02d}Z"


def make_record(rng, ts):
    """Build one log record as an ordered list of (key, value) pairs, with the
    timestamp field first (the common shape the fast path optimizes)."""
    status = rng.choice([200, 200, 200, 201, 204, 301, 400, 404, 500, 503])
    return [
        ("ts", ts),
        ("level", rng.choice(LEVELS)),
        ("service", rng.choice(SERVICES)),
        ("host", rng.choice(HOSTS)),
        ("pid", rng.randint(100, 65000)),
        ("thread", f"t-{rng.randint(1, 64)}"),
        ("request_id", f"{rng.randrange(16**12):012x}"),
        ("method", rng.choice(METHODS)),
        ("path", rng.choice(PATHS)),
        ("status", status),
        ("duration_ms", round(rng.uniform(0.2, 950.0), 2)),
        ("bytes", rng.randint(0, 200000)),
        ("user_id", rng.randint(1000, 999999)),
        ("region", rng.choice(REGIONS)),
        ("msg", rng.choice(MSGS)),
    ]


def _logfmt_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, str):
        if v == "" or any(c in v for c in ' "='):
            return '"' + v.replace('"', '\\"') + '"'
        return v
    return str(v)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lines", type=int, default=1_000_000)
    ap.add_argument("--outdir", default=os.path.join(os.path.dirname(__file__), "corpus"))
    args = ap.parse_args()
    os.makedirs(args.outdir, exist_ok=True)
    tag = "1m" if args.lines == 1_000_000 else str(args.lines)

    rng = random.Random(0x715DA7A + args.lines)
    logfmt_path = os.path.join(args.outdir, f"ts_logfmt_{tag}.log")
    json_path = os.path.join(args.outdir, f"ts_json_{tag}.log")

    with open(logfmt_path, "w") as lf, open(json_path, "w") as jf:
        for _ in range(args.lines):
            # Uniform-random second in the window -> unsorted timestamps.
            ts = _iso(rng.randrange(WINDOW_SECONDS))
            rec = make_record(rng, ts)
            lf.write(" ".join(f"{k}={_logfmt_value(v)}" for k, v in rec) + "\n")
            jf.write(json.dumps(dict(rec), separators=(",", ":")) + "\n")

    print(f"wrote {logfmt_path}")
    print(f"wrote {json_path}")
    print("thresholds: 90% mark = 2026-01-01T21:36:00Z, 0% mark = 2026-01-01T00:00:00Z")


if __name__ == "__main__":
    main()
