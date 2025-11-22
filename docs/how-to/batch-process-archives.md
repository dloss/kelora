# Process Archives at Scale

Crunch large log archives—compressed or not—while balancing throughput, ordering, and resource usage.

## When This Guide Helps
- Daily or monthly log archives need to be scanned for regressions or security events.
- You want to benchmark `--parallel` vs sequential performance on your hardware.
- Pipelines must stay reliable while processing tens of gigabytes of data.

## Before You Start
- Examples use `logs/2024-*.jsonl.gz` as a placeholder. Replace with your archive paths.
- Verify disk bandwidth and CPU capacity; the fastest settings differ between laptops and servers.
- Have a small sample ready for functional testing before running across huge datasets.

## Step 1: Measure a Sequential Baseline
Start simple to confirm filters, transformations, and outputs are correct.

```bash
time kelora -j logs/2024-04-01.jsonl.gz \
  -l error \
  -k timestamp,service,message \
  -J > errors-sample.json
```

- Record elapsed time, CPU usage, and memory footprint (`/usr/bin/time -v` can help).
- Keep this script for future regression checks.

## Step 2: Enable Parallel Processing
Use `--parallel` to leverage multiple cores. Let Kelora auto-detect thread count first.

```bash
time kelora -j logs/2024-04-*.jsonl.gz \
  --parallel \
  -l error \
  -e 'track_count(e.service)' \
  --metrics
```

- Default threads = number of logical CPU cores.
- On heavily I/O-bound workloads, try more threads with `--threads 2x` the core count; for CPU-heavy transforms, use equal or fewer threads.

## Step 3: Tune Batch Size and Ordering
Batch size controls how many events each worker processes before flushing.

```bash
kelora -j logs/2024-04-*.jsonl.gz \
  --parallel --batch-size 5000 \
  -l error \
  --stats
```

Guidelines:

- 1000 (default) balances throughput and memory.
- Increase to 5000–10000 for simple filters on machines with ample RAM.
- Decrease to 200–500 when transformations are heavy or memory is constrained.

Ordering options:

- `--file-order name` for deterministic alphabetical processing.
- `--file-order mtime` to scan oldest/newest archives first.

## Step 4: Drop Ordering When Safe
If output order is irrelevant (metrics only, exports to sorted files), add `--unordered` for higher throughput.

```bash
kelora -j logs/2024-04-*.jsonl.gz \
  --parallel --unordered \
  -e 'track_count(e.service)' \
  -e 'track_count("errors")' \
  --metrics
```

- `--unordered` flushes worker buffers immediately; expect non-deterministic ordering in the output stream.
- Combine with `--metrics` or `--stats-only` when you only care about aggregates.

## Step 5: Automate and Monitor
Wrap the tuned command in a script so you can schedule it via cron or CI.

```bash
#!/usr/bin/env bash
set -euo pipefail

ARCHIVE_GLOB="/var/log/app/app-$(date +%Y-%m)-*.jsonl.gz"
OUTPUT="reports/errors-$(date +%Y-%m-%d).json"

kelora -j $ARCHIVE_GLOB \
  --parallel --unordered --batch-size 5000 \
  -l error \
  -k timestamp,service,message \
  -J > "$OUTPUT"

kelora -j "$OUTPUT" --stats
```

- Log `--stats` output for traceability (processed lines, parse errors, throughput).
- Capture timing with `/usr/bin/time` or `hyperfine` to detect future regressions.

## Variations
- **Mixed compression**  
  ```bash
  kelora -j logs/2024-04-*.jsonl logs/2024-04-*.jsonl.gz \
    --parallel --threads 8 \
    -l critical \
    --stats
  ```

- **Recursive discovery**  
  ```bash
  find /archives/app -name "*.jsonl.gz" -print0 |
    xargs -0 kelora -j --parallel --unordered \
      -e 'track_count(e.service)' \
      --metrics
  ```

- **Performance sweep**  
  ```bash
  for size in 500 1000 5000; do
    echo "Batch size $size"
    time kelora -j logs/2024-04-*.jsonl.gz --parallel --batch-size $size -q
  done
  ```

## Performance Checklist
- **CPU-bound?** Lower thread count or batch size to reduce contention.
- **I/O-bound?** Increase threads, run from fast storage, or pre-uncompress archives.
- **Memory spikes?** Reduce batch size, avoid large `window_*` or `emit_each()` operations, or process archives sequentially.
- **Need reproducible results?** Avoid `--unordered`, process files in a consistent order, and archive the command used.

## See Also
- [Roll Up Logs with Span Windows](span-aggregation-cookbook.md) for time-based summaries after processing archives.
- [Prepare CSV Exports for Analytics](process-csv-data.md) to clean flattened outputs before sharing.
- [Concept: Performance Model](../concepts/performance-model.md) for a deeper explanation of Kelora’s execution modes.
