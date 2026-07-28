# Kelora v3 Behavior Notes

Candidate behavior changes to consider while moving Kelora to v3.0. Same
standard as the [v2 notes](v2-behavior-notes.md): only changes that remove a
place where Kelora is inconsistent with itself or silently hides data loss are
worth a major version.

## High-Value Breaking Changes

### Position `--since`/`--until` by Command-Line Order

**What:** Treat `--since`/`--until` as user stages placed at the position they
appear on the command line, the way `--filter`, `--levels`, and `--exec`
already are — instead of hoisting the time window ahead of every user stage.

Current behavior (2.x): the window is a fixed-position stage that runs first,
before any user stage. This landed as the fix for windowed aggregates reporting
whole-file numbers (`--since … --freq prog` naming a program that had no events
in the window at all). Running the window first was the only way to make those
counts honest inside a streaming pipeline: once an out-of-window event has
passed through an `--exec`, its `track_freq` contribution has already been
recorded and cannot be retracted.

Why revisit it: the fix is correct but it makes the time window the one
selection whose position you cannot control, and the pipeline model otherwise
promises that the order you write is the order that runs. Two consequences a
user cannot currently express:

- **Metrics over a wider span than the printed events.** The documented
  workaround is to drop `--since` and write the narrowing as
  `--filter 'meta.parsed_ts >= to_datetime("…")'` after the tracking stage.
  That works, but it means re-expressing a first-class flag as a hand-written
  filter, and the user has to know that `meta.parsed_ts` is the field to reach
  for.
- **Deriving the field the window reads.** `--since`/`--until` only ever see
  the timestamp the *parser* resolved. A timestamp that must be assembled —
  split date/time columns, an odd epoch unit, a timestamp inside a message —
  cannot be fed to the window at any position, because `parsed_ts` is
  write-once and set at parse time. Under CLI-order positioning, an `--exec`
  placed before the window could plausibly be allowed to populate it.

**Design questions to settle first:**

1. Does a script-writable `parsed_ts` make sense, or does CLI-order
   positioning only fix the aggregate-span case while leaving derived
   timestamps to `--filter` anyway? If the latter, the change buys less than
   it looks like it does. Note that `parsed_ts` being write-once is also what
   makes the window's verdict independent of script order today, which is a
   property worth losing deliberately rather than accidentally.
2. Where does a bare `--since` with no explicit position go? Defaulting to
   "first" preserves 2.x behavior and keeps aggregates honest for the common
   case; defaulting to "wherever it appears" is more consistent but silently
   re-introduces the whole-file-aggregates bug for anyone who happens to write
   `--exec` before `--since`.
3. If the window can be placed late, a windowed `--freq` can once again
   disagree with a windowed `--keys`. That is defensible when the user chose
   the order explicitly, but it needs a warning or at least a documented
   contract, because the disagreement is exactly the bug 2.x fixed.
4. `emit_each` events currently bypass the window entirely (they carry no
   parser timestamp, and the window has already run by the time they exist).
   CLI-order positioning would let a window placed after the emit stage see
   them — which is arguably the right answer, and worth folding into the same
   change rather than fixing separately.

**Not urgent.** The 2.x behavior is self-consistent and documented, and the
`--filter` workaround covers both gaps. This is a consistency improvement, not
a bug fix, so it should wait for a major version.

**Affected:** `src/pipeline/builders.rs` (both the sequential and parallel
stage builders hoist `TimestampFilterStage`), `src/config` stage ordering,
`docs/concepts/pipeline-model.md` ("Why the time window runs first" would be
rewritten).

### Return `()` Instead of `""` When a String Function Finds Nothing

**What:** Have the extraction and slicing functions report "nothing there" as
`()` (explicitly absent) instead of `""`, so a non-matching extraction removes
the field rather than leaving an empty one.

Current behavior (2.x): `extract_regex`, `extract_ip`/`extract_url`/
`extract_email`/`extract_domain`/`extract_json`, and the slicing family
(`before`, `after`, `between`, `starting_with`, `ending_with`) all return `""`
when there is no match or no delimiter; the plural forms return an empty array.
`or_empty()` exists to convert that to `()`, and `??` supplies a placeholder.

Why revisit it ([#342](https://github.com/dloss/kelora/issues/342)): the empty
string is a value, so a non-matching extraction is indistinguishable from a real
empty capture. It survives `!= ()`, and in `--freq` the non-matching events
collect into the largest bucket under an empty label — the one number a reader is
most likely to misread:

```
$ kelora Linux_2k.log --exec 'e.ip = e.msg.extract_regex(#"rhost=(\S+)"#, 1)' --freq ip
ip                            1503     <- events with no rhost= at all
ip   150.183.249.110            80
```

**Design questions to settle first:**

1. Is `""` really the wrong answer, or just the wrong *default*? These are string
   functions whose results are routinely concatenated and re-sliced; under `()`,
   `e.msg.extract_regex(p, 1).lower()` becomes a runtime error instead of `""`,
   and every chained call needs a `?? ""` or a guard. The parser type-annotation
   convention (`()` for a value that cannot satisfy its type) applies where
   nothing downstream chains off the value.
2. Scope: the whole family, or `extract_regex` alone? Alone is the smaller
   change but trades one inconsistency for another — `after()` and `between()`
   have exactly the same "not found" case.
3. Do the plural forms move too? An empty array is already falsy-ish and
   iterates zero times, so `()` mostly costs `for x in e.codes` its safety.
4. Would a narrower fix cover the reported case — `--freq` labelling an empty
   bucket distinctly, or a hint when a tracked field is empty on most events?
   That addresses the misread number without touching any return type.

**Not urgent.** `or_empty()` and `??` both express the intended behavior today,
and 2.x now documents the `""` return in `--help-functions`, `--help-rhai`, and
the function reference. This is a consistency question, not a defect.

**Affected:** `src/rhai_functions/strings/regex_ops.rs`,
`src/rhai_functions/extractors.rs`, `src/rhai_functions/strings/ops.rs` (the
slicing family), plus every doc example that chains off an extraction.

### Multiline: Unified Condition Model, Presets, and Join-as-Display

Background: `dev/multiline-exploration-2026-07.md` catalogs the 2.x multiline
failure modes and splits the fixes into a 2.x tier (seam fixes: file-boundary
flush, correct per-event metadata, colon-safe option parsing, blank-line
policy, `--multiline-timeout`, `--multiline-max-lines`, timestamp lock-in)
and this v3 tier. The 2.x tier changes no strategy semantics; everything
below does, which is what makes it v3 material.

**What (three related changes):**

1. **Unified condition model.** Replace the closed strategy set
   (timestamp/indent/regex/blank/all) with one boundary engine the strategies
   become sugar over:

   ```
   --multiline start=REGEX          # event begins at match (2.x regex:match=)
   --multiline start=timestamp      # timestamp-as-start-detector, same engine
   --multiline cont=indent          # line continues previous event (2.x indent)
   --multiline cont=REGEX           # e.g. cont='^\s|^Caused by|^$'
   --multiline cont-prev=REGEX      # continue while PREVIOUS line matches
                                    #   (trailing-backslash logs; Vector's
                                    #   continue_past — inexpressible in 2.x)
   --multiline until=REGEX[,inclusive|exclusive]   # 2.x regex:end=
   ```

   Conditions compose (`start=` + `cont=`), which covers Filebeat's
   `negate`/`match` matrix and Vector's four modes without a mode flag.

2. **Language presets.** `--multiline java|python|go|rust|node|csharp`
   mapping to tested rules in the unified model (Fluent Bit's most-loved
   multiline feature). Each preset ships with a corpus file under `examples/`
   and a golden test, so preset drift is caught by CI. Presets make the
   feature match how users think — "these are Python tracebacks", not "write
   me a boundary regex".

3. **Join becomes a display concern.** Store assembled events
   newline-joined as the single source of truth; formatters decide
   presentation (default formatter escapes or indents continuation lines,
   `-F json` emits `\n` as today). `--multiline-join` then only affects
   output, and the 2.x footgun — the default `space` join silently
   destroying stack-trace structure unless the user knows to pass
   `--multiline-join=newline` — disappears. Every doc example currently
   carries that flag, which is the tell that the default is wrong.

**Why v3, not 2.x:** all three change observable output for existing
commands: (1) retires the `strategy:key=value` micro-syntax (keep the old
spellings as aliases for one release), (3) changes what `e.raw` contains for
every multiline user who relied on space-joining. (2) is additive but only
pays for itself on top of (1)'s condition engine.

**Design questions to settle first:**

1. Alias lifetime: do `timestamp|indent|blank|all` remain permanently as
   sugar (they read better than `start=timestamp`), or are they deprecated
   spellings? Recommendation: keep permanently; presets and sugar are the
   primary UX, the condition model is the escape hatch.
2. Does `start=` + `until=` + `cont=` need a conflict rule, or do they
   compose (start opens, cont extends, until closes)? Compose seems right but
   needs a truth table before implementation, including what a line that
   matches both `start=` and `until=` does — the 2.x `pending_output` bug
   lived exactly there.
3. With join-as-display, what does `-f json` parse when a multiline JSON
   payload was assembled — the newline-joined text (breaks `{"a": 1,\n"b"}`
   parsers that choke on newlines? no, serde handles it) or a join the parser
   requests? Likely a non-issue; verify with the CEF/logfmt parsers, which
   are line-oriented.
4. Should `--section` be re-expressed over the same boundary engine? Both
   are stream-boundary machinery; unification is appealing but `--section`
   selects while `--multiline` groups, and forcing them together may cost
   more clarity than it saves. Decide after the engine exists.
5. Preset governance: how do preset rules evolve without silently changing
   users' event boundaries? Presets are versioned with kelora itself; a
   changed preset is a changelog-visible behavior change, never a patch-level
   tweak.

**Affected:** `src/pipeline/multiline.rs` (engine), `src/config.rs` (option
model), `src/main.rs`/`src/args.rs` (flag wiring), `src/help/multiline.rs`,
`docs/concepts/multiline-strategies.md`, `examples/` (preset corpora), the
default formatter (display-time join), CHANGELOG migration notes.
