# Cross-Event Logic with `state`

The `state` global map remembers values across events, so you can solve
problems that per-event `--exec` and the write-only `track_*()` functions
cannot: deduplication, request/response correlation, session reconstruction,
and state machines.

## When to reach for `state`

Use `state` when a decision about the current event depends on events you've
already seen. For plain counting and aggregation, prefer `track_*()` — they're
faster and work in parallel.

| | `state` | `track_*()` |
|---|---|---|
| **Purpose** | Cross-event logic | Metrics & aggregations |
| **Read during processing** | ✅ Yes | ❌ No (read in `--end`) |
| **Parallel mode** | ❌ Sequential only | ✅ Works in parallel |
| **Stores** | Any Rhai value | Any value |
| **Speed** | Slower (RwLock) | Faster |
| **Use for** | Dedup, FSMs, correlation | Counting, unique tracking, bucketing |

!!! warning "Sequential only"
    `state` requires sequential processing. Using it with `--parallel` raises a
    runtime error (`'state' is not available in --parallel mode`). For
    parallel-safe accumulation, use `track_*()` instead.

## Deduplication — process each ID once

Drop repeated entries for the same key, keeping only the first occurrence:

```bash
kelora -j logs.jsonl \
  --exec 'if !state.contains(e.request_id) {
    state[e.request_id] = true;
    e.is_first = true;
  } else {
    e.is_first = false;
  }' \
  --filter 'e.is_first == true' \
  -k request_id,status
```

## Track complex per-key state

Store nested maps to accumulate several attributes per user:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/user-events.jsonl \
      --exec 'if !state.contains(e.user) {
        state[e.user] = #{login_count: 0, last_seen: (), errors: []};
      }
      let user_state = state[e.user];
      user_state.login_count += 1;
      user_state.last_seen = e.timestamp;
      if e.has("error") {
        user_state.errors.push(e.error);
      }
      state[e.user] = user_state;
      e.user_login_count = user_state.login_count' \
      -k timestamp,user,user_login_count
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/user-events.jsonl
    ```

## Sequential event numbering

Assign a global counter across all events:

```bash
kelora -j logs.jsonl \
  --begin 'state["count"] = 0' \
  --exec 'state["count"] += 1; e.seq = state["count"]' \
  -k seq,timestamp,message -F csv
```

For simple counting by category, prefer `track_freq("category", e.category)`.

## Convert `state` to a regular map

`state` is a special `StateMap` with limited operations. Convert it before
using map helpers like `.to_logfmt()` or `.to_kv()`:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      --exec 'state[e.level] = (state.get(e.level) ?? 0) + 1' \
      --end 'print(state.to_map().to_logfmt())' -q
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -5 examples/simple_json.jsonl
    ```

## Event correlation (request/response pairs)

Match requests to responses, compute latency, and emit complete transactions:

```bash
kelora -j api-events.jsonl \
  --exec 'if e.event_type == "request" {
    state[e.request_id] = #{sent_at: e.timestamp, method: e.method};
    e = ();  # Don't emit until we see the response
  } else if e.event_type == "response" && state.contains(e.request_id) {
    let req = state[e.request_id];
    e.duration_ms = (e.timestamp - req.sent_at).as_millis();
    e.method = req.method;
    state.remove(e.request_id);  # Clean up
  }' \
  -k request_id,method,duration_ms,status
```

## State machines for protocol analysis

Track connections through their lifecycle and flag invalid transitions:

```bash
kelora -j network-events.jsonl \
  --exec 'if !state.contains(e.conn_id) {
    state[e.conn_id] = "NEW";
  }
  let current_state = state[e.conn_id];

  if current_state == "NEW" && e.event == "SYN" {
    state[e.conn_id] = "SYN_SENT";
  } else if current_state == "SYN_SENT" && e.event == "SYN_ACK" {
    state[e.conn_id] = "ESTABLISHED";
  } else if current_state == "ESTABLISHED" && e.event == "FIN" {
    state[e.conn_id] = "CLOSING";
  } else if e.event != "DATA" {
    e.protocol_error = true;  # Invalid transition
  }
  e.connection_state = state[e.conn_id]' \
  --filter 'e.has("protocol_error")' \
  -k timestamp,conn_id,event,connection_state
```

## Session reconstruction

Accumulate events into a session and emit only when it ends:

```bash
kelora -j user-events.jsonl \
  --exec 'if e.event == "login" {
    state[e.session_id] = #{ user: e.user, events: [], start: e.timestamp };
  }
  if state.contains(e.session_id) {
    state[e.session_id].events.push(#{event: e.event, ts: e.timestamp});
  }
  if e.event == "logout" {
    let session = state[e.session_id];
    session.end = e.timestamp;
    session.event_count = session.events.len();
    print(session.to_json());
    state.remove(e.session_id);
  }
  e = ()' -q  # Only emit complete sessions
```

## Rate limiting — first N per key

Emit only the first 100 events per API key, then suppress the rest:

```bash
kelora -j api-logs.jsonl \
  --exec 'if !state.contains(e.api_key) {
    state[e.api_key] = 0;
  }
  state[e.api_key] += 1;
  if state[e.api_key] > 100 {
    e = ();  # Drop after first 100 per key
  }' \
  -k timestamp,api_key,endpoint
```

## Memory management for large state

For millions of keys, cap and reset state periodically:

```bash
kelora -j huge-logs.jsonl \
  --exec 'if !state.contains("counter") { state["counter"] = 0; }
  state["counter"] += 1;

  # Periodic cleanup every 100k events
  if state["counter"] % 100000 == 0 {
    eprint("State size: " + state.len() + " keys");
    if state.len() > 500000 {
      state.clear();
      eprint("State cleared");
    }
  }

  if !state.contains(e.request_id) {
    state[e.request_id] = true;
  } else {
    e = ();
  }'
```

## See Also

- [Power-User Techniques](power-user-techniques.md) — the wider feature gallery
- [Advanced Scripting](../tutorials/advanced-scripting.md) — multi-stage transforms
- [Metrics and Tracking](../tutorials/metrics-and-tracking.md) — `track_*()` aggregation
- [Scripting Stages](../concepts/scripting-stages.md) — how `--begin`/`--exec`/`--end` run
