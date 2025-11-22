# Performance Comparisons

Understanding how Kelora compares to other command-line tools helps you choose the right tool for each task. This page provides honest benchmarks and guidance on when to reach for Kelora versus alternatives like grep, jq, awk, or specialized CSV tools.

!!! info "Philosophy: Right Tool for the Job"
    Kelora trades raw speed for **expressiveness and convenience**. For simple pattern matching, grep is often 10-100x faster. But when you need multi-stage pipelines, structured transformations, or windowed analysis, Kelora's unified scripting model can be both faster to write *and* faster to execute than complex bash pipelines.

## Quick Decision Matrix

| Task | Best Tool | Why | When to Use Kelora Instead |
|------|-----------|-----|---------------------------|
| Simple text search | `grep` / `rg` | 50-100x faster for pattern matching | When you need structured output or follow-up processing |
| Field extraction | `awk` / `cut` | Faster for simple column splits | When you need type conversion, validation, or enrichment |
| JSON filtering | `jq` | Comparable speed, ubiquitous | Multi-stage pipelines, metrics tracking, windowing |
| JSON transformation | `jq` | Similar performance for simple queries | Complex transforms using 100+ built-in functions |
| CSV analytics | `qsv` / `miller` | Often faster for pure CSV operations | When mixing CSV with other formats or scripting logic |
| Complex pipelines | Bash + multiple tools | Variable (often slower) | Always - simpler code, fewer moving parts |
| Large archive processing | Varies | - | When you can use `--parallel` for multi-core speedup |
| Windowed analysis | Custom scripts | Hard to compare | Built-in `--window` eliminates custom code |

!!! note "Raw benchmark files"
    Every table in this page is backed by `benchmarks/comparison_results/0X_*.md`. Regenerate them with `just bench-compare` to audit or extend the data.

## Benchmark Methodology

### Test Environment (2025-10-26)
- **OS:** macOS 15.6.1 (24G90) · `x86_64`
- **CPU:** Intel Core i5 · 6 cores @ 3.0 GHz · 16 GB RAM
- **Kelora build:** `cargo build --release` at `09faf43`
- **Toolchain:** grep 2.6.0 (BSD), ripgrep 15.1.0, jq 1.6, mlr 6.15.0, qsv 8.1.1, angle-grinder 0.19.5, klp 0.77.0, Rust 1.90.0 (stable)

### Procedure
- Generate deterministic 100k/500k JSONL fixtures plus derived CSV/log files via `benchmarks/generate_comparison_data.sh` (wraps `generate_test_data.py`).
- Run `./benchmarks/compare_tools.sh` (alias: `just bench-compare`). Each scenario executes three times; the script reports the arithmetic mean of the recorded wall-clock values (GNU `time` when available, otherwise `date` + `bc`).
- All commands stream to `/dev/null` (or minimal projections) so we measure processing cost rather than disk output.
- Raw markdown tables for every scenario are stored in `benchmarks/comparison_results/*.md` alongside the captured system metadata so results can be audited or re-plotted later.
- Throughput figures (`events/s`) in each table are simply dataset size divided by the mean runtime (100k lines unless noted; 500k for the parallel test).

---

## Benchmark Results

!!! tip "Running Your Own Benchmarks"
    ```bash
    brew install ripgrep jq miller qsv   # install peer tools
    cargo build --release                # build Kelora
    ./benchmarks/generate_comparison_data.sh
    just bench-compare                   # wraps ./benchmarks/compare_tools.sh
    ```
    Every run records the system info and scenario tables under `benchmarks/comparison_results/`. Copy those into [Benchmark Results](benchmark-results.md) when you contribute a new machine.

### 1. Simple Text Filtering

**Task:** Find all ERROR lines in 100,000-line log file

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| ripgrep (rg) | 0.021 s | ~4.76 M/s | Modern grep alternative |
| grep | 0.049 s | ~2.04 M/s | Baseline - fastest text search |
| angle-grinder | 0.327 s | ~0.31 M/s | `parse` + `where level == "ERROR"` |
| kelora (line) | 0.405 s | ~0.25 M/s | Full log parsing + structured output |
| klp | 1.952 s | ~0.05 M/s | `-l error` scan with line parser |

**Example Commands:**
```bash
# grep - simple and fast
grep 'ERROR' benchmarks/bench_100k.log

# kelora - slower but structured
kelora -f line benchmarks/bench_100k.log \
  --keep-lines 'ERROR' -q

# angle-grinder - parse + where
agrind '* | parse "* * *[*]: *" as timestamp, host, component, level, message | where level == "ERROR"' \
  -f benchmarks/bench_100k.log

# klp - built-in loglevel filter
klp --input-format line -l error benchmarks/bench_100k.log
```

Kelora is roughly **19× slower than ripgrep** (which itself is ≈2.3× faster than BSD `grep`; Kelora is ~8× slower than `grep`) because it tokenizes each line and materializes structured events. Angle-grinder falls in between—its Rust core plus lightweight `parse` operator keep it to ~0.33 s—while klp’s Python runtime makes it ~5× slower than Kelora for this raw-text scan. Use `grep`/`rg` when you just need text, but once you need follow-up filters or projections the extra ~0.4 s buys you typed fields and immediate Rhai hooks. To minimize the gap, combine `--keep-lines` with `-q` so Kelora skips formatter work your downstream pipeline does not need.

---

### 2. Field Extraction

**Task:** Extract timestamp, level, component from 100,000 log lines

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.368 s | ~0.27 M/s | `parse` + `fields` |
| awk | 0.389 s | ~0.26 M/s | Field splitting by whitespace |
| kelora (cols) | 4.744 s | ~0.02 M/s | Structured parsing + type awareness |
| klp | 6.898 s | ~0.01 M/s | line input + regex via `--input-exec` |

**Example Commands:**
```bash
# awk - fast field extraction
awk '{print $1, $3, $4}' benchmarks/bench_100k.log

# kelora - named fields with validation
kelora -f 'cols:timestamp host component level *message' \
  benchmarks/bench_100k.log \
  -k timestamp,level,component
```

Kelora trails `awk` and angle-grinder by **≈12×** because it parses into typed columns, validates timestamps, and keeps metadata for later stages; klp’s Python regex pipeline is even slower than Kelora here, so reach for it only when you need its advanced templating. Stick with those lighter tools for quick-and-dirty splits, but prefer Kelora when you immediately need type conversion, schema-aware errors, or downstream scripting. You can trim Kelora’s cost by projecting only the fields you need with `-k` and disabling expensive formatters (`-F none`).

---

### 3. JSON Filtering

**Task:** Filter 100,000 JSON logs where `level == 'ERROR'`

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.367 s | ~0.27 M/s | `json` + `where level == "ERROR"` |
| jq | 1.104 s | ~0.09 M/s | Standard JSON processor |
| klp | 4.695 s | ~0.02 M/s | jsonl input + `-l error` |
| kelora | 6.434 s | ~0.02 M/s | JSON parsing with Rhai filter |

**Example Commands:**
```bash
# jq - compact and fast
jq -c 'select(.level == "ERROR")' benchmarks/bench_100k.jsonl

# kelora - similar syntax
kelora -j benchmarks/bench_100k.jsonl \
  -l error -F json -q

# angle-grinder - json operator
agrind '* | json | where level == "ERROR"' -f benchmarks/bench_100k.jsonl

# klp - JSONL input with loglevel filter
klp --input-format jsonl -l error benchmarks/bench_100k.jsonl
```

Kelora runs **~6× slower than jq** (and ~17× slower than angle-grinder) because it pulls every event into a Rhai context with metrics/windowing hooks; klp pays a similar tax for its Python runtime. Reach for Kelora when you need that scripting environment (multiple `--filter` stages, stateful metrics) or want to mix formats. For pure filtering, enable `--no-emoji` and emit `-F json` to hold the gap to ~4× on this machine, or run the query in `agrind`/`jq` and feed the structured output back into Kelora.

---

### 4. JSON Transformation

**Task:** Filter API logs, extract status code, add computed field

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.232 s | ~0.43 M/s | `json` + computed `is_error` + `fields` |
| jq | 0.822 s | ~0.12 M/s | Complex jq query |
| kelora | 9.383 s | ~0.01 M/s | Multi-stage pipeline |
| klp | 11.082 s | ~0.01 M/s | jsonl + `-l error` + computed key |

**Example Commands:**
```bash
# jq - single complex query
jq -c 'select(.component == "api") |
  {timestamp, method, status, is_error: (.status >= 400)}' \
  benchmarks/bench_100k.jsonl

# kelora - readable pipeline
kelora -j benchmarks/bench_100k.jsonl \
  --filter "e.component == 'api'" \
  --exec "e.is_error = (e.status.to_int() >= 400)" \
  -k timestamp,method,status,is_error -q
```

Kelora is **≈11× slower than jq** (and ~40× slower than angle-grinder) on short declarative transforms, because it executes multiple Rhai stages instead of a single compiled expression; klp keeps similar flexibility but pays even more overhead. Kelora’s advantage shows up when the logic no longer fits into one expression—multiple `--filter` passes, reusable `--exec` snippets, or calls into Rhai helpers (`parse_jwt`, `track_percentile`, etc.). If you stay in Kelora, move heavy setup into `--begin` blocks and project only the final keys to keep execution closer to 6 – 7 s.

---

### 5. Complex Multi-Stage Pipeline

**Task:** Filter errors, count by component, sort by frequency

| Tool | Time | Throughput | Command Complexity |
|------|------|------------|-------------------|
| angle-grinder | 0.220 s | ~0.45 M/s | `agrind '* | json | where level == "ERROR" | count by component'` |
| bash + jq + sort + uniq | 0.753 s | ~0.13 M/s | `jq ... \| sort \| uniq -c \| sort -rn` |
| klp + sort + uniq | 3.851 s | ~0.03 M/s | `klp -l error --output-template '{component}' \| sort \| uniq -c \| sort -rn` |
| kelora | 7.086 s | ~0.01 M/s | `kelora --filter ... --exec 'track_count(...)' --metrics` |

**Example Commands:**
```bash
# Bash pipeline - multiple tools
jq -r 'select(.level == "ERROR") | .component' \
  benchmarks/bench_100k.jsonl | \
  sort | uniq -c | sort -rn

# Kelora - single command
kelora -j benchmarks/bench_100k.jsonl \
  -l error \
  --exec "track_count(e.component)" \
  --metrics -F none -q

# angle-grinder - aggregation
agrind '* | json | where level == "ERROR" | count by component | sort by _count desc' \
  -f benchmarks/bench_100k.jsonl

# klp - stream component names to sort/uniq
klp --input-format jsonl -l error --output-template '{component}' \
  --plain --no-color benchmarks/bench_100k.jsonl | sort | uniq -c | sort -rn
```

This scenario highlights ergonomics over raw speed: the Unix pipelines or angle-grinder’s built-in aggregators stay **10–30× faster**, but Kelora expresses the same logic in one command that is easier to evolve (add metrics, export JSON, window counts). klp sits in the middle because it needs an external `sort | uniq -c` stage. If you are scripting or automating, absorbing the extra seconds is usually worth the readability. For throwaway shell use, stick with the Unix toolkit or `agrind`.

---

### 6. Parallel Processing

**Task:** Process 500,000 JSON logs with filtering

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| angle-grinder | 0.979 s | ~0.51 M/s | `json` + `where component == "api"` |
| jq (sequential) | 4.665 s | ~0.11 M/s | Single-threaded processing |
| kelora (--parallel) | 8.832 s | ~0.06 M/s | Multi-core processing |
| klp (--parallel 0) | 12.108 s | ~0.04 M/s | Multiprocess (--parallel 0) |
| klp (sequential) | 27.122 s | ~0.02 M/s | jsonl input + `--where` |
| kelora (sequential) | 44.266 s | ~0.01 M/s | Single-threaded baseline |

**Example Commands:**
```bash
# jq - single threaded
jq -c 'select(.component == "api")' \
  benchmarks/bench_500k.jsonl | wc -l

# kelora - sequential
kelora -j benchmarks/bench_500k.jsonl \
  --filter "e.component == 'api'" -F json -q

# kelora - parallel (auto-scales to CPU cores)
kelora -j benchmarks/bench_500k.jsonl \
  --filter "e.component == 'api'" --parallel -F json -q
```

Kelora is far slower than angle-grinder’s Rust pipeline and about **9× slower than jq** when it runs sequentially, because it executes every event inside the Rhai interpreter. Turning on `--parallel` claws back the deficit—processing drops from 44 s to 8.8 s on this 6‑core machine, leaving Kelora only ~2× slower than jq while still delivering the richer scripting model. klp sees a similar 2.2× boost with `--parallel 0`. Use Kelora’s parallel mode for archival replays: set `--parallel`, keep `--batch-size` near cache-friendly values (1 000–4 000), and pin `--threads` to your physical cores if the workload competes with other services.

---

### 7. CSV Processing

**Task:** Filter CSV by level, select specific columns

| Tool | Time | Throughput | Notes |
|------|------|------------|-------|
| qsv | 0.073 s | ~1.37 M/s | High-performance CSV tool |
| miller | 0.228 s | ~0.44 M/s | CSV swiss army knife |
| kelora | 6.808 s | ~0.01 M/s | CSV + Rhai scripting |

**Example Commands:**
```bash
# miller - powerful CSV transforms
mlr --csv filter '$level == "ERROR"' \
  then cut -f timestamp,level,component \
  benchmarks/bench_100k.csv

# qsv - blazing fast
qsv search -s level 'ERROR' benchmarks/bench_100k.csv | \
  qsv select timestamp,level,component

# kelora - scriptable CSV
kelora -f csv benchmarks/bench_100k.csv \
  -l error \
  -k timestamp,level,component -q
```

`qsv` simply screams on CSV—Kelora is ~95× slower here because it still instantiates full events and Rhai scopes. When you only touch CSV/TSV, run `qsv` or `mlr` and hand the results to Kelora only for the structured steps they cannot express. Kelora becomes attractive as soon as you need to join CSV with JSON/syslog, mask fields with built-ins, or emit a single, scriptable pipeline for your team.

---

## When Kelora Shines

### 1. Multi-Format Pipelines

**Problem:** Parse syslog messages that contain embedded logfmt

```bash
# With grep/awk/jq - messy and fragile
grep "user=" syslog.log | \
  sed 's/.*msg="\([^"]*\)".*/\1/' | \
  # ...complex parsing...

# With Kelora - clean and explicit
kelora -f syslog syslog.log \
  --exec 'if e.msg.contains("=") {
            e += e.msg.parse_logfmt()
          }' \
  --filter 'e.has("user")' \
  -k timestamp,host,user,action
```

### 2. Windowed Analysis

**Problem:** Detect error bursts (3+ errors in 60 seconds)

```bash
# With awk - complex state management
awk '...[50 lines of window logic]...'

# With Kelora - built-in windows
kelora -j app.jsonl --window 60 \
  --exec 'let errors = window.pluck("level")
            .filter(|x| x == "ERROR");
          if errors.len() >= 3 {
            eprint("Burst at " + e.timestamp)
          }'
```

### 3. Enrichment & Privacy

**Problem:** Mask IPs, parse JWTs, add computed fields

```bash
# Kelora's built-in functions make this trivial
kelora -j security.jsonl \
  --exec 'e.ip = e.ip.mask_ip(2);
          if e.has("token") {
            let jwt = e.token.parse_jwt();
            e.role = jwt.get_path("claims.role", "guest")
          }'
```

---

## When to Use Something Else

### Use `grep` when:
- Simple pattern matching is all you need
- Speed is absolutely critical
- Output is for human eyes, not further processing

### Use `jq` when:
- Pure JSON transformations
- You need jq's advanced tree manipulation
- Integrating with existing jq-based workflows

### Use `awk` when:
- Simple field extraction from delimited files
- You're more familiar with awk's syntax
- Legacy scripts already use it

### Use `qsv` / `miller` when:
- Working exclusively with CSV/TSV
- Need specialized CSV operations (joins, statistics, sampling)
- Performance on CSV is paramount

### Use `lnav` / `SQLite` / `DuckDB` when:
- Interactive exploration and ad-hoc queries
- Need SQL for complex aggregations
- Building dashboards or reports

---

## Performance Tips

From fastest to slowest approaches:

### ✅ Fastest: Pre-filter before complex processing
```bash
# Filter cheap fields first
kelora -j huge.jsonl \
  --filter "e.level == 'ERROR'" \
  --exec "expensive_parsing(e)" \
  --filter "e.computed > 100"
```

### ⚡ Fast: Use parallel mode for CPU-bound work
```bash
kelora -j archive.jsonl --parallel --threads 0 \
  --exec "e.parsed = e.msg.parse_custom_format()"
```

### 🐌 Slower: Complex regex on every event
```bash
# Move static setup to --begin if possible
kelora --exec "e.extracted = complex_regex(e.line)"
```

See [Performance Model](performance-model.md) for deep dive on tuning.

---

## Reproduction Guide

1. **Install comparison tools.** `brew install ripgrep jq miller qsv` (macOS) or the equivalent packages via `apt`, `dnf`, etc.
2. **Build Kelora in release mode.**
   ```bash
   cargo build --release
   ```

3. **Generate the deterministic datasets.**
   ```bash
   ./benchmarks/generate_comparison_data.sh
   ```
   This script ensures the 100k/500k JSONL fixtures plus CSV/syslog derivatives match what the docs describe.

4. **Run the comparison suite.**
   ```bash
   just bench-compare    # wraps ./benchmarks/compare_tools.sh
   ```
   The runner prints human-friendly summaries and writes raw markdown tables to `benchmarks/comparison_results/0*.md`.

5. **Update the docs.** Copy the system-info block and any new tables into `docs/concepts/benchmark-results/` (one file per machine) and open a PR that references your hardware + date.

Every rerun should state the git commit, OS version, CPU, and tool versions so readers can compare apples to apples.

---

## Related Documentation

- **[Performance Model](performance-model.md)** - Kelora's internal performance characteristics, tuning `--parallel` and `--batch-size`
- **[How-To: Batch Process Archives](../how-to/batch-process-archives.md)** - Practical guide to parallel processing and maximizing throughput
- **[CLI Reference](../reference/cli-reference.md)** - Complete flag documentation
- **[Functions Reference](../reference/functions.md)** - All 100+ built-in functions

---

## Contributing Benchmarks

Have a different machine? Want to test additional tools? Benchmark contributions are welcome!

1. Run `./benchmarks/compare_tools.sh` on your machine
2. Save results with hardware info
3. Submit a PR with your results in `docs/concepts/benchmark-results/`

We're especially interested in:

- Different CPU architectures (ARM, x86, RISC-V)
- Linux vs macOS vs Windows
- Comparison with additional tools (angle, pv+awk, etc.)
