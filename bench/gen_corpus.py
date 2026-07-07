#!/usr/bin/env python3
"""Deterministic corpus generator for the level pre-filter benchmark.

Produces matched logfmt/JSON corpora with ~15 realistic fields per record and a
parameterized error-level distribution. The logfmt and JSON variants for a given
error fraction contain byte-for-byte the *same* records (same field values, same
order), so the two formats are directly comparable.

Usage:
    python3 gen_corpus.py [--lines N] [--outdir DIR]

Writes, for each error fraction in {1%, 20%, 100%}:
    <outdir>/logfmt_<n>_<pct>.log
    <outdir>/json_<n>_<pct>.log

Determinism: a fixed seed drives all randomness, so re-running yields identical
files. The same record stream is reused across the logfmt and JSON writers.
"""
import argparse
import json
import os
import random

# Non-error levels drawn for records that are not "error". Kept lowercase; the
# pre-filter and LevelFilterStage both match case-insensitively, and a separate
# mixed-case unit test covers the casing dimension.
NON_ERROR_LEVELS = ["info", "info", "info", "debug", "warn", "trace"]

SERVICES = ["auth", "api-gateway", "billing", "search", "worker", "cache", "ingest"]
HOSTS = [f"host-{i:02d}" for i in range(1, 25)]
METHODS = ["GET", "GET", "GET", "POST", "PUT", "DELETE", "PATCH"]
PATHS = [
    "/v1/users", "/v1/orders", "/v1/search", "/health", "/v1/login",
    "/v1/products", "/metrics", "/v1/cart", "/v1/checkout", "/v1/session",
]
REGIONS = ["us-east-1", "us-west-2", "eu-west-1", "ap-southeast-1"]
# Neutral message fragments. Deliberately free of the token "error" so that
# non-error records do not become pre-filter false positives; this keeps the
# benchmark measuring the intended skip path. A small, fixed fraction of
# messages do embed "error"/"failed" wording via ERROR_MSGS below.
INFO_MSGS = [
    "request completed", "cache hit", "user authenticated", "job scheduled",
    "connection established", "payload validated", "record persisted",
    "index refreshed", "session created", "rate limit ok",
]
ERROR_MSGS = [
    "request failed", "upstream timeout", "connection refused",
    "validation rejected", "database unavailable", "permission denied",
]


def make_record(rng, is_error, seq):
    """Build one log record as an ordered list of (key, value) pairs.

    Value types are mixed (str/int/float/bool) so the JSON encoding exercises
    non-string fields too. Timestamps advance monotonically from a fixed epoch.
    """
    level = "error" if is_error else rng.choice(NON_ERROR_LEVELS)
    msg = rng.choice(ERROR_MSGS if is_error else INFO_MSGS)
    # ISO-8601 timestamp at 2026-01-01T00:00:00Z + seq seconds.
    ts = _iso(seq)
    status = rng.choice([200, 200, 200, 201, 204, 301, 400, 404, 500, 503]) if not is_error \
        else rng.choice([500, 502, 503, 400, 404])
    return [
        ("ts", ts),
        ("level", level),
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
        ("msg", msg),
    ]


def _iso(offset_seconds):
    # Minimal UTC ISO formatter (days/seconds arithmetic) to avoid datetime
    # overhead in the hot generation loop. `offset_seconds` counts from
    # 2026-01-01T00:00:00Z. A 1M-second span is ~11.6 days, so the day-of-month
    # stays within January (31 days) and no month rollover is needed.
    days, rem = divmod(offset_seconds, 86400)
    hh, rem = divmod(rem, 3600)
    mm, ss = divmod(rem, 60)
    day = 1 + days
    return f"2026-01-{day:02d}T{hh:02d}:{mm:02d}:{ss:02d}Z"


def _logfmt_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, str):
        # Quote only when needed (spaces or empty), matching common logfmt output.
        if v == "" or any(c in v for c in ' "='):
            return '"' + v.replace('"', '\\"') + '"'
        return v
    return str(v)


def write_variant(lines, error_frac, outdir, tag):
    # Seed is a pure function of the parameters (no str hashing, which Python
    # randomizes per process) so re-runs are byte-for-byte reproducible.
    rng = random.Random(0xC0FFEE + int(round(error_frac * 100)) * 1_000_003 + lines)
    logfmt_path = os.path.join(outdir, f"logfmt_{tag}_{int(error_frac*100)}pct.log")
    json_path = os.path.join(outdir, f"json_{tag}_{int(error_frac*100)}pct.log")
    n_error = int(round(lines * error_frac))
    # Deterministic error placement: a shuffled boolean mask.
    mask = [True] * n_error + [False] * (lines - n_error)
    rng.shuffle(mask)
    with open(logfmt_path, "w") as lf, open(json_path, "w") as jf:
        for seq in range(lines):
            rec = make_record(rng, mask[seq], seq)
            lf.write(" ".join(f"{k}={_logfmt_value(v)}" for k, v in rec) + "\n")
            jf.write(json.dumps(dict(rec), separators=(",", ":")) + "\n")
    return logfmt_path, json_path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lines", type=int, default=1_000_000)
    ap.add_argument("--outdir", default=os.path.join(os.path.dirname(__file__), "corpus"))
    args = ap.parse_args()
    os.makedirs(args.outdir, exist_ok=True)
    # Tag encodes the line count so 1M and smoke-test corpora coexist.
    tag = "1m" if args.lines == 1_000_000 else str(args.lines)
    for frac in (0.01, 0.20, 1.00):
        lf, jf = write_variant(args.lines, frac, args.outdir, tag)
        print(f"wrote {lf}")
        print(f"wrote {jf}")


if __name__ == "__main__":
    main()
