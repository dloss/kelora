# Web and API Traffic

## Status codes over time

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz -e 'track_freq("class", e.status / 100 * 100)' \
  --span 30m --span-summary
```

## Which endpoints fail?

Strip query strings first, so `/products/42?utm_source=…` counts as `/products/42`:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log -e 'e.path = e.path.split("?")[0]' --filter 'e.status >= 400' --freq path
```

## The slowest endpoints

Rank endpoints by their worst response time:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -m \
  -e 'track_top_by("slowest", e.endpoint, e.response_time_ms, 3)'
```

## Latency percentiles per endpoint

Name the metric after the group to get one set of statistics per value:

```bash exec="on" source="above" result="ansi"
kelora examples/api_latency_incident.jsonl -m \
  -e 'track_percentiles("ms " + e.endpoint, e.response_time_ms, [0.5, 0.99])'
```

To rank by one percentile, pipe the TSV through `sort`:
`… | grep _p99 | sort -t$'\t' -k3 -rn`. Access logs parsed as `combined` call
the fields `path`, `status`, `bytes`, and (nginx) `request_time`.

## Browsers and operating systems

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz \
  -e 'let ua = e.user_agent.parse_user_agent(); e.browser = ua.get("agent_family") ?? "other"' \
  --freq browser
```

## Internal versus external clients

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log -m \
  -e 'track_freq("origin", if e.ip.is_private_ip() { "internal" } else { "external" })'
```
