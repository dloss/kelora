# Script Variables

Which built-in variables a Rhai script can use, by stage. Language syntax and field-access idioms: [Rhai Cheatsheet](rhai-cheatsheet.md).

| Variable | `--begin` | `--filter`, `--exec`, `--exec-file` | `--span-close` | `--end` |
|---|---|---|---|---|
| `e` | empty | current event, writable | empty | empty |
| `meta`, `line` | empty | current event | empty | empty |
| `conf` | writable | read-only | read-only | read-only |
| `state` | yes | yes | yes | yes |
| `window` | – | yes (use `--window N`) | – | – |
| `metrics` | – | – | read-only | read-only |
| `span` | – | – | yes | – |

– means the name is undefined there and using it is a script error. `state` exists only in sequential mode: any access with `--parallel` is a script error in every stage. `track_*()` can be called in every stage.

Each `--filter`/`--exec` is a separate script: `let` variables and `fn` definitions do not carry to the next stage, changes to `e`, `state`, and tracked metrics do. See [Stages and their scope](../how-it-works.md#stages-and-their-scope).

## `e`

The current event as a map. Assigning a field sets it, assigning `()` removes it, `e = ()` drops the event. In `--exec` the changes are passed on to later stages and the output; if the stage fails, they are rolled back.

## `line`

The input text of the current event as a string: one line, or the assembled block with `--multiline`. The line terminator is stripped. Assigning to `line` changes only the script's local copy.

## `meta`

Per-event metadata, filled in by Kelora. Assignments are visible only within the same script.

| Key | Type | Value |
|---|---|---|
| `line` | string | same as `line` |
| `line_num` | int | 1-based input line number |
| `filename` | string or `()` | input file; `()` for stdin |
| `parsed_ts` | datetime or `()` | the event's timestamp as parsed by Kelora, in UTC; `()` if none was found. Unlike the original string field (e.g. `e.timestamp`), it supports date arithmetic directly: `meta.parsed_ts.round_to("1h")`, `(meta.parsed_ts - other).as_seconds()` |
| `span_id` | string | with `--span`/`--span-idle`: id of the event's span (see `span.id`) |
| `span_start`, `span_end` | datetime or `()` | bounds of the event's span; `()` for count and field spans |
| `span_status` | string | with spans: `"included"`, `"late"` (time span already closed), `"unassigned"`, or `"filtered"` |

Missing keys read as `()`.

## `conf`

A map for settings and lookup tables. Fill it in `--begin`; in all later stages it is read-only.

```bash
kelora -j app.log \
  --begin 'conf.env = get_env("ENVIRONMENT", "dev"); conf.slow_ms = 500' \
  --filter 'e.duration_ms > conf.slow_ms'
```

More: [Lookups](../guide/scripting.md#lookups).

## `state`

A map that persists across all events and input files of one run, from `--begin` through `--end`. Use it when a decision about the current event depends on earlier events (deduplication, request/response pairing, sessions). For counts and totals, `track_*()` is simpler and also works with `--parallel`.

| Operation | Syntax |
|---|---|
| read, write | `state[key]` (`()` if absent), `state[key] = value` |
| read with default | `state.get(key, default)`, `state.get(key)` |
| test, remove | `state.contains(key)`, `state.remove(key)` (returns the value) |
| size, contents | `state.len()`, `state.is_empty()`, `state.keys()`, `state.values()` |
| merge | `state += map`, `state.mixin(map)` (overwrite existing keys) |
| replace all | `state.fill_with(map)` |
| reset | `state.clear()` |
| anything else | `state.to_map()` returns a plain map copy: `state.to_map().to_json()` |

Keys must be strings: `state[e.id]` fails if `e.id` is a number; use `state[e.id.to_string()]`. Values can be anything. To change a nested value, read it, modify it, store it back:

```rhai
let u = state.get(e.user, #{count: 0});
u.count += 1;
state[e.user] = u;
```

`state` keeps everything until the run ends; remove entries you no longer need. Patterns: [Cross-Event Logic](../guide/state.md).

## `window`

With `--window N`, an array of the current event and up to N previous ones: `window[0]` is the current event, `window[1]` the one before. Entries are events as parsed, before any script changed them, and they include events that later filters drop. Each entry also carries `line`, `line_num`, and `filename` (if known) as fields.

```rhai
if window.len() > 1 { e.delta = e.value - window[1].value }
window.pluck("latency")              // that field from every entry
window.pluck_as_nums("latency")      // as numbers, skipping missing/invalid
```

Without `--window`, `window` contains only the current event.

## `metrics`

The values recorded by `track_*()` calls so far, as a read-only map keyed by metric name. In `--end` these are the final totals; in `--span-close` they are the running totals since the start of the run (per-span values are in `span.metrics`).

```bash
kelora -j app.log -q \
  -e 'track_freq("level", e.level); track_inc("total")' \
  --end 'print(`${metrics.level.get("ERROR") ?? 0} errors in ${metrics.total} events`)'
```

## `span`

Only in `--span-close`, which runs once when a span (`--span`, `--span-idle`) closes. Read-only.

| Property | Type | Value |
|---|---|---|
| `span.id` | string | `#0`, `#1`, … for count spans; `2024-01-15T10:00:00Z/5m` for time spans; the field value for field spans; `idle-#0-<start>` for idle spans |
| `span.label` | string | the window start for time and idle spans, otherwise `span.id` (what `--span-summary` prints) |
| `span.start`, `span.end` | datetime or `()` | window bounds; `()` for count and field spans |
| `span.size` | int | number of events that passed the filters and entered the span |
| `span.events` | array of maps | those events, each with `line`, `line_num`, `filename`, `span_id`, `span_start`, `span_end`, `span_status` added as fields |
| `span.metrics` | map | what `track_*()` recorded while the span was open; zero values are omitted |
| `span.metric(name)` | value | one entry of `span.metrics`, `0` if absent; dotted names reach into `track_freq` maps: `span.metric("level.ERROR")` |

```bash
kelora -j app.log --span 5m -q \
  -e 'track_inc("events"); if e.level == "ERROR" { track_inc("errors") }' \
  --span-close 'print(`${span.label} ${span.metric("errors")}/${span.size}`)'
```

`span.metrics` contains only additive trackers: `track_freq`, `track_sum`, `track_inc`, `track_avg`, `track_unique`. Trackers that cannot be split per window (`track_min`, `track_max`, `track_percentiles`, `track_cardinality`, `track_top`/`track_bottom`, `track_top_by`/`track_bottom_by`, and the min/max/percentile parts of `track_stats`) are omitted with a one-time warning, and `span.metric()` returns `0` for them. Compute those from `span.events`:

```rhai
let rts = span.events.pluck_as_nums("rt");
let max_rt = if rts.is_empty() { () } else { rts.max() };
```

`span.events` holds every event of the span in memory. More: [Group into Spans](../guide/spans.md).
