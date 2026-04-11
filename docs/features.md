# Core Features

Kelora focuses on practical log-analysis workflows you can run, iterate on, and share from the terminal.

## Highlights at a glance

- **Pattern mining** with Drain templates (`--drain`)
- **Embedded JSON extraction** from text (`extract_json()`)
- **Deterministic sampling** for repeatable slices (`bucket()`)
- **Pseudonymization helpers** for privacy-safe analytics
- **Span windows** for rollups and service-level summaries
- **Composable output** for JSON/CSV/table pipelines

## Parse mixed input formats in one workflow

Process JSON, Logfmt, syslog, CSV/TSV, and unstructured lines without rewriting your pipeline for each source.

- Auto-detect common formats or force a parser explicitly
- Parse custom text with column and regex strategies
- Mix files and compressed inputs in a single run

See also: [Format Reference](reference/formats.md)

## Scriptable filtering and transformation with Rhai

Start with simple predicates and grow into richer logic as needed.

- `--filter` for expressive conditions
- `--exec`, `--begin`, `--end` for transformation and lifecycle stages
- Reusable helper scripts via includes and config aliases

See also: [Introduction to Rhai](tutorials/intro-to-rhai.md), [Advanced Scripting](tutorials/advanced-scripting.md)

## Extract structure from messy logs

Promote hidden structure into first-class fields so downstream analysis is easier.

- Extract embedded JSON from text payloads (`extract_json()`)
- Parse key-value fragments and nested paths
- Flatten nested arrays/objects for tabular output

See also: [Power-User Techniques](how-to/power-user-techniques.md), [Flatten Nested JSON for Analysis](how-to/fan-out-nested-structures.md)

## Stateful streaming analysis

Maintain running state while reading events, without handing off to a separate script.

- Counters, sums, minima/maxima, and per-key tracking
- Sliding windows and span-style rollups
- End-stage summaries and report-style outputs

See also: [Metrics and Tracking](tutorials/metrics-and-tracking.md), [Roll Up Logs with Span Windows](how-to/span-aggregation-cookbook.md)

## Output for downstream workflows

Shape results for humans or tools without post-processing scripts.

- Emit JSON, CSV/TSV, Logfmt, and table-friendly output
- Select fields and control formatting for report-style views
- Keep outputs stable so commands are easy to reuse and share

## Pattern discovery and normalization

Find repeated error shapes quickly, even when values differ.

- Template mining with Drain (`--drain`)
- Normalize UUID/IP/email-like values into comparable patterns
- Group noisy events into actionable clusters

See also: [Pattern Discovery Example](index.md#3-pattern-discovery-template-mining)

## Deterministic sampling and high-volume triage

Reduce volume while preserving stable slices across repeated runs.

- Deterministic bucketing with `bucket()`
- Selective inspection of high-volume traffic
- Combine sampling with field filters and aggregations

See also: [Power-User Techniques](how-to/power-user-techniques.md#deterministic-sampling-with-bucket)

## Privacy-aware processing

Analyze behavior without exposing sensitive identifiers.

- Built-in pseudonymization and hashing helpers
- Preserve joinability while masking raw values
- Support safer sharing of derived outputs

See also: [Pseudonymize Identifiers for Analytics](how-to/pseudonymize-identifiers-for-analytics.md), [Sanitize Logs Before Sharing](how-to/extract-and-mask-sensitive-data.md)

## Built for tool composition

Kelora does not require an all-or-nothing workflow.

- Pipe data in/out of `grep`, `jq`, `qsv`, and shell tools
- Export JSON/CSV for notebooks, BI, or follow-up automation
- Keep transformations reproducible as single commands

See also: [Integrate Kelora with External Tools](how-to/integrate-external-tools.md)
