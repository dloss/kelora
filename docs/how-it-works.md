# How It Works

A Kelora command is a pipeline: each line of input passes through a fixed
sequence of steps, and every option belongs to one of them. Knowing the
sequence explains most surprises — why a filter can't see a field, why a count
differs from the printed events, why an event vanished.

## The pipeline

```text
read → select lines → join lines → parse → time range → your stages → spans → pick fields → output
```

| Step | Question it answers | Options |
|---|---|---|
| **Read** | Which bytes? | files, globs, stdin, `.gz`/`.zst`, `--file-order`, `--merge-sorted` |
| **Select lines** | Which raw lines count? | `--skip-lines`, `--head`, `--section-*`, `--keep-lines`, `--ignore-lines` |
| **Join lines** | What is one event? | `--extract-prefix`, `-M`/`--multiline` |
| **Parse** | Which fields does it have? | `-f`, `--ts-field`, `--ts-format`, `--input-tz`, `--input-year` |
| **Time range** | Which period? | `--since`, `--until` |
| **Your stages** | Which events, and what changes? | `--filter`, `-e`/`--exec`, `-E`, `-l`, `-L`, `--assert` |
| **Spans** | Which window? | `--span`, `--span-idle`, `--span-close` |
| **Pick fields** | Which fields, how many events? | `--normalize-ts`, `-k`, `-K`, `-n` |
| **Output** | In what form? | `-F`, `-J`, `-o`; or a summary: `-s`, `-m`, `--freq`, `--drain`, `-d` |

Every command picks what it needs from each step and takes the defaults for
the rest. `kelora access.log -l error --freq path` reads one file, selects
every line, joins nothing, auto-detects the format, keeps errors, and outputs a
frequency table.

## Processing order

Events go through the steps in the order of the table. Around and within
them:

- `--begin` runs once before any input is read; `--end` runs once after the
  last event, followed by metrics and statistics.
- The time range drops events outside it, and events without a timestamp,
  before any of your stages — so counts always match the range.
- **Your stages run in the order you wrote them**: `--filter`, `--exec`, `-E`,
  `-l`, `-L`, and `--assert`, interleaved as on the command line. An event
  dropped by one stage never reaches the next.
- In the pick-fields step, `--normalize-ts`, `-k`/`-K`, and `-n` always apply in
  that order, wherever you typed them.

So:

- `-k` picks fields after your stages have run. An event left with no fields
  at all is not printed, so `-k status` also hides events without `status` —
  but your filters and counts saw them.
- `--freq`, `--describe`, and `--card` count events at the end of your
  stages, so they agree with what would have been printed. A `track_*()` call
  counts wherever you put it: before a `--filter`, it sees every event.
- `--since` can't use a timestamp you computed in `--exec`: the range has
  already been applied. Use a `--filter` after the `--exec` instead.

## Stages and their scope

Each `--filter` and `--exec` runs as a separate script with its own scope.

| Survives to the next stage | Doesn't |
|---|---|
| fields you set on `e` | `let` variables |
| `state` (sequential mode) | functions, unless loaded with `-I` for each stage |
| metrics recorded with `track_*()` | |

What each kind of script can use:

| Script | Can use |
|---|---|
| `--begin` | `conf` (writable), `read_lines()`, `read_file()` |
| `--filter`, `--exec` | `e`, `meta`, `conf` (read-only), `state`, `window` (with `--window`), `track_*()` |
| `--span-close` | `span`, `metrics`, `conf`, `state` |
| `--end` | `metrics`, `state`, `conf` |

The [script variables reference](reference/script-variables.md) lists every
variable and field.

## When something goes wrong

By default a bad line or a failing script is counted and skipped, and the run
continues:

| Problem | Default behavior |
|---|---|
| a line doesn't parse | the line is skipped and counted |
| a `--filter` fails on an event | the event counts as not matching |
| an `--exec` fails on an event | that stage's changes are rolled back; the event continues with the fields it had before |
| an `--assert` fails | the event is reported on stderr and still passes through |

At the end, a short summary on stderr names the problem and the first
affected lines. `-v` prints each error as it happens.

The exit code stays 0 as long as the run did its job, and is 1 when it
couldn't ([details](reference/exit-codes.md)). With `--strict`, the first
parse, filter, or script error stops the run with exit 1 — use it where bad
input must not pass silently.

An `--exec` stage is all-or-nothing: if its third statement fails, the first
two are undone too. So split independent work into separate `--exec` stages
when partial results are useful, and keep dependent work in one.

## What Kelora prints

A successful run prints its data and nothing else. Everything Kelora has to
say about the run goes to stderr, in four kinds:

| Kind | Marker | When | Turn off with |
|---|---|---|---|
| error | ⚠️ | the run hit a problem it had to report | `--silent` (a one-line fatal message remains) |
| warning | 🔸 | something is likely wrong, but the run continued | `--no-warnings` |
| hint | 💡 | a likely mistake with a concrete fix | `--no-hints` |
| status | 🔹 | what Kelora decided: detected format, loaded config | shown only with `-v` |

Warnings and hints appear only when something specific is off — a format
that fell back to plain lines, a filter that compares a number with a string —
never as routine chatter. Summary modes (`-s`, `-m`, `--freq`, `--drain`,
`-d`) keep their output clean by hiding hints and script `print()` output;
warnings still appear.

## Sequential and parallel

By default Kelora processes one event at a time, in order, and writes each
result immediately — right for `tail -f`, for anything that compares events,
and for interactive use.

`--parallel` splits the input into batches and processes them on all CPU
cores. Output stays in input order (unless you add `--unordered`), and
`track_*()` metrics from all workers are merged. Order-dependent features
don't work in parallel; [Big Files, Many Files](guide/files.md#what-parallel-cant-do)
lists what happens to each.

## Memory

Kelora streams, so most commands use little memory regardless of file size.
What does accumulate:

| Feature | Holds |
|---|---|
| `track_freq`, `track_unique`, `--freq` | one entry per distinct value — use `--card` for fields with millions of values |
| `state` | whatever your script stores |
| `--span-close` with `span.events` | every event of the current span |
| `--window N` | N events |
| `-M` | the lines of the current event (at most 10 000 by default; `-M all` holds the whole input) |
| `--merge-sorted` | one event per input file |
