# Filter

Kelora can drop data at several points on its way through the pipeline. They
differ in what they can see and what they cost:

| Filter on | Sees | Option | Example |
|---|---|---|---|
| [raw lines](#raw-lines-before-parsing) | unparsed text — cheapest | `--keep-lines`, `--ignore-lines` | `--ignore-lines 'healthz'` |
| [position](#regions-of-a-file) | where a line sits in the file | `--section-*`, `--skip-lines`, `--head` | `--section-from 'stage: test'` |
| [time](#time-ranges) | the parsed timestamp | `--since`, `--until` | `--since 1h` |
| [level](#log-levels) | the level field | `-l`, `-L` | `-l error,warn` |
| [any field](#expressions) | everything, via a Rhai expression | `--filter` | `--filter 'e.status >= 500'` |
| [count, sample](#first-n-and-samples) | event order, a hash of a value | `-n`, `bucket()` | `-n 20` |
| [neighbours](#context-around-matches) | events around each match | `-A`, `-B`, `-C` | `-l error -C 2` |
| [history](#duplicates-and-first-occurrences) | earlier events | `state` in `--exec` | first occurrence per user |

The rule of thumb: filter as early as the information allows. A line dropped
before parsing costs almost nothing; a `--filter` that runs after an expensive
`--exec` pays for both.

## Raw lines, before parsing

`--keep-lines REGEX` keeps only matching lines; `--ignore-lines REGEX` drops
matching lines. They run before parsing, so they work even on lines that
wouldn't parse, and they are the fastest way to cut a big file down.

```bash exec="on" source="above" result="ansi"
kelora examples/ci_pipeline.log --keep-lines 'payments' -c
```

The regex is unanchored: use `^…$` to match a whole line. Both options act on
physical lines, so with [multiline](parse.md#one-event-spans-several-lines)
grouping they can remove a line from the middle of an event — use `--filter`
for whole events.

## Regions of a file

To process only part of a file — one test run, one deploy, everything after a
restart — mark the start and end with regexes:

```bash exec="on" source="above" result="ansi"
kelora examples/ci_pipeline.log --section-from 'stage: test' --section-before 'stage: ' -c
```

| Option | Section starts or ends |
|---|---|
| `--section-from RE` | starts at the matching line (included) |
| `--section-after RE` | starts after the matching line |
| `--section-before RE` | ends before the matching line |
| `--section-through RE` | ends after the matching line (included) |
| `--max-sections N` | stop after N sections |

Without an end option, a section runs to the end of the input. `--skip-lines N`
skips the first N lines (a preamble); `--head N` reads only the first N lines
and stops reading — useful for trying a command on a huge file.

## Time ranges

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl --since 2024-01-15T10:20:00Z --until 2024-01-15T10:25:00Z -c
```

`--since` and `--until` accept absolute times (`2024-01-15 10:00`), relative
times (`1h`, `-30m`, `yesterday`), and anchors (`--until since+15m`). They read
the timestamp the parser found and run before every event filter and script, so events
without a timestamp are dropped (Kelora warns when that happens). Details and
time zones: [Work with Time](time.md).

## Log levels

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error,critical -k level,message
```

`-l` keeps the listed levels, `-L` drops them (`-L debug,trace`). Matching is
case-insensitive. They read the event's level field, whichever of `level`,
`lvl`, `severity`, `loglevel`, … it has. `-l` takes names, not a threshold: see
the [FAQ](../faq.md#how-do-i-filter-warn-and-above) for "WARN and above".

## Expressions

`--filter` keeps an event when a [Rhai](scripting.md) expression is true. `e`
is the current event:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz \
  --filter 'e.status >= 500 && e.method == "POST"' -k ts,status,method,path -n 3
```

Expressions you will use most:

| Test | Expression |
|---|---|
| equal, compare | `e.level == "ERROR"`, `e.duration_ms > 1000` |
| one of several values | `e.status in [500, 502, 503]` |
| substring | `e.msg.contains("timeout")` |
| regex | `e.msg.matches("time ?out")` |
| glob, case-insensitive glob | `e.path.like("/api/*")`, `e.host.ilike("WEB-*")` |
| prefix, suffix | `e.path.starts_with("/admin")`, `e.file.ends_with(".php")` |
| IP ranges | `e.ip.is_in_cidr("10.0.0.0/8")`, `e.ip.is_private_ip()` |
| field present / absent | `e.has("user")`, `!e.has("user")` |
| nested field | `e.get_path("user.role", "") == "admin"` |
| which file | `meta.filename.contains("api")` |
| negate | `!(e.msg.contains("healthz"))` |

A missing field compares as false, so `--filter 'e.duration_ms > 1000'`
skips events without `duration_ms`. Calling a method on a missing field is an
error; Kelora reports it and treats the event as not matching. See
[missing fields](scripting.md#missing-fields).

`--filter` takes a single expression; for logic that needs `let` statements,
use `--exec` and drop events there with `e = ()`.

Repeat `--filter` for AND. Filters and `--exec` stages run in the order you
write them, so a filter can use a field computed by an earlier `--exec`:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl \
  -e 'e.duration_s = e.get("duration_ms", 0) / 1000.0' \
  --filter 'e.duration_s > 60' -k service,duration_s
```

Inside an `--exec` script, `e = ()` drops the current event — handy when the
decision is part of a longer script.

## First N and samples

`-n N` (`--take`) stops after N events have been output. To sample, hash a
stable value, so the same requests or users are kept on every run and in every
file:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --filter 'e.ip.bucket() % 10 == 0' --stats | grep Events
```

`--filter 'sample_every(10)'` keeps every tenth event; `--filter 'sample_prob(0.1)'`
a random 10 %.

## Context around matches

Like `grep -A/-B/-C`: show events before and after each match. In the default
format, a marker shows each event's role: `◉` (or `*`) for a match, `/` before,
`\` after, `|` between two matches.

```bash exec="on" source="above" result="ansi"
kelora examples/ci_pipeline.log -l error -B 1 -A 1 -c
```

A match is an event that passes the filters (`--filter`, `-l`, `-L`) placed
before any `--exec`. With `--parallel`, context is ignored (with a warning).

## Duplicates and first occurrences

Filters that depend on earlier events — the first error per service, drop
repeated messages — need memory across events. That is what `state` is for:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl \
  --filter '!state.contains(e.service)' \
  --exec 'state[e.service] = true' -k service,message
```

See [Cross-Event Logic](state.md).

## Order matters

Kelora runs the time range first, then your `--filter`, `-l`, `-L`, and
`--exec` stages in the order you typed them, then `-n`. `-k`/`-K` choose which
fields are printed, after all filtering; they hide an event only if none of its
fields are left. The complete order is in
[How It Works](../how-it-works.md#processing-order).
