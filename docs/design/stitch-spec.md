# `stitch` — design spec (draft)

> Status: exploratory design. Name provisional (`stitch` / `knit` / `coalesce`).
> Derived from Kelora's multiline + timestamp-detection ideas, extracted into a
> single-purpose tool per the "if your product is great, it doesn't need to be
> good" principle: do one thing, do it fast, and refuse to guess when it can't.

## 1. Purpose

One line: **turn a messy multiline log into one record per logical event — fast,
with zero configuration — or decline cleanly when it can't tell.**

`stitch` answers the question that comes *before* every other log tool can work:
*where does one event begin and end?* A stack trace is eight physical lines but
one event; `wc -l`, `grep -c`, and `jq` are all wrong about it until the
boundaries are fixed. `stitch` fixes them and re-emits the stream so the rest of
the toolchain composes.

```
messy.log ──stitch──▶ one-event-per-record ──▶ grep | wc | jq | kelora | duckdb
```

## 2. The problem and the gap

Every existing tool makes *you* supply the multiline rule:

- Logstash / Vector / Fluent Bit: a regex you write.
- lnav: a format you register.
- Kelora: a strategy you pick (`--multiline timestamp:… | regex:… | indent`).
- grep / jq / drain: assume one record per line already.

Nobody **infers** the boundary with no configuration. That is the empty niche
`stitch` targets — narrowly, and honestly (see §10).

## 3. Scope

### Does
- Infer logical-event boundaries from an unconfigured stream.
- Re-emit one record per event (NDJSON piped, human view on a TTY, NUL raw).
- Report what it inferred and how confident it was (`--explain`, to stderr).
- Decompress gzip transparently.

### Explicitly does NOT (the cut list)
- No filtering, no `--exec`, no scripting/Rhai.
- No field parsing beyond the free byproducts of boundary detection (§7).
- No output-format zoo, no config file, no plugins.
- No timestamp normalization (the long tail is a different problem; `ts` is
  emitted as the raw matched string).

If it needs a config file, it has already failed.

## 4. Core model

- **Physical line**: bytes between newlines in the source.
- **Logical event**: one or more consecutive physical lines that belong together.
- **Start signature**: an anchored, line-prefix predicate that marks the first
  physical line of an event. Everything until the next start is a *continuation*
  appended to the current event.

Byte round-trip is a guarantee: concatenating event bodies in order, with
newlines restored, reproduces the input exactly (§9).

## 5. Boundary inference

Three phases. Sampling decides the rule once; the hot loop applies it in a single
streaming pass.

### Phase 1 — sample
Read a head window (~256 KB) and a tail window (~256 KB). Split into physical
lines. Sampling keeps the decision O(1) in file size; the gate (Phase 3) guards
against a sample that misrepresents the file.

### Phase 2 — detect & lock a signature
Evaluate candidate signatures in priority order; for each, compute the fraction
of sampled lines it matches as an **anchored prefix**:

1. **`ts` — leading timestamp.** Delegated to a bounded-prefix timestamp
   recognizer (inspect only the first ~64 chars / few tokens, à la Kelora's
   `TimestampDetector`). The concrete matched format is locked for the full pass.
   This is the highest-value signature: stack traces, tracebacks, and wrapped
   messages all have *untimestamped* continuation lines, so it segments them for
   free.
2. **`json` — leading `{` or `[` with balanced, string-aware brace tracking.**
   Handles both pretty-printed multiline JSON (event starts at a column-0 `{`,
   ends when braces balance) and single-line JSONL (balance completes on the same
   line ⇒ one event per line). Braces inside strings and escapes are ignored.

The first signature whose match rate ≥ `T_high` (default **0.70**) wins and is
locked. No lower-value signatures (bare log level, indentation) are used in v1:
they are the ones that shred stack traces and over-segment YAML (§10), so they
are deliberately excluded.

### Phase 3 — decide (the gate)
- **Lock succeeded** (≥ `T_high`): segment. In the hot loop, a line starts a new
  event iff it matches the locked signature (one anchored regex match per line,
  or brace-balance bookkeeping for `json` mode); otherwise it is appended to the
  current event.
- **No signature reached `T_high`**: **refuse.** Emit one event per physical line
  (identity segmentation) and report on stderr: *"no clear multiline structure;
  treating each line as one event."*

Refusal is a feature, not a fallback failure: for mixed single-line logs (e.g.
`examples/nightmare_mixed_formats.log`, where every line is its own event in a
different format) refusal produces the **correct** result. The design goal is
that the worst outcome is a safe no-op, never silent event-count corruption.

## 6. The confidence gate is the product

The hard part of this tool is not stitching; it is reliably detecting its own
failure cases. The gate is therefore first-class:

- `T_high` (default 0.70) is exposed as `--min-confidence`.
- `--explain` prints, to stderr, the chosen signature, its match rate, event
  count, line count, and a multiline summary, e.g.:

  ```
  3 events from 8 lines
  signature: leading timestamp "YYYY-MM-DD HH:MM:SS"  (matched 97% of starts)
  1 multiline event (avg 5 lines) · largest: 5-line trace at L3
  ```

- When `stitch` refuses, `--explain` shows the best-scoring signature and why it
  fell short, so the user can decide whether to pre-process or accept line mode.

## 7. Output

Format follows the consumer, detected the way Kelora's own formatter does it:
pretty on a TTY, machine-readable when piped.

### 7.1 Piped/redirected → NDJSON (default)
One logical event per physical line of JSON:

```json
{"line":3,"lines":5,"ts":"2024-01-15 10:01:00","body":"2024-01-15 10:01:00 ERROR Failed to process request\nTraceback (most recent call last):\n  File \"/app/server.py\", line 42..."}
```

Chosen because it is the one format where `wc -l` is correct, and `jq`,
`duckdb read_json`, and `kelora -f json` all work — i.e. the structured consumers
you stitch in order to reach.

Field set (thin by design):

| Field   | Always | Meaning |
|---------|--------|---------|
| `body`  | yes    | Full original event text, internal newlines intact. |
| `line`  | yes    | Starting physical line number in the source. |
| `lines` | yes    | Physical line count. `jq 'select(.lines>1)'` = every multiline event. |
| `ts`    | when `ts` signature locked | Raw matched timestamp string; **not** normalized. |

No `level`, no parsed payload — that is Kelora's job, and adding it here rebuilds
the format zoo this tool exists to avoid. `--body-key` renames `body` (default
`body`, OTel-aligned; use `_raw` for Splunk-shaped audiences).

### 7.2 TTY → human view
Original multiline text preserved and unescaped, events separated by a faint rule
with an index/line gutter, detected `ts` dimmed:

```
─ #2  L3  2024-01-15 10:01:00 ──────────────
2024-01-15 10:01:00 ERROR Failed to process request
Traceback (most recent call last):
  File "/app/server.py", line 42, in handle_request
ValueError: Invalid JSON format at line 3
```

### 7.3 `--raw` / `-0` → NUL-delimited
Original bytes per event, separated by `\0`. Byte-exact, no escaping, for
`grep -z` / `xargs -0` / `awk RS='\0'`. The escape hatch when NDJSON's UTF-8
requirement would be lossy (§9).

## 8. CLI surface

```
stitch [OPTIONS] [FILE...]          # stdin if no FILE; gzip auto-detected

  -J, --json            Force NDJSON (override TTY pretty)
      --pretty          Force human view (override pipe NDJSON)
  -0, --raw             NUL-delimited original bytes (byte-exact)
      --explain         Print inference verdict to stderr
      --min-confidence  Gate threshold (default 0.70)
      --body-key NAME   NDJSON body field name (default "body")
      --max-event-lines Safety cap on a single event (default 100000)
      --no-color
  -h, --help    -V, --version
```

Target: ≤ 8 flags, none required for the common path.

## 9. Cross-cutting guarantees

- **stdout = data, stderr = everything else.** `--explain` and all diagnostics go
  to stderr; stdout stays clean NDJSON. (Matches Kelora's "rule of silence".)
- **Byte round-trip.** Bodies concatenated in order (newlines restored) reproduce
  the input exactly: no trimming, no whitespace normalization. CI invariant.
- **Non-UTF-8.** Logs contain invalid bytes. NDJSON lossily escapes/replaces;
  `--raw` stays byte-exact. The tradeoff is stated, never silent.
- **Performance.** Single streaming pass after sampling; one anchored match per
  line. Target ≥ 200 MB/s single-core on timestamp-led logs. No Rhai, no
  per-event boxing — the inverse of Kelora's speed/programmability tradeoff.
- **Memory.** Buffer only the current event, O(largest event) not O(file).
  `--max-event-lines` flushes pathological runaway events (e.g. a lone timestamp
  at the top of a huge file) with a stderr warning.
- **Exit codes.** 0 success; 1 read/IO error; 2 CLI misuse.

## 10. Known limitations (honest contract)

`stitch` targets timestamp-anchored and JSON logs. It does **not** solve the
general "infer any grammar" problem, and it will refuse rather than guess on:

- **Mixed single-line formats** (`nightmare_mixed_formats.log`): refuses ⇒ one
  event per line ⇒ *correct*.
- **YAML/indent records** (`multiline_indent.log`): one logical event spans many
  column-0 keys whose boundary (`event:`) is schema knowledge `stitch` cannot
  infer. Refuses ⇒ one line per event ⇒ *safe but unhelpful* (does not merge).
- **Backslash continuation / `BEGIN`…`END` markers** (`multiline_continuation`,
  `multiline_boundary`): no timestamp/JSON lead ⇒ refuses. Possible future opt-in
  modes, but out of v1 scope (that road rebuilds Kelora).
- **Timestamp long tail** (`timezones_mixed.log`): accuracy is bounded by the
  recognizer's format coverage (BSD syslog `Oct 25`, ctime, etc. are genuinely
  ambiguous).

The deep reason the universal version is rejected: stack traces demand
*conservative* start signals (so `Caused by:` stays glued) while mixed-format
logs demand *liberal* ones (so `timestamp=…` splits) — and those two are
lexically identical column-0 lines. No global rule separates them; the
distinction is semantic. v1 picks the conservative side and leans on the gate.

## 11. Acceptance criteria

Run against `examples/`. Expected behavior is the contract:

| File | Expected |
|------|----------|
| `multiline_stacktrace.log` | **segment** via `ts`; 8 events, traces glued |
| `syslog_multiline.log` | **segment** via `ts`; indented traces glued |
| `multiline_json_arrays.log` | **segment** via `json`; one event per object |
| `nightmare_mixed_formats.log` | **refuse**; one event per line (correct) |
| `multiline_indent.log` | **refuse**; one event per line (safe) |
| `multiline_boundary.log` | **refuse** |
| `multiline_continuation.log` | **refuse** |
| `timezones_mixed.log` | **segment** via `ts` to the extent formats are recognized |

Plus: byte round-trip holds on every file; `--explain` reports the right
signature and match rate; throughput target met on a large synthetic ts-led log.

## 12. Open questions

- `body` vs `_raw` default (audience-dependent; current pick: `body`).
- Should `json` mode emit the *parsed* object instead of `body` text when the
  whole event is one JSON value? (Tempting, but edges toward Kelora's territory.)
- `--min-confidence` default: 0.70 is a guess; tune against a real corpus.
- Standalone binary vs a `kelora stitch` subcommand. Standalone keeps the focus
  and the "great single thing" framing; a subcommand reuses plumbing but reopens
  the Swiss-army-knife trap.
