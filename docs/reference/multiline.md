# Multiline

`-M` (`--multiline`) joins several physical lines into one event before
parsing — stack traces, wrapped messages, pretty-printed payloads. The
[guide](../guide/parse.md#one-event-spans-several-lines) shows when to use it;
this page has the details. `kelora --help-multiline` is the terminal version.

![How the multiline strategies group lines](../images/multiline-strategies-diagram.png#only-light)
![How the multiline strategies group lines](../images/multiline-strategies-diagram-dark.png#only-dark)

## Strategies

| Strategy | A new event starts at | Notes |
|---|---|---|
| `timestamp` | a line beginning with a timestamp | locks onto the first timestamp format it sees |
| `timestamp:format=FMT` | a line beginning with a timestamp in this chrono format | e.g. `timestamp:format=%d/%b/%Y:%H:%M:%S %z` |
| `timestamp:loose` | a line beginning with any recognizable timestamp | for files that genuinely mix formats |
| `indent` | a line that is not indented | indented and blank lines continue the event |
| `blank` | the first line after one or more blank lines | the blank lines belong to no event |
| `regex:match=RE` | a line matching RE | |
| `regex:match=RE:end=RE2` | a line matching RE; the event ends at a line matching RE2 | lines between events become their own events |
| `java`, `python`, `go` | any line that is not part of a recognized stack trace | for output without timestamps |
| `all` | — | the whole input is one event, held in memory |

Only one strategy is active at a time.

### `timestamp`

The default choice for application logs. Detection locks onto the first
timestamp format it sees, so a continuation line that merely starts with
something time-like (`17:03 was the incident window`) does not split an event.
If the first line isn't a real header, pin the format with
`timestamp:format=…`.

### `indent`

For continuation lines that start with whitespace (Java traces, YAML). If the
first line of a block is itself indented, or a trace starts with an unindented
line such as `Traceback (most recent call last):` or `Caused by:`, use
`timestamp` or a language preset instead.

### Language presets: `java`, `python`, `go`

For stack traces in output without reliable timestamps: raw stderr, container
stdout, CI logs. Every line is its own event unless it belongs to a recognized
trace; a trace attaches to the line that logged it.

| Preset | Recognizes |
|---|---|
| `java` | exception lines, `at …` frames, `Caused by:`, `Suppressed:`, `... N more` |
| `python` | `Traceback (most recent call last):`, frames, the final exception line, chained exceptions, exception groups, `SyntaxError` blocks |
| `go` | `panic:`, `fatal error:`, `[signal …]`, goroutine headers and frames, blank lines between goroutine blocks |

A line starting with a timestamp always starts a new event, so presets are safe
on timestamped files — but `timestamp` is better there, because it also keeps
non-trace continuation lines with their event. A multi-line exception
*message* is recognized only up to its first line.

### `regex`

```bash exec="on" source="above" result="ansi"
kelora examples/multiline_boundary.log -f raw -M 'regex:match=^BEGIN:end=^END' \
  --multiline-join=newline -n 1 -F json
```

Patterns may contain colons (`match=^\d{2}:\d{2}`); only `:match=`, `:end=`,
and `:format=` act as separators.

## Joining lines

| `--multiline-join` | Joins with | Default for |
|---|---|---|
| `space` | a space | `timestamp`, `indent`, `blank`, `regex` |
| `newline` | `\n` | `all`, `java`, `python`, `go` |
| `empty` | nothing | — |

Use `newline` to keep a stack trace readable or to `split("\n")` it in a
script.

## Parsing a joined event

Grouping happens before parsing, so the joined block must still match the
format:

- `-f raw` and `-f line` accept anything.
- `regex:` patterns ending in `(?P<msg>.*)`, `-f syslog`, and the built-in
  application formats (`log4j`, `python-logging`, `iso8601-level`, …) capture
  the continuation lines in the message field, newlines included.
- Strict formats (`logfmt`, `combined`, `json` for a single-line object) fail
  on a block with stack frames in it, and the whole event is dropped — so
  turning multiline on can *reduce* the number of events. Kelora hints at this
  when a parse error lands on a grouped event.

## Streams, timeouts, and limits

| Option | Default | Effect |
|---|---|---|
| `--multiline-timeout DUR` | off for files, `400ms` for stdin/FIFOs | emit a buffered event after this much input inactivity; `0` = never |
| `--multiline-max-lines N` | `10000` | split an event after N lines and warn; `0` = unlimited; ignored by `all` |

Other rules:

- Events never span input files.
- `--keep-lines` and `--ignore-lines` act on physical lines *before* grouping.
- `meta.line_num` points at the event's first line.
- Trailing blank lines are trimmed from events (except with `all`).

## Troubleshooting

| Symptom | Likely cause and fix |
|---|---|
| every line is still its own event | the start rule never matched — Kelora hints once; check the regex, or use `timestamp` only if timestamps sit at the start of the line |
| everything is one event | same cause with an end-less rule; the line cap splits after 10 000 lines with a warning |
| events merge that should split | `timestamp` locked onto the wrong format: use `timestamp:format=…` |
| warning `joined N lines into M events` | the start rule recognizes only some records (lines that don't *begin* with a timestamp, a too-narrow `regex:match=`), so the rest are glued on. Fires at 3+ lines per event over 50+ lines when most joined lines start at column 0; indented stack traces don't trigger it |
| events split on a live stream | a pause longer than the timeout: raise `--multiline-timeout` |
| fewer events with `-M` than without | joined blocks no longer parse: use a free-text format (see above) |
| a preset splits a trace | unusual trace shape: check with `-f raw -F json -n 5`; fall back to `regex:` |

`--stats` shows lines read versus events created, which is the quickest way to
see what grouping did.
