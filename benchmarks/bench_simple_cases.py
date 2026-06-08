#!/usr/bin/env python3
"""
Kelora "common simple cases" benchmark harness.

Measures the per-event hot path (read -> parse -> filter -> format -> write)
for the cases real users hit most often, and reports THROUGHPUT
(lines/s and MB/s) rather than raw wall-time so regressions are legible
across machines and dataset sizes.

This suite is deliberately separate from run_benchmarks.sh (the regression
suite): it generates its own uniform-schema datasets so that field-count
comparisons are controlled, and it isolates individual pipeline stages with
paired scenarios whose *delta* attributes cost to a specific stage.

No external timing tools required (no gtime/hyperfine): timing uses
time.perf_counter() around subprocess.run with output sent to /dev/null.

Usage:
  python3 benchmarks/bench_simple_cases.py            # full run (100k lines)
  python3 benchmarks/bench_simple_cases.py --quick    # 50k lines, fewer runs
  python3 benchmarks/bench_simple_cases.py --lines N  # custom dataset size
  python3 benchmarks/bench_simple_cases.py --filter width,filter   # categories
  python3 benchmarks/bench_simple_cases.py --update-baseline
  python3 benchmarks/bench_simple_cases.py --compare  # vs jq/grep/rg (informational)
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

if sys.stderr.isatty():
    RED, GREEN, YELLOW, BLUE, DIM, NC = (
        "\033[0;31m", "\033[0;32m", "\033[1;33m", "\033[0;34m", "\033[2m", "\033[0m",
    )
else:
    RED = GREEN = YELLOW = BLUE = DIM = NC = ""

LEVELS = ["DEBUG", "INFO", "WARN", "ERROR"]  # index % 4 -> ~25% ERROR
COMPONENTS = ["api", "database", "auth", "cache", "scheduler"]

# Width-curve points. 5 and 40 are reused by the narrow/wide datasets.
CURVE_WIDTHS = [5, 8, 12, 20, 40]
FILTER_NATIVE = 'e.level == "ERROR"'  # hits the native fast-path
FILTER_RHAI = "e.message.len() > 5"   # forces the Rhai VM


# --------------------------------------------------------------------------
# Deterministic data generation
# --------------------------------------------------------------------------
def _record(i, nfields):
    """Flat event with exactly `nfields` fields (>=5). Base 5 + padding."""
    rec = {
        "timestamp": f"2024-01-01T{(i // 3600) % 24:02d}:{(i // 60) % 60:02d}:{i % 60:02d}Z",
        "level": LEVELS[i % 4],
        "component": COMPONENTS[i % len(COMPONENTS)],
        "message": f"Operation {i} completed successfully",
        "status": 200 + (i % 5) * 100,
    }
    j = 0
    while len(rec) < nfields:
        rec[f"field{j:02d}"] = f"value-{(i + j) % 997}" if j % 2 == 0 else (i * (j + 1)) % 100000
        j += 1
    return rec


def _record_nested(i):
    """~40 leaf values, but nested under objects + an array (realistic shape)."""
    return {
        "timestamp": f"2024-01-01T00:00:{i % 60:02d}Z",
        "level": LEVELS[i % 4],
        "message": f"Operation {i} completed successfully",
        "http": {
            "method": ["GET", "POST", "PUT"][i % 3],
            "path": f"/api/v1/resource/{i % 100}",
            "status": 200 + (i % 5) * 100,
            "headers": {f"h{k:02d}": f"v{(i + k) % 97}" for k in range(8)},
        },
        "kv": {f"k{k:02d}": (i * (k + 1)) % 100000 for k in range(20)},
        "tags": [f"tag{(i + k) % 17}" for k in range(6)],
    }


def _record_longval(i):
    """Narrow (5 fields) but with a long message value (~400 chars)."""
    rec = _record(i, 5)
    rec["message"] = f"Operation {i}: " + ("lorem ipsum dolor sit amet " * 14)
    return rec


def _emit_jsonl(path, n, builder, ts_field="timestamp"):
    with open(path, "w") as f:
        for i in range(n):
            rec = builder(i)
            if ts_field != "timestamp":
                rec = {(ts_field if k == "timestamp" else k): v for k, v in rec.items()}
            f.write(json.dumps(rec, separators=(",", ":")))
            f.write("\n")


def _logfmt_val(v):
    s = str(v)
    if any(c in s for c in ' ="') or s == "":
        return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return s


def _emit_logfmt(path, n):
    with open(path, "w") as f:
        for i in range(n):
            f.write(" ".join(f"{k}={_logfmt_val(v)}" for k, v in _record(i, 5).items()))
            f.write("\n")


def _emit_csv(path, n):
    keys = list(_record(0, 5).keys())
    with open(path, "w") as f:
        f.write(",".join(keys) + "\n")
        for i in range(n):
            rec = _record(i, 5)
            row = []
            for k in keys:
                s = str(rec[k])
                if "," in s or '"' in s:
                    s = '"' + s.replace('"', '""') + '"'
                row.append(s)
            f.write(",".join(row) + "\n")


def _emit_line(path, n):
    with open(path, "w") as f:
        for i in range(n):
            lvl, comp = LEVELS[i % 4], COMPONENTS[i % len(COMPONENTS)]
            f.write(f"2024-01-01T00:00:00Z host-{i % 10} {comp}[{lvl}]: "
                    f"Operation {i} completed successfully\n")


def dataset_specs(n):
    specs = {
        "narrow_json": (f"simple_narrow_{n}.jsonl", lambda p: _emit_jsonl(p, n, lambda i: _record(i, 5))),
        "wide_json": (f"simple_wide_{n}.jsonl", lambda p: _emit_jsonl(p, n, lambda i: _record(i, 40))),
        "narrow_logfmt": (f"simple_narrow_{n}.logfmt", lambda p: _emit_logfmt(p, n)),
        "narrow_csv": (f"simple_narrow_{n}.csv", lambda p: _emit_csv(p, n)),
        "narrow_line": (f"simple_narrow_{n}.txt", lambda p: _emit_line(p, n)),
        "nots_json": (f"simple_nots_{n}.jsonl",
                      lambda p: _emit_jsonl(p, n, lambda i: _record(i, 5), ts_field="tstamp")),
        "nested_json": (f"simple_nested_{n}.jsonl", lambda p: _emit_jsonl(p, n, _record_nested)),
        "longval_json": (f"simple_longval_{n}.jsonl", lambda p: _emit_jsonl(p, n, _record_longval)),
    }
    # Width-curve datasets (5 and 40 reuse narrow/wide).
    for w in CURVE_WIDTHS:
        if w in (5, 40):
            continue
        specs[f"w{w}_json"] = (f"simple_w{w}_{n}.jsonl",
                               lambda p, w=w: _emit_jsonl(p, n, lambda i: _record(i, w)))
    return {name: (BENCH_DIR / fn, build) for name, (fn, build) in specs.items()}


def width_dataset_key(w):
    return {5: "narrow_json", 40: "wide_json"}.get(w, f"w{w}_json")


def ensure_datasets(n, needed=None):
    specs = dataset_specs(n)
    for name, (path, build) in specs.items():
        if needed is not None and name not in needed:
            continue
        if not path.exists() or path.stat().st_size == 0:
            print(f"{YELLOW}Generating {path.name} ({n} lines)...{NC}", file=sys.stderr)
            build(path)
    return {name: path for name, (path, _) in specs.items()}


# --------------------------------------------------------------------------
# Scenario matrix. (name, category, dataset key, extra args, note)
# Output suppressed with -q except where formatting itself is measured.
# --------------------------------------------------------------------------
def scenarios():
    scn = [
        # --- Parse throughput per format ---
        ("parse_json_narrow", "parse", "narrow_json", ["-f", "json", "-q"], "JSON parse, 5 fields"),
        ("parse_json_wide", "parse", "wide_json", ["-f", "json", "-q"], "JSON parse, 40 fields"),
        ("parse_logfmt", "parse", "narrow_logfmt", ["-f", "logfmt", "-q"], "logfmt parse, 5 fields"),
        ("parse_csv", "parse", "narrow_csv", ["-f", "csv", "-q"], "CSV parse, 5 fields"),
        ("parse_line", "parse", "narrow_line", ["-f", "line", "-q"], "raw line, no structured parse"),
    ]

    # --- Width curve: how throughput scales with field count ---
    for w in CURVE_WIDTHS:
        dk = width_dataset_key(w)
        scn.append((f"width_parse_{w:02d}", "width", dk, ["-f", "json", "-q"],
                    f"parse, {w} fields"))
        scn.append((f"width_filter_{w:02d}", "width", dk,
                    ["-f", "json", "-q", "--filter", FILTER_NATIVE],
                    f"native filter, {w} fields"))

    scn += [
        # --- Native fast-path vs Rhai VM, narrow vs wide ---
        ("filter_native_narrow", "filter", "narrow_json",
         ["-f", "json", "-q", "--filter", FILTER_NATIVE], "native predicate, 5 fields"),
        ("filter_native_wide", "filter", "wide_json",
         ["-f", "json", "-q", "--filter", FILTER_NATIVE], "native predicate, 40 fields"),
        ("filter_rhai_narrow", "filter", "narrow_json",
         ["-f", "json", "-q", "--filter", FILTER_RHAI], "Rhai VM (len()), 5 fields"),
        ("filter_rhai_wide", "filter", "wide_json",
         ["-f", "json", "-q", "--filter", FILTER_RHAI], "Rhai VM (len()), 40 fields"),

        # --- exec stage (mutate/add a field) ---
        ("exec_narrow", "exec", "narrow_json",
         ["-f", "json", "-q", "--exec", "e.big = e.message.len()"], "exec computed field, 5 fields"),
        ("exec_wide", "exec", "wide_json",
         ["-f", "json", "-q", "--exec", "e.big = e.message.len()"], "exec computed field, 40 fields"),

        # --- parallel mode (compare to sequential filter_native_*) ---
        ("parallel_narrow", "parallel", "narrow_json",
         ["-f", "json", "-q", "--parallel", "--threads", "4", "--filter", FILTER_NATIVE],
         "4 threads, 5 fields (vs filter_native_narrow)"),
        ("parallel_wide", "parallel", "wide_json",
         ["-f", "json", "-q", "--parallel", "--threads", "4", "--filter", FILTER_NATIVE],
         "4 threads, 40 fields (vs filter_native_wide)"),

        # --- Output / formatting cost (full output) ---
        ("output_quiet", "output", "narrow_json", ["-f", "json", "-q"], "floor: parse only, no format"),
        ("output_default", "output", "narrow_json", ["-f", "json", "--no-color"], "default formatter"),
        ("output_json", "output", "narrow_json", ["-f", "json", "-F", "json"], "JSON output"),
        ("output_logfmt", "output", "narrow_json", ["-f", "json", "-F", "logfmt"], "logfmt output"),

        # --- Selectivity ---
        ("select_high", "select", "narrow_json",
         ["-f", "json", "--no-color", "--filter", 'e.level != "NONE"'], "passes ~100% (output-bound)"),
        ("select_low", "select", "narrow_json",
         ["-f", "json", "--no-color", "--filter", FILTER_NATIVE], "passes ~25%"),

        # --- Auto timestamp extraction ---
        ("ts_on", "timestamp", "narrow_json", ["-f", "json", "-q"], "field 'timestamp' -> parsed"),
        ("ts_off", "timestamp", "nots_json", ["-f", "json", "-q"], "field 'tstamp' -> not parsed"),

        # --- Event shape: nested vs flat, long values ---
        ("shape_flat_wide", "shape", "wide_json", ["-f", "json", "-q"], "flat, 40 fields (reference)"),
        ("shape_nested", "shape", "nested_json", ["-f", "json", "-q"], "~40 leaves, nested objects+array"),
        ("shape_longval", "shape", "longval_json", ["-f", "json", "-q"], "5 fields, ~400-char message"),

        # --- grep-like substring search ---
        ("search_substr", "search", "narrow_line",
         ["-f", "line", "-q", "--filter", 'e.line.contains("ERROR")'], "substring match on raw lines"),
    ]
    return scn


def datasets_used(scn):
    return {dk for _n, _c, dk, _a, _note in scn}


# --------------------------------------------------------------------------
# Timing
# --------------------------------------------------------------------------
def validate(name, args, dataset_path):
    cmd = [str(BINARY), *args, str(dataset_path)]
    proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        err = proc.stderr.decode(errors="replace").strip().splitlines()
        msg = err[0] if err else f"exit {proc.returncode}"
        print(f"{RED}  ! {name}: command failed ({msg}){NC}", file=sys.stderr)
        print(f"{DIM}    cmd: {' '.join(cmd)}{NC}", file=sys.stderr)
        return False
    return True


def _median_time(cmd, runs, stdin_path=None):
    def once():
        stdin = open(stdin_path, "rb") if stdin_path else subprocess.DEVNULL
        try:
            start = time.perf_counter()
            subprocess.run(cmd, stdin=stdin, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True)
            return time.perf_counter() - start
        finally:
            if stdin_path:
                stdin.close()
    once()  # warmup
    return statistics.median(once() for _ in range(runs))


def run_scenario(name, args, dataset_path, n, runs):
    cmd = [str(BINARY), *args, str(dataset_path)]
    median = _median_time(cmd, runs)
    size_mb = dataset_path.stat().st_size / 1_000_000
    return {
        "name": name,
        "median_s": median,
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
            return {r["name"]: r for r in json.loads(BASELINE_FILE.read_text()).get("results", [])}
        except (json.JSONDecodeError, KeyError):
            return {}
    return {}


def fmt_throughput(lps, mbs=None):
    lstr = f"{lps / 1_000_000:.2f}M" if lps >= 1_000_000 else f"{lps / 1_000:.0f}k"
    return f"{lstr} lines/s" + (f"  {mbs:.0f} MB/s" if mbs is not None else "")


def compare(r, baseline):
    b = baseline.get(r["name"])
    base = b.get("lines_per_s", 0) if b else 0
    if base <= 0:
        return f"{DIM}(no baseline){NC}"
    change = (r["lines_per_s"] - base) / base * 100
    if change > 5:
        return f"{GREEN}+{change:.1f}% faster{NC}"
    if change < -10:
        return f"{RED}{change:.1f}% SLOWER{NC}"
    return f"{change:+.1f}%"


# --------------------------------------------------------------------------
# External-tool comparison (informational; not part of the baseline)
# --------------------------------------------------------------------------
def have(tool):
    return subprocess.run(["which", tool], capture_output=True).returncode == 0


def run_compare(n):
    print(f"{GREEN}=== External comparison (filter level==ERROR){NC}", file=sys.stderr)
    print(f"{DIM}kelora is field-aware; grep/rg do plain substring (not equivalent, shown as floor){NC}",
          file=sys.stderr)
    runs = 3
    points = [("12 fields", 12), ("40 fields", 40)]
    ds = ensure_datasets(n, needed={width_dataset_key(w) for _l, w in points})
    print(f"\n{'dataset':<12} {'kelora':>14} {'jq':>14} {'rg':>14} {'grep':>14}", file=sys.stderr)
    for label, w in points:
        path = ds[width_dataset_key(w)]
        out = {}
        kcmd = [str(BINARY), "-f", "json", "-q", "--filter", FILTER_NATIVE, str(path)]
        out["kelora"] = n / _median_time(kcmd, runs)
        if have("jq"):
            out["jq"] = n / _median_time(["jq", "-c", 'select(.level=="ERROR")'], runs, stdin_path=path)
        if have("rg"):
            out["rg"] = n / _median_time(["rg", "ERROR"], runs, stdin_path=path)
        if have("grep"):
            out["grep"] = n / _median_time(["grep", "ERROR"], runs, stdin_path=path)

        def cell(k):
            return fmt_throughput(out[k]) if k in out else "n/a"
        print(f"{label:<12} {cell('kelora'):>14} {cell('jq'):>14} "
              f"{cell('rg'):>14} {cell('grep'):>14}", file=sys.stderr)
    print("", file=sys.stderr)


def main():
    ap = argparse.ArgumentParser(description="Kelora simple-cases benchmark suite")
    ap.add_argument("--quick", action="store_true", help="50k lines, fewer runs")
    ap.add_argument("--lines", type=int, default=None, help="dataset size (lines)")
    ap.add_argument("--runs", type=int, default=None, help="timed runs per scenario")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--filter", default=None,
                    help="comma-separated categories "
                         "(parse,width,filter,exec,parallel,output,select,timestamp,shape,search)")
    ap.add_argument("--compare", action="store_true",
                    help="also run external comparison vs jq/grep/rg (informational)")
    args = ap.parse_args()

    if not BINARY.exists():
        print(f"{RED}Error: binary not found at {BINARY}{NC}\nRun: cargo build --release",
              file=sys.stderr)
        sys.exit(1)

    n = args.lines if args.lines else (50_000 if args.quick else 100_000)
    runs = args.runs if args.runs else (3 if args.quick else 5)
    cats = set(args.filter.split(",")) if args.filter else None

    print(f"{GREEN}=== Kelora simple-cases benchmark ==={NC}", file=sys.stderr)
    print(f"binary: {BINARY}\nlines:  {n}   timed runs: {runs}\n", file=sys.stderr)

    scn = [s for s in scenarios() if cats is None or s[1] in cats]
    datasets = ensure_datasets(n, needed=datasets_used(scn))

    print(f"{BLUE}Validating commands...{NC}", file=sys.stderr)
    if not all(validate(name, sargs, datasets[dk]) for name, _c, dk, sargs, _note in scn):
        print(f"{RED}Aborting: one or more scenarios failed validation.{NC}", file=sys.stderr)
        sys.exit(1)
    print("", file=sys.stderr)

    baseline = load_baseline()
    results = []
    last_cat = None
    for name, cat, dk, sargs, note in scn:
        if cat != last_cat:
            print(f"{BLUE}[{cat}]{NC}", file=sys.stderr)
            last_cat = cat
        r = run_scenario(name, sargs, datasets[dk], n, runs)
        r["category"], r["note"] = cat, note
        results.append(r)
        print(f"  {name:<22} {fmt_throughput(r['lines_per_s'], r['mb_per_s']):<28} "
              f"{r['median_s']*1000:7.1f} ms   {compare(r, baseline):<18} {DIM}{note}{NC}",
              file=sys.stderr)

    payload = {
        "lines": n, "runs": runs,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "results": results,
    }
    RESULTS_FILE.write_text(json.dumps(payload, indent=2))
    print(f"\n{GREEN}Results written to {RESULTS_FILE}{NC}", file=sys.stderr)

    if args.update_baseline:
        BASELINE_FILE.write_text(json.dumps(payload, indent=2))
        print(f"{GREEN}Baseline updated: {BASELINE_FILE}{NC}", file=sys.stderr)
    elif not baseline:
        print(f"{YELLOW}No baseline yet. Run with --update-baseline to set one.{NC}", file=sys.stderr)

    if args.compare:
        print("", file=sys.stderr)
        run_compare(n)


if __name__ == "__main__":
    main()
