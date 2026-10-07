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

## Keep only the JSON lines (or only the rest)

```bash exec="on" source="above" result="ansi"
kelora examples/mixed_format.log -f json,line --filter 'e._format == "line"' -k line
```

## A custom format

Work through [Get Logs into Shape](../guide/parse.md): named format, cascade, `cols:`,
`regex:`, then extraction in a script.
