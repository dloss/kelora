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
