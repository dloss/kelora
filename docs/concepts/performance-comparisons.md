# Performance Comparisons

This page is now intentionally short.

Use it to answer one question quickly: **Should I use Kelora for this job, or pair it with a specialized tool?**

For raw numbers and per-machine snapshots, see [Benchmark Results](benchmark-results.md).

## TL;DR Decision Guide

| If your primary goal is… | Usually best first tool | Use Kelora when… |
|---|---|---|
| Fast plain-text matching | `rg` / `grep` | You need structured fields, scripting, or metrics in the same run. |
| Pure JSON filtering/transforms | `jq` | You need multi-stage logic, built-in tracking/windowing, or mixed formats. |
| Pure CSV filtering/projection | `qsv` / `mlr` | Your pipeline also needs Rhai logic or non-CSV inputs/outputs. |
| One maintainable end-to-end pipeline | Kelora | You prefer one script/command over multi-tool shell pipelines. |

## Honest Performance Positioning

Kelora is generally slower than narrow, single-purpose tools on micro-benchmarks.
That is expected: Kelora spends CPU on structured parsing, Rhai execution, and optional metrics/window features.

Kelora usually wins on **pipeline simplicity and expressiveness**, especially when requirements evolve beyond a one-liner.

## Fast-Path Tuning Checklist

When using Kelora and performance matters, apply these first:

1. Prefer native selectors (`--levels`, `--keep-lines`, format-specific flags) before generic Rhai filters.
2. Suppress non-essential output during throughput runs (`--silent`, `--no-diagnostics`, `-q` where applicable).
3. Use `--parallel` for large CPU-bound batches and tune `--batch-size` for your workload.
4. Project only needed fields (`-k`) and avoid expensive per-event regex when simpler filters work.
5. Benchmark with release builds and realistic data sizes.

For deeper rationale, see [Performance Model](performance-model.md).

## Better Benchmarking Workflow (Recommended)

The old style (very long doc tables that age quickly) is replaced by a **two-layer model**:

- **Layer 1 (this page):** Stable guidance and tool-selection heuristics.
- **Layer 2 ([Benchmark Results](benchmark-results.md)):** Time-stamped raw snapshots tied to machine + commit.

This keeps guidance readable while preserving reproducibility.

## Reproducing Comparisons

```bash
cargo build --release
./benchmarks/generate_comparison_data.sh
just bench-compare
```

The runner writes markdown artifacts to `benchmarks/comparison_results/`.
Use those files as the source of truth when updating [Benchmark Results](benchmark-results.md).

## Related Documentation

- [Performance Model](performance-model.md) - internal levers (`--parallel`, batching, output costs)
- [Benchmark Results](benchmark-results.md) - raw snapshots by hardware/date
- [How-To: Batch Process Archives](../how-to/batch-process-archives.md) - practical large-file patterns
