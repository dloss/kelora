# Glossary

**`()`** — Rhai's "nothing", like `null`: a missing field reads as `()`, and
assigning `()` removes a field. [Missing fields](guide/scripting.md#missing-fields)

**Alias** — a named set of options stored in a config file and used with
`-a NAME`. [Configuration](guide/config.md)

**Auto-detection** — choosing the input format by sampling the input; the
default when no `-f` is given. [Get Logs into Shape](guide/parse.md#1-let-auto-detection-do-it)

**Cascade** — a list of formats tried in order on each line (`-f json,line`);
each event records the winner in `_format`. [Get Logs into Shape](guide/parse.md#3-mixed-files-try-several-parsers-per-line)

**Context lines** — events shown before and after each match (`-A`, `-B`,
`-C`). [Filter](guide/filter.md#context-around-matches)

**Core fields** — timestamp, level, and message; `-c` shows only these.

**Diagnostics** — what Kelora reports about a run on stderr: errors (⚠️),
warnings (🔸), hints (💡), and, with `-v`, status (🔹).
[What Kelora prints](how-it-works.md#what-kelora-prints)

**Drain** — the algorithm behind `--drain`, which groups messages into
templates. [Summarize](guide/summarize.md#message-templates)

**Event** — one log record after parsing: a map of fields. Usually one line;
with multiline grouping, several. In scripts it is `e`.

**Field** — a named value in an event, such as `level` or `status`. Values can
be strings, numbers, booleans, maps, or arrays.

**Late event** — with time spans, an event whose timestamp falls in a span that
already closed; it passes through but isn't counted in that span.

**Level** — an event's severity (`ERROR`, `WARN`, …), read from a field such as
`level` or `severity`. Filtered with `-l` and `-L`.

**`meta`** — information about the current event that isn't one of its fields:
`meta.line`, `meta.line_num`, `meta.filename`, `meta.parsed_ts`.
[Script variables](reference/script-variables.md)

**Metrics** — values collected by `track_*()` functions and by `--freq`,
`--describe`, `--card`; printed with `-m`, readable as `metrics` in `--end`.
[Summarize](guide/summarize.md)

**Multiline** — joining several physical lines into one event before parsing
(`-M`). [Multiline reference](reference/multiline.md)

**Naive timestamp** — a timestamp without a zone offset, read in `--input-tz`
(or `TZ`, else UTC). [Time zones](guide/time.md#time-zones)

**Parallel mode** — processing batches of events on all CPU cores
(`--parallel`). [Big Files, Many Files](guide/files.md#make-it-faster)

**Resilient mode** — the default: lines that fail to parse and scripts that
fail on an event are reported and skipped, and the run continues.
[How It Works](how-it-works.md#when-something-goes-wrong)

**Rhai** — the scripting language used in `--filter`, `--exec`, and the other
script options. [Transform with Scripts](guide/scripting.md)

**Span** — a group of consecutive events: a time window, N events, a run of the
same field value, or a burst between pauses. [Group into Spans](guide/spans.md)

**Stage** — one `--filter`, `--exec`, `-l`, `-L`, or `--assert` step. Stages
run in command-line order. [How It Works](how-it-works.md#processing-order)

**State** — the `state` map, which keeps values from one event to the next.
[Cross-Event Logic](guide/state.md)

**Stats** — the processing summary printed by `-s`: counts, errors, time span,
fields seen.

**Strict mode** — `--strict`: the first parse or script error stops the run
with exit code 1.

**Template** — a message pattern with its variable parts replaced by
placeholders, as found by `--drain`: `Connection timeout after <duration>`.

**Time range** — the period selected with `--since` and `--until`, applied
before any other filter. [Work with Time](guide/time.md#filter-by-time)

**Window** — with `--window N`, the current event and the N before it,
available to scripts as `window`. [Cross-Event Logic](guide/state.md#-window-look-at-previous-events)
