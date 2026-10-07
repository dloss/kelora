# Errors and Incidents

## Which services fail most?

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error,critical --freq service
```

[Summarize](../guide/summarize.md#count-describe-estimate-one-field)

## What kinds of errors are there?

Group messages that differ only in IDs, IPs, or numbers:

```bash exec="on" source="above" result="ansi"
kelora examples/production-errors.jsonl --drain -k message
```

To keep the pattern as a field for further filtering, use `normalized()`,
which replaces IPs, emails, UUIDs, and the like with placeholders:

```bash exec="on" source="above" result="ansi"
kelora examples/production-errors.jsonl -m -e 'track_freq("pattern", e.message.normalized())'
```

[Summarize](../guide/summarize.md#message-templates)

## When did it start?

Count errors per time window; the first busy window is your start. For the
exact first and last error, `-s` prints the output time span.

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -l error --span 5m --span-summary
```

[Group into Spans](../guide/spans.md)

## What changed after the deploy?

`kelora --drain-diff before.log after.log -k msg` lists message templates that
appeared, disappeared, or changed rate. With one file, split it at the deploy:
`--cut-before 'e.msg.contains("deploy")'` or `--cut-at 2025-01-20T14:00Z`.
[Summarize](../guide/summarize.md#what-changed-between-two-logs)

## Which exceptions do the stack traces contain?

Join each trace into one event, then extract the exception class:

```bash exec="on" source="above" result="ansi"
kelora examples/stacktrace_java.log -f line -M java \
  -e 'e.exception = e.line.extract_regex("([\\w.]+(?:Exception|Error)):", 1).or_empty()' \
  --freq exception
```

`-M java` suits output without timestamps; for timestamped logs use
`-M timestamp`. [Get Logs into Shape](../guide/parse.md#one-event-spans-several-lines)

## When did each exception first appear?

Remember the first timestamp per value in `state`, count with `track_freq`,
and print both at the end:

```bash exec="on" source="above" result="ansi"
kelora examples/multiline_stacktrace.log -M timestamp -q \
  -e 'let x = e.msg.extract_regex("([\\w.]+(?:Exception|Error)):", 1);
      if x != "" { track_freq("n", x); if !state.contains(x) { state[x] = e.ts } }' \
  --end 'for k in state.keys() { print(`${k}  first: ${state[k]}  count: ${metrics.n[k]}`) }'
```

[Cross-Event Logic](../guide/state.md)

## Show the events around each error

`kelora app.log -l error -B 2 -A 2` — like `grep -B/-A`, with each event's role
marked. [Filter](../guide/filter.md#context-around-matches)

## One timeline from several services

```bash exec="on" source="above" result="ansi"
kelora --merge-sorted examples/merge_api.jsonl examples/merge_worker.jsonl -l error,warn -k ts,service,msg
```

Each file must be in time order. [Big Files, Many Files](../guide/files.md#merge-files-by-time)
