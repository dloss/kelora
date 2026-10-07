# Work with Time

Time filters, time windows, ordering, and time display all depend on one thing:
Kelora knowing **which field holds the timestamp and how to read it**.

## Which timestamp Kelora uses

Kelora looks for common field names — `ts`, `timestamp`, `time`, `@timestamp`,
`datetime`, `created_at`, `t`, and a few more — and recognizes ISO 8601,
RFC 3339, syslog, Apache, many application-log layouts, and Unix epochs in
seconds, milliseconds, or microseconds. `--stats` says what it found:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -s | grep Timestamp
```

The parsed value is available to scripts as `meta.parsed_ts`. The field itself
(`e.timestamp`) keeps the original text.

## When detection needs help

| Situation | Option |
|---|---|
| The field has an unusual name | `--ts-field logged` |
| The format is unusual | `--ts-format '%d.%m.%Y %H:%M:%S'` |
| No zone in the timestamp, and the source isn't UTC | `--input-tz Europe/Berlin` (or `local`) |
| No year in the timestamp (syslog, glog) | `--input-year 2024` |

`--ts-format` uses [chrono format codes](../reference/time-reference.md):
`%Y-%m-%d %H:%M:%S,%3f` for Python logging, `%d/%b/%Y:%H:%M:%S %z` for Apache.
Quote the format so the shell leaves `%` alone.

### Time zones

A timestamp with an offset (`+02:00`, `Z`) is always read as written. A
timestamp without one is **naive**: Kelora reads it in the zone given by
`--input-tz`, else the `TZ` environment variable, else UTC. (In containers `TZ`
is often set — check it if times look shifted.) When that assumption affects the result — a time
filter, a time window, `--normalize-ts` — Kelora prints a hint:

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' --input-year 2024 --normalize-ts -n 1
```

A trailing zone abbreviation (`CEST`, `PST`) stops auto-detection, because
abbreviations are ambiguous. Add `%Z` to `--ts-format` to skip over it, and set
the zone with `--input-tz`:
`--ts-format '%Y-%m-%d %H:%M:%S %Z' --input-tz Europe/Berlin`.

### Missing years

Syslog-style timestamps (`Jan 15 10:00:00`) have no year. Kelora picks the date
closest to now that is at most a day in the future — right for recent logs,
wrong for archives; `--stats` says when it had to guess. Pass
`--input-year 2024` for old files.

## Filter by time

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl --since 2024-01-15T10:15:00Z --until since+5m -c
```

| Form | Example | Meaning |
|---|---|---|
| absolute | `2024-01-15T10:00:00Z`, `'2024-01-15 10:00'`, `10:00` | that moment (a bare time means today) |
| relative to now | `1h`, `-30m`, `2d`, `now-15m`, `yesterday`, `today` | that far back |
| relative to the other bound | `--until since+30m`, `--since until-1h` | a window of fixed length |
| Unix epoch | `1735566123` | seconds since 1970 |

`--since` is inclusive (at or after), `--until` too (at or before). Relative
times count from *now*, so for an archived file use absolute times.

- The time range is applied **before** every other filter and script, so
  `--freq` and other counts always cover exactly the events in the range.
- Events without a timestamp can't be placed in the range and are dropped.
  Kelora warns with a count. Usually the fix is upstream: point `--ts-field` at
  the right field, or join continuation lines with
  [`-M`](parse.md#one-event-spans-several-lines).
- The range uses the timestamp the parser found. If you build a timestamp in a
  script (separate date and time fields, say), filter on it with `--filter`
  instead:

```bash
kelora app.jsonl \
  -e 'e.when = to_datetime(e.date + "T" + e.time + "Z")' \
  --filter 'e.when >= to_datetime("2024-05-01T00:00:00Z")'
```

## Show timestamps differently

| Option | Effect |
|---|---|
| `-Z` / `-z` | display timestamps in UTC / local time (default output format only) |
| `--normalize-ts` | rewrite the timestamp field as RFC 3339 — in every output format, including JSON and CSV |
| `--mark-gaps 5m` | print a divider wherever consecutive events are 5 minutes or more apart |

## Time in scripts

`meta.parsed_ts` is a datetime value, so you can use its methods directly:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl \
  -e 'e.hour = meta.parsed_ts.hour(); e.day = meta.parsed_ts.format("%a %d %b")' \
  -k timestamp,hour,day -n 2
```

| Want | Write |
|---|---|
| a part of the time | `.year()`, `.month()`, `.day()`, `.hour()`, `.minute()`; weekday: `.format("%a")` |
| custom text | `.format("%Y-%m-%d %H:%M")`, `.to_iso()` |
| another zone | `.to_timezone("America/New_York")`, `.to_utc()`, `.to_local()` |
| round to a bucket | `.round_to("5m")` (down), `.ceil_to("1h")` (up) |
| parse another field | `to_datetime(e.started_at)`, with format and zone: `to_datetime(s, "%d.%m.%Y %H:%M", "Europe/Berlin")` |
| the difference | `(end - start).as_seconds()`, `.as_milliseconds()`, `.to_string()` |
| shift | `meta.parsed_ts + to_duration("90m")` |
| compare | `meta.parsed_ts > to_datetime("2024-05-01T00:00:00Z")` |
| now | `now()` |
| readable duration | `humanize_duration(e.duration_ms)` → `"1m 30s"` |

Count events per hour, but only those before 11:00 Berlin time:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz \
  --filter 'meta.parsed_ts.to_timezone("Europe/Berlin").hour() < 11' \
  -m -e 'track_freq("per_hour", meta.parsed_ts.round_to("1h"))'
```

`round_to` and `ceil_to` compute boundaries in UTC.

For per-window rows with their own metrics, use [spans](spans.md).

## Related

- [Merge files by timestamp](files.md#merge-files-by-time) when several
  sorted logs need to become one timeline.
- [Time format reference](../reference/time-reference.md) for every format code.
