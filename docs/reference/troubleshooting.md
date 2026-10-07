# Troubleshooting

Three commands answer most questions:

```bash
kelora app.log -d          # which fields did the parser produce, with which types?
kelora app.log -s          # how many lines parsed, how many events passed, which errors?
kelora app.log -n 5 -v     # the first events, with every error and decision printed
```

Then look up the symptom below. Read stderr, too: Kelora prints a hint (💡)
when it recognizes a likely mistake, and a summary when lines or scripts
failed.

## No output, or fewer events than expected

| Check | How |
|---|---|
| Did parsing work? | A single `line` field means the format wasn't recognized: [Get Logs into Shape](../guide/parse.md). |
| Is the field name right? | `-d` lists the names. `msg` vs `message`, `ts` vs `timestamp`. Kelora hints when a filter names a field it never saw. |
| Does the case match? | `e.level == "error"` doesn't match `"ERROR"`. Use `-l error` (case-insensitive) or `e.level.to_lower() == "error"`. |
| Is it a number or a string? | `e.status == "500"` never matches a numeric `500`, and vice versa. `-d` and `-F inspect` show types. |
| Did the time range drop it? | `--since 1h` counts from *now*, not from the end of the file. Events without a timestamp are dropped by any time range (with a warning). |
| Did a script fail? | A `--filter` that errors counts as "no match"; see the error summary on stderr. |
| Did `-k` hide it? | An event without any of the `-k` fields isn't printed. |
| Did multiline merge it? | With `-M`, compare `Lines processed` and `Events created` in `-s`. |

Example: the quotes around 503 make the comparison always false, and Kelora
says so:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl --filter 'e.status == "503"' -n 2
```

## Script errors

`Exec errors: N total` means an `--exec` failed on N events. Kelora undid that
stage's changes for those events and continued. The first few lines of the
summary name the line and the problem; `-v` prints all of them.

| Message or symptom | Cause and fix |
|---|---|
| `Missing field during expression evaluation` | arithmetic or a method on a missing field: guard with `e.has("f")` or use `e.get("f", default)` — see [missing fields](../guide/scripting.md#missing-fields) |
| `Function not found: hour (… String)` | a datetime method on a string; the event timestamp is already parsed as `meta.parsed_ts` |
| `Function not found` for your own function | load it with `-I file.rhai` before *each* stage that uses it |
| `Variable not found` | `let` variables don't carry over to the next `--exec`; combine the statements, or store the value on `e` |
| `e.y = e.x.trim()` leaves `y` unset | built-in `trim()` and `replace()` return nothing; use `strip()` and `replace_regex()` |
| `conf map is read-only outside --begin` | set `conf` values in `--begin` |
| `'state' is not available in --parallel mode` | drop `--parallel`, or use `track_*()` |
| a division gives `0` | integer division: write `e.ms / 1000.0` |

## Parse errors

`-s` shows how many lines failed to parse; `-v` shows each one. Common
causes: a format that doesn't fit every line (use a cascade such as
`-f json,line` to keep the rest), multi-line records without `-M`, or a
`regex:` that must match the whole line. `--strict` makes the first failure
fatal.

## Wrong times

| Symptom | Fix |
|---|---|
| times shifted by a few hours | the log has no zone and isn't UTC: `--input-tz Europe/Berlin` |
| wrong year on syslog-style timestamps | `--input-year 2024` |
| timestamp not found | `--ts-field name`, and `--ts-format` for unusual formats |
| `--since` drops everything on an old file | use absolute times; relative ones count from now |

See [Work with Time](../guide/time.md).

## Output problems

| Symptom | Fix |
|---|---|
| `CSV output requires --keys` | add `-k field1,field2,…` |
| escape codes in a file | colors are off automatically when output isn't a terminal; force with `--no-color` |
| long events wrap | wrapping happens only in a terminal; `--no-wrap` turns it off |
| a summary prints TSV instead of a table | output is piped; add `--metrics=full` for the table |

## Shell quoting

Wrap Rhai code in single quotes and use double quotes inside:
`--filter 'e.level == "ERROR"'`. Regexes with backslashes are easiest as Rhai
raw strings: `#"\d+"#`. On Windows, run `kelora` with no arguments for an
interactive prompt that avoids nested quoting.

## Config surprises

A `.kelora.ini` in the current directory or a parent adds defaults and
aliases. `-v` shows what was applied; `--ignore-config` runs without it. See
[Configuration](../guide/config.md).

## Still stuck?

Open an issue at [github.com/dloss/kelora](https://github.com/dloss/kelora/issues)
with `kelora --version`, the command, a few sample lines, and what you
expected.
