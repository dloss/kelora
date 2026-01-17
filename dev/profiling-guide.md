# Profiling Guide for Kelora Developers

This guide shows how to profile Kelora to identify and implement performance optimizations on macOS and Linux.

## Quick Start

### macOS (Recommended for Development)

```bash
# Install samply (fast, no sudo required)
cargo install samply

# Profile with interactive flamegraph
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "e.level == 'ERROR'" > /dev/null
```

### Linux (CI/Production)

```bash
# Install flamegraph
cargo install flamegraph

# Profile with perf + flamegraph
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "e.level == 'ERROR'" > /dev/null
```

## Why Profile?

Benchmarks tell you **how long** operations take. Profiling shows **where** time is spent, revealing:

- Hot paths consuming 80% of runtime
- Unexpected allocations in tight loops
- Regex recompilation overhead
- JSON parsing bottlenecks
- Thread contention in parallel mode
- I/O vs CPU bound operations

**Goal**: Find the 20% of code consuming 80% of runtime and optimize it.

## Known Kelora Hotspots

Based on architecture, these areas typically dominate profiles:

### 1. JSON Parsing (30-50% of runtime)

**Location**: `src/formats/json.rs`

**Profile signature**: Wide `serde_json::from_str` bar in flamegraph

**Optimization opportunities**:
- Use `simd-json` for SIMD-accelerated parsing (2-3x faster)
- Implement custom deserializer for common schemas
- Add streaming JSON parser for multiline mode
- Cache parsed objects in LRU for duplicate detection

**How to profile**:
```bash
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "true" \
  > /dev/null
```

**What to look for**:
- `serde_json::from_str` should be 30-40% (expected)
- If >50%, JSON parsing dominates → optimize parser
- If `String::from_utf8` is high → input validation overhead

### 2. Regex Compilation (5-20% of runtime)

**Location**: `src/formats/regex.rs`, `src/rhai_functions/regex.rs`

**Profile signature**: `regex::Regex::new` appearing in flamegraph

**Optimization opportunities**:
- Cache compiled regexes in `lazy_static!` or `once_cell`
- Use `RegexSet` for multiple patterns
- Implement regex literal optimization (detect static patterns)
- Add regex precompilation pass in `--begin` phase

**How to profile**:
```bash
cargo flamegraph --bin kelora -- \
  -f combined benchmarks/web_access.log \
  > /dev/null
```

**What to look for**:
- `regex::Regex::new` should appear at most once per pattern
- If appearing repeatedly → regex is being recompiled per event
- If `regex::compile::Hir::parse` is high → complex patterns need optimization

### 3. Rhai Script Execution (10-40% of runtime)

**Location**: `src/engine.rs`, `src/rhai_functions/`

**Profile signature**: Wide `rhai::eval`, `rhai::call_fn` bars

**Optimization opportunities**:
- Move hot Rhai functions to native Rust (e.g., `regex_match` → `--filter`)
- Precompile scripts using `rhai::AST::compile`
- Cache function lookups with `Engine::register_fn`
- Implement custom Rhai types to avoid JSON roundtrips

**How to profile**:
```bash
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --exec "
    if regex_match(e.message, 'ERROR.*timeout') then
      e.severity = 'high'
    end
  " \
  > /dev/null
```

**What to look for**:
- `rhai::eval` should be <20% for simple scripts
- If >40%, consider moving logic to native filters
- `rhai::Dynamic::clone` high → excessive type conversions

### 4. String Allocations (5-15% of runtime)

**Location**: Throughout codebase

**Profile signature**: `String::clone`, `alloc::string::String::from`

**Optimization opportunities**:
- Use `Cow<str>` for conditional cloning
- Implement zero-copy parsing with string slices
- Use `SmallVec` for short strings (avoid heap)
- Add string interning for repeated values (field names, log levels)

**How to profile allocations (macOS)**:
```bash
cargo instruments -t alloc --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  > /dev/null
```

**What to look for**:
- Peak memory usage (should be <100MB for 500k events)
- Allocation hotspots in `formats/` parsers
- Repeated allocations of identical strings

### 5. Parallel Processing Overhead (0-30% of runtime)

**Location**: `src/parallel/mod.rs`

**Profile signature**: `crossbeam::channel::send`, thread contention

**Optimization opportunities**:
- Tune `--batch-size` for better throughput
- Reduce channel communication (batch events)
- Implement work-stealing for unbalanced loads
- Add MPMC queue optimization

**How to profile**:
```bash
cargo instruments -t sys --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --parallel --threads 4 \
  > /dev/null
```

**What to look for**:
- Thread utilization (should be >90% on all cores)
- Lock contention (should be minimal)
- `crossbeam::send` overhead (should be <5%)

### 6. Format Auto-Detection (1-5% of runtime)

**Location**: `src/formats/mod.rs::detect_format`

**Profile signature**: Multiple `parse_*` attempts in flamegraph

**Optimization opportunities**:
- Implement smarter heuristics (JSON starts with `{` or `[`)
- Cache format detection result for streaming
- Add format hints to skip detection
- Use SIMD for fast pattern matching

**How to profile**:
```bash
cargo flamegraph --bin kelora -- \
  benchmarks/bench_500k.jsonl \
  > /dev/null
```

**What to look for**:
- `detect_format` should be <1% (called once)
- If higher, format detection is being repeated per event

## Profiling Tools

### macOS

#### 1. Samply (Recommended)

**Best for**: Fast, interactive profiling without sudo

```bash
cargo install samply

# Profile and open in browser
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "e.level == 'ERROR'" \
  > /dev/null
```

**Advantages**:
- No sudo required (unlike dtrace)
- Interactive web UI with search
- Fast sampling overhead
- Works on macOS 10.12+

#### 2. Instruments (Deep Analysis)

**Best for**: Memory profiling, allocation tracking, I/O analysis

```bash
cargo install cargo-instruments

# Time Profiler
cargo instruments -t time --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  > /dev/null

# Allocations Profiler
cargo instruments -t alloc --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  > /dev/null

# System Trace (I/O, threads, syscalls)
cargo instruments -t sys --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --parallel --threads 4 \
  > /dev/null
```

**Open results**:
```bash
open target/instruments/*.trace
```

### Linux

#### 1. Flamegraph (Standard)

```bash
cargo install flamegraph

# Setup perf for non-root (one-time)
echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid

# Profile
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "e.level == 'ERROR'" \
  > /dev/null

# Opens flamegraph.svg
```

#### 2. Samply (Also Works on Linux)

```bash
cargo install samply

samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  > /dev/null
```

#### 3. Valgrind (Detailed but Slow)

**Best for**: Cache analysis, instruction counting

```bash
# Build with debug symbols
cargo build --release

# Profile
valgrind --tool=callgrind \
  --callgrind-out-file=callgrind.out \
  target/release/kelora \
  -f json benchmarks/bench_100k.jsonl \
  > /dev/null

# Visualize
kcachegrind callgrind.out
```

**Warning**: 10-100x slower, use small datasets (bench_50k or bench_100k)

## Build Configuration

### Release with Debug Symbols (Recommended for Profiling)

Add to `Cargo.toml`:

```toml
[profile.release]
debug = true
```

Or use environment variable:

```bash
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --bin kelora -- [args]
```

**Benefits**:
- Full optimization (same speed as release)
- Better symbol resolution in profilers
- More accurate stack traces

### Never Profile Debug Builds

```bash
# ❌ WRONG - 10-100x slower than release
cargo build
cargo flamegraph --bin kelora -- [args]

# ✅ CORRECT - Realistic performance
cargo build --release
cargo flamegraph --bin kelora -- [args]
```

## Performance Improvement Workflow

### Step 1: Establish Baseline

```bash
# Generate test data
cd benchmarks && ./generate_test_data.py && cd ..

# Run benchmark suite
just bench

# Save results
cp target/criterion /tmp/criterion-baseline -r
```

### Step 2: Profile Workload

Choose a representative workload:

```bash
# JSON filtering (most common)
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "e.level == 'ERROR'" \
  > /dev/null

# Rhai execution
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --exec "if e.level == 'ERROR' then e.severity = 'high' end" \
  > /dev/null

# Parallel processing
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --parallel --threads 4 \
  > /dev/null

# Format parsing
samply record cargo run --release --bin kelora -- \
  -f combined benchmarks/web_access.log \
  > /dev/null
```

### Step 3: Identify Hotspots

**Look for**:
- Functions consuming >10% of total time
- Unexpected allocations in tight loops
- Repeated regex compilation
- Thread contention (locks, channel overhead)

**Focus on**:
- Top 3 functions by weight
- Unexpected library calls (e.g., `alloc` in hot path)
- Functions with high "self time" (excluding children)

### Step 4: Implement Optimization

**Example 1: Cache compiled regexes**

Before (slow):
```rust
// In src/rhai_functions/regex.rs
pub fn regex_match(pattern: &str, text: &str) -> bool {
    let re = Regex::new(pattern).unwrap(); // ❌ Compiles every call
    re.is_match(text)
}
```

After (fast):
```rust
use lru::LruCache;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static REGEX_CACHE: Lazy<Mutex<LruCache<String, Regex>>> =
    Lazy::new(|| Mutex::new(LruCache::new(100)));

pub fn regex_match(pattern: &str, text: &str) -> bool {
    let mut cache = REGEX_CACHE.lock().unwrap();
    let re = cache.get_or_insert(pattern.to_string(), || {
        Regex::new(pattern).unwrap()
    });
    re.is_match(text)
}
```

**Example 2: Reduce string allocations**

Before (allocates):
```rust
// In src/formats/json.rs
fn parse_event(&self, line: &str) -> Result<Event> {
    let json: Value = serde_json::from_str(line)?;
    let mut fields = IndexMap::new();
    for (k, v) in json.as_object().unwrap() {
        fields.insert(k.clone(), v.to_string()); // ❌ Clones every key
    }
    Ok(Event { fields })
}
```

After (zero-copy):
```rust
fn parse_event<'a>(&self, line: &'a str) -> Result<Event<'a>> {
    let json: Value = serde_json::from_str(line)?;
    let mut fields = IndexMap::new();
    for (k, v) in json.as_object().unwrap() {
        fields.insert(k.as_str(), v); // ✅ Borrows key
    }
    Ok(Event { fields })
}
```

**Example 3: Optimize parallel batching**

Before (small batches):
```rust
// In src/parallel/mod.rs
const DEFAULT_BATCH_SIZE: usize = 100; // ❌ Too small, high overhead
```

After (tuned):
```rust
const DEFAULT_BATCH_SIZE: usize = 1000; // ✅ Better throughput
```

### Step 5: Verify Improvement

```bash
# Re-run benchmarks
just bench

# Compare with baseline
# Look for >10% improvement in relevant benchmarks

# Re-profile to confirm hotspot reduced
samply record cargo run --release --bin kelora -- [same args]
```

### Step 6: Document and Test

```bash
# Run full test suite
just test

# Add regression test if needed
# Document optimization in commit message
# Update CHANGELOG.md if user-visible
```

## Concrete Optimization Ideas

### High-Impact (>20% speedup potential)

1. **Replace serde_json with simd-json**
   - **Where**: `src/formats/json.rs`
   - **Effort**: Medium (API compatibility layer needed)
   - **Gain**: 2-3x faster JSON parsing
   - **Profile**: Wide `serde_json::from_str` bar

2. **Implement regex literal optimization**
   - **Where**: `src/formats/regex.rs`, `src/rhai_functions/regex.rs`
   - **Effort**: Low (detect static strings, use `contains()`)
   - **Gain**: 5-10x faster for simple patterns
   - **Profile**: `regex::Regex::new` appearing repeatedly

3. **Add string interning for field names**
   - **Where**: `src/formats/json.rs`, `src/formats/logfmt.rs`
   - **Effort**: Medium (implement `Interner` with `Arc<str>`)
   - **Gain**: 20-30% less memory, 10-15% faster
   - **Profile**: Many `String::clone` of field names

4. **Optimize parallel batching**
   - **Where**: `src/parallel/mod.rs`
   - **Effort**: Low (tune batch size, reduce channel sends)
   - **Gain**: 15-25% faster in `--parallel` mode
   - **Profile**: High `crossbeam::send` overhead

### Medium-Impact (5-15% speedup potential)

5. **Use `SmallVec` for event fields**
   - **Where**: `src/processing/event.rs`
   - **Effort**: Low (replace `Vec` with `SmallVec<[_; 8]>`)
   - **Gain**: 5-10% less allocations for small events
   - **Profile**: `Vec::push` allocations in hot path

6. **Implement fast-path for common filters**
   - **Where**: `src/engine.rs`
   - **Effort**: Medium (detect simple filters, skip Rhai)
   - **Gain**: 10-20% faster for `--filter "e.level == 'ERROR'"`
   - **Profile**: `rhai::eval` dominating simple filters

7. **Cache format detection**
   - **Where**: `src/formats/mod.rs`
   - **Effort**: Low (detect once, cache result)
   - **Gain**: 2-5% faster for auto-detection
   - **Profile**: `detect_format` called repeatedly

8. **Use `Cow<str>` in formatters**
   - **Where**: `src/formatters/`
   - **Effort**: Low (avoid cloning when possible)
   - **Gain**: 5-10% less allocations
   - **Profile**: `String::clone` in output path

### Low-Impact (<5% speedup potential)

9. **Optimize timestamp parsing**
   - **Where**: `src/timestamp.rs`
   - **Effort**: Medium (cache timezone lookups)
   - **Gain**: 2-5% faster for timezone conversions
   - **Profile**: `chrono_tz::parse` appearing

10. **Reduce `IndexMap` overhead**
    - **Where**: Event field storage
    - **Effort**: High (custom hash map for common cases)
    - **Gain**: 3-5% faster field lookups
    - **Profile**: `IndexMap::get` in hot path

## Architecture-Specific Profiling

### Profile JSON Pipeline

```bash
# Focus on JSON parsing path
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --filter "true" \
  > /dev/null
```

**Expected profile**:
- `serde_json::from_str`: 30-40%
- `crossbeam::recv`: 5-10% (if parallel)
- `write!`: 10-15% (output formatting)
- `rhai::eval`: <5% (simple filter)

**Red flags**:
- `regex::Regex::new` appearing (should be cached)
- `String::clone` >15% (too many allocations)
- `IndexMap::insert` >10% (field storage overhead)

### Profile Rhai Execution

```bash
# Focus on script evaluation
cargo flamegraph --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --exec "
    if e.level == 'ERROR' then
      e.severity = 'high'
      e.priority = 1
    end
  " \
  > /dev/null
```

**Expected profile**:
- `rhai::eval`: 20-30%
- `serde_json::from_str`: 25-35%
- `rhai::call_fn`: 5-10%

**Red flags**:
- `rhai::Dynamic::clone` >10% (type conversion overhead)
- `rhai::eval` >50% (consider native filter)
- `parse_json` appearing (avoid parsing in scripts)

### Profile Parallel Processing

```bash
# Focus on threading overhead
cargo instruments -t sys --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  --parallel --threads 4 \
  --filter "e.level == 'ERROR'" \
  > /dev/null
```

**Expected profile**:
- All 4 worker threads at >90% utilization
- `crossbeam::send`: <5%
- No lock contention

**Red flags**:
- Unbalanced thread utilization (work-stealing needed)
- High `crossbeam::send` (batch size too small)
- Lock contention on `Mutex` (shared state problem)

### Profile Format Parsers

```bash
# JSON
cargo flamegraph --bin kelora -- -f json benchmarks/bench_500k.jsonl > /dev/null

# Logfmt
cargo flamegraph --bin kelora -- -f logfmt benchmarks/bench_logfmt.log > /dev/null

# Combined (Apache)
cargo flamegraph --bin kelora -- -f combined benchmarks/web_access.log > /dev/null

# Regex
cargo flamegraph --bin kelora -- \
  -f 'regex:^(?P<date>\S+) (?P<level>\S+) (?P<message>.*)$' \
  examples/simple_syslog.log \
  > /dev/null
```

**Compare**:
- JSON should be fastest (SIMD potential)
- Logfmt should be 2nd (simple nom parser)
- Regex should be slowest (backtracking)

## Common Pitfalls

### Pitfall 1: Profiling Debug Builds

**Problem**: Debug builds are 10-100x slower

**Solution**:
```bash
# Always use --release
cargo build --release
cargo flamegraph --bin kelora -- [args]
```

### Pitfall 2: Small Datasets

**Problem**: Profiling overhead dominates with <10k events

**Solution**:
```bash
# Use bench_500k for realistic profiles
samply record cargo run --release --bin kelora -- \
  -f json benchmarks/bench_500k.jsonl \
  > /dev/null
```

### Pitfall 3: Terminal Output Overhead

**Problem**: Printing to terminal adds 20-50% overhead

**Solution**:
```bash
# Redirect to /dev/null
cargo flamegraph --bin kelora -- [args] > /dev/null
```

### Pitfall 4: Not Generating Test Data

**Problem**: Missing benchmark files cause errors

**Solution**:
```bash
cd benchmarks && ./generate_test_data.py && cd ..
```

### Pitfall 5: Ignoring Allocations

**Problem**: CPU profile looks fine but memory usage is high

**Solution**:
```bash
# Profile allocations separately (macOS)
cargo instruments -t alloc --release --bin kelora -- [args]
```

## Interpreting Flamegraphs

### Anatomy

```
┌────────────────────────────────────────────────────┐
│ main                                    (100%)     │ ← Entry point
├────────────────────────────────────────────────────┤
│ run_pipeline                            (98%)      │ ← Main logic
├─────────────────┬──────────────────────────────────┤
│ parse (35%)     │ process (63%)                    │ ← Parallel paths
├─────────────────┼──────────────────────┬───────────┤
│ serde_json::    │ rhai::eval (30%)     │ write     │ ← Hot functions
│ from_str (30%)  │                      │ (18%)     │
└─────────────────┴──────────────────────┴───────────┘
```

**Reading**:
- **Width** = % of total CPU time
- **Height** = Call stack depth
- **Hot path** = Wide bar deep in stack

### Example Interpretations

**Profile 1**: JSON parsing bottleneck
```
serde_json::from_str ████████████████████████ 45%
rhai::eval            ████████ 15%
write!                ██████ 12%
```
**Diagnosis**: JSON parsing dominates
**Action**: Consider simd-json or parallel mode

**Profile 2**: Regex recompilation
```
serde_json::from_str ██████████████ 30%
regex::Regex::new    ██████████████ 28%  ← RED FLAG
rhai::eval           ██████ 12%
```
**Diagnosis**: Compiling regex per event
**Action**: Cache compiled regexes

**Profile 3**: Balanced profile
```
serde_json::from_str ████████████ 28%
rhai::eval           ██████████ 22%
write!               ████████ 18%
crossbeam::recv      ████ 8%
```
**Diagnosis**: No clear bottleneck (well-optimized)
**Action**: Focus on algorithmic improvements

## CI Integration

### GitHub Actions Profiling

```yaml
name: Performance Profile

on:
  pull_request:
    paths:
      - 'src/**'
      - 'benchmarks/**'

jobs:
  profile:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install flamegraph
        run: cargo install flamegraph

      - name: Setup perf
        run: echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid

      - name: Generate test data
        run: cd benchmarks && ./generate_test_data.py

      - name: Profile baseline
        run: |
          cargo flamegraph --bin kelora -- \
            -f json benchmarks/bench_500k.jsonl \
            --filter "e.level == 'ERROR'" \
            > /dev/null
          mv flamegraph.svg flamegraph-baseline.svg

      - name: Upload profile
        uses: actions/upload-artifact@v4
        with:
          name: flamegraph
          path: flamegraph-baseline.svg

      - name: Run benchmarks
        run: cargo bench --no-fail-fast
```

## Profiling Checklist

**Before profiling**:
- [ ] Generate test data: `cd benchmarks && ./generate_test_data.py`
- [ ] Build release: `cargo build --release`
- [ ] Install profiler: `cargo install samply` or `cargo install flamegraph`
- [ ] Choose representative workload (JSON, Rhai, parallel, etc.)

**During profiling**:
- [ ] Use `--release` builds (never debug)
- [ ] Redirect output to `/dev/null` (avoid terminal overhead)
- [ ] Run multiple times to average out noise
- [ ] Profile realistic datasets (bench_500k, not tiny examples)

**After profiling**:
- [ ] Focus on >10% contributors (ignore minor functions)
- [ ] Identify root cause (parsing, regex, allocations, threading)
- [ ] Implement targeted optimization
- [ ] Verify with benchmarks: `just bench`
- [ ] Re-profile to confirm improvement

## Quick Reference

**macOS - Fast profiling**:
```bash
cargo install samply
samply record cargo run --release --bin kelora -- -f json benchmarks/bench_500k.jsonl > /dev/null
```

**Linux - Standard profiling**:
```bash
cargo install flamegraph
cargo flamegraph --bin kelora -- -f json benchmarks/bench_500k.jsonl > /dev/null
```

**Verify optimizations**:
```bash
just bench-update  # Before changes
# ... make changes ...
just bench         # After changes (compare)
```

**Common hotspots**:
- `serde_json::from_str` (30-50%) → JSON parsing
- `regex::Regex::new` (appearing) → Regex recompilation
- `rhai::eval` (>40%) → Script execution overhead
- `String::clone` (>15%) → Excessive allocations
- `crossbeam::send` (>10%) → Batching too small

## Related Documentation

- `benchmarks/README.md` - Benchmark suite documentation
- `AGENTS.md` - Build commands and development workflow
- `dev/vision-and-design.md` - Architecture and design philosophy

## Summary

**Profile to find hotspots** → **Optimize targeted code** → **Verify with benchmarks** → **Repeat**

The key is profiling realistic workloads with release builds, focusing on >10% contributors, and validating improvements with `just bench`. Most performance gains come from optimizing the top 3-5 hotspots, not micro-optimizing everything.
