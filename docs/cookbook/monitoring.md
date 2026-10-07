# Monitoring and Time

## Alert when errors spike

Print a line whenever a minute has more than a threshold of failures:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -q --span 1m \
  -e 'if e.status >= 500 { track_inc("failed") }' \
  --span-close 'let n = span.metric("failed"); if n >= 3 { print(`ALERT ${span.label}: ${n} failed requests`) }'
```

On a live stream (`tail -F app.log | kelora -j -q --span 1m …`), a minute's
alert prints when the first line of the next minute arrives.

## Fail a CI job when the log has errors

`kelora ci.log -q --assert 'e.level != "ERROR"'` exits 1 if any event breaks the
assertion. [Output and Integration](../guide/output.md#exit-codes-in-scripts-and-ci)

## Detect silence

Compare each event with the previous one using `--window 1`.
[Cross-Event Logic](../guide/state.md#-window-look-at-previous-events) has the
command.

## Requests that never got a response

Remember requests in `state`, remove them when the response arrives, and print
what's left in `--end`. [Cross-Event Logic](../guide/state.md#reports-at-the-end)

## Activity by hour of day

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -m -e 'track_freq("hour", meta.parsed_ts.to_timezone("Europe/Berlin").hour())'
```

## Only business hours

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -k timestamp,message -n 3 \
  --filter 'meta.parsed_ts.to_timezone("Europe/Berlin").hour() in 9..17'
```

[Work with Time](../guide/time.md#time-in-scripts)
