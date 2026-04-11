# Analyze Sequential Changes with `prev`, `lag`, `delta`, and `ewma`

Use inter-record helpers when you need to compare each event to earlier events in the same stream.
These helpers are ideal for latency jumps, drift detection, and smoothing noisy metrics.

> These functions are **sequential-only**. In `--parallel`, Kelora raises a runtime error.

## 1) Alert on sudden latency jumps (`delta`)

Detect requests whose latency increased sharply compared to the previous record:

```bash
cat <<'JSON' > latency.jsonl
{"svc":"api","duration_ms":100}
{"svc":"api","duration_ms":120}
{"svc":"api","duration_ms":900}
{"svc":"api","duration_ms":910}
JSON

kelora -f json -F json latency.jsonl \
  --exec 'e.delta_ms = delta("duration_ms")' \
  --filter 'e.delta_ms != () && e.delta_ms > 500'
```

Expected output includes the jump event with `delta_ms` around `780`.

## 2) Compare against an older baseline (`lag(..., n)` + `delta(..., n)`)

Compare current values to values from three records ago:

```bash
cat <<'JSON' > throughput.jsonl
{"value":10}
{"value":20}
{"value":30}
{"value":50}
JSON

kelora -f json -F json throughput.jsonl \
  --exec '
    e.baseline_3 = lag("value", 3);
    e.delta_3 = delta("value", 3);
  ' \
  --filter 'e.delta_3 != () && e.delta_3 >= 40'
```

This returns the last event with `baseline_3 = 10` and `delta_3 = 40`.

## 3) Smooth noisy telemetry in-stream (`ewma`)

Compute an EWMA for latency:

```bash
cat <<'JSON' > noisy.jsonl
{"latency_ms":100}
{"latency_ms":200}
{"latency_ms":50}
JSON

kelora -f json -F json noisy.jsonl \
  --exec 'e.latency_smooth = ewma("latency_ms", e.latency_ms.to_float(), 0.5)'
```

With `alpha = 0.5`, the smoothed values are approximately `100`, `150`, `100`.

## 4) Fail fast with strict variants

Use strict variants when missing history or non-numeric values should stop the run:

```bash
kelora -f json data.jsonl --strict \
  --exec 'e.delta = delta_strict("duration_ms")'
```

If the field is missing or non-numeric, Kelora exits with a runtime error that points to the offending function/value.
