# Benchmarks

How Kelora compares with specialized tools on common log tasks. Single-purpose
tools are usually faster at their one job; Kelora's numbers vary with the task —
built-in options are quick, scripts that run on every event are slower. These
are single runs on one machine, so read them as rough orders of magnitude, and
combine tools: `rg` in front of Kelora, `jq` or `qsv` behind it.

| Your main job | Fastest tool | Kelora is worth it when |
|---|---|---|
| finding text | `rg`, `grep` | you need fields, scripts, or counts in the same run |
| filtering JSON | `jq`, Kelora | you also need counts, windows, or non-JSON input |
| reshaping JSON | `jq` | the logic is multi-step, or you need counts, windows, or mixed formats |
| CSV analytics | `qsv`, `mlr` | the input isn't CSV, or you need Rhai logic |

## Results

Single runs, wall-clock time, 100 000 lines unless noted. Lower is better.

Machine: a 2018 Mac mini (Intel Core i5-8500B, 6 cores, 16 GB RAM), macOS 15.7,
Kelora 2.1.1 (`c5d80e2b5`), October 2026. Current hardware is considerably
faster, so compare rows with each other rather than reading the absolute times. Tools: ripgrep 15.1.0, ugrep 7.8.4 (as `grep`),
jq 1.6, mlr 6.15.0, qsv 8.1.1, angle-grinder 0.19.5, klp 0.77.0.

| Task | Kelora | Others |
|---|---:|---|
| find ERROR lines in plain text | 0.18 s | grep 0.05 s, rg 0.05 s, angle-grinder 0.34 s, klp 1.91 s |
| extract three columns from text | 0.63 s | awk 0.37 s, angle-grinder 0.34 s, klp 8.18 s |
| filter JSON by level | 0.14 s | jq 0.95 s, angle-grinder 0.34 s, klp 4.07 s |
| filter JSON, add a computed field | 0.71 s | jq 0.65 s, angle-grinder 0.22 s, klp 12.6 s |
| count errors by component | 0.78 s | jq + sort + uniq 0.59 s, angle-grinder 0.21 s, klp + sort 3.32 s |
| 500 000 JSON lines, filter and count | 2.60 s; `--parallel` 0.81 s | jq 3.95 s, angle-grinder 1.01 s, klp 29.3 s |
| filter CSV, select columns | 2.22 s | qsv 0.19 s, mlr 0.34 s |

As a rule of thumb: built-in options like `-l`, `--keep-lines`, and `--freq` are
several times faster than Rhai scripts that run on every event, and `--parallel`
gave about 3× on six cores. [Big Files, Many Files](../guide/files.md#make-it-faster)
lists what makes a run faster.

## Run them yourself

```bash
cargo build --release
./benchmarks/generate_comparison_data.sh
just bench-compare
```

The script writes one table per task to `benchmarks/comparison_results/`.
Numbers vary with hardware and tool versions; compare tools on the same
machine rather than across machines.
