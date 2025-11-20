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

```bash
echo '{"msg":"User 192.168.1.1 sent email to alice@example.com with ID a1b2c3d4-e5f6-7890-1234-567890abcdef"}' | \
  kelora -j --exec 'e.pattern = e.msg.normalized()' \
  -k msg,pattern -J
```

Output:
```json
{
  "msg": "User 192.168.1.1 sent email to alice@example.com with ID a1b2c3d4-e5f6-7890-1234-567890abcdef",
  "pattern": "User <ipv4> sent email to <email> with ID <uuid>"
}
```

### Real-World Use Case: Error Grouping

Group errors by pattern rather than exact message:

```bash
kelora -j production-errors.jsonl \
  --exec 'e.error_pattern = e.message.normalized()' \
  --metrics \
  --exec 'track_count(e.error_pattern)' \
  --end 'for pattern in metrics.keys() {
    print(pattern + ": " + metrics[pattern])
  }' -q
```

This reveals that 500 different error messages are actually 3 patterns repeated with different IPs/UUIDs.

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
Random sampling (`--head N` or `random() < 0.1`) gives different results each run, making it impossible to track specific requests across multiple log files or rotations.

### The Solution: Hash-Based Sampling

The `bucket()` function returns a consistent integer hash for any string, enabling deterministic sampling:

```bash
# Always get the same 10% of requests
kelora -j api-logs.jsonl \
  --filter 'e.request_id.bucket() % 10 == 0' \
  -k timestamp,request_id,path,status
```

The same `request_id` always hashes to the same number, so you'll get consistent sampling across:
- Multiple log files
- Log rotations
- Different days
- Distributed systems (as long as the hash input is the same)

### Use Cases

**Consistent user sampling for behavior analysis:**
```bash
# Always analyze the same 5% of users
kelora -j user-activity.jsonl \
  --filter 'e.user_id.bucket() % 20 == 0'
```

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

```bash
kelora -j deeply-nested.jsonl \
  --exec 'e.flat = e.api.flattened()' \
  --exec 'print(e.flat.to_json())' -q
```

Output:
```json
{
  "queries[0].results.users[0].id": 1,
  "queries[0].results.users[0].permissions.read": true,
  "queries[0].results.users[0].permissions.write": true
}
```

### Advanced: Multi-Level Fan-Out

For extremely nested data, combine `flattened()` with `emit_each()`:

```bash
kelora -j examples/nightmare_deeply_nested_transform.jsonl \
  --filter 'e.request_id == "req_002"' \
  --exec 'emit_each(e.get_path("api.queries[0].results.orders", []))' \
  --exec 'emit_each(e.items)' \
  -k sku,quantity,unit_price,final_price -F csv
```

This chains three levels of nesting (request → orders → items) into flat CSV records in a single pipeline.

## JWT Parsing Without Verification

### The Problem
You need to inspect JWT claims for debugging but don't want to set up signature verification.

### The Solution: `parse_jwt()`

Extract header and claims without cryptographic validation:

```bash
kelora -j auth-logs.jsonl \
  --filter 'e.has("token")' \
  --exec 'let jwt = e.token.parse_jwt();
          e.user = jwt.claims.sub;
          e.role = jwt.claims.role;
          e.expires = jwt.claims.exp;
          e.token = ()' \
  -k timestamp,user,role,expires
```

**Security Warning:** This does NOT validate signatures. Use only for debugging or parsing tokens you already trust.

### Use Case: Track Token Expiration Issues

```bash
kelora -j api-errors.jsonl \
  --filter 'e.status == 401' \
  --exec 'if e.has("token") {
    let jwt = e.token.parse_jwt();
    let now = to_datetime("now").as_epoch();
    e.expired = jwt.claims.exp < now;
    e.expires_in = jwt.claims.exp - now
  }' \
  --filter 'e.expired == true' \
  -k request_id,user,expires_in
```

## Advanced String Extraction

Kelora provides powerful string manipulation beyond basic regex:

### Extract Text Between Delimiters

```bash
# Extract content between XML-like tags
echo '{"log":"Response: <data>secret content</data>"}' | \
  kelora -j --exec 'e.content = e.log.between("<data>", "</data>")' \
  -k content
```

### Extract Before/After Markers

```bash
# Parse custom log format
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

```bash
# Extract all URLs from a field
kelora -j logs.jsonl \
  --exec 'e.urls = e.message.extract_all_re(#"https?://[^\s]+"#)' \
  -F inspect
```

## Fuzzy Matching with Edit Distance

### Use Case: Find Typos or Similar Errors

The `edit_distance()` function calculates Levenshtein distance:

```bash
# Find error messages similar to a known issue
kelora -j error-logs.jsonl \
  --exec 'e.similarity = e.error.edit_distance("connection timeout")' \
  --filter 'e.similarity < 5' \
  -k error,similarity
```

### Use Case: Detect Configuration Drift

```bash
# Compare server hostnames to expected pattern
echo -e '{"host":"prod-web-01"}\n{"host":"prod-web-02"}\n{"host":"prd-web-01"}' | \
  kelora -j --exec 'e.distance = e.host.edit_distance("prod-web-01")' \
  --filter 'e.distance > 2' \
  -k host,distance
```

## Multiple Hash Algorithms

### The Problem
Different systems use different hash algorithms. You might need SHA-256 for one system, MD5 for legacy compatibility, or Blake3 for performance.

### The Solution: Multi-Algorithm Hashing

```bash
# Generate multiple hash formats
kelora -j user-data.jsonl \
  --exec 'e.sha256 = e.email.hash("sha256");
          e.md5 = e.email.hash("md5");
          e.blake3 = e.email.hash("blake3");
          e.xxh3 = e.email.hash("xxh3");
          e.email = ()' \
  -k user_id,sha256,blake3 -F csv
```

**Available algorithms:**
- `sha256` - SHA-256 (default, most common)
- `sha1` - SHA-1 (legacy)
- `md5` - MD5 (legacy, fast but not secure)
- `blake3` - BLAKE3 (fastest modern algorithm)
- `xxh3` - xxHash3 (non-cryptographic, extremely fast)

### Use Case: Privacy-Preserving Analytics

```bash
# Create consistent anonymous IDs
KELORA_SECRET="your-secret-key" kelora -j analytics.jsonl \
  --exec 'e.anon_user = pseudonym(e.email, "users");
          e.anon_session = pseudonym(e.session_id, "sessions");
          e.email = ();
          e.session_id = ()' \
  -F csv > anonymized.csv
```

The `pseudonym()` function uses HMAC-SHA256 with a secret key for domain-separated hashing.

## Extract JSON from Unstructured Text

### The Problem
Logs contain JSON snippets embedded in plain text:

```
2024-01-15 ERROR: Failed with response: {"code":500,"message":"Internal error"}
```

### The Solution: `extract_json()` and `extract_jsons()`

**Extract first JSON object:**
```bash
kelora logs.log \
  --exec 'e.json_str = e.line.extract_json()' \
  --filter 'e.json_str != ""' \
  --exec 'e.error_data = e.json_str; e.parsed = true' \
  -k line,error_data
```

**Extract all JSON objects:**
```bash
echo '{"log":"Found errors: {\"a\":1} and {\"b\":2} in output"}' | \
  kelora -j --exec 'e.all_jsons = e.log.extract_jsons()' \
  -F inspect
```

Output shows an array: `["{"a":1}", "{"b":2}"]`

## Parse Key-Value Pairs from Text

### The Solution: `absorb_kv()`

Extract `key=value` pairs from unstructured log lines:

```bash
kelora examples/kv_pairs.log \
  --exec 'e.absorb_kv("line")' \
  -k timestamp,action,user,ip,success -F csv
```

This automatically parses:
```
2024-01-15T10:00:00Z action=login user=alice ip=192.168.1.10 success=true
```

Into structured fields: `timestamp`, `action`, `user`, `ip`, `success`.

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

```bash
kelora -j api-logs.jsonl \
  --metrics \
  --exec 'let bucket = (e.response_time / 0.5).floor() * 0.5;
          track_bucket("response_ms", bucket)' \
  --end 'print("Response time distribution:");
         for bucket in metrics.response_ms.keys() {
           print("  " + bucket + "s: " + metrics.response_ms[bucket])
         }' -q
```

Output:
```
Response time distribution:
  0s: 1523
  0.5s: 234
  1s: 89
  1.5s: 23
  2s: 12
  5s: 3
```

### Use Case: HTTP Status Code Distribution

```bash
kelora -f combined web-access.log \
  --metrics \
  --exec 'track_bucket("status", e.status / 100 * 100)' \
  --end 'print(metrics.status)' -q
```

Shows `200: 5000, 300: 234, 400: 89, 500: 12`

## Format Conversion in Pipelines

### Convert Between Formats On-The-Fly

```bash
# JSON to logfmt
kelora -j app.jsonl \
  --exec 'print(e.to_logfmt())' -q

# Logfmt to JSON
kelora -f logfmt app.log \
  --exec 'print(e.to_json())' -q

# Any format to CSV with specific fields
kelora mixed-logs.log \
  --exec 'e.parse_if_needed()' \
  -k timestamp,level,message -F csv
```

### Use Case: Normalize Multi-Format Logs

```bash
# Handle logs with mixed JSON and logfmt lines
kelora mixed.log \
  --exec 'if e.line.contains("{") {
    let json_str = e.line.extract_json();
    e.data = json_str
  } else if e.line.contains("=") {
    e.data = e.line.parse_kv()
  }' \
  --filter 'e.has("data")' \
  -F json
```

## Combining Techniques

The real power comes from combining these features. Here's a complex real-world example:

```bash
# Process deeply nested API logs with privacy controls
kelora -j api-responses.jsonl \
  --filter 'e.api_version == "v2"' \
  --exec 'emit_each(e.get_path("data.orders", []))' \
  --exec 'emit_each(e.items)' \
  --exec 'e.error_pattern = e.get("error_msg", "").normalized();
          e.user_hash = e.user_id.hash("blake3");
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
- **Use `blake3` or `xxh3` hashes** for non-cryptographic use cases (much faster)
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
