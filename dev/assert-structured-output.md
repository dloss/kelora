# Spec: `--assert` Structured Output

## Problem

Assert failures via `exit(1)` in Rhai scripts give no structured information to
callers. CI systems get a non-zero exit code and whatever landed on stderr —
unparseable for artifact storage, PR comments, or downstream tooling.

---

## New Rhai function: `assert_fail`

```
assert_fail(message)
assert_fail(message, context)
```

- `message`: string, required. Human-readable description of the violation.
- `context`: map, optional. Arbitrary key-value pairs attached to the failure
  record. Typically `e` or a subset of it, but callers decide. Values that are
  maps or arrays serialize as-is. Values with no JSON-native representation
  (custom types, closures, etc.) are converted via `to_string()`. If omitted,
  recorded as `{}`.

Callable from any stage (`--begin`, `--filter`, `--exec`, `--assert`,
`--end`). Does not halt processing. Returns unit.

```rhai
// Per-event example
if e.level == "debug" && e.env == "prod" {
    assert_fail("debug log in production", #{
        level: e.level,
        service: e.service,
        ts: e.ts,
    });
}

// Aggregate example (in --end)
if metrics.error_rate > 0.05 {
    assert_fail("error rate exceeded threshold", #{
        actual: metrics.error_rate,
        threshold: 0.05,
    });
}
```

`exit(1)` in scripts continues to work as today — immediate abort, no
structured output. If `exit(1)` is called, the run aborts and **no assert
report is written**. Scripts can use both: `assert_fail` for structured
collection during normal processing, `exit(1)` for truly unrecoverable
conditions.

---

## New flags

### `--assert-report=<path>`

Write structured failure report to `<path>` as JSON at end of processing. If
omitted, failures are still counted and influence exit code, but no file is
written. Path is created or overwritten each run. Written even when there are
zero failures (useful for idempotent CI artifact storage).

Does **not** require `--allow-fs-writes`. This is a Kelora-initiated write
(like `--metrics-file`), not a script-initiated one.

### `--assert-max-failures=<n>` (default: 100)

Stop collecting failure *records* after `n` entries. A separate counter
continues incrementing for every `assert_fail` call regardless, so
`run.total_failures` is always accurate. Processing continues to completion.
The report marks truncation explicitly. Zero means unlimited (use with care on
high-volume streams).

---

## Report format

```json
{
  "kelora_assert_version": 1,
  "run": {
    "exit_code": 1,
    "total_failures": 4,
    "truncated": false,
    "truncated_at": null,
    "sources": ["checks/prod.rhai", "--assert"]
  },
  "failures": [
    {
      "message": "debug log in production",
      "stage": "exec",
      "source": "checks/prod.rhai",
      "line_number": 4401,
      "source_file": "app-2026-04-11.jsonl",
      "context": {
        "level": "debug",
        "service": "auth",
        "ts": "2026-04-11T09:14:22Z"
      }
    },
    {
      "message": "error rate exceeded threshold",
      "stage": "end",
      "source": "--end",
      "line_number": null,
      "source_file": null,
      "context": {
        "actual": 0.067,
        "threshold": 0.05
      }
    }
  ]
}
```

### Field reference

**Top-level**

- `kelora_assert_version`: integer, always 1. Incremented only on breaking
  changes (field removal, type change, semantic change). New fields may be
  added without a version bump; consumers must ignore unknown fields.

**`run`**

- `exit_code`: 0 if no failures, 1 otherwise.
- `total_failures`: total `assert_fail` calls across the run, including those
  past the truncation limit.
- `truncated`: `true` if `--assert-max-failures` was hit and some records were
  dropped.
- `truncated_at`: the `--assert-max-failures` value that triggered truncation,
  or `null`.
- `sources`: deduplicated list of script paths or flag names where `assert_fail`
  was called, in first-seen order. For file-backed scripts (`--exec-file`,
  etc.), the file path. For inline scripts, the flag name (`"--exec"`,
  `"--assert"`, etc.). Multiple inline scripts at the same flag are
  indistinguishable and all appear as the same name.

**`failures[]`**

- `stage`: one of `begin`, `filter`, `exec`, `assert`, `end`.
- `source`: script path if from a `--*-file` flag, otherwise the flag name.
- `line_number`: 1-based line number within `source_file` of the event that
  triggered the failure. `null` for `begin`/`end` stage failures and `null`
  for stdin input.
- `source_file`: input file path the triggering event came from. `null` for
  `begin`/`end` stage failures. `"<stdin>"` for stdin input.
- `context`: the map passed as second argument, or `{}` if omitted.

---

## Exit code behaviour

- Any call to `assert_fail` during the run → exit 1, regardless of
  `--assert-report`.
- `--assert-report` controls file output only, not exit code.
- If `exit(1)` is called in a script: run aborts immediately, no report is
  written.

---

## Stderr output

**`--assert-report` not set** — one line per failure:

```
[assert] debug log in production (exec, app-2026-04-11.jsonl:4401)
[assert] error rate exceeded threshold (end)
```

**`--assert-report` set** — suppress per-failure lines, print only:

```
4 assertion failure(s). Report written to report.json
```

The `[assert]` prefix is intentionally bracket-style (not emoji) to distinguish
user-defined assertion outcomes from Kelora's own operational errors (`⚠️`).
`--no-emoji` suppresses the `[assert]` prefix, leaving bare message text.
Both modes respect `--quiet` / `--silent` (stderr suppressed when those flags
are active).

---

## Interaction with `--stats-json`

When both flags are active, `events.assert_failures` in the stats report
reflects the same count as `run.total_failures` in the assert report.

---

## What is not in scope

- No built-in assert helpers (`assert_equals`, `assert_range`, etc.) — callers
  write conditions in Rhai and call `assert_fail` explicitly.
- No `--assert-fail-fast` — `exit(1)` covers that use case.
- No diff against a previous report.
- No partial report when `exit(1)` aborts the run.
