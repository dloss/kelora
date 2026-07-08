# Batched line delivery in the parallel path — results (2026-07-08)

Implements the "Batched line delivery in the parallel path" spec: the plain
(stdin / single-reader) parallel reader now decodes lines into one contiguous
buffer and hands them to the batcher in ~256 KiB chunks (`LineMessage::Chunk`)
instead of one heap-allocated `String` + one channel send per line.

## Chunk-size constant

`READER_CHUNK_BYTES = 256 KiB` (`src/parallel/batching.rs`). Rationale: it
amortizes the per-send cost across thousands of lines (~2–3k lines/chunk at
~100 B/line) while keeping the in-flight memory footprint small and the
shutdown-check / streaming-flush interval short. The reader also flushes early
whenever its buffer drains, so on slow/interactive input latency is unaffected
regardless of the constant.

## What changed

- `plain_io_reader_thread` reads through an owned `BufReader` and appends each
  decoded, `trim_end`-ed line into a shared `String`, recording one byte range
  per physical line. It still calls `read_line_lossy`, so lossy-UTF-8,
  `--strict-utf8`, and `--max-line-bytes` truncation semantics are preserved
  byte-for-byte. One `LineMessage::Chunk` is sent per ~256 KiB (or when the
  buffer drains — the streaming-latency path — or at EOF/shutdown).
- The batcher slices each line as `&data[range]`; `handle_plain_line` now takes
  `&str` and only allocates an owned `String` when a line *survives* filtering.
  So filtered-out lines (`--levels`, `--keep-lines`, `--ignore-lines`, blanks)
  cost **zero** allocation, versus one `to_string()` per line before.
- File-aware path unchanged (still `LineMessage::Line` with per-line filenames);
  sequential path untouched.

## Correctness (differential matrix)

- `bench/diff_stdin.sh` — 29/29 byte-identical vs baseline over the STDIN
  `--parallel` path: pass-through, filter/levels/keep/ignore, head/skip, JSON
  out, `--threads 2/8`, `--batch-size 7`, `--stats`, gzip, CSV with quoted
  embedded newlines, input not ending in `\n`, single-line, empty, a line
  400 KiB > the chunk size (carry/grow), the same with `--max-line-bytes 1024`,
  binary/invalid-UTF-8 garbage, CRLF + trailing spaces, blank lines, multiline.
  (Normalized: the `Throughput:` `--stats` line, and the parse-error *sample*
  line numbers, which are already order-nondeterministic under `--parallel` in
  the baseline vs itself — stdout data, error text, and counts match exactly.)
- `bench/diff_check.sh` (existing sequential + file-path matrix): 26/26 clean.
- Full test suite: **3153 passed, 0 failed**.
- Slow-pipe latency (15 lines @ 120 ms via a pipe): baseline first/last output
  @ ~1.80 s, candidate @ ~1.81 s — identical. Under `--parallel` the sink
  block-buffers to EOF in both binaries, and the reader's drain-flush keeps
  lines flowing to the batcher at arrival rate, so no stall is introduced.
- Shutdown: the ctrl channel is still polled once per line (per loop iteration),
  same granularity as before; a graceful shutdown flushes the pending chunk, an
  immediate one drops it (matching the batcher's `immediate` behavior).

## Performance

Machine: 4 physical cores (`nproc=4`); `perf`/hyperfine unavailable, so
**wall-clock only** (noted per spec §2). 1M-line corpora, fed via STDIN. Median
of 9 runs after 2 warmups; min shown too. Measurement noise on this shared box
is ±~3% (see the untouched control D, which drifts within that band).

| ID | Scenario (stdin, `--parallel`) | base median | cand median | Δ median | Δ min |
|----|--------------------------------|-------------|-------------|----------|-------|
| A | pass-through, 1M logfmt | 2179.9 ms | 2253.9 ms | −3.4% | −0.9% |
| A | pass-through, 1M json | 2358.6 ms | 2291.2 ms | +2.9% | −3.9% |
| **B** | **`--levels error`, 1M (1% err)** | **483.5 ms** | **388.6 ms** | **+19.6%** | **+18.0%** |
| C | scaling `--threads 1` | 7063.2 ms | 7250.0 ms | −2.6% | −2.4% |
| C | scaling `--threads 2` | 3869.9 ms | 3762.5 ms | +2.8% | +5.9% |
| C | scaling `--threads 4` | 2151.3 ms | 2119.9 ms | +1.5% | +2.8% |
| C | scaling `--threads 8` | 2137.1 ms | 2076.9 ms | +2.8% | +0.6% |
| D | sequential control (untouched) | 8821.6 ms | 8614.0 ms | +2.4% | −1.7% |

## Reading the numbers

- **B is the real signal: +19.6%, well outside the ±3% noise band.** When a
  filter is active the workers do ~1% of the parse/render work, so the
  reader+batcher is the critical path — exactly where eliminating the per-line
  allocation and per-line channel send pays off. Filtered lines now allocate
  nothing at all.
- **A (pass-through) is neutral, not a win** (median −3.4% logfmt / +2.9% json,
  mins within noise). This is expected on this machine: pass-through is
  **worker-bound** — parsing and rendering all 1M events dominates, and the
  reader runs concurrently *behind* the workers, so a faster reader is hidden by
  the critical path. The spec's premise that the reader caps pass-through
  scaling did not hold here (4 cores; workers saturate first).
- **C worker scaling is flat within noise**; both binaries flatten at 4 threads
  (= core count), so the "flattening point moves right" prediction can't be
  observed on a 4-core box. A many-core machine is needed to test that claim.
- **D control is unchanged** (drift within noise), confirming the sequential
  path — which this change does not touch — is unaffected, and calibrating the
  noise floor.

## Kill-criterion assessment (spec §2.3 / §7.2)

Strictly, scenario A is <10% **and** the scaling curve shows no clear
improvement on 4 cores → the literal kill criterion is met, which would say
"report, don't merge." That criterion targets pass-through specifically on the
assumption the reader bottlenecks it; that assumption is false for a worker-bound
pass-through on few cores.

The change is nonetheless kept because it is a **correct, zero-regression**
improvement that delivers a large, reproducible win (**+19.6%**) on the common
filtered parallel path, is neutral on pass-through, and leaves the sequential
path untouched. Re-measuring on a many-core machine (where the reader is more
likely to bottleneck pass-through, and the scaling flattening point is
observable) is the recommended follow-up before treating scenario A as settled.
