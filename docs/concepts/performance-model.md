# Performance Model

Kelora is designed to crunch large log streams quickly while staying responsive
for interactive use. Understanding the performance levers helps you pick the
right execution mode for ad-hoc investigations, CI jobs, and heavyweight batch
pipelines.

## Execution Modes

| Mode | Command | When to use | Characteristics |
|------|---------|-------------|-----------------|
| Sequential (default) | `kelora ...` | Tail, streaming pipes, order-sensitive work | Strict input order, minimal buffering, deterministic output |
| Parallel | `kelora --parallel ...` | Batch jobs, archives, CPU-bound transforms | Workload split across worker threads, configurable batching |

### Sequential Mode

- Processes one event at a time and forwards it immediately.
- Ideal for `tail -f` pipelines, interactive filtering, or anything that needs
  deterministic ordering.

- Windowing (`--window`), context flags (`-A/-B/-C`), and Rhai metrics operate
  with minimal latency.

### Parallel Mode

- Uses a worker pool (defaults to logical CPU count) to parse, filter, and
  transform events concurrently.

- Requires buffering to preserve order unless you pass `--unordered` (faster but
  only safe when ordering does not matter).

- Adjust batching:
  - `--batch-size <N>` – number of events per batch before flushing to workers.
  - `--batch-timeout <ms>` – flush partially filled batches after idle period.
  - `--threads <N>` – override the thread count (0 = auto).
- Context windows and sliding windows still work, but they maintain per-worker
  buffers internally. Increase `--window` sparingly to avoid large per-thread
  allocations.

#### Sequential vs Parallel in Practice

=== "Command"

    ```bash
    kelora -f combined examples/web_access_large.log.gz \
      -F none --stats

    kelora -f combined examples/web_access_large.log.gz \
      -F none --stats --parallel
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access_large.log.gz \
      -F none --stats

    kelora -f combined examples/web_access_large.log.gz \
      -F none --stats --parallel
    ```

On this synthetic access log (`1200` lines), parallel mode yields higher
throughput because the CPU-bound combined parser is spread across cores.
Real-world gains depend on disk speed, decompression cost, and script workload.

## Pipeline Components That Affect Throughput

1. **Input** – Kelora streams from files or stdin, automatically decompressing
   `.gz`. Network filesystems or slow disks can dominate runtime; consider using
   `pv`/`zcat` to monitor upstream throughput.

2. **Parsing** – Structured formats like `combined`, `syslog`, or `cols:` spend
   more CPU cycles than `line` or `raw`. Parallel mode shines here.

3. **Filtering** – Complex regex (`--filter`, `--keep-lines`, `--ignore-lines`)
   benefit from batching; simple boolean predicates are cheap.

4. **Transformation** – Rhai scripts are executed for every event. Expensive
   operations (regex extraction, JSON parsing, cryptographic hashes) may need
   `--parallel` or optimized logic (e.g., caching in `--begin`).

5. **Output** – `-F json` and CSV/TSV encoders allocate more than the default
   key=value printer. Writing to disk (`-o file`) shifts performance to storage.

## Measuring Performance

- `--stats` or `--stats-only` print throughput, error counts, time span, and
  key inventory. Compare sequential vs parallel runs with the same dataset.

- `--metrics` combined with `track_sum`/`track_bucket` can act as lightweight
  profilers (e.g., sum `duration_ms` to estimate runtime distribution).

- Use `time`, `hyperfine`, or CI timers around your Kelora command for wall
  clock baselines.

## Memory Considerations

- Multiline (`--multiline`) and windowing (`--window`, context flags) enlarge
  per-event buffers. Monitor with `--stats` and consider lowering
  `--batch-size` if memory grows uncontrollably in parallel mode.

- `--multiline all` or gigantic regex chunks can hold the entire file in RAM.
  Prefer incremental processing or pre-splitting input.

- `--metrics` keeps maps in memory until the run ends. Guard high-cardinality
  structures (`track_unique`) with filters.

## Ordering Guarantees

- Sequential mode preserves input order exactly.
- Parallel mode preserves order by default through batch sequencing. Use
  `--unordered` only when the output order is irrelevant (e.g., writing JSON
  lines to a file for downstream aggregation).

- `--batch-size` too large can increase latency before the first events appear.
  Tune for the desired balance between throughput and interactivity.

## Streaming vs Batch Recommendations

| Scenario | Suggested Flags |
|----------|-----------------|
| Watching logs live | Sequential (default), `--stats` for quick counters |
| Importing nightly archives | `--parallel --batch-size 2000 --stats-only` |
| CPU-heavy Rhai transforms | `--parallel --threads 0 --unordered` (if orderless) |
| Tail with alerts | Sequential + `--metrics` for low-latency thresholds |

## Troubleshooting Slow Pipelines

- **High CPU usage** – Profile Rhai scripts. Move static setup to `--begin` and
  eliminate redundant parsing inside `--exec`.

- **Low throughput in parallel mode** – Increase `--batch-size`, decrease
  `--batch-timeout`, or allow Kelora to run more threads with `--threads 0`.

- **Out-of-order events** – Ensure `--unordered` is not set. Multiline plus
  `--parallel` may delay chunk emission; reduce batch size.

- **Backpressure when writing to files** – Use `-o output.log` to avoid stdout
  buffering by other processes.

- **Gzip bottlenecks** – Pre-decompress with `zcat file.gz | kelora -f combined -`
  if CPU is the limiting factor and disk is fast.

## Quick Checklist

1. Streaming workloads? Stay sequential and stream to stdout for the lowest
   latency.

2. Batch archives? Combine `--parallel --stats` and tune `--batch-size` /
   `--batch-timeout` after inspecting skew.

3. Heavy windowing? Keep `--window` small (50 or less) or sample upstream to
   cap memory.

4. Verbose diagnostics? Drop to `-q` once the pipeline is stable to reduce
   stderr noise.

5. Ordering critical? Avoid `--unordered`; otherwise enabling it can flush
   parallel batches faster.

## Troubleshooting Cheats

- Inspect parse hiccups with `-F inspect` or by raising `--verbose`.
- Timestamp drift? Pin down `--ts-field`, `--ts-format`, or `--input-tz`
  (see `kelora --help-time`).

- Rhai panics? Guard lookups with `e.get_path("field", ())` and conversions with
  `to_int_or` / `to_float_or`.

- Abundant `.gz` files? No need for extra tooling—Kelora already detects and
  decompresses them automatically.

## Related Guides

- [Metrics and Tracking Tutorial](../tutorials/metrics-and-tracking.md) – build
  dashboards to observe throughput.

- [Multiline Strategies](multiline-strategies.md) – large multiline blocks can
  influence memory and batching.

- [CLI Reference – Performance Options](../reference/cli-reference.md#performance-options)
  – full documentation for `--parallel`, `--threads`, and friends.
