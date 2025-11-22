# Advanced Scripting

Master Kelora's Rhai scripting stages to reshape events, enrich payloads, and
fan out nested structures. This guide focuses on practical patterns you can drop
into your own pipelines.

## What You'll Learn

- Normalize inconsistent fields and add derived values
- Build multi-stage transformations with `--exec`
- Validate and discard malformed events safely
- Fan out nested arrays and preserve context
- Share reusable helpers with `--include` / `--exec-file`
- Understand resilient vs strict error handling

## Prerequisites

- Completed the [Quickstart](../quickstart.md)
- Reviewed [Metrics and Tracking](metrics-and-tracking.md) for pipeline context

## Step 1 – Normalize Fields Safely

Start with `examples/errors_exec_transform.jsonl`, which includes messy values.

=== "Command"

    ```bash
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'e.status_code = e.status.to_int_or(-1)' \
      -e 'e.bytes_int = e.bytes.to_int_or(0)' \
      -J -n 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'e.status_code = e.status.to_int_or(-1)' \
      -e 'e.bytes_int = e.bytes.to_int_or(0)' \
      -J -n 3
    ```

Using method-style `.to_int_or(fallback)` converts strings to integers and
substitutes the fallback when conversion fails. Stacking `--exec` scripts lets you layer
transformations without writing monolithic expressions.

## Step 2 – Derive New Fields and Filter by Them

Use the normalized values to add severity classification, then keep only error
events.

=== "Command"

    ```bash
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'e.status_code = e.status.to_int_or(-1)' \
      -e 'e.severity = if e.status_code >= 500 { "critical" } else if e.status_code >= 400 { "error" } else { "ok" }' \
      --filter 'e.severity != "ok"' \
      -k timestamp,status_code,severity \
      -F json
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'e.status_code = e.status.to_int_or(-1)' \
      -e 'e.severity = if e.status_code >= 500 { "critical" } else if e.status_code >= 400 { "error" } else { "ok" }' \
      --filter 'e.severity != "ok"' \
      -k timestamp,status_code,severity \
      -F json
    ```

Filters run between exec scripts. Any fields you create earlier are immediately
available to later filters or transformations.

### Enrich Logs with Derived Fields

=== "Command"

    ```bash
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let status = e.status.to_int_or(0); if status >= 500 { e.family = "server_error"; } else if status >= 400 { e.family = "client_error"; } else { e.family = "ok"; }'
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access_large.log.gz \
      -e 'let status = e.status.to_int_or(0); if status >= 500 { e.family = "server_error"; } else if status >= 400 { e.family = "client_error"; } else { e.family = "ok"; }'
    ```

### Pseudonymise Sensitive Attributes

=== "Command"

    ```bash
    kelora -j examples/security_audit.jsonl \
      -e 'e.user_alias = pseudonym(e.user, "users"); e.ip_masked = e.ip.mask_ip(1)' \
      -k timestamp,event,user_alias,ip_masked
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/security_audit.jsonl \
      -e 'e.user_alias = pseudonym(e.user, "users"); e.ip_masked = e.ip.mask_ip(1)' \
      -k timestamp,event,user_alias,ip_masked
    ```

## Step 3 – Guard Against Bad Data

Kelora defaults to resilient mode: exec errors roll back the event and
processing continues. Still, it is better to guard the data and drop malformed
records explicitly.

=== "Command"

    ```bash
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'if !("tags" in e) || type_of(e.tags) != "array" { e = () } else { e.tag_count = e.tags.len(); }' \
      -F json
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/errors_exec_transform.jsonl \
      -e 'if !("tags" in e) || type_of(e.tags) != "array" { e = () } else { e.tag_count = e.tags.len(); }' \
      -F json
    ```

`e = ()` removes the event from the pipeline. The type check avoids runtime
errors when the `tags` field contains a string instead of an array.

!!! warning
    Add `--strict` if you want Kelora to abort on the first error instead of
    skipping bad events. Strict mode is ideal for CI pipelines where silent
    fallback would hide problems.

## Step 4 – Fan Out Nested Data

`emit_each()` converts arrays into individual events while keeping context. The
orders fixture demonstrates nested arrays that you can flatten in two stages.

=== "Command"

    ```bash
    kelora -j examples/fan_out_batches.jsonl \
      -e 'if e.has_path("orders") { emit_each(e.orders, #{batch_id: e.batch_id, created: e.created}) }' \
      -e 'if e.has_path("items") { emit_each(e.items, #{batch_id: e.batch_id, order_id: e.order_id}) }' \
      --filter 'e.has_path("sku")' \
      -k batch_id,order_id,sku,qty,price \
      -J -n 4
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/fan_out_batches.jsonl \
      -e 'if e.has_path("orders") { emit_each(e.orders, #{batch_id: e.batch_id, created: e.created}) }' \
      -e 'if e.has_path("items") { emit_each(e.items, #{batch_id: e.batch_id, order_id: e.order_id}) }' \
      --filter 'e.has_path("sku")' \
      -k batch_id,order_id,sku,qty,price \
      -J -n 4
    ```

The first exec fans out orders while copying batch metadata. The second exec
fans out individual items, enriching each emitted event with both batch and
order identifiers.

!!! tip
    If this chaining feels dense, try the same pattern with fewer steps first:
    run a single `emit_each(e.orders)` call, inspect the output, and then add the
    second `emit_each(e.items, …)` once you’re comfortable with the flow.

## Step 5 – Window-Aware Enrichment

Enable the sliding window to compare the current event to recent history.

=== "Command"

    ```bash
    kelora -j examples/window_metrics.jsonl \
      --filter 'e.metric == "cpu"' \
      --window 3 \
      -e $'let values = window.pluck_as_nums("value");
    if values.len() >= 2 {
        let diff = values[0] - values[1];
        e.delta_vs_prev = round(diff * 100.0) / 100.0;
    }' \
      -k timestamp,value,delta_vs_prev \
      -J -n 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/window_metrics.jsonl \
      --filter 'e.metric == "cpu"' \
      --window 3 \
      -e $'let values = window.pluck_as_nums("value");
    if values.len() >= 2 {
        let diff = values[0] - values[1];
        e.delta_vs_prev = round(diff * 100.0) / 100.0;
    }' \
      -k timestamp,value,delta_vs_prev \
      -J -n 5
    ```

`window` holds the current event plus the previous `N` events (here `N = 3`).
Using `window.pluck_as_nums("FIELD")` avoids manual parsing and gracefully
skips missing values.

## Step 6 – Reuse Logic with `--include` and `--exec-file`

Keep complex logic in separate files. The snippet below defines a helper and
reuses it across multiple commands.

=== "Command"

    ```bash
    cat <<'RHAI' > classifiers.rhai
    fn classify_status(status) {
        let code = status.to_int_or(-1);
        if code >= 500 {
            "critical"
        } else if code >= 400 {
            "error"
        } else if code >= 200 {
            "ok"
        } else {
            "other"
        }
    }
    RHAI

    kelora -j examples/errors_exec_transform.jsonl \
      -I classifiers.rhai \
      -e 'e.severity = classify_status(e.status)' \
      -k timestamp,status,severity \
      -J -n 3
    rm classifiers.rhai
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    cat <<'RHAI' > classifiers.rhai
    fn classify_status(status) {
        let code = status.to_int_or(-1);
        if code >= 500 {
            "critical"
        } else if code >= 400 {
            "error"
        } else if code >= 200 {
            "ok"
        } else {
            "other"
        }
    }
    RHAI

    kelora -j examples/errors_exec_transform.jsonl \
      -I classifiers.rhai \
      -e 'e.severity = classify_status(e.status)' \
      -k timestamp,status,severity \
      -J -n 3
    rm classifiers.rhai
    ```

You can also move long exec blocks to a dedicated Rhai file:

```bash
kelora -j app.jsonl --exec-file transforms.rhai
```

Put shared helpers under `scripts/` or `dev/` and version them alongside your
pipeline definitions.

## Step 7 – Understand Error Handling Semantics

- **Resilient mode (default)**: Kelora rolls back the event when an exec throws
  and prints a warning. The pipeline continues with the next event. Use this for
  exploratory work and long-running tail sessions.

- **Strict mode (`--strict`)**: The first exec error stops the process with
  exit code `1`. Ideal for CI, production batch jobs, or when you have unit
  tests guarding your scripts.

- **Filtering events**: Set `e = ()` to drop a record after you detect invalid
  data. This is preferable to letting bad events fail later stages.

## Troubleshooting Checklist

- **Function not found**: Ensure supporting files are loaded via `-I` or
  `--exec-file`, and double-check spelling—Rhai treats names as case-sensitive.

- **Unexpected fields missing**: Remember that filters remove events between
  exec stages. If a later exec cannot find a field, verify the event survived the
  earlier filters.

- **Decimal precision**: Functions like `round(value)` operate on whole numbers.
  Multiply before rounding (`round(value * 100.0) / 100.0`) to keep two decimal
  places.

- **Fan-out explosion**: Each `emit_each` can multiply events. Add guard
  conditions (`if e.has_path("orders")`) to avoid emitting empty rows.

## Next Steps

- Deepen your understanding of the [Scripting Stages concept](../concepts/scripting-stages.md).
- Learn how Kelora [handles multiline input](../concepts/multiline-strategies.md)
  for stack traces and payloads.

- Explore the [Function Reference](../reference/functions.md) for every helper
  available in the scripting environment.
