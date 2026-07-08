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


# --- Wide corpus (projection-pushdown benchmark) ----------------------------
#
# ~40 fields per record with Kubernetes-shaped names, including two nested
# objects (`kubernetes`, `resource`). A query like `-k _ts,level,msg` names 3
# of them, so ~37 fields — several of them nested maps whose `Dynamic`
# construction is the dominant per-field cost — are materialized only to be
# dropped by KeyFilterStage. This is the corpus projection pushdown targets.

WIDE_STREAMS = ["stdout", "stdout", "stderr"]
WIDE_NAMESPACES = ["default", "kube-system", "payments", "search", "ingest", "mesh"]
WIDE_NODES = [f"ip-10-0-{i}-{j}" for i in range(1, 6) for j in (11, 42, 77)]
WIDE_APPS = ["auth", "api-gateway", "billing", "search", "worker", "cache", "ingest"]
WIDE_TIERS = ["frontend", "backend", "cache", "queue"]
WIDE_VERSIONS = ["1.2.3", "1.2.4", "2.0.0", "2.1.0-rc1"]
WIDE_ZONES = ["us-east-1a", "us-east-1b", "us-west-2a", "eu-west-1b"]
WIDE_CLUSTERS = ["prod-blue", "prod-green", "staging"]
WIDE_LOGGERS = ["http.access", "app.worker", "db.pool", "auth.session", "cache.redis"]


def make_wide_record(rng, is_error, seq):
    """Build one wide (~40-field) k8s-shaped record as ordered (key, value) pairs.

    `_ts`, `level`, `msg` are placed among the fields (not first) so a
    projection that keeps them still has to skip fields on both sides. Two
    nested objects exercise the `Dynamic` map/array construction that pushdown
    avoids for unwanted keys.
    """
    level = "error" if is_error else rng.choice(NON_ERROR_LEVELS)
    msg = rng.choice(ERROR_MSGS if is_error else INFO_MSGS)
    status = rng.choice([500, 502, 503, 400, 404]) if is_error \
        else rng.choice([200, 200, 200, 201, 204, 301, 400, 404])
    app = rng.choice(WIDE_APPS)
    return [
        ("stream", rng.choice(WIDE_STREAMS)),
        ("logtag", "F"),
        ("_ts", _iso(seq)),
        ("node_name", rng.choice(WIDE_NODES)),
        ("host_ip", f"10.0.{rng.randint(0,255)}.{rng.randint(1,254)}"),
        ("pod_ip", f"10.1.{rng.randint(0,255)}.{rng.randint(1,254)}"),
        ("pod_name", f"{app}-{rng.randrange(16**5):05x}-{rng.randrange(16**5):05x}"),
        ("namespace", rng.choice(WIDE_NAMESPACES)),
        ("container_name", app),
        ("container_id", f"{rng.randrange(16**16):016x}"),
        ("image", f"registry.example.com/{app}:{rng.choice(WIDE_VERSIONS)}"),
        ("image_id", f"sha256:{rng.randrange(16**16):016x}"),
        ("restart_count", rng.randint(0, 5)),
        ("level", level),
        ("request_id", f"{rng.randrange(16**12):012x}"),
        ("trace_id", f"{rng.randrange(16**16):016x}"),
        ("span_id", f"{rng.randrange(16**8):08x}"),
        ("method", rng.choice(METHODS)),
        ("path", rng.choice(PATHS)),
        ("status", status),
        ("duration_ms", round(rng.uniform(0.2, 950.0), 2)),
        ("bytes_sent", rng.randint(0, 200000)),
        ("bytes_recv", rng.randint(0, 20000)),
        ("user_agent", rng.choice(["curl/8.4.0", "Go-http-client/2.0", "Mozilla/5.0"])),
        ("remote_addr", f"203.0.{rng.randint(0,255)}.{rng.randint(1,254)}"),
        ("referer", rng.choice(["-", "https://example.com/app", "https://example.com/"])),
        ("protocol", "HTTP/1.1"),
        ("scheme", rng.choice(["https", "https", "http"])),
        ("upstream", f"{app}.svc.cluster.local:8080"),
        ("upstream_status", status),
        ("cache_hit", rng.choice([True, False])),
        ("region", rng.choice(REGIONS)),
        ("zone", rng.choice(WIDE_ZONES)),
        ("cluster", rng.choice(WIDE_CLUSTERS)),
        ("account_id", rng.randint(100000, 999999)),
        ("thread", f"t-{rng.randint(1, 64)}"),
        ("logger", rng.choice(WIDE_LOGGERS)),
        ("msg", msg),
        ("kubernetes", {
            "uid": f"{rng.randrange(16**16):016x}",
            "labels": {"app": app, "version": rng.choice(WIDE_VERSIONS),
                       "tier": rng.choice(WIDE_TIERS)},
            "annotations": {"checksum": f"{rng.randrange(16**8):08x}"},
        }),
        ("resource", {
            "cpu_m": rng.randint(10, 2000),
            "memory_mb": rng.randint(32, 4096),
            "limits": {"cpu_m": 4000, "memory_mb": 8192},
        }),
    ]


def _logfmt_flatten(pairs):
    """Flatten nested objects into dotted logfmt keys, matching how a logfmt
    emitter would render the same k8s record (kubernetes.labels.app=...)."""
    out = []

    def walk(prefix, value):
        if isinstance(value, dict):
            for k, v in value.items():
                walk(f"{prefix}.{k}" if prefix else k, v)
        else:
            out.append((prefix, value))

    for k, v in pairs:
        walk(k, v)
    return out


def write_wide_variant(lines, error_frac, outdir, tag):
    rng = random.Random(0x5EED + int(round(error_frac * 100)) * 7_777_777 + lines)
    logfmt_path = os.path.join(outdir, f"wide_logfmt_{tag}_{int(error_frac*100)}pct.log")
    json_path = os.path.join(outdir, f"wide_json_{tag}_{int(error_frac*100)}pct.log")
    n_error = int(round(lines * error_frac))
    mask = [True] * n_error + [False] * (lines - n_error)
    rng.shuffle(mask)
    with open(logfmt_path, "w") as lf, open(json_path, "w") as jf:
        for seq in range(lines):
            rec = make_wide_record(rng, mask[seq], seq)
            flat = _logfmt_flatten(rec)
            lf.write(" ".join(f"{k}={_logfmt_value(v)}" for k, v in flat) + "\n")
            jf.write(json.dumps(dict(rec), separators=(",", ":")) + "\n")
    return logfmt_path, json_path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lines", type=int, default=1_000_000)
    ap.add_argument("--outdir", default=os.path.join(os.path.dirname(__file__), "corpus"))
    ap.add_argument(
        "--profile",
        choices=("narrow", "wide", "both"),
        default="narrow",
        help="narrow: ~15-field level-prefilter corpus; wide: ~40-field "
        "k8s-shaped projection corpus; both: generate each.",
    )
    ap.add_argument(
        "--fracs",
        default="0.01,0.20,1.00",
        help="comma-separated error fractions to emit (default 0.01,0.20,1.00).",
    )
    args = ap.parse_args()
    os.makedirs(args.outdir, exist_ok=True)
    # Tag encodes the line count so 1M and smoke-test corpora coexist.
    tag = "1m" if args.lines == 1_000_000 else str(args.lines)
    fracs = [float(x) for x in args.fracs.split(",") if x.strip()]

    if args.profile in ("narrow", "both"):
        for frac in fracs:
            lf, jf = write_variant(args.lines, frac, args.outdir, tag)
            print(f"wrote {lf}")
            print(f"wrote {jf}")
    if args.profile in ("wide", "both"):
        for frac in fracs:
            lf, jf = write_wide_variant(args.lines, frac, args.outdir, tag)
            print(f"wrote {lf}")
            print(f"wrote {jf}")


if __name__ == "__main__":
    main()
