#!/usr/bin/env python3
"""
Kelora "common simple cases" benchmark harness.

Measures the per-event hot path (read -> parse -> filter -> format -> write)
for the cases real users hit most often, and reports THROUGHPUT
(lines/s and MB/s) rather than raw wall-time so regressions are legible
across machines and dataset sizes.

This suite is deliberately separate from run_benchmarks.sh (the regression
suite): it generates its own uniform-schema datasets so that narrow-vs-wide
event comparisons are controlled, and it isolates individual pipeline stages
with paired scenarios whose *delta* attributes cost to a specific stage.

No external timing tools required (no gtime/hyperfine): timing uses
time.perf_counter() around subprocess.run with output sent to /dev/null.

Usage:
  python3 benchmarks/bench_simple_cases.py            # full run (200k lines)
  python3 benchmarks/bench_simple_cases.py --quick    # 50k lines, fewer runs
  python3 benchmarks/bench_simple_cases.py --lines N  # custom dataset size
  python3 benchmarks/bench_simple_cases.py --update-baseline
  python3 benchmarks/bench_simple_cases.py --filter parse,filter   # categories
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

BENCH_DIR = Path(__file__).resolve().parent
REPO_ROOT = BENCH_DIR.parent
BINARY = REPO_ROOT / "target" / "release" / "kelora"
RESULTS_FILE = BENCH_DIR / "simple_cases_results.json"
BASELINE_FILE = BENCH_DIR / "simple_cases_baseline.json"

# ANSI colors (suppressed when not a tty)
if sys.stderr.isatty():
    RED, GREEN, YELLOW, BLUE, DIM, NC = (
        "\033[0;31m",
        "\033[0;32m",
        "\033[1;33m",
        "\033[0;34m",
        "\033[2m",
        "\033[0m",
    )
else:
    RED = GREEN = YELLOW = BLUE = DIM = NC = ""

LEVELS = ["DEBUG", "INFO", "WARN", "ERROR"]  # index % 4 -> ~25% ERROR
COMPONENTS = ["api", "database", "auth", "cache", "scheduler"]
WIDE_EXTRA = 35  # padding fields for "wide" events => ~40 total


# --------------------------------------------------------------------------
# Deterministic data generation (uniform schema for controlled comparisons)
# --------------------------------------------------------------------------
def _record(i, wide):
    """Build an ordered dict of fields for event i. Uniform schema per width."""
    sec = i
    rec = {
        "timestamp": f"2024-01-01T00:00:{sec % 60:02d}Z"
        if i < 60
        else f"2024-01-01T{(sec // 3600) % 24:02d}:{(sec // 60) % 60:02d}:{sec % 60:02d}Z",
        "level": LEVELS[i % 4],
        "component": COMPONENTS[i % len(COMPONENTS)],
        "message": f"Operation {i} completed successfully",
        "status": 200 + (i % 5) * 100,  # 200..600
    }
    if wide:
        for j in range(WIDE_EXTRA):
            if j % 2 == 0:
                rec[f"field{j:02d}"] = f"value-{(i + j) % 997}"
            else:
                rec[f"field{j:02d}"] = (i * (j + 1)) % 100000
    return rec


def _emit_jsonl(path, n, wide, ts_field="timestamp"):
    with open(path, "w") as f:
        for i in range(n):
            rec = _record(i, wide)
            if ts_field != "timestamp":
                rec = {(ts_field if k == "timestamp" else k): v for k, v in rec.items()}
            f.write(json.dumps(rec, separators=(",", ":")))
            f.write("\n")


def _logfmt_val(v):
    s = str(v)
    if any(c in s for c in " =\"") or s == "":
        return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return s


def _emit_logfmt(path, n, wide):
    with open(path, "w") as f:
        for i in range(n):
            rec = _record(i, wide)
            f.write(" ".join(f"{k}={_logfmt_val(v)}" for k, v in rec.items()))
            f.write("\n")


def _emit_csv(path, n, wide):
    keys = list(_record(0, wide).keys())
    with open(path, "w") as f:
        f.write(",".join(keys))
        f.write("\n")
        for i in range(n):
            rec = _record(i, wide)
            row = []
            for k in keys:
                s = str(rec[k])
                if "," in s or '"' in s:
                    s = '"' + s.replace('"', '""') + '"'
                row.append(s)
            f.write(",".join(row))
            f.write("\n")


def _emit_line(path, n):
    """Plain text lines for parse-line and grep-like substring search."""
    with open(path, "w") as f:
        for i in range(n):
            lvl = LEVELS[i % 4]
            comp = COMPONENTS[i % len(COMPONENTS)]
            f.write(
                f"2024-01-01T00:00:00Z host-{i % 10} {comp}[{lvl}]: "
                f"Operation {i} completed successfully\n"
            )


def dataset_specs(n):
    """Map logical dataset name -> (path, builder-callable)."""
    return {
        "narrow_json": (BENCH_DIR / f"simple_narrow_{n}.jsonl", lambda p: _emit_jsonl(p, n, False)),
        "wide_json": (BENCH_DIR / f"simple_wide_{n}.jsonl", lambda p: _emit_jsonl(p, n, True)),
        "narrow_logfmt": (BENCH_DIR / f"simple_narrow_{n}.logfmt", lambda p: _emit_logfmt(p, n, False)),
        "narrow_csv": (BENCH_DIR / f"simple_narrow_{n}.csv", lambda p: _emit_csv(p, n, False)),
        "narrow_line": (BENCH_DIR / f"simple_narrow_{n}.txt", lambda p: _emit_line(p, n)),
        "nots_json": (BENCH_DIR / f"simple_nots_{n}.jsonl", lambda p: _emit_jsonl(p, n, False, ts_field="tstamp")),
    }


def ensure_datasets(n):
    specs = dataset_specs(n)
    for name, (path, build) in specs.items():
        if not path.exists() or path.stat().st_size == 0:
            print(f"{YELLOW}Generating {path.name} ({n} lines)...{NC}", file=sys.stderr)
            build(path)
    return {name: path for name, (path, _) in specs.items()}


# --------------------------------------------------------------------------
# Scenario matrix.  Each scenario: name, category, dataset key, extra args.
# Output is always discarded; -q isolates parse/filter from formatting/write.
# Paired scenarios (see notes) let a delta attribute cost to one stage.
# --------------------------------------------------------------------------
def scenarios():
    return [
        # --- Parsing throughput per format (output suppressed) ---
        ("parse_json_narrow", "parse", "narrow_json", ["-f", "json", "-q"],
         "JSON parse, 5 fields"),
        ("parse_json_wide", "parse", "wide_json", ["-f", "json", "-q"],
         "JSON parse, ~40 fields"),
        ("parse_logfmt", "parse", "narrow_logfmt", ["-f", "logfmt", "-q"],
         "logfmt parse, 5 fields"),
        ("parse_csv", "parse", "narrow_csv", ["-f", "csv", "-q"],
         "CSV parse, 5 fields"),
        ("parse_line", "parse", "narrow_line", ["-f", "line", "-q"],
         "raw line, no structured parse"),

        # --- Filter cost: native fast-path vs Rhai fallback x narrow vs wide ---
        # delta((rhai_wide-rhai_narrow) - (native_wide-native_narrow))
        #   ~= cost of cloning all fields into the Rhai event map.
        ("filter_native_narrow", "filter", "narrow_json",
         ["-f", "json", "-q", "--filter", 'e.level == "ERROR"'],
         "native predicate, 5 fields"),
        ("filter_native_wide", "filter", "wide_json",
         ["-f", "json", "-q", "--filter", 'e.level == "ERROR"'],
         "native predicate, ~40 fields"),
        ("filter_rhai_narrow", "filter", "narrow_json",
         ["-f", "json", "-q", "--filter", "e.message.len() > 5"],
         "Rhai VM (len()), 5 fields"),
        ("filter_rhai_wide", "filter", "wide_json",
         ["-f", "json", "-q", "--filter", "e.message.len() > 5"],
         "Rhai VM (len()), ~40 fields  <- map-clone cost"),

        # --- Output / formatting cost (full output, pass-all) ---
        ("output_quiet", "output", "narrow_json", ["-f", "json", "-q"],
         "floor: parse only, no format"),
        ("output_default", "output", "narrow_json", ["-f", "json", "--no-color"],
         "default formatter"),
        ("output_json", "output", "narrow_json", ["-f", "json", "-F", "json"],
         "JSON output"),
        ("output_logfmt", "output", "narrow_json", ["-f", "json", "-F", "logfmt"],
         "logfmt output"),

        # --- Filter selectivity (full default output) ---
        ("select_high", "select", "narrow_json",
         ["-f", "json", "--no-color", "--filter", 'e.level != "NONE"'],
         "passes ~100% (output-bound)"),
        ("select_low", "select", "narrow_json",
         ["-f", "json", "--no-color", "--filter", 'e.level == "ERROR"'],
         "passes ~25%"),

        # --- Auto timestamp extraction on vs off ---
        ("ts_on", "timestamp", "narrow_json", ["-f", "json", "-q"],
         "field 'timestamp' -> parsed"),
        ("ts_off", "timestamp", "nots_json", ["-f", "json", "-q"],
         "field 'tstamp' -> not parsed"),

        # --- grep-like substring search ---
        ("search_substr", "search", "narrow_line",
         ["-f", "line", "-q", "--filter", 'e.line.contains("ERROR")'],
         "substring match on raw lines"),
    ]


# --------------------------------------------------------------------------
# Timing
# --------------------------------------------------------------------------
def validate(name, args, dataset_path):
    """Run once capturing output; ensure exit code 0 so we never time errors."""
    cmd = [str(BINARY), *args, str(dataset_path)]
    proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        err = proc.stderr.decode(errors="replace").strip().splitlines()
        msg = err[0] if err else f"exit {proc.returncode}"
        print(f"{RED}  ! {name}: command failed ({msg}){NC}", file=sys.stderr)
        print(f"{DIM}    cmd: {' '.join(cmd)}{NC}", file=sys.stderr)
        return False
    return True


def time_run(args, dataset_path):
    cmd = [str(BINARY), *args, str(dataset_path)]
    start = time.perf_counter()
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return time.perf_counter() - start


def run_scenario(name, args, dataset_path, n, runs):
    time_run(args, dataset_path)  # warmup (page cache, etc.)
    times = [time_run(args, dataset_path) for _ in range(runs)]
    median = statistics.median(times)
    size_mb = dataset_path.stat().st_size / 1_000_000
    return {
        "name": name,
        "median_s": median,
        "min_s": min(times),
        "max_s": max(times),
        "lines_per_s": n / median if median > 0 else 0,
        "mb_per_s": size_mb / median if median > 0 else 0,
        "runs": runs,
    }


# --------------------------------------------------------------------------
# Reporting
# --------------------------------------------------------------------------
def load_baseline():
    if BASELINE_FILE.exists():
        try:
            data = json.loads(BASELINE_FILE.read_text())
            return {r["name"]: r for r in data.get("results", [])}
        except (json.JSONDecodeError, KeyError):
            return {}
    return {}


def fmt_throughput(r):
    lps = r["lines_per_s"]
    if lps >= 1_000_000:
        lstr = f"{lps / 1_000_000:.2f}M"
    else:
        lstr = f"{lps / 1_000:.0f}k"
    return f"{lstr} lines/s  {r['mb_per_s']:.0f} MB/s"


def compare(r, baseline):
    """Return colored delta string (higher lines/s is better)."""
    b = baseline.get(r["name"])
    if not b:
        return f"{DIM}(no baseline){NC}"
    base = b.get("lines_per_s", 0)
    if base <= 0:
        return f"{DIM}(no baseline){NC}"
    change = (r["lines_per_s"] - base) / base * 100
    if change > 5:
        return f"{GREEN}+{change:.1f}% faster{NC}"
    if change < -10:
        return f"{RED}{change:.1f}% SLOWER{NC}"
    return f"{change:+.1f}%"


def main():
    ap = argparse.ArgumentParser(description="Kelora simple-cases benchmark suite")
    ap.add_argument("--quick", action="store_true", help="50k lines, fewer runs")
    ap.add_argument("--lines", type=int, default=None, help="dataset size (lines)")
    ap.add_argument("--runs", type=int, default=None, help="timed runs per scenario")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--filter", default=None,
                    help="comma-separated categories to run "
                         "(parse,filter,output,select,timestamp,search)")
    args = ap.parse_args()

    if not BINARY.exists():
        print(f"{RED}Error: binary not found at {BINARY}{NC}", file=sys.stderr)
        print("Run: cargo build --release", file=sys.stderr)
        sys.exit(1)

    n = args.lines if args.lines else (50_000 if args.quick else 200_000)
    runs = args.runs if args.runs else (3 if args.quick else 5)
    cats = set(args.filter.split(",")) if args.filter else None

    print(f"{GREEN}=== Kelora simple-cases benchmark ==={NC}", file=sys.stderr)
    print(f"binary: {BINARY}", file=sys.stderr)
    print(f"lines:  {n}   timed runs: {runs}", file=sys.stderr)
    print("", file=sys.stderr)

    datasets = ensure_datasets(n)
    scn = [s for s in scenarios() if cats is None or s[1] in cats]

    # Validation pass: never time a command that errors.
    print(f"{BLUE}Validating commands...{NC}", file=sys.stderr)
    ok = True
    for name, _cat, dkey, sargs, _note in scn:
        if not validate(name, sargs, datasets[dkey]):
            ok = False
    if not ok:
        print(f"{RED}Aborting: one or more scenarios failed validation.{NC}", file=sys.stderr)
        sys.exit(1)
    print("", file=sys.stderr)

    baseline = load_baseline()
    results = []
    last_cat = None
    for name, cat, dkey, sargs, note in scn:
        if cat != last_cat:
            print(f"{BLUE}[{cat}]{NC}", file=sys.stderr)
            last_cat = cat
        r = run_scenario(name, sargs, datasets[dkey], n, runs)
        r["category"] = cat
        r["note"] = note
        results.append(r)
        print(
            f"  {name:<22} {fmt_throughput(r):<28} "
            f"{r['median_s']*1000:7.1f} ms   {compare(r, baseline):<18} {DIM}{note}{NC}",
            file=sys.stderr,
        )

    payload = {
        "lines": n,
        "runs": runs,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "results": results,
    }
    RESULTS_FILE.write_text(json.dumps(payload, indent=2))
    print(f"\n{GREEN}Results written to {RESULTS_FILE}{NC}", file=sys.stderr)

    if args.update_baseline:
        BASELINE_FILE.write_text(json.dumps(payload, indent=2))
        print(f"{GREEN}Baseline updated: {BASELINE_FILE}{NC}", file=sys.stderr)
    elif not baseline:
        print(f"{YELLOW}No baseline yet. Run with --update-baseline to set one.{NC}",
              file=sys.stderr)


if __name__ == "__main__":
    main()
