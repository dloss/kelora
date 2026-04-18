# Benchmark Results

This page stores **raw benchmark snapshots** (machine + date + tool versions + command outputs).

If you want interpretation or tool-choice advice, see [Performance Comparisons](performance-comparisons.md).

## How to Read This Page

- Treat each section as a point-in-time snapshot, not a universal ranking.
- Compare runs only when dataset, commands, and versions are aligned.
- Prefer ratios (tool A vs tool B on same host) over absolute wall-clock numbers.

## Snapshot Format (Use This for New Entries)

When adding a new run, append a section with:

1. **Date (UTC)**
2. **Git commit**
3. **Hardware + OS**
4. **Tool versions**
5. **Scenario table** (copy directly from `benchmarks/comparison_results/*.md`)
6. **Notes** (anything unusual: thermal throttling, background load, etc.)

### Minimal template

```markdown
## System: <machine-name> (<YYYY-MM-DD>)

- Commit: `<sha>`
- CPU: <model>
- Cores/threads: <value>
- RAM: <value>
- OS: <value>
- Tool versions: <list>

### Results

| Scenario | Tool | Time | Throughput | Notes |
|---|---|---:|---:|---|
| ... | ... | ... | ... | ... |

### Notes

- ...
```

## Current Snapshots

### Apple M1 (2024-10-26)

- Commit: development build at measurement time
- OS: macOS (Darwin)
- CPU/RAM: Apple M1, 8 cores (4P+4E), 16 GB
- Tools: jq 1.7.1, ripgrep 14.1.0, BSD awk, miller 6.12.0, qsv 0.131.0

Source artifacts: `benchmarks/comparison_results/01_simple_filter.md` through `07_csv.md` (from that run).

### Intel Core i5 (2025-10-26)

- Commit: `09faf43`
- OS: macOS 15.6.1 (`x86_64`)
- CPU/RAM: Intel Core i5 @ 3.0 GHz, 6 cores, 16 GB
- Tools: grep 2.6.0, ripgrep 15.1.0, jq 1.6, mlr 6.15.0, qsv 8.1.1, angle-grinder 0.19.5, klp 0.77.0

Source artifacts: `benchmarks/comparison_results/01_simple_filter.md` through `07_csv.md` (from that run).

## Add or Refresh a Snapshot

```bash
cargo build --release
./benchmarks/generate_comparison_data.sh
just bench-compare
```

Then copy the generated markdown tables from `benchmarks/comparison_results/` into a new section using the template above.
