# Summarize

How many, how often, how slow, what kinds? Kelora answers these in one pass
without printing the events. The examples use `api_latency_incident.jsonl`:
800 API requests across three endpoints, some of them slow.

## Count, describe, estimate: one field

Three options take a field name and need no script:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl --freq endpoint --freq status
```

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl --filter 'e.endpoint == "/api/posts"' --describe response_time_ms
```

| Option | Gives you |
|---|---|
| `--freq FIELD` | events per value, most frequent first |
| `--describe FIELD` | count, min, max, average, sum, p50/p95/p99 of a number |
| `--card FIELD` | an estimate of the number of distinct values (about 1 % error, constant memory) |

They count only events that pass your filters, accept dotted paths for nested
fields (`--freq user.id`), and can be repeated. Look at the average and p99
above: the average looks fine, the tail does not.

## Output: table, TSV, JSON

In a terminal you get the table. Piped or redirected, the same command writes
one tab-separated row per value — metric name, value, count — sorted by count,
so `head` gives the top N and `tail` the rarest (statistics like `--describe`
print as `name_stat`, empty column, value):

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --freq status | head -3
```

| Option | Output |
|---|---|
| `--metrics=full` / `--metrics=tsv` | force table / TSV regardless of where output goes |
| `--metrics=json` | one JSON object on stdout |
| `--metrics-file FILE` | also write JSON to a file |
| `--metrics=short` | only the first 5 values of each metric |

## Count anything: `track_*()` in a script

`--freq` takes a field *name*. To count something you compute — a status class,
an hour, the first path segment — call the tracking function in `--exec` and
add `-m` (`--metrics`) to print the results instead of events:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -m \
  -e 'track_freq("status_class", e.status / 100 * 100)' \
  -e 'if e.response_time_ms > 500 { track_top("slow_endpoint", e.endpoint, 3) }'
```

Each call names its metric, so one run can track many things.

| Function | Tracks |
|---|---|
| `track_freq(name, value)` | count per distinct value (what `--freq` uses) |
| `track_inc(name)` | a counter |
| `track_sum(name, n)`, `track_avg`, `track_min`, `track_max` | running numbers |
| `track_stats(name, n)` | min, max, avg, sum, count, p50/p95/p99 (what `--describe` uses) |
| `track_percentiles(name, n, [0.5, 0.999])` | chosen percentiles |
| `track_top(name, value, k)` / `track_bottom` | the k most / least frequent values (bounded memory) |
| `track_top_by(name, value, score, k)` / `track_bottom_by` | the k values with the highest / lowest score — slowest endpoints, biggest responses |
| `track_unique(name, value)` | the distinct values themselves |
| `track_cardinality(name, value)` | an estimate of the distinct count (what `--card` uses) |

`()` values are skipped, so `track_freq("user", e.get("user"))` ignores
events without a user. Percentiles and cardinality are approximate (t-digest,
HyperLogLog) and use constant memory, so they work on files of any size.

Where you put the tracking stage matters: a `track_*` call before a
`--filter` counts every event, one after it counts only events that passed.
To format the results yourself, read the `metrics` map in an `--end` script —
see [Cross-Event Logic](state.md#reports-at-the-end).

## Per minute, per hour, per session

`--freq` and `track_*` total the whole run. For one row per time window, add
`--span` and `--span-summary`:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl --span 20m --freq level --span-summary
```

[Group into Spans](spans.md) covers count-based windows, sessions, and
per-window scripts.

## Message templates

Messages that differ only in IDs, hostnames, numbers, or durations are the same
event type. `--drain` finds those templates in the field you name with `-k`:

```bash exec="on" source="above" result="ansi"
kelora examples/syslog_errors.log --drain -k msg
```

`--drain=full` adds the line range and a sample line for each template;
`--drain=json` and `--drain=id` are for scripts. Filters run first, so
`-l error --drain -k msg` summarizes only errors.

### What changed between two logs?

`--drain-diff` compares template frequencies: before and after a deploy, a good
day and a bad day.

```bash exec="on" source="above" result="ansi"
kelora --drain-diff examples/deploy_before.jsonl examples/deploy_after.jsonl -k msg
```

`+` is a template only the second log has, `-` one that disappeared, and `*` a
template whose rate changed by more than random variation would explain. With a
single file, split it at a time (`--cut-at 2025-01-20T14:00Z`) or at the first
event matching an expression (`--cut-before 'e.msg.contains("deploy")'`).

## See it over time

Map formats print one character per event, one line per stretch of time:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -F tailmap -k response_time_ms
```

| Format | Each character shows |
|---|---|
| `-F levelmap` | the event's level (`E`, `W`, `I`, …) |
| `-F keymap -k FIELD` | the first character of FIELD |
| `-F tailmap -k FIELD` | where a number falls: `_` below p90, `1` p90–p95, `2` p95–p99, `3` above p99 |

## Processing statistics

`--stats` (`-s`) reports what Kelora did: lines read, events output and
filtered, parse and script errors, the time span, and the levels and fields it
saw. `--stats=json` is for scripts; `--with-stats` prints the stats after the
events instead of replacing them.

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -l error -s
```
