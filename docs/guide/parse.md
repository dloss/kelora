# Get Logs into Shape

Parsing has two jobs: split each line into named **fields**, and find a
correct **timestamp**. Filters, scripts, and output work on fields; time
ranges, windows, and ordering work on the timestamp — and a wrong timestamp
fails silently.

For the fields, try these approaches in order and stop at the first one that
works:

| Step | Situation | What to use |
|---|---|---|
| [1](#1-let-auto-detection-do-it) | The format is common | nothing — auto-detection |
| [2](#2-name-the-format) | You know the format, or detection guessed wrong | `-f json`, `-f logfmt`, `-f syslog`, … |
| [3](#3-mixed-files-try-several-parsers-per-line) | One file mixes formats | cascade: `-f json,line` |
| [4](#4-columns-cols) | Whitespace-separated columns | `-f 'cols:ts(2) level *msg'` |
| [5](#5-patterns-regex) | Anything with a pattern | `-f 'regex:(?P<ts>\S+) …'` |
| [6](#6-finish-the-job-in-a-script) | Fields are buried inside a text field | `absorb_kv()`, `extract_regex()`, … in `--exec` |

Then [check the timestamp](#get-the-timestamp-right). If one event spans
several lines, [join them first](#one-event-spans-several-lines).

## How to tell whether parsing worked

Parsed events show several named fields. An unparsed event has a single `line`
field holding the whole line, and Kelora prints a hint:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_line.log -n 2
```

For a field-by-field profile with types and sample values, use `--discover`
(`-d`); its last line also names the timestamp field. Use `-v` to see which
format was detected.

## Split lines into fields

### 1. Let auto-detection do it

With no `-f`, Kelora samples the input and picks a parser. It recognizes JSON,
syslog, CEF, Apache/Nginx access logs, Kubernetes CRI, logfmt, CSV/TSV, and a
set of application-log layouts (`log4j`, `python-logging`, `glog`,
`nginx-error`, `postgres`, `iso8601-level`, …):

```bash exec="on" source="above" result="ansi"
kelora -v examples/app.log -n 2
```

Detection runs once per run (on files, it samples the head plus a few probes
deeper in). If a file mixes a dominant format with stray text lines, it
switches to a cascade automatically (see 3 below). On stdin, it uses the first
line only.

### 2. Name the format

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

### 3. Mixed files: try several parsers per line

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

### 4. Columns: `cols:`

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
| `name(3)` | three columns, joined (e.g. syslog-style `Jan 15 10:00:00`) |
| `-` / `-(2)` | skip one / two columns |
| `*name` | the rest of the line (must be last) |
| `name:int` | convert: `int`, `float`, `bool` |

Count the timestamp's whitespace tokens: `ts(2)` for `2024-01-15 10:00:00`,
`ts(3)` for `Jan 15 10:00:00`. The no-format hint names the count when the
sampled lines agree on one. Use `--cols-sep '|'` for a different separator.

### 5. Patterns: `regex:`

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

### 6. Finish the job in a script

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

There are two kinds of functions for this.

**`absorb_*` take a field's name, in quotes,** and do the whole job: parse the
text in that field, add what they find to the event as new fields, and remove
the source field once it is fully used. Here the `key=value` pairs in `msg`
become fields of their own:

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error -n 2 \
  -e 'e.absorb_kv("msg")'
```

| Function | Parses the named field as |
|---|---|
| `e.absorb_kv("msg")` | `key=value` tokens; leftover text stays in the field (not quote-aware) |
| `e.absorb_logfmt("msg")` | logfmt — the whole field (quote-aware, typed) |
| `e.absorb_json("msg")` | a JSON object stored as a string |
| `e.absorb_regex("msg", pattern)` | a regex; named groups become fields |

They return a status (`"applied"`, `"parse_error"`, …) you can branch on, as the
`incident_story.log` example does.

**The other functions work on a field's value** and return one result, which
you assign to a field yourself:

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error -n 2 \
  -e 'e.order = e.msg.extract_regex("order=(\\d+)", 1).or_empty()' -k msg,order
```

| Function | Returns |
|---|---|
| `e.msg.extract_regex(pattern, 1)` | one capture group |
| `e.msg.between("[", "]")`, `.after(":")`, `.before(" ")` | text around delimiters |
| `e.msg.extract_json()`, `.extract_ip()`, `.extract_url()` | the first JSON / IP / URL in the text |
| `e.url.parse_url()`, `e.agent.parse_user_agent()`, `e.token.parse_jwt()` | a map of parts |

They return `""` when they find nothing; `.or_empty()` turns that into "no
field", as on the second line above. The full list is in the
[function reference](../reference/functions.md#parsing-functions).

The field doesn't have to come from a parser: on a file Kelora couldn't
parse, everything is in `line`, so `-e 'e.absorb_kv("line")'` turns any
`KEY=VALUE` pairs in it straight into fields.

The same approach parses a field that is itself a log line — a JSON payload
inside a Kubernetes CRI `msg`, a `key=value` list inside a syslog message:

```bash exec="on" source="above" result="ansi"
kelora examples/pod_cri.log -e 'e.absorb_json("msg")' -k ts,stream,level,msg,status -n 5
```

Lines whose `msg` isn't JSON (the panic) keep their text; `absorb_json` just
returns a `parse_error` status for them.

Quick column grabs work too: `e.line.col(2)` returns the third whitespace column,
and `e.line.split(" ")` an array.

## Get the timestamp right

Time ranges (`--since`), time windows (`--span`), merging files, and the time
shown in the output all depend on the timestamp. When it is wrong, nothing
errors: events just fall outside a range, land in the wrong window, or show
the wrong time. So check it once for every new log source. `--stats` says
which field Kelora used and how many values it could read:

```bash exec="on" source="above" result="ansi"
kelora examples/cols_fixed.log -f 'cols:ts(2) level service *msg' --stats | grep -E 'Timestamp|Time span'
```

Kelora looks for common field names (`ts`, `timestamp`, `time`, `@timestamp`,
…) and recognizes most formats, including Unix epochs. Four things can go wrong:

| Symptom in `--stats` | Cause | Fix |
|---|---|---|
| `Timestamp: (none found …)` | unusual field name | `--ts-field logged` |
| `0/… parsed` | unusual format | `--ts-format '%d.%m.%Y %H:%M:%S'` |
| a time span shifted by whole hours | no zone in the log, and it isn't UTC | `--input-tz Europe/Berlin` |
| a guessed year (syslog-style dates) | the log has no year | `--input-year 2024` |

A field Kelora doesn't recognize by name:

```bash exec="on" source="above" result="ansi"
echo '{"logged":"2024-09-05 10:02:00","msg":"disk check ok"}' | kelora -j --stats | grep Timestamp
echo '{"logged":"2024-09-05 10:02:00","msg":"disk check ok"}' | kelora -j --ts-field logged --stats | grep Timestamp
```

The quiet one is the time zone. This server logs Berlin local time without a
zone. Read as UTC (the default), every timestamp is two hours off — 09:55 UTC
instead of 07:55:

```bash exec="on" source="above" result="ansi"
kelora examples/berlin_local.log -f 'cols:ts(2) *msg' --stats | grep 'Time span'
kelora examples/berlin_local.log -f 'cols:ts(2) *msg' --input-tz Europe/Berlin --stats | grep 'Time span'
```

Kelora prints a hint when a time filter or window relies on the UTC
assumption. [Work with Time](time.md) covers zones, years, and format codes in
detail. The parsed timestamp is available to scripts as `meta.parsed_ts`.

## One event spans several lines

Stack traces, wrapped messages, and pretty-printed payloads occupy several
physical lines. `-M` (`--multiline`) joins them into one event **before**
parsing:

```bash exec="on" source="above" result="ansi"
kelora examples/multiline_stacktrace.log -M timestamp -l error -n 1 -k ts,level,msg
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
the line structure (`java`, `python`, and `go` already join with newlines). Multiline grouping happens
before parsing, so the joined block must still match your format — free-text
parsers (`line`, `raw`, regex with a trailing `.*`, layouts that end in the
message such as `log4j` or `glog`) handle that. Details: `kelora --help-multiline` and
[Multiline reference](../reference/multiline.md).

## Prefixes added by other tools

`docker compose logs` and similar tools prepend `name | ` to each line.
`--extract-prefix` moves it into a field before the rest is parsed:

```bash exec="on" source="above" result="ansi"
kelora examples/prefix_docker.log --extract-prefix container \
  -f 'regex:(?P<ts>\S+ \S+) \[(?P<level>\w+)\] (?P<msg>.*)' -n 3
```

The separator defaults to `|`; change it with `--prefix-sep`.

## When lines don't parse

Kelora is **resilient** by default: a line that doesn't match the format is
counted, reported in a summary on stderr, and skipped; the rest of the file is
processed. The exit code stays 0 unless *no* line parsed at all.

- `--stats` shows the counts; `-v` prints each failure.
- `--strict` stops at the first failure (exit 1) — use it in pipelines where
  bad input must not pass silently.
- To *keep* unparsable lines instead of skipping them, end a cascade with
  `line`: `-f json,line`.
