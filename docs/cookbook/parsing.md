# Parsing

## Firewall logs (`KEY=VALUE` pairs)

`ufw.log` and iptables logs are syslog; the pairs are in `msg`:

```bash exec="on" source="above" result="ansi"
kelora examples/ufw_firewall.log --filter 'e.msg.contains("[UFW BLOCK]")' \
  -e 'e.absorb_kv("msg")' --freq SRC --freq DPT
```

If your firewall log isn't recognized (`-d` shows only `line`), use `line`
instead of `msg`.

## `key=value` pairs inside messages

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' --input-year 2024 -l error \
  -e 'e.absorb_kv("msg")' -J
```

[Get Logs into Shape](../guide/parse.md#6-finish-the-job-in-a-script)

## Kubernetes container logs with JSON payloads

`kelora pod.log -e 'e.absorb_json("msg")'` — Kelora detects the CRI format, and
`absorb_json` turns the JSON inside `msg` into fields.
[Get Logs into Shape](../guide/parse.md#6-finish-the-job-in-a-script)

## The systemd journal

Let `journalctl` pick the entries (`-p err` is error and worse) and print them
as syslog lines with year and offset; Kelora parses those as `syslog`:

```bash
journalctl -p err --since today -o short-iso | kelora --drain -k msg
```

[Kelora in a pipeline](../guide/output.md#kelora-in-a-pipeline)

## A file that is one JSON array

Exports often hold `[{...}, {...}]` instead of one object per line. Read the
whole file as one event, parse it, and emit each element as its own event:

```bash exec="on" source="above" result="ansi"
kelora examples/json_array.json -f line -M all -e 'emit_each(e.line.parse_json())' -l error
```

[One event per array element](../guide/scripting.md#one-event-per-array-element)

## Keep only the JSON lines (or only the rest)

```bash exec="on" source="above" result="ansi"
kelora examples/mixed_format.log -f json,line --filter 'e._format == "line"' -k line
```

## Read JWT claims

`parse_jwt()` decodes a token without verifying the signature — for
debugging, not for trusting:

```bash exec="on" source="above" result="ansi"
kelora examples/auth-logs.jsonl \
  -e 'let t = e.token.parse_jwt(); e.sub = t.claims.sub; e.role = t.claims.role; e.expires = t.expires_at' \
  -k timestamp,sub,role,expires
```

## A custom format

Work through [Get Logs into Shape](../guide/parse.md): named format, cascade, `cols:`,
`regex:`, then extraction in a script.
