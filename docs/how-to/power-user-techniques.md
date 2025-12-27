# Power-User Techniques

Kelora includes powerful features that solve complex log analysis problems with minimal code. These techniques often go undiscovered but can dramatically simplify workflows that would otherwise require custom scripts or multiple tools.

## When to Use These Techniques

- You're dealing with deeply nested JSON from APIs or microservices
- You need to group similar errors that differ only in variable data
- You want deterministic sampling for consistent analysis across log rotations
- You're extracting structured data from unstructured text logs
- You need privacy-preserving analytics with consistent hashing
- You're working with JWTs, URLs, or other complex embedded formats

## Pattern Normalization

### The Problem
Error messages and log lines often contain variable data (IPs, emails, UUIDs, numbers) that make grouping difficult:

```
"Failed to connect to 192.168.1.10"
"Failed to connect to 10.0.5.23"
"Failed to connect to 172.16.88.5"
```

These are the same error pattern but appear as three different messages.

### The Solution: `normalized()`

The `normalized()` function automatically detects and replaces common patterns with placeholders:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"msg":"User 192.168.1.1 sent email to alice@example.com with ID a1b2c3d4-e5f6-7890-1234-567890abcdef"}' | \
      kelora -j --exec 'e.pattern = e.msg.normalized()' \
      -k pattern
    ```

### Real-World Use Case: Error Grouping

Group errors by pattern rather than exact message to see that many different error messages are actually the same pattern repeated with different IPs/UUIDs:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/production-errors.jsonl \
      --exec 'e.error_pattern = e.message.normalized()' \
      --metrics \
      --exec 'track_count(e.error_pattern)'
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/production-errors.jsonl
    ```

### Supported Patterns

By default, `normalized()` replaces:

- IPv4 addresses → `<ipv4>`
- IPv6 addresses → `<ipv6>`
- Email addresses → `<email>`
- UUIDs → `<uuid>`
- URLs → `<url>`
- Numbers → `<num>`

Specify specific patterns if you only want certain replacements:

```bash
# Only normalize IPs and emails
kelora -j logs.jsonl \
  --exec 'e.pattern = e.message.normalized(["ipv4", "email"])'
```

## Deterministic Sampling with `bucket()`

### The Problem
Approximate sampling (`--head N`, `sample_every(n)`, or `rand() < 0.1`) gives different results each run, making it impossible to track specific requests across multiple log files or rotations.

### The Solution: Hash-Based Sampling

The `bucket()` function returns a consistent integer hash for any string, enabling deterministic sampling.

The same `request_id` always hashes to the same number, so you'll get consistent sampling across multiple log files, log rotations, different days, and distributed systems.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/user-activity.jsonl \
      --filter 'e.user_id.bucket() % 20 == 0' \
      -k user_id,action,timestamp
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/user-activity.jsonl
    ```

This always returns the same 5% of users - run it multiple times and you'll get identical results.

**Partition logs for parallel processing:**
```bash
# Process logs in 4 partitions
for i in {0..3}; do
  kelora -j huge.jsonl \
    --filter "e.request_id.bucket() % 4 == $i" \
    > partition_$i.log &
done
wait
```

**Debug specific sessions across microservices:**
```bash
# All logs for session IDs ending in 0-2 (30% sample)
kelora -j service-*.jsonl \
  --filter 'e.session_id.bucket() % 10 < 3'
```

## Deep Structure Flattening

### The Problem
APIs return deeply nested JSON that's hard to query or export to flat formats (CSV, SQL):

```json
{
  "api": {
    "queries": [
      {
        "results": {
          "users": [
            {"id": 1, "permissions": {"read": true, "write": true}}
          ]
        }
      }
    ]
  }
}
```

### The Solution: `flattened()`

The `flattened()` function creates a flat map with bracket-notation keys:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/deeply-nested.jsonl \
      --exec 'e.flat = e.api.flattened()' \
      --exec 'print(e.flat.to_json())' -q
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/deeply-nested.jsonl
    ```

### Advanced: Multi-Level Fan-Out

For extremely nested data, combine `flattened()` with `emit_each()` to chain multiple levels of nesting into flat records:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/nightmare_deeply_nested_transform.jsonl \
      --filter 'e.request_id == "req_002"' \
      --exec 'emit_each(e.get_path("api.queries[0].results.orders", []))' \
      --exec 'emit_each(e.items)' \
      -k sku,quantity,unit_price,final_price -F csv
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -3 examples/nightmare_deeply_nested_transform.jsonl
    ```

## JWT Parsing Without Verification

### The Problem
You need to inspect JWT claims for debugging but don't want to set up signature verification.

### The Solution: `parse_jwt()`

Extract header and claims without cryptographic validation:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/auth-logs.jsonl \
      --filter 'e.has("token")' \
      --exec 'let jwt = e.token.parse_jwt();
              e.user = jwt.claims.sub;
              e.role = jwt.claims.role;
              e.expires = jwt.claims.exp;
              e.token = ()' \
      -k timestamp,user,role,expires
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/auth-logs.jsonl
    ```

**Security Warning:** This does NOT validate signatures. Use only for debugging or parsing tokens you already trust.

### Use Case: Track Token Expiration Issues

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_errors.jsonl \
      --filter 'e.status == 401 && e.has("token")' \
      --exec 'let jwt = e.token.parse_jwt();
              let now = 1732000000;
              e.expired = jwt.claims.exp < now;
              e.expires_in = jwt.claims.exp - now' \
      --filter 'e.expired == true' \
      -k request_id,user,expires_in
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/api_errors.jsonl
    ```

## Advanced String Extraction

Kelora provides powerful string manipulation beyond basic regex:

### Extract Text Between Delimiters

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"log":"Response: <data>secret content</data>"}' | \
      kelora -j --exec 'e.content = e.log.between("<data>", "</data>")' \
      -k content
    ```

### Extract Before/After Markers

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"line":"2024-01-15 10:00:00 | INFO | User logged in"}' | \
      kelora -j --exec 'e.timestamp = e.line.before(" | ");
                         e.level = e.line.after(" | ").before(" | ");
                         e.message = e.line.after(" | ", -1)' \
      -k timestamp,level,message
    ```

**Nth occurrence support:**

- `e.text.after(" | ", 1)` - after first occurrence (default)
- `e.text.after(" | ", -1)` - after last occurrence
- `e.text.after(" | ", 2)` - after second occurrence

### Extract Multiple Items

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"message":"Check https://example.com and http://test.org for more info"}' | \
      kelora -j --exec 'e.urls = e.message.extract_regexes(#"https?://[^\s]+"#)' \
      -F inspect
    ```

## Fuzzy Matching with Edit Distance

### Use Case: Find Typos or Similar Errors

The `edit_distance()` function calculates Levenshtein distance to find errors with typos or slight variations:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/error-logs.jsonl \
      --exec 'e.similarity = e.error.edit_distance("connection timeout")' \
      --filter 'e.similarity < 5' \
      -k error,similarity
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/error-logs.jsonl
    ```

### Use Case: Detect Configuration Drift

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo -e '{"host":"prod-web-01"}\n{"host":"prod-web-02"}\n{"host":"prd-web-01"}' | \
      kelora -j --exec 'e.distance = e.host.edit_distance("prod-web-01")' \
      --filter 'e.distance > 2' \
      -k host,distance
    ```

## Hash Algorithms {#multiple-hash-algorithms}

### The Problem
You need to hash data for checksums, deduplication, or correlation with external systems.

### The Solution: Cryptographic and Non-Cryptographic Hashing

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/user-data.jsonl \
      --exec 'e.sha256 = e.email.hash("sha256");
              e.xxh3 = e.email.hash("xxh3");
              e.email = ()' \
      -k user_id,sha256,xxh3 -F csv
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/user-data.jsonl
    ```

**Available algorithms:**

- `sha256` - SHA-256 (default, cryptographic)
- `xxh3` - xxHash3 (non-cryptographic, extremely fast)

**When to use which:**

- Use `sha256` for checksums, integrity verification, or when you need cryptographic properties
- Use `xxh3` for bucketing, sampling, or deduplication where speed matters and cryptographic security isn't needed

### Use Case: Privacy-Preserving Analytics

Create consistent anonymous IDs using HMAC-SHA256 with a secret key for domain-separated hashing:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    KELORA_SECRET="your-secret-key" kelora -j examples/analytics.jsonl \
      --exec 'e.anon_user = pseudonym(e.email, "users");
              e.anon_session = pseudonym(e.session_id, "sessions");
              e.email = ();
              e.session_id = ()' \
      -k anon_user,anon_session,page,duration -F csv
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/analytics.jsonl
    ```

## Extract JSON from Unstructured Text

### The Problem
Logs contain JSON snippets embedded in plain text:

```
2024-01-15 ERROR: Failed with response: {"code":500,"message":"Internal error"}
```

### The Solution: `extract_json()` and `extract_jsons()`

**Extract first JSON object:**

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '2024-01-15 ERROR: Failed with response: {"code":500,"message":"Internal error"}' | \
      kelora --exec 'e.json_str = e.line.extract_json()' \
      --filter 'e.has("json_str")' \
      --exec 'e.error_data = e.json_str' \
      -k line,error_data
    ```

**Extract all JSON objects:**

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    echo '{"log":"Found errors: {\"a\":1} and {\"b\":2} in output"}' | \
      kelora -j --exec 'e.all_jsons = e.log.extract_jsons()' \
      -F inspect
    ```

## Parse Key-Value Pairs from Text

### The Solution: `absorb_kv()`

Extract `key=value` pairs from unstructured log lines and convert them to structured fields:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/kv_pairs.log \
      --exec 'e.absorb_kv("line")' \
      -k timestamp,action,user,ip,success -F csv
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/kv_pairs.log
    ```

### Options

```bash
# Custom separators
kelora logs.log \
  --exec 'e.absorb_kv("line", #{sep: ";", kv_sep: ":"})'

# Keep original line
kelora logs.log \
  --exec 'e.absorb_kv("line", #{keep_source: true})'
```

## Histogram Bucketing with `track_bucket()`

### The Problem
You want to see the distribution of response times, not just average/max.

### The Solution: Bucket Tracking

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_logs.jsonl \
      --filter 'e.has("response_time")' \
      --metrics \
      --exec 'let bucket = (e.response_time / 0.5).floor() * 0.5;
              track_bucket("response_ms", bucket)'
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/api_logs.jsonl
    ```

### Use Case: HTTP Status Code Distribution

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f combined examples/web_access.log \
      --metrics \
      --exec 'track_bucket("status", e.status / 100 * 100)'
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -5 examples/web_access.log
    ```

## Format Conversion in Pipelines

### Convert Between Formats On-The-Fly

**JSON to logfmt:**

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      --exec 'print(e.to_logfmt())' -q | head -3
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -3 examples/simple_json.jsonl
    ```

**Logfmt to JSON:**

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f logfmt examples/app.log \
      --exec 'print(e.to_json())' -q | head -3
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -3 examples/app.log
    ```

### Use Case: Normalize Multi-Format Logs

Handle logs with mixed JSON and logfmt lines:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/nightmare_mixed_formats.log \
      --exec 'if e.line.contains("{") {
        let json_str = e.line.extract_json();
        e.data = json_str
      } else if e.line.contains("=") {
        e.data = e.line.parse_kv()
      }' \
      --filter 'e.has("data")' \
      -F json | head -5
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -5 examples/nightmare_mixed_formats.log
    ```

## Stateful Processing with `state`

### When to Use `state`

The `state` global map enables complex stateful processing that `track_*()` functions cannot handle:

- **Deduplication**: Track which IDs have already been seen
- **Cross-event dependencies**: Make decisions based on previous events
- **Complex objects**: Store nested maps, arrays, or other structured data
- **Conditional logic**: Remember arbitrary state across events
- **State machines**: Track connection states, session lifecycles
- **Event correlation**: Match request/response pairs, build sessions

**Quick Decision Guide:**

| Feature | `state` | `track_*()` |
|---------|---------|-------------|
| **Purpose** | Complex stateful logic | Simple metrics & aggregations |
| **Read access** | ✅ Yes (during processing) | ❌ No (write-only, read in `--end`) |
| **Parallel mode** | ❌ Sequential only | ✅ Works in parallel |
| **Storage** | Any Rhai value | Any value (strings, numbers, etc.) |
| **Performance** | Slower (RwLock) | Faster (atomic/optimized) |
| **Use for** | Deduplication, FSMs, correlation | Counting, unique tracking, bucketing |

**Important**: For simple counting and metrics, prefer `track_count()`, `track_sum()`, etc.—they work in both sequential and parallel modes. `state` only works in sequential mode.

### The Problem: Deduplication

You have logs with duplicate entries for the same request ID, but you only want to process each unique request once:

```
{"request_id": "req-001", "status": "start"}
{"request_id": "req-002", "status": "start"}
{"request_id": "req-001", "status": "duplicate"}  ← Skip this
{"request_id": "req-003", "status": "start"}
```

### The Solution: Track Seen IDs with `state`

```bash
kelora -j logs.jsonl \
  --exec 'if !state.contains(e.request_id) {
    state[e.request_id] = true;
    e.is_first = true;
  } else {
    e.is_first = false;
  }' \
  --filter 'e.is_first == true' \
  -k request_id,status
```

Only first occurrences pass through; duplicates are filtered out.

### Use Case: Track Complex Per-User State

Store nested maps to track multiple attributes per user:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/user-events.jsonl \
      --exec 'if !state.contains(e.user) {
        state[e.user] = #{login_count: 0, last_seen: (), errors: []};
      }
      let user_state = state[e.user];
      user_state.login_count += 1;
      user_state.last_seen = e.timestamp;
      if e.has("error") {
        user_state.errors.push(e.error);
      }
      state[e.user] = user_state;
      e.user_login_count = user_state.login_count' \
      -k timestamp,user,user_login_count
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/user-events.jsonl
    ```

### Use Case: Sequential Event Numbering

Assign a global sequence number across all events:

```bash
kelora -j logs.jsonl \
  --begin 'state["count"] = 0' \
  --exec 'state["count"] += 1; e.seq = state["count"]' \
  -k seq,timestamp,message -F csv
```

**Note**: For simple counting by category, use `track_count(e.category)` instead.

### Converting State to Regular Map

`state` is a special `StateMap` type with limited operations. To use map functions like `.to_logfmt()` or `.to_kv()`, convert it first:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
      --exec 'state[e.level] = (state.get(e.level) ?? 0) + 1' \
      --end 'print(state.to_map().to_logfmt())' -q
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    head -5 examples/simple_json.jsonl
    ```

### Use Case: Event Correlation (Request/Response Pairs)

Match request and response events, calculating latency and emitting complete transactions:

```bash
kelora -j api-events.jsonl \
  --exec 'if e.event_type == "request" {
    state[e.request_id] = #{sent_at: e.timestamp, method: e.method};
    e = ();  # Don't emit until we see response
  } else if e.event_type == "response" && state.contains(e.request_id) {
    let req = state[e.request_id];
    e.duration_ms = (e.timestamp - req.sent_at).as_millis();
    e.method = req.method;
    state.remove(e.request_id);  # Clean up
  }' \
  -k request_id,method,duration_ms,status
```

### Use Case: State Machines for Protocol Analysis

Track connection states through their lifecycle:

```bash
kelora -j network-events.jsonl \
  --exec 'if !state.contains(e.conn_id) {
    state[e.conn_id] = "NEW";
  }
  let current_state = state[e.conn_id];

  # State transitions
  if current_state == "NEW" && e.event == "SYN" {
    state[e.conn_id] = "SYN_SENT";
  } else if current_state == "SYN_SENT" && e.event == "SYN_ACK" {
    state[e.conn_id] = "ESTABLISHED";
  } else if current_state == "ESTABLISHED" && e.event == "FIN" {
    state[e.conn_id] = "CLOSING";
  } else if e.event != "DATA" {
    e.protocol_error = true;  # Invalid transition
  }
  e.connection_state = state[e.conn_id]' \
  --filter 'e.has("protocol_error")' \
  -k timestamp,conn_id,event,connection_state
```

### Use Case: Session Reconstruction

Accumulate events into complete sessions, emitting only when session ends:

```bash
kelora -j user-events.jsonl \
  --exec 'if e.event == "login" {
    state[e.session_id] = #{
      user: e.user,
      events: [],
      start: e.timestamp
    };
  }
  if state.contains(e.session_id) {
    state[e.session_id].events.push(#{event: e.event, ts: e.timestamp});
  }
  if e.event == "logout" {
    let session = state[e.session_id];
    session.end = e.timestamp;
    session.event_count = session.events.len();
    print(session.to_json());
    state.remove(e.session_id);
  }
  e = ()' -q  # Suppress individual events, only emit complete sessions
```

### Use Case: Rate Limiting - Sample First N per Key

Only emit the first 100 events per API key, then suppress the rest:

```bash
kelora -j api-logs.jsonl \
  --exec 'if !state.contains(e.api_key) {
    state[e.api_key] = 0;
  }
  state[e.api_key] += 1;
  if state[e.api_key] > 100 {
    e = ();  # Drop after first 100 per key
  }' \
  -k timestamp,api_key,endpoint
```

### Performance and Memory Management

For large state maps (millions of keys), consider periodic cleanup:

```bash
kelora -j huge-logs.jsonl \
  --exec 'if !state.contains("counter") { state["counter"] = 0; }
  state["counter"] += 1;

  # Periodic cleanup every 100k events
  if state["counter"] % 100000 == 0 {
    eprint("State size: " + state.len() + " keys");
    if state.len() > 500000 {
      state.clear();  # Reset if too large
      eprint("State cleared");
    }
  }

  # Your stateful logic here
  if !state.contains(e.request_id) {
    state[e.request_id] = true;
  } else {
    e = ();
  }'
```

### Parallel Mode Restriction

`state` requires sequential processing to maintain consistency. Using it with `--parallel` causes a runtime error:

```bash
# This will fail:
kelora -j logs.jsonl --parallel \
  --exec 'state["count"] += 1'
# Error: 'state' is not available in --parallel mode
```

For parallel-safe tracking, use `track_*()` functions instead.

## Combining Techniques

The real power comes from combining these features. Here's a complex real-world example:

```bash
# Process deeply nested API logs with privacy controls
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
  --exec 'track_count(e.error_pattern);
          track_sum("revenue", e.price * e.quantity)' \
  -k order_id,sku,quantity,price,error_pattern -F csv \
  > processed_orders.csv
```

This pipeline:

1. Filters to API v2 only
2. Fans out nested orders → items (multi-level)
3. Normalizes error patterns
4. Hashes user IDs for privacy
5. Creates deterministic 30% sample
6. Tracks error patterns and revenue
7. Exports flat CSV

All in a single command without temporary files or custom scripts.

## Performance Tips

- **Use `bucket()` for sampling before heavy processing** - reduces work by 90% with 10% sample
- **Apply filters early** - before fan-out or expensive transformations
- **Chain operations in one `--exec`** when sharing variables (semicolon-separated)
- **Use `xxh3` hash** for non-cryptographic use cases (much faster than `sha256`)
- **Limit window size** (`--window N`) to minimum needed for sliding calculations

## Troubleshooting

**"Function not found" errors:**

- Check spelling and capitalization (Rhai is case-sensitive)
- Verify the function exists in `kelora --help-functions`

**`()` (unit) value errors:**

- Guard optional fields: `if e.has("field") { ... }`
- Use safe conversions: `to_int_or(e.field, 0)`

**Pattern normalization doesn't work:**

- Check that patterns exist in input: `echo "test 192.168.1.1" | kelora --exec '...'`
- Verify pattern names: `normalized(["ipv4", "email"])` not `["ip", "emails"]`

**Hash consistency issues:**

- Same input + same algorithm = same hash (deterministic)
- Different Kelora versions may use different hash implementations
- Use `KELORA_SECRET` env var for `pseudonym()` to ensure domain separation

## See Also

- [Advanced Scripting Tutorial](../tutorials/advanced-scripting.md) - Multi-stage transformations
- [Metrics and Tracking Tutorial](../tutorials/metrics-and-tracking.md) - Aggregation patterns
- [Function Reference](../reference/functions.md) - Complete function catalog
- [Flatten Nested JSON](fan-out-nested-structures.md) - Deep dive on `emit_each()`
- [Extract and Mask Sensitive Data](extract-and-mask-sensitive-data.md) - Privacy techniques
