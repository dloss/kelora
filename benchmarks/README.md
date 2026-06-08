# Kelora Performance Benchmark Suite

A comprehensive benchmarking system for both internal performance testing and external tool comparisons.

## Quick Start

### Internal Benchmarks (Regression Testing)

```bash
# Run quick benchmarks (50k dataset)
just bench-quick

# Run full benchmark suite (100k + 500k datasets)
just bench

# Update performance baseline
just bench-update
```

### Simple-Cases Suite (Throughput)

```bash
# Full run (200k lines) - reports lines/s and MB/s per scenario
just bench-simple

# Quick run (50k lines, fewer runs)
just bench-simple-quick

# Set/refresh the local baseline
just bench-simple-update
```

See [Simple-Cases Throughput Suite](#simple-cases-throughput-suite) below for details.

### External Tool Comparisons

```bash
# Generate comparison datasets (CSV, syslog formats)
just bench-datasets

# Run comparisons against grep, jq, awk, miller, etc.
just bench-compare

# Run all benchmarks (internal + external)
just bench-all
```

## Benchmark Tests

| Test | Dataset | Description |
|------|---------|-------------|
| **small_filter** | 100k lines | Filter ERROR level logs - tests basic filtering performance |
| **medium_processing** | 500k lines | Filter + tracking - tests exec scripts and state management |
| **large_parallel** | 500k lines | Parallel processing with 4 threads - tests concurrency |
| **sequential_throughput** | 500k lines | Raw sequential processing - tests baseline throughput |

## Test Data

Datasets are **generated on-demand** to avoid bloating the repository:

- **bench_50k.jsonl**: 50,000 synthetic log entries (~11MB) - for quick tests
- **bench_100k.jsonl**: 100,000 synthetic log entries (~23MB) - for small_filter test
- **bench_500k.jsonl**: 500,000 synthetic log entries (~113MB) - for full benchmarks

Generated with realistic log structures including:
- Multiple log levels (DEBUG, INFO, WARN, ERROR)
- Various components (api, database, auth, cache, etc.)
- Structured fields for filtering and processing

## Usage Examples

### Basic Benchmarking
```bash
# Quick development cycle benchmarks
make bench-quick

# Compare against baseline
./benchmarks/run_benchmarks.sh
```

### Setting Baselines
```bash
# After implementing optimizations
make bench-update
```

### CI Integration
```bash
# In CI pipeline - fails if >10% regression
./benchmarks/run_benchmarks.sh --quick
if [ $? -ne 0 ]; then
    echo "Performance regression detected!"
    exit 1
fi
```

## Output

Results are saved to `benchmarks/benchmark_results.json`:

```json
{
  "timestamp": "2025-06-27T09:13:54Z",
  "results": [
    {
      "test": "small_filter",
      "runs": 3,
      "times": [0.012, 0.011, 0.012],
      "avg_time": 0.012,
      "min_time": 0.011,
      "max_time": 0.012,
      "timestamp": "2025-06-27T09:13:54Z"
    }
  ]
}
```

## Baseline Comparison

The benchmark system automatically compares results against a baseline stored in `benchmarks/baseline_results.json`:

- **Green**: >5% improvement
- **Yellow**: <±10% change (acceptable)
- **Red**: >10% regression (needs investigation)

## Development Workflow

1. **Before changes**: `make bench-baseline` (establish baseline)
2. **Make changes**: Implement optimizations or features
3. **Test**: `make bench-quick` (verify no major regressions)
4. **Full validation**: `make bench` (comprehensive test)
5. **Update baseline**: `make bench-update` (if improvements are intentional)

## File Structure

```
benchmarks/
├── README.md                 # This documentation
├── run_benchmarks.sh         # Main benchmark runner
├── generate_test_data.py     # Test data generator
├── bench_10k.jsonl          # 10k test dataset
├── bench_50k.jsonl          # 50k test dataset
├── benchmark_results.json   # Latest results
└── baseline_results.json    # Baseline for comparison
```

## Performance Expectations

Based on current optimizations (MacBook with SSD):

| Test | Expected Time | Notes |
|------|---------------|-------|
| **small_filter** | ~0.12s | Fast filtering on 100k dataset (includes cold start) |
| **medium_processing** | ~0.01s | Filter + tracking (highly selective filter) |
| **large_parallel** | ~0.01s | Parallel processing (highly selective filter) |
| **sequential_throughput** | ~6.3s | Raw processing 500k records baseline |

**Note**: Times include "cold start" overhead (~0.3s first run). Subsequent runs are much faster (~0.01s).

These are rough guidelines - actual performance depends on hardware and system load.

---

## Simple-Cases Throughput Suite

`bench_simple_cases.py` targets the **per-event hot path** for the cases real
users hit most often (parse → filter → format → write). Unlike
`run_benchmarks.sh` it reports **throughput** (lines/s and MB/s) rather than
raw wall-time, so results are comparable across machines and dataset sizes.

It is self-contained: it generates its own **uniform-schema** datasets (so
narrow-vs-wide event comparisons are controlled) and needs no external timing
tools — timing uses Python's `perf_counter` around the binary with output sent
to `/dev/null`. A validation pass runs every command once first and aborts if
any exits non-zero, so a typo never gets silently timed as an error.

```bash
python3 benchmarks/bench_simple_cases.py            # full (200k lines)
python3 benchmarks/bench_simple_cases.py --quick    # 50k lines, 3 runs
python3 benchmarks/bench_simple_cases.py --lines 1000000
python3 benchmarks/bench_simple_cases.py --filter parse,filter   # categories
python3 benchmarks/bench_simple_cases.py --update-baseline
```

### Scenarios

Scenarios are grouped by category, and several are **paired** so that the
*delta* between them attributes cost to a single pipeline stage. Output is
suppressed with `-q` except where formatting itself is being measured.

| Category | Scenarios | What the comparison isolates |
|----------|-----------|------------------------------|
| **parse** | `parse_json_narrow/_wide`, `parse_logfmt`, `parse_csv`, `parse_line` | Parser cost per format; narrow vs wide isolates per-field parse cost |
| **filter** | `filter_native_narrow/_wide`, `filter_rhai_narrow/_wide` | Native fast-path vs Rhai VM, each at 5 vs ~40 fields |
| **output** | `output_quiet`, `output_default`, `output_json`, `output_logfmt` | Formatter cost over the parse-only floor (`output_quiet`) |
| **select** | `select_high` (~100% pass), `select_low` (~25% pass) | Output/write cost as a function of selectivity |
| **timestamp** | `ts_on` vs `ts_off` | Cost of automatic timestamp extraction |
| **search** | `search_substr` | grep-like substring match on raw lines |

The headline pairing:
`(filter_rhai_wide − filter_rhai_narrow) − (filter_native_wide − filter_native_narrow)`
estimates the cost of cloning every field into the Rhai event map.

### Datasets (generated, git-ignored)

Uniform schema for controlled comparisons:

- `simple_narrow_<N>.jsonl` — 5 fields (timestamp, level, component, message, status)
- `simple_wide_<N>.jsonl` — ~40 fields (same 5 + 35 padding fields)
- `simple_narrow_<N>.logfmt`, `simple_narrow_<N>.csv` — same 5 fields, other formats
- `simple_narrow_<N>.txt` — plain text lines (for `-f line` and substring search)
- `simple_nots_<N>.jsonl` — narrow, but the timestamp field is renamed `tstamp`
  (not an auto-recognized name) so timestamp extraction is skipped

Results: `simple_cases_results.json`; baseline: `simple_cases_baseline.json`
(both local-only). Baseline comparison flags >5% faster (green) / >10% slower (red).

---

## External Tool Comparisons

The `compare_tools.sh` script benchmarks Kelora against common command-line tools:

### Comparison Matrix

| Category | Tools Compared | Purpose |
|----------|---------------|---------|
| **Text Filtering** | grep, ripgrep, kelora | Simple pattern matching baseline |
| **Field Extraction** | awk, kelora | Column splitting and extraction |
| **JSON Processing** | jq, kelora | JSON filtering and transformation |
| **Complex Pipelines** | bash+jq+sort, kelora | Multi-stage processing |
| **Parallel Processing** | jq, kelora (sequential vs parallel) | Multi-core scaling |
| **CSV Analytics** | miller, qsv, kelora | Structured data operations |

### Running Comparisons

```bash
# Install comparison tools (macOS)
brew install grep ripgrep jq miller qsv

# Install comparison tools (Ubuntu/Debian)
apt install grep ripgrep jq miller qsv

# Generate test datasets
just bench-datasets

# Run all comparisons
just bench-compare
```

Results are saved to `benchmarks/comparison_results/` as individual markdown files.

### Comparison Datasets

The comparison benchmarks use additional test datasets:

- **bench_100k.csv** - CSV format for miller/qsv comparison
- **bench_100k.log** - Syslog-style text for grep/awk comparison
- **bench_100k.jsonl** - JSON lines (already exists)
- **bench_500k.jsonl** - Larger dataset for parallel testing

These are generated automatically from the JSON test data.

### Documentation

Comparison results and analysis are documented in:
- **[docs/concepts/performance-comparisons.md](../docs/concepts/performance-comparisons.md)** - Comprehensive guide with decision matrix
- Results include honest assessment of when to use each tool