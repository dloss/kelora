# Flatten Nested JSON for Analysis

Turn events that contain arrays or nested objects into a table of individual records, keeping the context needed for aggregation and reporting.

## When to Use This
- Logs contain batches (orders with items, users with activities) that must become one row per item.
- Analytics tools downstream expect flat records in CSV or JSON lines format.
- You need to enrich child objects with parent metadata before summarising.

## Before You Start
- Examples reference `examples/fan_out_batches.jsonl` and `examples/json_arrays.jsonl`.
- `emit_each()` creates new events and suppresses the original. Emit context manually if you still need the parent (see Step 3).
- Large arrays increase memory usage. Filter or sample before fan-out when possible.

## Step 1: Inspect the Source Shape
Look at a minimal sample to identify the array or object path you want to explode.

```bash
kelora -j examples/fan_out_batches.jsonl -n 2
```

Note field names such as `orders`, `items`, or nested maps you might need later.

## Step 2: Fan Out a Single Array
Use `emit_each()` to turn array elements into individual events.

```bash
kelora -j examples/json_arrays.jsonl \
  -e 'emit_each(e.users)' \
  -k id,name,score
```

Key points:

- After `emit_each()`, `e` refers to the array element.
- The original event is not emitted unless you capture it beforehand.

## Step 3: Preserve Parent Context
Merge parent metadata into each emitted child event using the `base` parameter.

```bash
kelora -j examples/fan_out_batches.jsonl \
  -e 'let ctx = #{batch_id: e.batch_id, created_at: e.created_at};
        emit_each(e.orders, ctx)' \
  -k batch_id,order_id,customer,total
```

- `ctx` values merge with each emitted event.
- Use descriptive keys (e.g., `batch_id`, `parent_service`) so downstream tools can trace lineage.

## Step 4: Handle Multiple Levels
Chain `emit_each()` calls when arrays are nested.

```bash
kelora -j examples/fan_out_batches.jsonl \
  -e 'let batch_ctx = #{batch_id: e.batch_id};
        emit_each(e.orders, batch_ctx)' \
  -e 'let order_ctx = #{batch_id: e.batch_id, order_id: e.order_id};
        emit_each(e.items, order_ctx)' \
  -k batch_id,order_id,sku,qty,price
```

Tips:

- Apply filters between stages to cut unnecessary data early.
- Rename conflicting keys (e.g., parent `id` vs child `id`) to avoid overwriting.

## Step 5: Aggregate or Export
Once events are flat, use metrics or writers to produce the final dataset.

```bash
kelora -j examples/fan_out_batches.jsonl \
  -e 'let ctx = #{batch_id: e.batch_id};
        emit_each(e.orders, ctx)' \
  -e 'let order_ctx = #{batch_id: e.batch_id, order_id: e.order_id};
        emit_each(e.items, order_ctx)' \
  -e 'e.line_total = e.qty * e.price' \
  -e 'track_sum("revenue", e.line_total)' \
  -k batch_id,order_id,sku,qty,price,line_total \
  -F csv > flattened_orders.csv
```

- `track_sum`, `track_count`, and `track_unique` work as usual on flattened events.
- Use `-F csv` or `-J` to ship results to downstream systems.

## Variations
- **Conditional fan-out**  
  ```bash
  kelora -j data.jsonl \
    --filter 'e.items.len() > 0' \
    -e 'let ctx = #{order_id: e.id}; emit_each(e.items, ctx)' \
    --filter 'e.priority == "high"'
  ```

- **Count emitted records**  
  ```bash
  kelora -j data.jsonl \
    -e 'e.emitted = emit_each(e.rows)' \
    -e 'track_sum("row_count", e.emitted)' \
    --metrics
  ```

- **Guard against missing arrays**  
  ```bash
  kelora -j data.jsonl \
    -e 'if e.has_path("logs") { emit_each(e.logs) } else { () }'
  ```

## Good Practices
- Filter parent events before fan-out to avoid unnecessary work (`--filter 'e.type == "invoice"'`).
- Rename or rename conflicting keys immediately after fan-out to keep schemas tidy.
- Document the resulting schema (field names, types) alongside the export so analysts know what to expect.

## See Also
- [Prepare CSV Exports for Analytics](process-csv-data.md) for post-flattening clean-up.
- [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md) if nested objects contain sensitive data.
- [Tutorial: Advanced Scripting](../tutorials/advanced-scripting.md) for more complex Rhai patterns.
