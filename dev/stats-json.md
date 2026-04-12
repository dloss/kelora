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

### `--stats-json-fields` (default: off)

Include per-field coverage statistics in the report. Off by default because it
requires tracking field presence across all events, with memory proportional to
the number of distinct fields seen.

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
    "bytes_read": 104857600,
    "aborted": false
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
  changes. New fields may be added without a version bump; consumers must
  ignore unknown fields. `null` is used for "not available" throughout, never
  field omission, so consumers can rely on key presence.

**`run`**

- `started_at` / `finished_at`: ISO 8601 UTC wall-clock timestamps.
- `wall_seconds`: float, elapsed seconds.
- `mode`: `"sequential"` or `"parallel"`.
- `input_format`: format string as passed (e.g. `"json"`, `"json,syslog"` for
  cascade, `"auto"`).
- `sources`: array of input paths. `["<stdin>"]` for stdin.
- `bytes_read`: total bytes consumed across all sources. `null` if unavailable
  (some stdin scenarios).
- `aborted`: `true` if the run was cut short by `exit(1)` in a script;
  `false` otherwise.

**`events`**

- `read`: lines/chunks presented to the parser.
- `parsed`: successfully parsed into events.
- `parse_errors`: lines that failed parsing.
- `parse_error_rate`: `parse_errors / read`, float.
- `filtered_out`: events dropped by `--filter` scripts.
- `emitted`: events written to output.
- `assert_failures`: count of `assert_fail()` calls. Equal to
  `run.total_failures` in the assert report when `--assert-report` is also
  active. `null` when `assert_fail` was never called during the run.

**`time_range`**

- `first` / `last`: ISO 8601 UTC timestamps of earliest and latest events
  parsed from logs (not wall clock).
- `source_field`: which field was used for timestamps (e.g. `"ts"`,
  `"timestamp"`). `null` if no timestamp field was found.
- Entire `time_range` object is `null` if no timestamps were parsed.

**`parse_errors.sample`**

- Up to 5 parse error examples, sampled from across the run (not just the
  first 5).
- `raw` is truncated at 200 characters.
- `line_number` is 1-based within the source file. `null` for stdin.
- `source_size`: always 5. Consumers should not assume all 5 slots are filled.

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
  across all events. Useful for detecting accidental type mixing.
- Nested fields are dot-notated (`"request.headers.content_type"`), up to the
  same depth cap as `--discover`.
- `truncated`: `true` if more than 200 distinct fields were seen.
- `truncated_at`: 200 when truncated, `null` otherwise.
- `fields` is `null` (not the object) when `--stats-json-fields` is not set.

---

## Interaction with other flags

- `--stats`: both can be set simultaneously; `--stats-json` does not suppress
  human-readable output.
- `--assert-report`: when both are active, `events.assert_failures` reflects
  the same count as `run.total_failures` in the assert report.
- `--metrics-file`: complementary, not overlapping. Metrics from `track_*()`
  functions appear in `--metrics-file`; aggregate run statistics appear here.
- `--discover-final`: separate output, not subsumed by `--stats-json`.
- Parallel mode: counts are aggregated correctly across threads, following the
  existing internal stats aggregation.
- Aborted run (`exit(1)` in a script): stats file is written with whatever was
  collected; `run.aborted` is set to `true`.

---

## Schema stability contract

- `kelora_stats_version` increments only on breaking changes (field removal,
  type change, semantic change).
- New fields may be added at any version without a version bump.
- `null` is used for "not available" throughout; consumers can rely on all
  documented keys being present.

---

## What is not in scope

- Per-file breakdown (bytes, events per source) — aggregate only.
- Histogram of event timestamps — `time_range.first`/`last` is sufficient.
- Metrics from `track_*()` functions — covered by `--metrics-file`.
