# Benchmark Results

Raw performance benchmarks comparing Kelora against common command-line tools across different hardware. Throughput values are inferred directly from dataset size ÷ mean runtime (100k events per test unless noted; 500k for the parallel scenario).

!!! abstract "Looking for Guidance?"
    This page shows **raw benchmark data**. For interpretation, decision matrices, and honest performance guidance, see [Performance Comparisons](performance-comparisons.md).

---

## Test System: Apple M1 (2024-10-26)

**Hardware:**

- **CPU:** Apple M1
- **Cores:** 8 (4 performance + 4 efficiency)
- **OS:** macOS (Darwin)
- **RAM:** 16GB

**Software Versions:**

- kelora: (current development build)
- jq: 1.7.1
- ripgrep: 14.1.0
- awk: BSD awk (macOS built-in)
- miller: 6.12.0
- qsv: 0.131.0

---

### 1. Simple Text Filtering

**Task:** Find all ERROR lines in 100,000-line log file

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| grep | 0.048s | ~2.08 M/s | Baseline - fastest text search |
| ripgrep (rg) | 0.014s | ~7.14 M/s | Modern grep alternative |
| kelora (line) | 0.381s | ~0.26 M/s | Full log parsing + structured output |

**Interpretation:** grep/ripgrep are 8-27x faster for simple text matching.

---

### 2. Field Extraction

**Task:** Extract timestamp, level, component from 100,000 log lines

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| awk | 0.374s | ~0.27 M/s | Field splitting by whitespace |
| kelora (cols) | 4.723s | ~0.02 M/s | Structured parsing + type awareness |

**Interpretation:** awk is 12.6x faster for simple field extraction.

---

### 3. JSON Filtering

**Task:** Filter 100,000 JSON logs where level == 'ERROR' (25,256 matches)

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| jq | 1.056s | ~0.09 M/s | Standard JSON processor |
| kelora | 6.157s | ~0.02 M/s | JSON parsing with level filter |

**Interpretation:** jq is 5.8x faster for JSON filtering.

---

### 4. JSON Transformation

**Task:** Filter API logs, extract status code, add is_error field

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| jq | 0.763s | ~0.13 M/s | Complex jq query |
| kelora | 9.009s | ~0.01 M/s | Multi-stage pipeline |

**Interpretation:** jq is 11.8x faster for JSON transformations.

---

### 5. Complex Multi-Stage Pipeline

**Task:** Filter errors, count by component, sort by frequency

| Tool | Time | Throughput | Command Complexity |
|------|------|------------|-------------------|
| bash + jq + sort + uniq | 0.722s | ~0.14 M/s | `jq ... \| sort \| uniq -c \| sort -rn` |
| kelora | 6.786s | ~0.01 M/s | `kelora -l error --exec 'track_count(...)' --metrics` |

**Interpretation:** Multi-tool bash pipeline is 9.4x faster, but Kelora uses a single command with built-in aggregation.

---

### 6. Parallel Processing

**Task:** Process 500,000 JSON logs, filter by component

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| jq (sequential) | 4.426s | ~0.11 M/s | Single-threaded processing |
| kelora (sequential) | 42.717s | ~0.01 M/s | Single-threaded baseline |
| kelora (--parallel) | 8.143s | ~0.06 M/s | Multi-core processing (8 cores) |

**Interpretation:**

- Kelora sequential is 9.7x slower than jq
- Kelora `--parallel` provides 5.2x speedup over sequential
- Still 1.8x slower than jq overall

---

### 7. CSV Processing

**Task:** Filter CSV by level, select columns

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| miller | 0.147s | ~0.68 M/s | CSV swiss army knife |
| qsv | 0.050s | ~2.00 M/s | High-performance CSV tool |
| kelora | 5.787s | ~0.02 M/s | CSV + level filtering |

**Interpretation:** qsv is 115x faster, miller is 39x faster. Use specialized CSV tools for pure CSV work.

---

## Contributing Results

Have different hardware? Want to add Linux/Windows results? Contributions welcome!

**To add your results:**

1. Run benchmarks on your system:
   ```bash
   just bench-compare
   ```

2. Add a new section above with your system specs:
   ```markdown
   ## Test System: [Your System Name] (YYYY-MM-DD)
   ```

3. Submit a PR with your results

**Especially interested in:**

- Linux (x86_64, ARM)
- Windows (WSL, native)
- Different CPU architectures
- Older vs newer hardware comparisons

---

## Historical Results

Track performance improvements across Kelora versions:

| Version | Test | M1 Result | Intel Mac Result | Notes |
|---------|------|-----------|------------------|-------|
| 0.x.x | JSON Filter | 6.157s | 6.434s | Initial benchmark |
| (future) | | | | |

---

## Test System: Intel Core i5 (2025-10-26)

**Hardware:**

- **CPU:** Intel Core i5 (3.0 GHz, 6 cores)
- **Cores:** 6
- **OS:** macOS 15.6.1 (24G90) · `x86_64`
- **RAM:** 16 GB

**Software Versions:**

- Kelora: `target/release/kelora` @ `09faf43`
- Rust: 1.90.0
- grep: 2.6.0 (BSD)
- ripgrep: 15.1.0
- jq: 1.6
- mlr: 6.15.0
- qsv: 8.1.1
- angle-grinder: 0.19.5
- klp: 0.77.0

---

### 1. Simple Text Filtering

**Task:** Find all ERROR lines in 100,000-line log file

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| ripgrep (rg) | 0.021s | ~4.76 M/s | Modern grep alternative |
| grep | 0.049s | ~2.04 M/s | Baseline - fastest text search |
| angle-grinder | 0.327s | ~0.31 M/s | `parse "* * *[*]: *"` + `where level == "ERROR"` |
| kelora (line) | 0.405s | ~0.25 M/s | Full log parsing + structured output |
| klp | 1.952s | ~0.05 M/s | `klp --input-format line -l error` |

**Interpretation:** Kelora runs ~19× slower than ripgrep (and ~8× slower than BSD `grep`) because it builds structured events; angle-grinder lands in the middle thanks to its lightweight parser, while klp’s Python runtime lags well behind. Reach for Kelora only when you need the structured output for follow-up scripting.

---

### 2. Field Extraction

**Task:** Extract timestamp, level, component from 100,000 log lines

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.368s | ~0.27 M/s | `parse` + `fields` |
| awk | 0.389s | ~0.26 M/s | Field splitting by whitespace |
| kelora (cols) | 4.744s | ~0.02 M/s | Structured parsing + type awareness |
| klp | 6.898s | ~0.01 M/s | regex via `--input-exec`, output via `--keys` |

**Interpretation:** Kelora is ≈12× slower than awk/agrind because it validates and types every field; klp’s Python pipeline is slower still. Choose Kelora when that extra structure matters for downstream processing.

---

### 3. JSON Filtering

**Task:** Filter 100,000 JSON logs where level == 'ERROR'

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.367s | ~0.27 M/s | `json` + `where level == "ERROR"` |
| jq | 1.104s | ~0.09 M/s | Standard JSON processor |
| klp | 4.695s | ~0.02 M/s | `klp --input-format jsonl -l error` |
| kelora | 6.434s | ~0.02 M/s | JSON parsing with Rhai filter |

**Interpretation:** Kelora lags jq by ~6× (and angle-grinder by ~17×) because it executes the filter inside Rhai; klp trails for similar reasons in Python. Use Kelora when the flexibility of multi-stage filters or metrics hooks outweighs the speed hit.

---

### 4. JSON Transformation

**Task:** Filter API logs, extract status code, add is_error field

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.232s | ~0.43 M/s | `json` + computed `is_error` |
| jq | 0.822s | ~0.12 M/s | Complex jq query |
| kelora | 9.383s | ~0.01 M/s | Multi-stage pipeline |
| klp | 11.082s | ~0.01 M/s | jsonl + `-l error` + computed key |

**Interpretation:** Kelora (and klp) are 11–40× slower than jq/agrind on this tight transform, trading raw speed for the ability to express multi-stage, scriptable pipelines.

---

### 5. Complex Multi-Stage Pipeline

**Task:** Filter errors, count by component, sort by frequency

| Tool | Time | Throughput | Command Complexity |
|------|------|------------|-------------------|
| angle-grinder | 0.220s | ~0.45 M/s | `agrind '* | json | where level == "ERROR" | count by component'` |
| bash + jq + sort + uniq | 0.753s | ~0.13 M/s | `jq ... \| sort \| uniq -c \| sort -rn` |
| klp + sort + uniq | 3.851s | ~0.03 M/s | `klp -l error --output-template '{component}' \| sort \| uniq -c \| sort -rn` |
| kelora | 7.086s | ~0.01 M/s | `kelora -l error --exec 'track_count(...)' --metrics` |

**Interpretation:** Kelora is 10–30× slower than the Unix/agrind pipelines, but it keeps the whole aggregation in one command that’s easier to evolve. klp sits between those extremes when coupled with `sort | uniq`.

---

### 6. Parallel Processing

**Task:** Process 500,000 JSON logs, filter by component

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.979s | ~0.51 M/s | `json` + `where component == "api"` |
| jq (sequential) | 4.665s | ~0.11 M/s | Single-threaded processing |
| kelora (--parallel) | 8.832s | ~0.06 M/s | Multi-core processing |
| klp (--parallel 0) | 12.108s | ~0.04 M/s | Multiprocess (--parallel 0) |
| klp (sequential) | 27.122s | ~0.02 M/s | `klp --input-format jsonl --where 'component == \"api\"'` |
| kelora (sequential) | 44.266s | ~0.01 M/s | Single-threaded baseline |

**Interpretation:** Kelora is the slowest sequential option (≈9× behind jq and far behind angle-grinder), but `--parallel` cuts runtime to 8.8 s and keeps the scripting model; klp sees a similar gain with `--parallel 0`.

---

### 7. CSV Processing

**Task:** Filter CSV by level, select columns

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| qsv | 0.073s | ~1.37 M/s | High-performance CSV tool |
| miller | 0.228s | ~0.44 M/s | CSV swiss army knife |
| kelora | 6.808s | ~0.01 M/s | CSV + Rhai scripting |

**Interpretation:** CSV-specialized tools dominate (Kelora is 95× slower than qsv). Use Kelora when you need to mix CSV with other formats or invoke Rhai logic inline.

---

### Raw Files

Source measurements live in `benchmarks/comparison_results/01_simple_filter.md` through `07_csv.md` from the same commit/date for full reproducibility.
