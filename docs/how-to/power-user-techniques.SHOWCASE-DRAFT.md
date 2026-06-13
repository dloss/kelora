# Power-User Techniques

Things Kelora does in one line that would otherwise need a custom script or a
chain of tools. Skim the gallery, find the trick you didn't know existed, and
follow the link when you want the full guide.

!!! tip "How to read this page"
    Each entry is a teaser: a problem, one command, and a link to the deep
    dive. Nothing here is the complete reference — that lives in the
    [Function Reference](../reference/functions.md).

## Group similar errors — `normalized()`

`"Failed to connect to 192.168.1.10"` and `"...10.0.5.23"` are the *same*
error. `normalized()` swaps variable data (IPs, emails, UUIDs, numbers) for
placeholders so they collapse into one pattern.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"msg":"User 192.168.1.1 sent email to alice@example.com with ID a1b2c3d4-e5f6-7890-1234-567890abcdef"}' | \
      kelora -j --exec 'e.pattern = e.msg.normalized()' \
      -k pattern
    ```

→ Pair it with `track_count()` to rank error patterns, or let
[`--drain` mine templates automatically](#template-mining). Full pattern list
and options: [`normalized()` reference](../reference/functions.md).

## Discover log templates automatically — `--drain`

No normalization rules to maintain: Drain clusters raw lines into templates.

```bash
kelora -j examples/app_monitoring.jsonl --drain -k message
```

Formats: `--drain` (table), `=full` (line ranges + samples), `=id` (stable
IDs for diffs), `=json` (programmatic). → [`--drain` reference](../reference/cli-reference.md).

## Deterministic sampling — `bucket()`

`--head`, `sample_prob()`, and `rand()` give different rows every run.
`bucket()` hashes a key to a stable integer, so the *same* request shows up in
every run, every rotation, every service.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/user-activity.jsonl \
      --filter 'e.user_id.bucket() % 20 == 0' \
      -k user_id,action,timestamp
    ```

Same key → same bucket, so you can also shard a huge file into N partitions
(`bucket() % 4 == $i`) for parallel processing. → [Function Reference](../reference/functions.md).

## Flatten deeply nested JSON — `flattened()`

Turn nested API payloads into flat, bracket-keyed fields ready for CSV or SQL.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/deeply-nested.jsonl \
      --exec 'e.flat = e.api.flattened()' \
      --exec 'print(e.flat.to_json())' -q
    ```

For arrays-within-arrays, chain `emit_each()` to fan out multiple levels into
flat rows. → [Flatten Nested JSON for Analysis](fan-out-nested-structures.md).

## Inspect JWT claims — `parse_jwt()`

Read header and claims for debugging, no signature setup.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/auth-logs.jsonl \
      --filter 'e.has("token")' \
      --exec 'let jwt = e.token.parse_jwt();
              e.user = jwt.claims.sub;
              e.role = jwt.claims.role;
              e.token = ()' \
      -k timestamp,user,role
    ```

!!! warning
    Does **not** verify signatures — debugging / trusted tokens only.

→ [Function Reference](../reference/functions.md).

## Surgical string extraction — `between` / `before` / `after`

Pull fields out of semi-structured lines without writing a regex.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"line":"2024-01-15 10:00:00 | INFO | User logged in"}' | \
      kelora -j --exec 'e.timestamp = e.line.before(" | ");
                         e.level = e.line.after(" | ").before(" | ");
                         e.message = e.line.after(" | ", -1)' \
      -k timestamp,level,message
    ```

Nth-occurrence (`after(sep, 2)`), last (`-1`), `between()`, and
`extract_regexes()` for multiple matches. → [Function Reference](../reference/functions.md).

## Fuzzy matching — `edit_distance()`

Levenshtein distance finds typo'd errors or config drift (`prod-web` vs
`prd-web`).

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/error-logs.jsonl \
      --exec 'e.similarity = e.error.edit_distance("connection timeout")' \
      --filter 'e.similarity < 5' \
      -k error,similarity
    ```

→ [Function Reference](../reference/functions.md).

## Hashing & pseudonymization — `hash()` / `pseudonym()`

`sha256` for integrity, `xxh3` for fast bucketing, and `pseudonym()` for
consistent anonymous IDs (HMAC with `KELORA_SECRET`).

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    KELORA_SECRET="your-secret-key" kelora -j examples/analytics.jsonl \
      --exec 'e.anon_user = pseudonym(e.email, "users");
              e.email = ()' \
      -k anon_user,page,duration -F csv
    ```

→ [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md) ·
[Pseudonymize Identifiers](pseudonymize-identifiers-for-analytics.md).

## Extract JSON & key-values from text — `extract_json()` / `absorb_kv()`

Lift structured data out of plain-text log lines.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '2024-01-15 ERROR: Failed with response: {"code":500,"message":"Internal error"}' | \
      kelora --exec 'e.data = e.line.extract_json()' \
      --filter 'e.has("data")' -k line,data
    ```

`extract_jsons()` grabs every object; `absorb_kv("line")` promotes `key=value`
pairs to fields. → [Function Reference](../reference/functions.md).

## Histogram buckets — `track_count()`

See the *distribution*, not just the average.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.has("response_time")' \
      --metrics \
      --exec 'let bucket = (e.response_time / 0.5).floor() * 0.5;
              track_count("response_ms", bucket)'
    ```

→ [Metrics and Tracking](../tutorials/metrics-and-tracking.md).

## Format conversion on the fly — `to_json()` / `to_logfmt()` / cascade

Convert between JSON, logfmt, CSV mid-pipeline, or let cascade mode
(`-f json,logfmt,line`) auto-detect mixed streams line by line.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f json,logfmt,line examples/nightmare_mixed_formats.log \
      -F json | head -5
    ```

→ [Format Reference](../reference/formats.md).

## Cross-event logic — `state`

When `track_*()` isn't enough — deduplication, request/response correlation,
session reconstruction, state machines — the `state` map remembers anything
across events.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      --exec 'state[e.level] = (state.get(e.level) ?? 0) + 1' \
      --end 'print(state.to_map().to_logfmt())' -q
    ```

!!! note
    `state` is sequential-only (not available under `--parallel`). For simple
    counting prefer `track_*()`, which works in parallel.

→ Full recipes (dedup, correlation, FSMs, session rebuild, memory management):
[Advanced Scripting](../tutorials/advanced-scripting.md).

## Combine them

The payoff is composition — fan out nested orders, normalize errors, hash
users, take a deterministic sample, and aggregate, in one command:

```bash
kelora -j api-responses.jsonl \
  --filter 'e.api_version == "v2"' \
  --exec 'emit_each(e.get_path("data.orders", []))' \
  --exec 'emit_each(e.items)' \
  --exec 'e.error_pattern = e.get("error_msg", "").normalized();
          e.user_hash = e.user_id.hash("xxh3");
          e.sample_group = e.order_id.bucket() % 10;
          e.user_id = ()' \
  --filter 'e.sample_group < 3' \
  --metrics \
  --exec 'track_count("error_pattern", e.error_pattern)' \
  -k order_id,sku,quantity,error_pattern -F csv
```

## See Also

- [Advanced Scripting](../tutorials/advanced-scripting.md) — multi-stage transforms & full `state` recipes
- [Metrics and Tracking](../tutorials/metrics-and-tracking.md) — aggregation patterns
- [Function Reference](../reference/functions.md) — complete catalog
- [Flatten Nested JSON](fan-out-nested-structures.md) · [Sanitize Logs](extract-and-mask-sensitive-data.md)
