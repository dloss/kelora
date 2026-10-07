# Group into Spans

A **span** is a run of consecutive events: everything in one five-minute
window, every 1000 events, one burst of activity between quiet periods.
Kelora closes each span as soon as it is complete and reports on it, so this
works on endless streams as well as files.

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl --span 10m --freq status --span-summary
```

## Choose how events are grouped

| Option | A span closes | Typical use |
|---|---|---|
| `--span 5m` | at each aligned time boundary (`:00`, `:05`, …) | rates per minute or hour |
| `--span 1000` | after every 1000 events that passed the filters | batches |
| `--span FIELD` | when the value of FIELD changes | runs of the same request, job, or host |
| `--span-idle 5m` | after 5 minutes without events | sessions, bursts |

Time spans and idle spans need timestamps ([Work with Time](time.md)). Only
events that pass your filters are counted in a span, so
`-l error --span 5m --span-summary` gives errors per five minutes.

`--span FIELD` closes whenever the value changes, so interleaved values
(`a, b, a`) produce several spans for `a`. Sort the input first, or use
`--freq FIELD` if you only need totals.

## One row per span: `--span-summary`

`--span-summary` prints one row per span with its label, the event count,
and the per-span value of every metric. Add `--freq`, `--describe`, or
`track_*()` calls for the columns you need:

```bash exec="on" source="above" result="ansi"
kelora examples/worker_bursts.jsonl --span-idle 5m --freq job --span-summary
```

| Format | Use for |
|---|---|
| `--span-summary` | text in a terminal, TSV when piped |
| `--span-summary=tsv` | long-format rows (`label`, `metric`, `key`, `value`) for DuckDB, pandas, gnuplot |
| `--span-summary=json` | one JSON object per span, nested metrics intact |

A window that received no input produces no row, so a time series from
`--span-summary` has gaps where nothing was logged — fill them downstream if
you need a dense series. A window whose events were all filtered out still
gets a row, with `events=0`.

Only metrics that can be split per span appear in the rows: counts, sums,
averages, distinct values. Minimum, maximum, percentiles, and top-N have no
per-span value; Kelora says so and leaves them out:

```bash exec="on" source="above" result="ansi"
kelora examples/worker_bursts.jsonl --span-idle 5m --describe ms --span-summary
```

For per-span extremes, use a `--span-close` script with `span.events`.

## Your own report per span: `--span-close`

`--span-close` runs a script each time a span closes. Inside it, `span`
describes the span that just closed:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -q --span 10m \
  -e 'track_inc("requests"); if e.status >= 500 { track_inc("errors") }' \
  --span-close 'let r = span.metric("requests"); let x = span.metric("errors");
                print(`${span.label}  ${x}/${r} failed  ${x * 100.0 / r}%`)'
```

| Variable | Contains |
|---|---|
| `span.label` | the start time (time spans) or the span ID (`#0`, `#1`, … for count spans) |
| `span.start`, `span.end` | window boundaries as datetimes |
| `span.size` | number of events |
| `span.metric("name")` | this span's value of a metric, `0` if the span had none |
| `span.metrics` | all per-span metric values as a map |
| `span.events` | the events themselves, as an array of maps |
| `metrics` | running totals since the start |

`-q` suppresses the events, so only the hook's output remains. `span.events`
is collected only when a `--span-close` script exists, and holds every event in
the span in memory — fine for minutes, not for days.

## Events and their span

In `--exec`, each event knows its span through `meta.span_id`,
`meta.span_start`, and `meta.span_end`. With time spans, an event whose
timestamp falls in a span that already closed is **late**
(`meta.span_status == "late"`); it is passed through, but not counted in the
closed span. Out-of-order input — merged files, parallel shippers — produces
late events; sort it first if every event must count.

Spans need sequential processing, so they can't be combined with `--parallel`.
