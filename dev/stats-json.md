# Spec: `--stats-json`

## Problem

Kelora's existing `--stats` output is human-readable and unstable across
versions. Machine consumers (CI artifacts, dashboards, evidence packs) need a
stable, versioned JSON equivalent.

---

## New flags

### `--stats-json=<path>`

Write a machine-readable run summary to `<path>` at end of processing.
Compatible with all processing modes (sequential, parallel, streaming). Does
not suppress `--stats` human output if both are specified. Does **not** require
`--allow-fs-writes` (Kelora-initiated write, same as `--metrics-file`).

Written only when the run completes normally. If `exit(1)` aborts the run, no
stats file is written (same behaviour as `--metrics-file`).

### `--stats-json-fields` (default: off)

Include per-field coverage statistics in the report. Off by default because it
requires tracking field presence across all events, with memory proportional to
the number of distinct fields seen.

**Not supported in parallel mode.** When `--stats-json-fields` is set and
`--parallel` is active, Kelora emits a warning to stderr and the `fields` key
is `null` in the report.

---

## Report format

```json
{
  "kelora_stats_version": 1,
  "run": {
    "started_at": "2026-04-11T09:14:00Z",
    "finished_at": "2026-04-11T09:14:22Z",
    "wall_seconds": 22.1,
    "mode": "sequential",
    "input_format": "json",
    "sources": ["app-2026-04-11.jsonl", "app-2026-04-10.jsonl"],
    "bytes_read": null
  },
  "events": {
    "read": 948000,
    "parsed": 947100,
    "parse_errors": 900,
    "parse_error_rate": 0.00095,
    "filtered_out": 12400,
    "emitted": 934700,
    "assert_failures": null
  },
  "time_range": {
    "first": "2026-04-10T00:00:01Z",
    "last": "2026-04-11T09:13:59Z",
    "source_field": "ts"
  },
  "parse_errors": {
    "sample": [
      {
        "line_number": 4401,
        "source": "app-2026-04-10.jsonl",
        "raw": "{malformed",
        "reason": "unexpected end of input"
      }
    ],
    "sample_size": 5
  },
  "fields": null
}
```

### Field reference

**Top-level**

- `kelora_stats_version`: integer, always 1. Incremented only on breaking
  changes (field removal, type change, semantic change). New fields may be
  added without a version bump; consumers must ignore unknown fields. `null` is
  used for "not available" throughout, never field omission, so consumers can
  rely on key presence.

**`run`**

- `started_at` / `finished_at`: ISO 8601 UTC wall-clock timestamps. Captured
  via `Utc::now()` at run start and at the point the stats file is written;
  this is new infrastructure (not derived from the existing `Instant`-based
  throughput timer).
- `wall_seconds`: float, elapsed seconds.
- `mode`: `"sequential"` or `"parallel"`.
- `input_format`: format string as passed on the CLI (e.g. `"json"`,
  `"json,syslog"` for cascade, `"auto"`). Taken from config, not from
  auto-detection results — use `--stats` human output to see what `"auto"`
  resolved to.
- `sources`: array of input paths exactly as passed on the CLI, in order.
  `["<stdin>"]` for stdin. Sourced from config, not from runtime measurement;
  glob patterns are expanded before this list is populated.
- `bytes_read`: always `null` in v1. Byte tracking requires reader-level
  instrumentation that is not yet implemented. Reserved for a future version;
  consumers should treat it as always `null` and not branch on its presence.

**`events`**

- `read`: lines/chunks presented to the parser (`lines_read` in internal
  stats).
- `parsed`: events successfully parsed (`events_created`).
- `parse_errors`: lines that failed parsing (`lines_errors`).
- `parse_error_rate`: `parse_errors / read` as a float. `0.0` when `read` is
  zero.
- `filtered_out`: events dropped by `--filter` scripts (`events_filtered`).
- `emitted`: events written to output (`events_output`, not `lines_output`;
  counts events before any `--emit-each` expansion).
- `assert_failures`: count of `assert_fail()` calls. Equal to
  `run.total_failures` in the assert report when `--assert-report` is also
  active. `null` when `assert_fail` was never called during the run.

**`time_range`**

- `first` / `last`: ISO 8601 UTC timestamps of earliest and latest events
  parsed from logs (not wall clock).
- `source_field`: the timestamp field name used for `first`/`last`. When
  multiple field names are observed across events, the most frequently seen
  name is used. If `--timestamp-field` is set, that value is used regardless.
  `null` if no timestamp field was found.
- Entire `time_range` object is `null` if no timestamps were parsed.

**`parse_errors`**

- `sample`: up to 5 structured parse error examples, sampled from across the
  run (not just the first 5). Each entry has:
  - `line_number`: 1-based within the source file. `null` for stdin.
  - `source`: input file path. `"<stdin>"` for stdin.
  - `raw`: the failing line, truncated at 200 characters.
  - `reason`: the parser's error message.
- `sample_size`: maximum number of samples collected (5). The `sample` array
  may have fewer entries. Requires a new structured sample vec in
  `ProcessingStats`; the current `recoverable_error_samples: Vec<String>` is
  insufficient and must be replaced or supplemented.

---

## `fields` object (with `--stats-json-fields`)

```json
"fields": {
  "kelora_fields_version": 1,
  "event_count": 947100,
  "top_fields": [
    {
      "name": "ts",
      "present_in": 947100,
      "coverage": 1.0,
      "types_seen": ["string"]
    },
    {
      "name": "level",
      "present_in": 946800,
      "coverage": 0.9997,
      "types_seen": ["string"]
    },
    {
      "name": "request_id",
      "present_in": 610000,
      "coverage": 0.644,
      "types_seen": ["string", "null"]
    }
  ],
  "field_count": 24,
  "truncated": false,
  "truncated_at": null
}
```

- `top_fields`: sorted by `present_in` descending. Capped at 200 fields.
- `types_seen`: all distinct Kelora-inferred types observed for that field
  across all events (`string`, `int`, `float`, `bool`, `null`, `array`,
  `map`). Useful for detecting accidental type mixing.
- Nested fields are dot-notated (`"request.headers.content_type"`), up to the
  same depth cap as `--discover`.
- `truncated`: `true` if more than 200 distinct fields were seen.
- `truncated_at`: 200 when truncated, `null` otherwise.
- `fields` is `null` (not the object) when `--stats-json-fields` is not set,
  or when running in parallel mode.

---

## Implementation notes

Fields sourced from existing infrastructure, mapped directly:

| Report field | Internal source |
|---|---|
| `events.read` | `stats.lines_read` |
| `events.parsed` | `stats.events_created` |
| `events.parse_errors` | `stats.lines_errors` |
| `events.filtered_out` | `stats.events_filtered` |
| `events.emitted` | `stats.events_output` |
| `events.assert_failures` | `stats.assertion_failures` (or `null`) |
| `time_range.first/last` | `stats.first_timestamp` / `stats.last_timestamp` |
| `run.input_format` | `config.input.format.to_display_string()` |
| `run.sources` | `config.input.files` |
| `run.mode` | `config.should_use_parallel()` |

Fields requiring new infrastructure:

| Report field | What's needed |
|---|---|
| `run.started_at` / `finished_at` | Capture `Utc::now()` at start and write time |
| `parse_errors.sample` (structured) | New `Vec<ParseErrorSample>` in `ProcessingStats` |
| `bytes_read` | Reader-level byte counter; deferred to future version (`null` for now) |

---

## Interaction with other flags

- `--stats`: both can be set simultaneously; human output is not suppressed.
- `--assert-report`: `events.assert_failures` reflects `run.total_failures`.
- `--metrics-file`: complementary. `track_*()` metrics go there; run
  statistics go here.
- `--discover-final`: separate output, not subsumed by `--stats-json`.
- Parallel mode: base stats aggregated correctly across threads via existing
  merge step. `fields` is `null` (see above).

---

## Schema stability contract

- `kelora_stats_version` increments only on breaking changes.
- New fields may be added at any version without a version bump.
- `null` is used for "not available" throughout; consumers can rely on all
  documented keys being present.

---

## What is not in scope

- Per-file breakdown (bytes, events per source) — aggregate only.
- Histogram of event timestamps — `time_range.first`/`last` is sufficient.
- Metrics from `track_*()` functions — covered by `--metrics-file`.
- Partial stats on `exit(1)` abort — no file is written if the run is cut
  short.
