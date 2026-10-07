# Get Logs into Shape

Everything in Kelora — filters, scripts, metrics, output — works on **fields**.
Parsing turns each line into an event with named fields. This page is a ladder:
start at the top, and stop at the first rung that gives you the fields you need.

| Rung | Use when | Tool |
|---|---|---|
| 1 | The format is common | nothing — auto-detection |
| 2 | You know the format, or detection guessed wrong | `-f json`, `-f logfmt`, `-f syslog`, … |
| 3 | One file mixes formats | cascade: `-f json,line` |
| 4 | Whitespace-separated columns | `-f 'cols:ts(2) level *msg'` |
| 5 | Anything with a pattern | `-f 'regex:(?P<ts>\S+) …'` |
| 6 | Fields are buried inside a text field | `absorb_kv()`, `extract_regex()`, … in `--exec` |

Two adjustments apply at any rung: [join multi-line events](#one-event-spans-several-lines)
before parsing, and [tell Kelora which field is the timestamp](#make-the-timestamp-work)
if it can't find it.

## How to tell whether parsing worked

Parsed events show several named fields. An unparsed event has a single `line`
field, and Kelora prints a hint:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_line.log -n 2
```

For a field-by-field profile with types and sample values, use `--discover`
(`-d`). Use `-v` to see which format was detected.

## 1. Let auto-detection do it

With no `-f`, Kelora samples the input and picks a parser. It recognizes JSON,
syslog, CEF, Apache/Nginx access logs, Kubernetes CRI, logfmt, CSV/TSV, and a
set of application-log layouts (`log4j`, `python-logging`, `glog`,
`nginx-error`, `postgres`, `iso8601-level`, …):

```bash exec="on" source="above" result="ansi"
kelora -v examples/app.log -n 2
```

Detection runs once per run (on files, it samples the head plus a few probes
deeper in). If a file mixes a dominant format with stray text lines, it
switches to a cascade automatically — see rung 3. On stdin, it uses the first
line only.

## 2. Name the format

Pass `-f` when detection guesses wrong, or in scripts, so a future detection
change can't surprise you. `-j` is short for `-f json`.

```bash exec="on" source="above" result="ansi"
kelora -f logfmt examples/simple_logfmt.log -n 2
```

Most formats keep their natural types: JSON numbers stay numbers, and logfmt
values that look numeric become numbers. **CSV/TSV values are strings** unless
you annotate them:

```bash exec="on" source="above" result="ansi"
kelora -f 'csv status:int bytes:int' examples/simple_csv.csv -k path,status,bytes -n 2
```

[Format reference](../reference/formats.md) lists every format and the fields
it produces; `kelora --help-formats` prints the same in the terminal.

## 3. Mixed files: try several parsers per line

A comma-separated list is a **cascade**: each line goes to the first parser
that accepts it, and the event records the winner in `_format`. Typical case:
JSON logs interrupted by plain-text banners and stack traces.

```bash exec="on" source="above" result="ansi"
kelora -f json,line examples/mixed_format.log -k _format,level,msg,line -n 5
```

Put strict parsers first and catch-alls (`line`, `raw`) last. `cols:` and
`regex:` can't go in a comma list (their specs contain commas) — repeat `-f`
instead:

```bash
kelora -f json -f 'cols:ts(2) level *msg' app.log
```

`cols:` accepts any line, so it can only be last; a `regex:` declines lines it
doesn't match and can sit anywhere.

Different formats in different *files* (JSON from one service, logfmt from
another)? Use `-f auto-per-file` to detect each file separately.

## 4. Columns: `cols:`

For logs whose fields are separated by whitespace, describe the columns in
order:

```text title="examples/cols_fixed.log"
--8<-- "cols_fixed.log:1:3"
```

```bash exec="on" source="above" result="ansi"
kelora -f 'cols:ts(2) level service *msg' examples/cols_fixed.log -n 3
```

| Token | Meaning |
|---|---|
| `name` | one column |
| `name(3)` | three columns, joined (e.g. a date and a time) |
| `-` / `-(2)` | skip one / two columns |
| `*name` | the rest of the line (must be last) |
| `name:int` | convert: `int`, `float`, `bool` |

Use `--cols-sep '|'` for a different separator.

## 5. Patterns: `regex:`

When columns aren't enough — brackets, quotes, `key=` prefixes — write a regex
with **named groups**. Each group becomes a field; `:int`, `:float`, `:bool`
convert it.

```text title="examples/regex_custom_format.log"
--8<-- "regex_custom_format.log:1:3"
```

```bash exec="on" source="above" result="ansi"
kelora examples/regex_custom_format.log \
  -f 'regex:(?P<ts>\S+) \[(?P<level>\w+)\] code=(?P<code:int>\d+) duration=(?P<ms:float>[\d.]+)ms msg="(?P<msg>[^"]*)"' \
  -n 3
```

- The pattern must match the **whole line** (it is anchored automatically).
  Lines that don't match are counted as parse errors and skipped — or, in a
  cascade, handed to the next parser.
- Unnamed groups `(?:…)` are allowed for structure; only named groups become
  fields.
- `.` also matches newlines, so a trailing `(?P<msg>.*)` captures a whole
  [multi-line event](#one-event-spans-several-lines).

`kelora --help-regex` has more patterns and common mistakes.

## 6. Finish the job in a script

Often the line has a clean prefix and a messy rest: free text with
`key=value` pairs, embedded JSON, a bracketed host. Parse the prefix, then pull
fields out of the remainder in `--exec`.

```text title="examples/incident_story.log"
--8<-- "incident_story.log"
```

The prefix is timestamp, source, and an optional `[node]`; the rest mixes
logfmt pairs, quoted values, and free text. A regex takes the prefix, and two
`absorb_*` calls handle the rest — `absorb_logfmt` for lines that are pure
logfmt, `absorb_kv` (which keeps the leftover text in `msg`) for the others:

```bash exec="on" source="above" result="ansi"
kelora examples/incident_story.log \
  -f 'regex:(?P<ts>\S+) (?P<source>[^\[ ]+)(?:\[(?P<node>[^\]]+)\])? (?P<msg>.*)' \
  -e 'if e.absorb_logfmt("msg").status != "applied" { e.absorb_kv("msg") }'
```

The tools for this rung:

| Function | Pulls out |
|---|---|
| `e.absorb_kv("f")` | `key=value` tokens; leftover text stays in `f` (not quote-aware) |
| `e.absorb_logfmt("f")` | a field that is entirely logfmt (quote-aware, typed) |
| `e.absorb_json("f")` | a JSON object stored as a string |
| `e.absorb_regex("f", pattern)` | named groups from a regex |
| `e.f.extract_regex(pattern, 1)` | one capture group |
| `e.f.between("[", "]")`, `.after(":")`, `.before(" ")` | text around delimiters |
| `e.f.extract_json()`, `.extract_ip()`, `.extract_url()` | the first JSON / IP / URL in free text |
| `e.f.parse_url()`, `.parse_user_agent()`, `.parse_jwt()` | structured values into maps |

`absorb_*` functions merge the extracted fields into the event, remove the source
field when it is fully consumed, and return a status (`"applied"`,
`"parse_error"`, …) you can branch on, as above. The full list is in the
[function reference](../reference/functions.md#parsing-functions).

The same approach parses a field that is itself a log line — a JSON payload
inside a Kubernetes CRI `msg`, a `key=value` list inside a syslog message:

```bash exec="on" source="above" result="ansi"
kelora examples/pod_cri.log -e 'e.absorb_json("msg")' -k ts,stream,level,msg,status -n 5
```

Lines whose `msg` isn't JSON (the panic) keep their text; `absorb_json` just
returns a `parse_error` status for them.

Quick column grabs work too: `e.line.col(2)` returns the third whitespace column,
and `e.line.split(" ")` an array.

## One event spans several lines

Stack traces, wrapped messages, and pretty-printed payloads occupy several
physical lines. `-M` (`--multiline`) joins them into one event **before**
parsing:

```bash exec="on" source="above" result="ansi"
kelora examples/multiline_stacktrace.log -M timestamp -l error -n 1
```

| Strategy | A new event starts at | Good for |
|---|---|---|
| `timestamp` | a line beginning with a timestamp | most application logs with stack traces |
| `indent` | a line that is not indented | indented continuations without timestamps |
| `java`, `python`, `go` | anything that isn't a recognized trace line | raw stderr, container output, CI logs |
| `blank` | the line after a blank line | paragraph-style reports |
| `regex:match=^BEGIN[:end=^END]` | your start (and optional end) pattern | explicit record markers |
| `all` | — (the whole input is one event) | whole-file processing |

Lines are joined with spaces by default; add `--multiline-join=newline` to keep
the line structure (the presets do this already). Multiline grouping happens
before parsing, so the joined block must still match your format — free-text
parsers (`line`, `raw`, regex with a trailing `.*`, the built-in application
formats) handle that. Details: `kelora --help-multiline` and
[Multiline reference](../reference/multiline.md).

## Prefixes added by other tools

`docker compose logs` and similar tools prepend `name | ` to each line.
`--extract-prefix` moves it into a field before the rest is parsed:

```bash exec="on" source="above" result="ansi"
kelora examples/prefix_docker.log --extract-prefix container \
  -f 'regex:(?P<ts>\S+ \S+) \[(?P<level>\w+)\] (?P<msg>.*)' -n 3
```

The separator defaults to `|`; change it with `--prefix-sep`.

## Make the timestamp work

Time filters (`--since`), time windows (`--span`), and time display all need
to know which field is the timestamp and how to read it. Kelora looks for
common names (`ts`, `timestamp`, `time`, `@timestamp`, …) and common formats.
`--stats` reports what it found:

```bash exec="on" source="above" result="ansi"
kelora examples/cols_fixed.log -f 'cols:ts(2) level service *msg' --stats 2>&1 | grep Timestamp
```

If it found nothing, or the wrong field:

| Problem | Fix |
|---|---|
| Field has an unusual name | `--ts-field when` |
| Unusual format | `--ts-format '%d.%m.%Y %H:%M:%S'` ([format codes](../reference/time-reference.md)) |
| No zone in the timestamp, and it isn't UTC | `--input-tz Europe/Berlin` (or `local`) |
| No year (syslog, glog) | `--input-year 2024` |

The parsed timestamp is available to scripts as `meta.parsed_ts`.
[Working with time](time.md) covers filtering, zones, and time arithmetic.

## When lines don't parse

Kelora is **resilient** by default: a line that doesn't match the format is
counted, reported in a summary on stderr, and skipped; the rest of the file is
processed. The exit code stays 0 unless *no* line parsed at all.

- `--stats` shows the counts; `-v` prints each failure.
- `--strict` stops at the first failure (exit 1) — use it in pipelines where
  bad input must not pass silently.
- To *keep* unparsable lines instead of skipping them, end a cascade with
  `line`: `-f json,line`.
