# Time Reference

Timestamp detection, parsing, time windows and time functions. For a task-oriented walkthrough see [guide/time.md](../guide/time.md).

## Which Field Is the Timestamp

Kelora parses one timestamp per event. The parsed value drives `--since`/`--until`, `--span`, `--mark-gaps`, `--merge-sorted`, `-z`/`-Z` and `--normalize-ts`, and is available in scripts as `meta.parsed_ts` (UTC datetime, `()` if missing). The original field keeps its raw string.

Without `--ts-field`, the first of these names present in the event is used, in this priority order (exact, case-sensitive match):

`ts`, `_ts`, `timestamp`, `at`, `time`, `@timestamp`, `log_timestamp`, `event_time`, `datetime`, `date_time`, `created_at`, `logged_at`, `_t`, `@t`, `t`

Only that field is tried: if `ts` exists but cannot be parsed, a later `time` field is not used. `Timestamp` or `TIME` are not detected. Name the field with `--ts-field`:

```bash
echo '{"timestamp":"2024-01-15T10:30:00Z","created_at":"2024-01-15T10:31:00Z"}' \
  | kelora -j --ts-field created_at
```

Parsers that produce a timestamp (`syslog`, `combined`, `cri`, `log4j`, `glog`, … — see [Input Formats](formats.md#input-formats)) put it in `ts`.

Without `--ts-format`, common layouts are recognized automatically: RFC 3339/ISO 8601 (with `T` or space, any fraction, `Z` or offset), Apache `15/Jan/2024:10:30:00 +0000`, syslog `Jan 15 10:30:00`, Python `2024-01-15 10:30:00,123`, and Unix epochs as numbers or numeric strings:

| Input | Read as |
|-------|---------|
| `1735566123` | seconds (10 digits) |
| `1735566123000` | milliseconds (13 digits) |
| `1735566123000000` | microseconds (16 digits) |
| `1735566123000000000` | nanoseconds (19 digits) |
| `1735566123.456` | seconds with fraction |

## Parsing Options

| Flag | Purpose |
|------|---------|
| `--ts-field <FIELD>` | field to parse instead of the auto-detected one |
| `--ts-format <FMT>` | chrono format of that field; see [Format Codes](#format-codes) |
| `--input-tz <TZ>` | zone for timestamps without an offset: `UTC` (default), `local`, or an IANA name such as `Europe/Berlin`. The `TZ` environment variable also sets it. |
| `--input-year <YEAR>` | year for year-less timestamps, or `auto` (default) |

## Year and Timezone

**Timezone.** A numeric offset in the timestamp (`+0200`, `Z`) is always used. A timestamp without one (syslog, log4j, python-logging, glog, apache-error, postgres, `2024-01-15 10:30:00`, …) is read in `--input-tz`, else the `TZ` environment variable, else UTC. A trailing zone abbreviation such as `CEST` or `EST` makes auto-detection fail, because abbreviations are ambiguous and carry no DST information; put `%Z` in `--ts-format` to skip it and set `--input-tz`. If logs were written in local time and `--input-tz` is not set, every timestamp is shifted, and so are time windows, span boundaries and merge order. Kelora prints a hint once when a time filter, `--span` or `--normalize-ts` relies on the UTC assumption.

**Year.** Year-less layouts (syslog `Jan 15 10:30:00`, glog, haproxy) get last year, this year or next year: the candidate nearest the current clock that is at most a day in the future. That is right for recent logs and logs that cross New Year, wrong for archives. `-s/--stats` reports how many years were guessed. `--input-year 2005` puts every year-less timestamp in 2005, which means a December→January log gets January 2005, not 2006; split such files or keep `auto`.

```bash
kelora Linux_2k.log --input-year 2005 --since 2005-06-01 --until 2005-07-01
kelora app.log --ts-format '%Y-%m-%d %H:%M:%S' --input-tz Europe/Berlin
```

## Format Codes

`--ts-format`, `to_datetime(text, fmt)`, `dt.format(fmt)` and `-M timestamp:format=` use [chrono format strings](https://docs.rs/chrono/latest/chrono/format/strftime/index.html). Quote the format in single quotes so the shell leaves `%` alone.

| Code | Example | Meaning |
|------|---------|---------|
| `%Y` / `%y` | `2024` / `24` | year, 4 / 2 digits |
| `%m` | `01` | month 01–12 |
| `%b` / `%B` | `Jan` / `January` | month name |
| `%d` | `15` | day 01–31 |
| `%j` | `015` | day of year |
| `%H` / `%I` | `14` / `02` | hour, 24h / 12h |
| `%p` | `PM` | AM/PM |
| `%M` | `30` | minute |
| `%S` | `45` | second |
| `%.f` | `.123`, `.123456` | dot plus any number of fraction digits; a comma is accepted too |
| `%3f` / `%6f` / `%9f` | `123` / `123456` / `123456789` | exactly 3 / 6 / 9 fraction digits (put the `.` or `,` before it) |
| `%f` | `123456789` | fraction digits read as a nanosecond count: `.%f` on `.123456` gives 123456 ns, not 0.123456 s. Prefer `%.f` or `%6f`. |
| `%z` / `%:z` | `+0100` / `+01:00` | numeric offset |
| `%Z` | `EST` | zone abbreviation; consumed but ignored |
| `%a` / `%A` | `Mon` / `Monday` | weekday name |
| `%w` | `1` | weekday, 0 = Sunday |
| `%W` / `%U` | `03` | week number, Monday / Sunday first |
| `%s` | `1705318200` | Unix seconds |

| `--ts-format` | Matches |
|---------------|---------|
| `'%Y-%m-%dT%H:%M:%S%.f%:z'` | `2024-01-15T10:30:00.123+02:00` |
| `'%Y-%m-%d %H:%M:%S'` | `2024-01-15 10:30:00` |
| `'%Y-%m-%d %H:%M:%S,%3f'` | `2024-01-15 10:30:00,123` (Python logging) |
| `'%d/%b/%Y:%H:%M:%S %z'` | `15/Jan/2024:10:30:00 +0100` (Apache) |
| `'%b %d %H:%M:%S'` | `Jan 15 10:30:00` (syslog; year guessed) |
| `'%m/%d/%Y %I:%M:%S %p'` | `01/15/2024 02:30:00 PM` |
| `'%a %b %d %H:%M:%S %Y'` | `Mon Jan 15 14:30:45 2024` |

## Time Windows: `--since`, `--until`

`--since` keeps events at or after a time, `--until` at or before.

| Value | Meaning |
|-------|---------|
| `2024-01-15T10:00:00Z`, `2024-01-15T10:00:00+01:00` | absolute |
| `2024-01-15 10:00`, `2024-01-15T10:00:00` | absolute, read in `--input-tz` |
| `2024-01-15`, `2024/01/15`, `01/15/2024`, `15.01.2024`, `January 15, 2024`, `15 January 2024` | midnight UTC of that date (not `--input-tz`) |
| `10:30`, `10:30:00` | that UTC clock time on today's local date |
| `1705318200`, `1705318200.5`, `1705318200000` | Unix epoch, same digit rules as above |
| `now` | now |
| `today`, `yesterday`, `tomorrow` | 00:00 UTC on that local date |
| `1h`, `-1h`, `30m`, `2d`, `1w`, `90s`, `3 days` | that long ago |
| `+1h`, `+30m` | that far in the future |
| `now-15m`, `now+5m` | relative to now |
| `since+30m`, `since-30m` | relative to the `--since` value (use in `--until`) |
| `until+1h`, `until-1h` | relative to the `--until` value (use in `--since`) |

Units: `s`, `m`, `h`, `d`, `w`, also spelled out (`min`, `hours`, `days`, `weeks`, …). One number and one unit only: `1h30m` and `500ms` are rejected; write `90m`.

```bash
kelora app.log --since 1h                                 # last hour
kelora app.log --since now-15m
kelora app.log --since 2024-01-15T10:00:00Z --until 2024-01-15T11:00:00Z
kelora app.log --since 2024-01-15T10:00:00Z --until since+45m
kelora app.log --since until-1h --until 2024-01-15T11:00:00Z
kelora app.log --since -2h --until since+1h               # one hour, starting two hours ago
```

- Only one bound may refer to the other: `--since until-1h --until since+1h` is an error, as is `since+…` without `--since`.
- `1h`, `today` and bare clock times are relative to the current clock, not to the log. On an archived log use absolute timestamps.
- The window reads the timestamp the parser produced and runs before every script stage. Setting a timestamp field in `--exec` does not move an event in or out of the window. For a timestamp you build in a script, filter on it instead:

  ```bash
  kelora -f json app.log \
    --exec 'e.derived = to_datetime(e.date + "T" + e.time + "Z")' \
    --filter 'e.derived >= to_datetime("2024-05-01T00:00:00Z")'
  ```

- An event without a parsed timestamp is dropped by the window (it never reaches `--filter`, `--exec` or `--assert`), and a warning gives the count. Fix the timestamp with `--ts-field`/`--ts-format`, merge continuation lines with `-M`, or use `--filter` instead of a window for undated cascade `line` events. `--strict` aborts on the first such event.
- `--cut-at` (for `--drain-diff`) accepts the same values except the `since`/`until` anchors.

## Display and Normalization

| Flag | Effect |
|------|--------|
| `-z, --show-ts-local` | show the timestamp field as local RFC 3339; default output format only |
| `-Z, --show-ts-utc` | show it as UTC RFC 3339; default output format only |
| `--normalize-ts` | rewrite the timestamp field as RFC 3339 UTC (`2024-01-15T10:30:00+00:00`) in every output format. Applied at output: scripts still see the raw value. |

`-z`/`-Z` have no effect on `-F json`, `logfmt` or `csv`; use `--normalize-ts` there.

Other time-based flags: `--span 5m` (fixed time windows, see [guide/spans.md](../guide/spans.md)), `--mark-gaps 5m` (divider line where consecutive events are at least that far apart), `--merge-sorted` (merge files by timestamp, see [guide/files.md](../guide/files.md)).

## Rhai Functions

Use `meta.parsed_ts` for the event's own timestamp; `to_datetime()` is for other fields. Full list: [functions.md](functions.md#datetime-functions).

| Function | Returns / notes |
|----------|-----------------|
| `now()` | current time, UTC |
| `to_datetime(text)` | parse with the auto-detection rules above; naive text is read as UTC |
| `to_datetime(text, fmt)` | parse with a chrono format; error if it does not match |
| `to_datetime(text, fmt, tz)` | text without an offset is read as wall-clock time in `tz`: `to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S", "America/New_York")` is `10:30-05:00` (15:30 UTC) |
| `dt.year()` `.month()` `.day()` `.hour()` `.minute()` `.second()` | int, in `dt`'s zone |
| `dt.ts_nanos()` | Unix nanoseconds |
| `dt.to_iso()` | `2024-01-15T10:30:00+00:00` |
| `dt.format(fmt)` | chrono format, e.g. `"%Y-%m-%d %I:%M %p"` → `2024-01-15 10:30 AM` |
| `dt.to_utc()`, `dt.to_local()`, `dt.to_timezone("Europe/Berlin")` | same instant in another zone |
| `dt.timezone_name()` | `"UTC"`, `"Europe/Berlin"` |
| `dt.round_to("5m")` | floor to the interval (`12:34:56` → `12:30:00`). Boundaries are computed in UTC, so `"1d"` on a New York time gives `19:00-05:00`; the result keeps `dt`'s zone |
| `dt.ceil_to("5m")` | up to the next boundary (UTC-based, like `round_to`); unchanged if already on one |
| `dt + dur`, `dt - dur` | datetime |
| `dt1 - dt2` | duration, always positive (absolute difference) |
| `==` `!=` `<` `<=` `>` `>=` | compare datetimes, or durations |
| `to_duration("1h30m")` | duration; accepts combined units, spaces, `ms`, fractions (`"1.5h"`) |
| `duration_from_seconds(n)`, `_milliseconds`, `_nanoseconds`, `_minutes`, `_hours`, `_days` | duration |
| `dur.as_seconds()`, `as_milliseconds()`, `as_nanoseconds()`, `as_minutes()`, `as_hours()`, `as_days()` | int, truncated: 90 minutes `.as_hours()` is `1` |
| `dur.to_string()` | humanized, largest two units: `"1h 30m"` |
| `humanize_duration(ms)` | `5000` → `"5s"` |
| `d1 + d2`, `d1 - d2` (positive), `d * n`, `d / n` | duration |

```bash
kelora app.log --filter 'meta.parsed_ts.hour() >= 9 && meta.parsed_ts.hour() < 17'
kelora app.log -m --exec 'track_freq("per_5m", meta.parsed_ts.round_to("5m"))'
kelora app.log --exec 'e.age_s = (now() - meta.parsed_ts).as_seconds()'
kelora app.log --exec 'e.duration_ms = (to_datetime(e.end_time) - to_datetime(e.start_time)).as_milliseconds()'
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| `-s` shows `Timestamp: (none found…)` | field name not in the list above (case matters): `--ts-field` |
| `-s` shows `0/N parsed` | layout not recognized: `--ts-format` |
| times off by whole hours | naive timestamps read as UTC: `--input-tz` |
| syslog/glog dated in the wrong year | `--input-year` |
| `--since 1h` keeps nothing | it means one hour before now; use an absolute time for old logs |
| events vanish with `--since`/`--until` | events without a parsed timestamp are dropped; see the warning count |
