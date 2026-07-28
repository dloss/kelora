# Multiline: why it still feels brittle, and what a better design looks like

Exploration of the current `--multiline` implementation (2026-07, v2.0.1).
Every failure mode below was reproduced against a debug build; repro commands
are included. Verdict up front:

**Yes, significant improvements are possible before v3.0.** Most of what makes
multiline brittle is not the strategy set — it's the *seams*: where chunking
sits in the pipeline, what the chunker can't see (file boundaries, time), what
runs before it (line filters), and what it reports (metadata). Those are
fixable in 2.x as bug fixes. The genuinely v3.0-sized items are a unified
condition model, language presets, and possibly a new default for joining.

---

## 1. Confirmed failure catalog

### 1.1 Blank lines corrupt events — differently per `-f` (data loss / split)

```
2024-01-01T10:00:00 ERROR boom
  at foo
                     <- blank line
  at bar
2024-01-01T10:00:01 INFO ok
```

```bash
kelora -f line blank.log --multiline indent --multiline-join=newline -F json
# {"line":"2024-01-01T10:00:00 ERROR boom\n  at foo"}
# {"line":"\n  at bar"}            <- event split; junk event created
kelora -f raw blank.log --multiline indent --multiline-join=newline -F json
# {"raw":"...boom\n  at foo\n  at bar"}   <- joined, but blank line silently DELETED
```

Cause: `process_line_sequential` / `handle_*_line` drop empty lines before the
chunker for every format except `line` (runner.rs:1572, batching.rs:891), and
the indent strategy treats a blank line as "not indented" ⇒ new event
(`is_line_indented` returns false, unit test `test_empty_line_handling_indent`
codifies it). Real stack traces (Python `traceback`, Java `Caused by` blocks,
Go panics) contain blank lines. Same input + same strategy ⇒ two different
corruptions depending on parser choice.

### 1.2 Hardcoded 400 ms idle flush splits events nondeterministically

```bash
( printf 'HEADER...\n'; sleep 0.6; printf '  cont\nNEXT HEADER...\n' ) \
  | kelora -f raw --multiline timestamp -F json
# HEADER emitted alone; "  cont" becomes its own event
```

`DEFAULT_MULTILINE_FLUSH_TIMEOUT_MS = 400` (pipeline/defaults.rs:6) is not
configurable, not documented in `--help-multiline` or the docs page, and fires
whenever a live pipe (`app | kelora`, `tail -f |`) pauses mid-event. The same
input produces different events depending on arrival timing — this directly
contradicts vision-and-design.md's "deterministic chunking strategies", and is
the most likely source of "sometimes it loses/splits events" reports.

### 1.3 Events merge across file boundaries; metadata is wrong

```bash
kelora -f raw file_a.log file_b.log --multiline timestamp \
  --exec 'e.file = meta.filename; e.n = meta.line_num' -F json
# {"raw":"...boom  at foo  at bar  at baz_from_next_file","file":"file_b.log","n":5}
# {"raw":"...INFO next","file":"file_b.log","n":5}
```

Three defects in one run:
- The unterminated event at EOF of `file_a.log` swallows the head of
  `file_b.log`. Nothing flushes the chunker on file change (only
  `FormatDetected` does, runner.rs:1376).
- `meta.filename` is the flush-time filename, not the event's origin.
- `meta.line_num` is the flush-time line counter in sequential mode; in
  `--parallel` it's `start_line_num + event_index` (worker.rs:647) — i.e. the
  index of the event in its batch, not a line number at all
  (event 2 spanning lines 4–6 reports `2`).

### 1.4 Regex patterns cannot contain `:` — which log headers are full of

```bash
kelora --multiline 'regex:match=^\d{2}:\d{2}' ...
# kelora: ... Unknown regex option: \d{2} (supported: match=..., end=...)
```

`MultilineConfig::parse` splits the whole value on `:` (config.rs:533). Any
time-of-day header — the single most common event-start anchor — is
inexpressible without `\x3A` gymnastics (mentioned only in a NOTES bullet of
`--help-multiline`). The error message compounds the confusion.

### 1.5 Line filters amputate events before assembly, silently

```bash
kelora -f raw kl.log --multiline timestamp --keep-lines 'ERROR' -F json
# {"raw":"2024-01-01T10:00:00 ERROR boom"}   <- stack trace gone
```

`--keep-lines`, `--ignore-lines`, `--skip-lines`, `--section` all run on
physical lines before chunking. Filtering-then-assembling is occasionally what
you want; usually it quietly deletes continuation lines — exactly the failure
multiline exists to prevent. No warning, no hint, and the docs never call it
out. (The `--levels` raw prefilter, by contrast, was correctly hooked in
*after* assembly — commit 009dcdf — so the precedent for "filters see events"
already exists.)

### 1.6 Timestamp strategy false positives; no format lock-in

```bash
# continuation line "17:03 was the incident window" -> new event
```

`TimestampDetector` accepts *any* prefix the adaptive parser can interpret
(up to 6 tokens / 64 chars, with punctuation-trimmed variants). A file whose
headers are all `2024-01-01T10:00:00` will still split on a continuation line
starting `17:03`, `Jan 5`, etc. The detector never locks onto the format that
actually matched the first headers, so its precision is capped by the most
permissive format the adaptive parser knows.

### 1.7 No safety cap on the buffer

A `regex:match=` that never matches again buffers the entire input in memory
with no warning, no max-lines, no max-bytes. `--help-multiline` shrugs
("If buffers grow unbounded, tighten the regex"). Every peer tool caps this
(Filebeat `max_lines` 500 default, promtail `max_lines`, Vector timeouts).

### 1.8 Small but telling inconsistencies

- args.rs:66 suggests `--multiline blank` — a strategy that doesn't exist.
  (It's also the missing feature in 1.1: "blank line separates events" /
  "blank line continues" isn't expressible.)
- `--multiline all` silently ignores `--multiline-join` (multiline.rs:92).
- `pipeline/mod.rs:287` still labels the `Chunker` trait "(future feature)".
- Default `--multiline-join=space` destroys stack-trace structure unless the
  user knows to add `--multiline-join=newline`; every doc example has to
  carry that flag — a sign the default is wrong for the primary use case.

### 1.9 Fragile internals: the `pending_output` band-aid

`Chunker::feed_line(&mut self, String) -> Option<String>` can only return one
event per input line, but one line can complete two (regex `start`+`end` both
matching). The fix was a one-slot `pending_output` buffer plus drop-silently
code for a third event (`produced.into_iter().next()`, multiline.rs:309).
Auditing this, correctness rests on an unstated invariant ("pending set ⇒
buffer empty") that holds today by accident of which strategies exist. Nothing
enforces it; the next strategy added can violate it and silently lose events.
The parallel path duplicates chunker driving (worker.rs `chunker_thread`) with
its own filename-juggling (`pending_event_filename`), which is where the
metadata bugs of 1.3 live.

What did check out: sequential and parallel produce identical events and order
on well-formed input (2000-event diff test), and the state machine does not
lose events in the strategies as currently shipped.

---

## 2. Root-cause summary

All of the above reduce to four design decisions:

1. **The chunker's input alphabet is too small.** It sees only
   `feed_line(String)` and `flush()`. It cannot see file boundaries, elapsed
   time, or byte/line budgets — so those concerns were bolted on outside it
   (or not at all), each seam growing its own bug.
2. **The chunker's output shape is too small.** `Option<String>` forced
   `pending_output`; returning bare strings forced metadata to be
   reconstructed downstream, wrongly.
3. **Chunking sits after per-line filtering** instead of being the first
   transformation on raw lines, so filters and empty-line policy corrupt
   events before assembly.
4. **The CLI micro-syntax and defaults optimize for the parser, not the
   user** (`:`-splitting, space join, undocumented timeout).

None of these are the *strategy set* being wrong. timestamp/indent/regex/all
is a reasonable core; peers converge on nearly the same set.

---

## 3. Proposal

### Tier 1 — fixable in 2.x, mostly bug-fix semantics

**T1.1 Reshape the chunker interface** (internal, enables everything else):

```rust
enum ChunkInput<'a> { Line(&'a str), FileBoundary, IdleTimeout, Eof }

struct AssembledEvent {
    text: String,           // joined per join-mode
    filename: Option<String>,   // of first line
    first_line_num: usize,
    line_count: usize,
}

trait Chunker: Send {
    fn feed(&mut self, input: ChunkInput<'_>, out: &mut Vec<AssembledEvent>);
}
```

Multiple events per feed become natural (delete `pending_output` and the
drop-third-event path); file-boundary flush becomes one match arm; metadata
becomes correct by construction in both sequential and parallel modes. The
parallel `chunker_thread` shrinks to a driver loop.

**T1.2 Flush on file boundary.** An event never spans input files. (If someone
truly needs concatenation, `cat a b | kelora` still expresses it.)

**T1.3 Make the idle flush explicit.** `--multiline-timeout DUR` (accepting
`0`/`off`). Default: off for regular files (input arrives at disk speed; the
timeout can only misfire), current 400 ms-ish for pipes/tty. When it fires
mid-stream, emit a 🔸 warning once per run ("multiline buffer idle-flushed
after 400ms; event may be split — tune --multiline-timeout"). Document it in
`--help-multiline` and the concepts page. This restores determinism for file
input and makes stream behavior visible and tunable.

**T1.4 Fix option parsing.** Split the `--multiline` value only on `:` that
introduces a known key (`:end=`, `:format=`, …); everything else stays in the
value. `regex:match=^\d{2}:\d{2}` then parses; `regex:match=^A:end=^B` still
works. Fix the error message; drop the `\x3A` advice.

**T1.5 Define blank-line policy.** When `--multiline` is active, blank lines
always reach the chunker (stop the pre-chunk empty-line drop for all formats).
Default: a blank line *continues* the current event (matches Python/Java/Go
trace reality); trailing blank lines are trimmed at flush. Add
`blank` as a first-class strategy (blank-line-separated records) — the error
message in args.rs already promises it.

**T1.6 Filters see assembled events.** With `--multiline` active, apply
`--keep-lines`/`--ignore-lines` to the assembled event text instead of
physical lines (grep-on-event semantics — almost always the intent), or at
minimum emit a hint when both are combined. `--section` likely stays
line-based (it's a stream selector), but document the interaction.

**T1.7 Safety cap.** `--multiline-max-lines N` (default e.g. 1000) and/or
max-bytes; on hitting the cap, flush and warn once (🔸). Prevents the silent
whole-file buffer.

**T1.8 Timestamp lock-in.** Once N (=2?) consecutive headers parse with the
same concrete format, require that format for subsequent headers;
`timestamp:loose` opts out. Kills the `17:03` false-positive class while
keeping zero-config startup.

**T1.9 Sweep the small stuff.** `all` + `--multiline-join` → warn or honor;
fix stale trait comment; align `--help-multiline` with reality.

**T1.10 Enforce the invariants with property tests.** The losslessness
invariant is checkable: for any input and any strategy,
`concat(events) == concat(surviving input lines)` (modulo join separators) —
no line dropped, duplicated, or reordered — plus "output is independent of
batch sizes / arrival timing (timeout off)" for sequential-vs-parallel. A
proptest would have caught 1.1, 1.2, 1.3, and any future `pending_output`
regression mechanically. This is the cheapest durable fix for "brittle".

### Tier 2 — v3.0 material

**T2.1 Unified condition model.** The four strategies become sugar over one
engine (the Vector/Filebeat lesson):

```
--multiline start=REGEX            # event begins at match (today: regex:match=)
--multiline start=timestamp        # timestamp-as-start-detector, same engine
--multiline cont=indent            # line continues previous (today: indent)
--multiline cont=REGEX             # e.g. cont='^\s|^Caused by'
--multiline cont-prev=REGEX        # continue if PREVIOUS line matches (trailing '\')
--multiline until=REGEX[,inclusive|exclusive]   # today: end=
```

This adds the two capabilities users currently cannot express at all —
"previous line signals continuation" (`continue_past`) and negated/or-ed
conditions — without a strategy zoo.

**T2.2 Language presets.** `--multiline java|python|go|rust|node|csharp`
mapping to tested rules (Fluent Bit's most-loved multiline feature). Presets
make the feature *intuitive*: users say what their logs are, not how to
tokenize them. Ship each preset with a corpus test file in `examples/`.

**T2.3 Rethink the join default.** Keep the physical lines internally and join
at output time: `newline` as the stored truth, and let the *formatter* decide
presentation (default formatter escapes/indents, `-F json` emits `\n`). Then
`--multiline-join` becomes a display concern and the "space destroys my stack
trace" default disappears. Breaking; belongs in 3.0.

**T2.4 Possible unification with `--section`.** Both features are boundary
engines over the line stream. Not urgent, but the T1.1 interface is the shared
substrate if it ever happens.

### Suggested sequencing

1. T1.1 interface reshape + T1.10 property tests (foundation, no behavior
   change intended — the proptests pin current good behavior first).
2. T1.2 file boundaries, metadata fixes (bug fixes).
3. T1.4 parsing, T1.3 timeout flag, T1.7 caps, T1.9 sweep (small, independent).
4. T1.5 blank policy, T1.6 filter ordering, T1.8 lock-in (behavior changes —
   changelog + hints; still defensible in 2.x as bug fixes, or gate on 3.0 if
   preferred).
5. T2.x behind the v3.0 gate.

---

## 4. What peers do (for reference)

| Tool | Model | Caps/timeout | Notable |
|---|---|---|---|
| Filebeat | `pattern` + `negate` + `match: after/before` | `max_lines` 500, `timeout` 5s | `flush_pattern` |
| Vector | `start_pattern` + `condition_pattern` + mode (`continue_through/continue_past/halt_before/halt_with`) | `timeout_ms` | modes = expressiveness |
| Fluent Bit | stateful rule machines | yes | **built-in java/python/go presets** |
| promtail | `firstline` regex | `max_wait_time`, `max_lines` | minimal but capped |
| lnav | automatic via per-format timestamp | — | format lock-in is why it feels magic |

Kelora's strategy set is competitive; the gaps are capping/timeout controls,
continuation modes, presets, and precision (lock-in).
