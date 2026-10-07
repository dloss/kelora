# Cookbook

Short answers to common questions, each a command you can adapt. Every
command runs against a file in
[`examples/`](https://github.com/dloss/kelora/tree/main/examples); replace it
with your own log and adjust the field names (`kelora yourfile -d` lists
them). Each recipe links to the guide page that explains the options.

## Errors and incidents

### Which services fail most?

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error,critical --freq service
```

[Summarize](guide/summarize.md#count-describe-estimate-one-field)

### What kinds of errors are there?

Group messages that differ only in IDs, IPs, or numbers:

```bash exec="on" source="above" result="ansi"
kelora examples/production-errors.jsonl --drain -k message
```

To keep the pattern as a field for further filtering, use `normalized()`,
which replaces IPs, emails, UUIDs, and the like with placeholders:

```bash exec="on" source="above" result="ansi"
kelora examples/production-errors.jsonl -m -e 'track_freq("pattern", e.message.normalized())'
```

[Summarize](guide/summarize.md#message-templates)

### When did it start?

Count errors per time window; the first busy window is your start:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -l error --span 5m --span-summary
```

[Group into Spans](guide/spans.md)

### What changed after the deploy?

```bash exec="on" source="above" result="ansi"
kelora --drain-diff examples/deploy_before.jsonl examples/deploy_after.jsonl -k msg
```

With one file, split it at the deploy: `--cut-before 'e.msg.contains("deploy")'`
or `--cut-at 2025-01-20T14:00Z`. [Summarize](guide/summarize.md#what-changed-between-two-logs)

### Which exceptions do the stack traces contain?

Join each trace into one event, then extract the exception class:

```bash exec="on" source="above" result="ansi"
kelora examples/stacktrace_java.log -f line -M java \
  -e 'e.exception = e.line.extract_regex("([\\w.]+(?:Exception|Error)):", 1).or_empty()' \
  --freq exception
```

[Get Logs into Shape](guide/parse.md#one-event-spans-several-lines)

### Show the events around each error

```bash exec="on" source="above" result="ansi"
kelora examples/ci_pipeline.log -l error -B 1 -A 1 -c
```

[Filter](guide/filter.md#context-around-matches)

### One timeline from several services

```bash exec="on" source="above" result="ansi"
kelora --merge-sorted examples/merge_api.jsonl examples/merge_worker.jsonl -l error,warn -k ts,service,msg
```

Each file must be in time order. [Big Files, Many Files](guide/files.md#merge-files-by-time)

## Web and API traffic

### Status codes over time

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -e 'track_freq("class", e.status / 100 * 100)' \
  --span 30m --span-summary
```

### Which endpoints fail?

Strip query strings first, so `/products/42?utm_source=…` counts as `/products/42`:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log -e 'e.path = e.path.split("?")[0]' --filter 'e.status >= 400' --freq path
```

### The slowest endpoints

`--describe` gives percentiles for one group; for a ranking, keep each
endpoint's worst response time:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -m \
  -e 'track_top_by("slowest", e.endpoint, e.response_time_ms, 3)' \
  -e 'track_stats("ms", e.response_time_ms)'
```

### Latency percentiles per endpoint

Name the metric after the group to get one set of statistics per value:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -m \
  -e 'track_percentiles("ms " + e.endpoint, e.response_time_ms, [0.5, 0.99])'
```

### Browsers and operating systems

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz \
  -e 'let ua = e.user_agent.parse_user_agent(); e.browser = ua.get("agent_family") ?? "other"' \
  --freq browser
```

### Internal versus external clients

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log -m \
  -e 'track_freq("origin", if e.ip.is_private_ip() { "internal" } else { "external" })'
```

## Security

### Failed SSH logins by IP and user

```bash exec="on" source="above" result="ansi"
kelora examples/sshd_auth.log --input-year 2024 \
  --filter 'e.msg.starts_with("Failed password")' \
  -e 'e.ip = e.msg.extract_ip(); e.user = e.msg.extract_regex("for (?:invalid user )?(\\S+)", 1)' \
  --freq ip --freq user
```

### Read JWT claims

`parse_jwt()` decodes a token without verifying the signature — for
debugging, not for trusting:

```bash exec="on" source="above" result="ansi"
kelora examples/auth-logs.jsonl \
  -e 'let t = e.token.parse_jwt(); e.sub = t.claims.sub; e.role = t.claims.role; e.expires = t.expires_at' \
  -k timestamp,sub,role,expires
```

### Requests from outside a network

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log --filter '!e.ip.is_in_cidr("10.0.0.0/8") && !e.ip.is_private_ip()' -k ip,path,status
```

## Parsing

### `key=value` pairs inside messages

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' --input-year 2024 -l error \
  -e 'e.absorb_kv("msg")' -J
```

[Get Logs into Shape](guide/parse.md#6-finish-the-job-in-a-script)

### Kubernetes container logs with JSON payloads

Kelora detects the CRI format; the JSON inside `msg` needs one more step.
Lines that aren't JSON, like the panic, keep their text:

```bash exec="on" source="above" result="ansi"
kelora examples/pod_cri.log -e 'e.absorb_json("msg")' -k ts,stream,level,msg,status,ms
```

### Keep only the JSON lines (or only the rest)

```bash exec="on" source="above" result="ansi"
kelora examples/mixed_format.log -f json,line --filter 'e._format == "line"' -k line
```

### A custom format

Walk the [parsing ladder](guide/parse.md): named format, cascade, `cols:`,
`regex:`, then extraction in a script.

## Privacy

### Mask addresses inside free text

```bash exec="on" source="above" result="ansi"
kelora examples/email_logs.log -f 'cols:ts level *msg' -e 'e.msg = e.msg.normalized(["email", "ipv4"])' -n 3
```

### Consistent pseudonyms

The same input gives the same alias as long as `KELORA_SECRET` stays the
same, so you can still count, join, and follow a user across files:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl \
  -e 'e.user = pseudonym(e.email, "user"); e.email = (); e.ip = e.ip.mask_ip(1)' -k user,action,ip
```

### Drop sensitive fields

`-K token,password` hides fields from the output; `e.token = ()` in `--exec`
removes them before later stages and summaries see them.

## Exports and samples

### CSV for a spreadsheet

```bash exec="on" source="above" result="ansi"
kelora examples/api_logs.jsonl -l error -k timestamp,service,status,message -F csv
```

### A reproducible 10 % sample

Hash a stable field, so the same requests are chosen every time:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --filter 'e.ip.bucket() % 10 == 0' -J -n 2
```

### One row per array element

```bash exec="on" source="above" result="ansi"
kelora examples/fan_out_batches.jsonl -e 'emit_each(e.orders, #{batch_id: e.batch_id})' \
  -e 'emit_each(e.items, #{batch_id: e.batch_id, order_id: e.order_id})' \
  -k batch_id,order_id,sku,qty,price -F csv -n 4
```

[Transform with Scripts](guide/scripting.md#one-event-per-array-element)

## Monitoring

### Alert when errors spike

Print a line whenever a minute has more than a threshold of failures:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -q --span 1m \
  -e 'if e.status >= 500 { track_inc("failed") }' \
  --span-close 'let n = span.metric("failed"); if n >= 3 { print(`ALERT ${span.label}: ${n} failed requests`) }'
```

On a live stream: `tail -F app.log | kelora -j -q --span 1m …`.

### Fail a CI job when the log has errors

```bash exec="on" source="above" result="ansi" returncode="1"
kelora examples/ci_pipeline.log -q --assert 'e.level != "ERROR"'
```

[Output and Integration](guide/output.md#exit-codes)

### Detect silence

Flag pauses longer than five minutes between events:

```bash exec="on" source="above" result="ansi"
kelora examples/worker_bursts.jsonl --window 1 \
  -e 'if window.len() > 1 && (meta.parsed_ts - to_datetime(window[1].ts)).as_seconds() > 300 { e.after_gap = true }' \
  --filter 'e.has("after_gap")' -k ts,job
```

[Cross-Event Logic](guide/state.md#-window-look-at-previous-events)

### Requests that never got a response

```bash exec="on" source="above" result="ansi"
kelora examples/rpc_pairs.jsonl -q \
  -e 'if e.kind == "request" { state[e.req] = e.path } else { state.remove(e.req) }' \
  --end 'for k in state.keys() { print(`no response: ${k} ${state[k]}`) }'
```

## Time

### Activity by hour of day

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -m -e 'track_freq("hour", meta.parsed_ts.to_timezone("Europe/Berlin").hour())'
```

### Only business hours

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -k timestamp,message -n 3 \
  --filter 'meta.parsed_ts.to_timezone("Europe/Berlin").hour() in 9..17'
```

`--filter` takes a single expression; use `--exec` for scripts with `let`
statements and drop events there with `e = ()`.

[Work with Time](guide/time.md#time-in-scripts)
