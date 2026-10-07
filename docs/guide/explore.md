# Explore a Log File

The first few minutes with a log you've never seen: what's in it, what it looks
like, and what stands out. Every command on this page works on any file Kelora
can parse; the examples use a gzipped web server log from the
[`examples/`](https://github.com/dloss/kelora/tree/main/examples) directory.

## What fields are there?

`--discover` (`-d`) profiles every field: how often it appears, its type, how
many distinct values it has, and a few samples.

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -d
```

Kelora decompressed the file, recognized the Apache/Nginx `combined` format, and
found the timestamp in `ts`. The field names in the first column are what you
use in every later command. If you see a single `line` field instead, the
format wasn't recognized — see [Get Logs into Shape](parse.md).

## How much, and over what period?

`--stats` (`-s`) summarizes the run: event counts, parse errors, time span,
levels, and keys.

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -s
```

## Read some events

`-n` (`--take`) stops after N events. `-k` (`--keys`) picks fields, in the
order you give:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -k ts,status,method,path -n 3
```

| Option | Shows |
|---|---|
| `-k a,b,c` | only these fields, in this order |
| `-K a,b` | everything except these fields |
| `-c` | only the core fields: timestamp, level, message |
| `-b` | values without field names |

The default output is `key='value'` with the timestamp, level, and message
first. In a terminal, long events wrap onto indented lines; piped output keeps
one event per line.

## What's common, what's rare?

`--freq FIELD` counts events per value, most frequent first:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --freq method
```

`--describe FIELD` summarizes a numeric field (min, max, average, percentiles):

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --describe bytes
```

For a field with thousands of distinct values, `--freq` output is long; pipe
it to `head` for the top values (piped output switches to one tab-separated
row per value). [Summarize](summarize.md) has more ways to count.

## Which messages repeat?

Free-text messages rarely repeat exactly — IDs, hostnames, and durations
differ. `--drain` groups messages into templates and replaces the parts that
vary with placeholders:

```bash exec="on" source="above" result="ansi"
kelora examples/syslog_errors.log --drain -k msg
```

## How does it change over time?

`-F levelmap` prints one character per event, so bursts of errors stand out:

```bash exec="on" source="above" result="ansi"
kelora examples/levelmap-demo.jsonl -F levelmap
```

`-F keymap -k FIELD` does the same for any field (the first character of each
value); `-F tailmap -k FIELD` marks slow outliers in a numeric field.

## Narrow it down

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --filter 'e.status >= 500' -k ts,status,path -n 3
```

`--filter` keeps events for which a [Rhai](scripting.md) expression is true;
`e` is the current event. Level filters (`-l error,warn`) and time ranges
(`--since`, `--until`) need no expressions. [Filter](filter.md) covers them
all.

## Hand it on

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --filter 'e.status >= 500' -k ts,status,path -n 3 -F csv
```

`-J` (or `-F json`) writes JSON Lines, `-F csv`/`-F tsv` writes tables with a
header, and `-o FILE` writes to a file. See [Output and Integration](output.md).

## Getting help in the terminal

| Command | Shows |
|---|---|
| `kelora -h` | one-screen cheat sheet |
| `kelora --help` | every option, grouped |
| `kelora --help KEYWORD` | options matching a keyword, e.g. `kelora --help time` |
| `kelora --help-functions KEYWORD` | built-in functions matching a keyword |
| `kelora --help-formats`, `--help-time`, `--help-rhai` | topic references |

Run `kelora` with no arguments to open an interactive prompt with history and
proper quoting — handy on Windows, where nested quotes are painful.
