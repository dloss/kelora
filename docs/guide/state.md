# Cross-Event Logic

Most of Kelora looks at one event at a time. Some questions need memory: Is
this the first time we've seen this user? How long did the request take
until its response arrived? Which jobs never finished? Four tools help:

| Tool | Gives you | Typical use |
|---|---|---|
| `state` | a map you can read and write from every event | deduplication, pairing, sessions, state machines |
| `--window N` | the previous N events | comparing with the previous event, moving averages |
| `--begin` | a script that runs once before the first event | lookup tables, initial values |
| `--end` | a script that runs once after the last event | final reports from `metrics` and `state` |

For counting and totals, `track_*()` functions ([Summarize](summarize.md)) are
simpler and faster; reach for `state` when a decision about the current event
depends on earlier ones.

## `state`: remember anything

`state` is a map that survives from one event to the next. Pair each response
with its request, compute the latency, and print only complete pairs:

```bash exec="on" source="above" result="ansi"
kelora examples/rpc_pairs.jsonl \
  -e 'if e.kind == "request" {
        state[e.req] = #{at: meta.parsed_ts, method: e.method, path: e.path};
        e = ();
      } else if state.contains(e.req) {
        let r = state.remove(e.req);
        e.method = r.method; e.path = r.path;
        e.ms = (meta.parsed_ts - r.at).as_milliseconds();
      }' \
  -k req,method,path,status,ms
```

Requests are stored and dropped (`e = ()`); each response picks up its request
and removes it from `state`, so memory stays bounded.

| Operation | Write |
|---|---|
| set, read | `state[key] = value`, `state[key]`, `state.get(key, default)` |
| test, remove | `state.contains(key)`, `state.remove(key)` (returns the value) |
| size, keys | `state.len()`, `state.keys()` |
| reset | `state.clear()` |

Keys are strings; values can be anything, including maps and arrays. To change
a nested value, read it, modify it, and store it back:
`let u = state[e.user]; u.count += 1; state[e.user] = u;`.

Common patterns:

```rhai
// first occurrence only (use as --filter, then remember in --exec)
!state.contains(e.request_id)

// first 100 events per key
state[e.key] = state.get(e.key, 0) + 1; if state[e.key] > 100 { e = () }

// running sequence number
state["n"] = state.get("n", 0) + 1; e.seq = state["n"]
```

`state` holds everything you put in it for the whole run. Remove entries you
no longer need, as in the request/response example. It requires sequential
processing: with `--parallel`, every `state` access is a script error — the
run still exits 0, but your logic never ran ([details](files.md#what-parallel-cant-do)).

## `--window`: look at previous events

`--window N` makes the current event and the N before it available as
`window` (`window[0]` is the current event, `window[1]` the previous one).
Find long pauses between consecutive events:

```bash exec="on" source="above" result="ansi"
kelora examples/worker_bursts.jsonl --window 1 \
  -e 'if window.len() > 1 {
        let gap = (meta.parsed_ts - to_datetime(window[1].ts)).as_seconds();
        if gap > 300 { e.idle_before = humanize_duration(gap * 1000) }
      }' \
  --filter 'e.has("idle_before")' -k ts,job,idle_before
```

`window.pluck("field")` returns that field from every event in the window,
and `window.pluck_as_nums("field")` returns numbers only — the basis for
moving averages. Window entries hold events as they were parsed, before
your scripts changed them.

## `--begin` and `--end`

`--begin` runs once before any input is read. Values stored in `conf` there
are read-only afterwards, which makes it the place for lookup tables (see
[Lookups](scripting.md#lookups)). It can also initialize `state`.

### Reports at the end

`--end` runs once after the last event. It can read `metrics` (everything
`track_*()` collected) and `state`:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -q \
  -e 'track_freq("level", e.level); track_inc("total")' \
  --end 'let errs = metrics.level.get("ERROR") ?? 0;
         print(`${errs} of ${metrics.total} events are errors (${format_percent(errs / metrics.total.to_float(), 1)})`)'
```

Combined with `state`, `--end` reports what is *missing* — here, requests that
never got a response:

```bash exec="on" source="above" result="ansi"
kelora examples/rpc_pairs.jsonl -q \
  -e 'if e.kind == "request" { state[e.req] = e.path } else { state.remove(e.req) }' \
  --end 'for k in state.keys() { print(`no response: ${k} ${state[k]}`) }'
```

`-q` suppresses the events, so only the report is printed. With
`--allow-fs-writes`, `--end` (and other stages) can also write files with
`append_file()`.

## Per-window state

For logic that resets every five minutes or every session, use
[spans](spans.md) instead: the `--span-close` hook sees each window's events
and metrics, and Kelora resets the per-window values for you.
