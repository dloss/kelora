#!/usr/bin/env python3
"""Deterministic corpus generator for the level pre-filter benchmark.

Produces matched logfmt and JSON-lines corpora with a controllable fraction of
ERROR-level records. The same seeded record stream backs both formats, so the
only difference between ``logfmt_1m.log`` and ``json_1m.log`` is the encoding —
the level distribution and field values are identical line for line.

Usage:
    ./gen_corpus.py [--lines N] [--out DIR]

Generates, for each of the error fractions 1%, 20% and 100%:
    logfmt_<pct>.log   json_<pct>.log

plus the canonical 1,000,000-line ``logfmt_1m.log`` / ``json_1m.log`` aliases
for the 1%-error variant (scenario A's target corpus).
"""

import argparse
import json
import os
import random

# Realistic-ish logfmt/JSON key set (~15 fields per record).
SERVICES = ["auth", "api", "worker", "gateway", "billing", "search", "cache"]
HOSTS = [f"host-{i:02d}" for i in range(1, 13)]
METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"]
PATHS = [
    "/v1/users", "/v1/orders", "/v1/login", "/health", "/metrics",
    "/v1/search", "/v1/cart", "/v1/checkout", "/v1/items", "/static/app.js",
]
# Non-error levels used to fill the remaining share. "error" is added on top
# according to the requested fraction.
NON_ERROR_LEVELS = ["info", "debug", "warn", "trace"]
USERS = [f"u{n:05d}" for n in range(0, 5000)]
MESSAGES = [
    "request completed",
    "cache miss",
    "connection established",
    "retrying upstream",
    "validation passed",
    "token refreshed",
    "background job scheduled",
    "payload accepted",
]
# Messages used for error records. Deliberately none of them contain the
# substring "error" so that the pre-filter's only "error" token comes from the
# level field itself — this keeps the 1%/20% selectivity honest rather than
# leaking extra matches through the message body.
ERROR_MESSAGES = [
    "upstream timeout",
    "connection refused",
    "invalid credentials",
    "quota exceeded",
    "downstream 503",
    "deadline exceeded",
]


def make_record(rng, is_error):
    """Build one record as an ordered list of (key, value) pairs (~15 fields)."""
    level = "error" if is_error else rng.choice(NON_ERROR_LEVELS)
    msg = rng.choice(ERROR_MESSAGES if is_error else MESSAGES)
    status = rng.choice([500, 502, 503] if is_error else [200, 201, 204, 301, 404])
    # A fixed synthetic timestamp stream (monotonic-ish, deterministic).
    secs = rng.randint(0, 86399)
    ts = f"2026-07-04T{secs // 3600:02d}:{(secs % 3600) // 60:02d}:{secs % 60:02d}Z"
    return [
        ("ts", ts),
        ("level", level),
        ("service", rng.choice(SERVICES)),
        ("host", rng.choice(HOSTS)),
        ("pid", rng.randint(100, 32000)),
        ("method", rng.choice(METHODS)),
        ("path", rng.choice(PATHS)),
        ("status", status),
        ("dur_ms", round(rng.uniform(0.1, 900.0), 1)),
        ("bytes", rng.randint(0, 1_048_576)),
        ("user", rng.choice(USERS)),
        ("req_id", f"{rng.getrandbits(48):012x}"),
        ("region", rng.choice(["us-east", "us-west", "eu-central", "ap-south"])),
        ("retries", rng.randint(0, 3)),
        ("msg", msg),
    ]


def logfmt_encode(pairs):
    out = []
    for k, v in pairs:
        if isinstance(v, str) and (" " in v or v == ""):
            out.append(f'{k}="{v}"')
        else:
            out.append(f"{k}={v}")
    return " ".join(out)


def json_encode(pairs):
    # Preserve key order; compact separators to keep lines tight.
    return json.dumps(dict(pairs), separators=(",", ":"))


def generate_syslog(path, lines, error_fraction, seed):
    """RFC3164-ish syslog whose level is encoded ONLY in the <priority> number.

    This is the correctness trap the pre-filter must avoid: priority 11 means
    err (severity 3) but the word "error" never appears in the line. Used by the
    differential harness with ``-f syslog`` to confirm the gate keeps the
    pre-filter inert for level-mapping parsers.
    """
    rng = random.Random(seed)
    months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
              "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    with open(path, "w") as f:
        for _ in range(lines):
            is_error = rng.random() < error_fraction
            # facility 1 (user) * 8 + severity. severity 3 = err, 6 = info.
            sev = 3 if is_error else rng.choice([5, 6, 7])
            pri = 8 + sev
            mon = rng.choice(months)
            day = rng.randint(1, 28)
            ts = f"{mon} {day:2d} {rng.randint(0,23):02d}:{rng.randint(0,59):02d}:{rng.randint(0,59):02d}"
            host = rng.choice(HOSTS)
            tag = rng.choice(SERVICES)
            msg = rng.choice(ERROR_MESSAGES if is_error else MESSAGES)
            f.write(f"<{pri}>{ts} {host} {tag}[{rng.randint(100,9999)}]: {msg}\n")


def generate(path_logfmt, path_json, lines, error_fraction, seed):
    rng = random.Random(seed)
    # Decide error/non-error up front so both encodings see the identical
    # sequence regardless of per-record RNG draws.
    with open(path_logfmt, "w") as fl, open(path_json, "w") as fj:
        for _ in range(lines):
            is_error = rng.random() < error_fraction
            rec = make_record(rng, is_error)
            fl.write(logfmt_encode(rec))
            fl.write("\n")
            fj.write(json_encode(rec))
            fj.write("\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--lines", type=int, default=1_000_000)
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "corpus"))
    ap.add_argument("--seed", type=int, default=1729)
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)

    variants = [("1pct", 0.01), ("20pct", 0.20), ("100pct", 1.00)]
    for name, frac in variants:
        lf = os.path.join(args.out, f"logfmt_{name}.log")
        jf = os.path.join(args.out, f"json_{name}.log")
        # Distinct seed per variant so distributions don't correlate, but each
        # run is fully reproducible.
        generate(lf, jf, args.lines, frac, args.seed + int(frac * 1000))
        print(f"wrote {lf} and {jf} ({args.lines} lines, {name} error)")

    # Canonical aliases for scenario A (1% error target corpus).
    import shutil
    shutil.copyfile(
        os.path.join(args.out, "logfmt_1pct.log"),
        os.path.join(args.out, "logfmt_1m.log"),
    )
    shutil.copyfile(
        os.path.join(args.out, "json_1pct.log"),
        os.path.join(args.out, "json_1m.log"),
    )
    print("wrote logfmt_1m.log and json_1m.log (aliases of the 1% variant)")

    # A smaller syslog corpus for the differential harness (level lives only in
    # the <priority> number, so the pre-filter must stay inert here).
    syslog_lines = min(args.lines, 100_000)
    sf = os.path.join(args.out, "syslog_20pct.log")
    generate_syslog(sf, syslog_lines, 0.20, args.seed + 7)
    print(f"wrote {sf} ({syslog_lines} lines, 20% error via priority)")


if __name__ == "__main__":
    main()
