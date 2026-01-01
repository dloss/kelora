# Function Reference

Complete reference for all 150+ built-in Rhai functions available in Kelora. Functions are organized by category for easy lookup.

!!! tip "Function Call Syntax"
    Rhai allows two styles: `value.method(args)` or `function(value, args)`. Use whichever feels more natural.

## Quick Navigation

- [String Functions](#string-functions) - Text manipulation, parsing, encoding
- [Array Functions](#array-functions) - Array operations, sorting, filtering
- [Map/Object Functions](#mapobject-functions) - Field access, manipulation, conversion
- [DateTime Functions](#datetime-functions) - Time parsing, formatting, arithmetic
- [Math Functions](#math-functions) - Numeric operations
- [Type Conversion](#type-conversion-functions) - Safe type conversions
- [Utility Functions](#utility-functions) - Environment, files, pseudonyms
- [Tracking/Metrics](#trackingmetrics-functions) - Counters, aggregations
- [File Output](#file-output-functions) - Writing data to files
- [Event Manipulation](#event-manipulation) - Field removal, fan-out
- [Span Context](#span-context-span-close-only) - Per-span metadata & rollups

---

## String Functions

### Extraction and Searching

#### `text.extract_regex(pattern [, group])`
Extract first regex match or capture group.

```rhai
e.error_code = e.message.extract_regex(r"ERR-(\d+)", 1)  // "ERR-404" → "404"
e.full_match = e.line.extract_regex(r"\d{3}")            // First 3-digit number
```

#### `text.extract_regexes(pattern [, group])`
Extract all regex matches as array.

```rhai
e.numbers = e.line.extract_regexes(r"\d+")             // All numbers
e.codes = e.message.extract_regexes(r"ERR-(\d+)", 1)   // All error codes
```

#### `text.extract_re_maps(pattern, field)`
Extract regex matches as array of maps for fan-out with `emit_each()`.

```rhai
// Extract all error codes with context
let errors = e.log.extract_re_maps(r"(?P<code>ERR-\d+): (?P<msg>[^\n]+)", "error");
emit_each(errors)  // Each match becomes an event with 'code' and 'msg' fields
```

#### `text.extract_ip([nth])`
Extract IP address from text (nth: 1=first, -1=last).

```rhai
e.client_ip = e.headers.extract_ip()                  // First IP
e.origin_ip = e.forwarded.extract_ip(-1)              // Last IP
```

#### `text.extract_ips()`
Extract all IP addresses as array.

```rhai
e.all_ips = e.headers.extract_ips()                   // ["192.168.1.1", "10.0.0.1"]
```

#### `text.extract_url([nth])`
Extract URL from text (nth: 1=first, -1=last).

```rhai
e.link = e.message.extract_url()                      // First URL
```

#### `text.extract_email([nth])`
Extract email address from text (nth: 1=first, -1=last).

```rhai
e.contact = e.message.extract_email()                 // First email
e.sender = e.log.extract_email(1)                     // First email
e.recipient = e.log.extract_email(-1)                 // Last email
```

#### `text.extract_emails()`
Extract all email addresses as array.

```rhai
e.all_contacts = e.message.extract_emails()           // ["alice@example.com", "bob@test.org"]
```

#### `text.extract_domain()`
Extract domain from URL or email address.

```rhai
e.domain = "https://api.example.com/path".extract_domain()  // "example.com"
e.mail_domain = "user@corp.example.com".extract_domain()    // "corp.example.com"
```

### String Slicing and Position

#### `text.before(delimiter [, nth])`
Text before occurrence of delimiter (nth: 1=first, -1=last).

```rhai
e.user = e.email.before("@")                          // "user@host.com" → "user"
e.path = e.url.before("?")                            // Strip query string
```

#### `text.after(delimiter [, nth])`
Text after occurrence of delimiter (nth: 1=first, -1=last).

```rhai
e.extension = e.filename.after(".")                   // "file.txt" → "txt"
e.domain = e.email.after("@")                         // "user@host.com" → "host.com"
```

#### `text.between(start, end [, nth])`
Text between start and end delimiters (nth: 1=first, -1=last).

**Note:** `text.between(left, right, nth)` is equivalent to `text.after(left, nth).before(right)`.

```rhai
e.quoted = e.line.between('"', '"')                   // Extract quoted string
"[a][b][c]".between("[", "]", 2)                      // "b" - same as .after("[", 2).before("]")
```

#### `text.starting_with(prefix [, nth])`
Return substring from prefix to end (nth: 1=first, -1=last).

```rhai
e.from_error = e.log.starting_with("ERROR:")          // "INFO: ok ERROR: bad" → "ERROR: bad"
```

#### `text.ending_with(suffix [, nth])`
Return substring from start to end of suffix (nth: 1=first, -1=last).

```rhai
e.up_to_end = e.log.ending_with(".txt")               // "file.txt more" → "file.txt"
```

#### `text.slice(spec)`
Slice text using Python notation (e.g., "1:5", ":3", "-2:").

```rhai
e.first_three = e.code.slice(":3")                    // "ABCDEF" → "ABC"
e.last_two = e.code.slice("-2:")                      // "ABCDEF" → "EF"
e.middle = e.code.slice("2:5")                        // "ABCDEF" → "CDE"
```

### Column Extraction

#### `text.col(spec [, separator])`
Extract columns by index/range/list (e.g., '1', '1,3,5', '1:4').

```rhai
e.first = e.line.col("1")                             // First column (1-indexed)
e.cols = e.line.col("1,3,5")                          // Columns 1, 3, 5
e.range = e.line.col("2:5", "\t")                     // Columns 2-5, tab-separated
```

#### `text.cols(col1, col2 [, col3, ...] [, separator])`
Extract multiple columns as an array. Supports up to 6 column indices (1-indexed). Returns an array of column values.

```rhai
// Extract columns 1, 3, 5 as array
let values = e.line.cols(1, 3, 5)                     // ["value1", "value3", "value5"]
e.user = values[0]
e.action = values[1]

// With custom separator
let data = e.line.cols(2, 4, "\t")                    // Tab-separated columns

// Practical example: Apache log parsing
let parts = e.log.cols(1, 4, 7, 9)                    // IP, timestamp, path, status
e.ip = parts[0]
e.timestamp = parts[1]
e.path = parts[2]
e.status = parts[3]
```

### Parsing Functions

#### `text.parse_json()`
Parse JSON string into map/array.

```rhai
e.data = e.payload.parse_json()
e.value = e.data["key"]
```

#### `text.parse_logfmt()`
Parse logfmt line into structured fields.

```rhai
let fields = e.line.parse_logfmt()
e.level = fields["level"]
```

#### `text.parse_syslog()`
Parse syslog line into structured fields.

```rhai
let syslog = e.line.parse_syslog()
e.priority = syslog["priority"]
e.message = syslog["message"]
```

#### `text.parse_combined()`
Parse Apache/Nginx combined log line.

```rhai
let access = e.line.parse_combined()
e.ip = access["ip"]
e.status = access["status"]
```

#### `text.parse_cef()`
Parse Common Event Format line into fields.

```rhai
let cef = e.line.parse_cef()
e.severity = cef["severity"]
```

#### `text.parse_kv([sep [, kv_sep]])`
Parse key-value pairs from text. Only extracts tokens containing the key-value separator; tokens without the separator are skipped (e.g., prose words or unpaired values).

```rhai
e.params = e.query.parse_kv("&", "=")                 // "a=1&b=2" → {a: "1", b: "2"}
e.fields = e.msg.parse_kv()                           // "Payment timeout order=1234" → {order: "1234"}
```

#### `text.parse_url()`
Parse URL into structured components.

```rhai
let url = e.request.parse_url()
e.scheme = url["scheme"]
e.host = url["host"]
e.path = url["path"]
```

#### `text.parse_query_params()`
Parse URL query string into map.

```rhai
e.params = e.query_string.parse_query_params()        // "a=1&b=2" → {a: "1", b: "2"}
```

#### `text.parse_email()`
Parse email address into parts.

```rhai
let email = "User Name <user@example.com>".parse_email()
e.name = email["name"]       // "User Name"
e.address = email["address"] // "user@example.com"
```

#### `text.parse_user_agent()`
Parse common user-agent strings into components.

```rhai
let ua = e.user_agent.parse_user_agent()
e.browser = ua["browser"]
e.os = ua["os"]
```

#### `text.parse_jwt()`
Parse JWT header/payload without verification.

```rhai
let jwt = e.token.parse_jwt()
e.user_id = jwt["payload"]["sub"]
```

#### `text.parse_path()`
Parse filesystem path into components.

```rhai
let path = "/var/log/app.log".parse_path()
e.dir = path["dir"]          // "/var/log"
e.file = path["file"]        // "app.log"
```

#### `text.parse_media_type()`
Parse media type tokens and parameters.

```rhai
let mt = "text/html; charset=utf-8".parse_media_type()
e.type = mt["type"]          // "text"
e.subtype = mt["subtype"]    // "html"
```

#### `text.parse_content_disposition()`
Parse Content-Disposition header parameters.

```rhai
let cd = e.header.parse_content_disposition()
e.filename = cd["filename"]
```

### Encoding and Hashing

#### `text.encode_b64()` / `text.decode_b64()`
Base64 encoding/decoding.

```rhai
e.encoded = e.data.encode_b64()
e.decoded = e.payload.decode_b64()
```

#### `text.encode_hex()` / `text.decode_hex()`
Hexadecimal encoding/decoding.

```rhai
e.hex = e.bytes.encode_hex()
e.bytes = e.hex_string.decode_hex()
```

#### `text.encode_url()` / `text.decode_url()`
URL percent encoding/decoding.

```rhai
e.encoded = e.param.encode_url()                      // "hello world" → "hello%20world"
e.decoded = e.url_param.decode_url()
```

#### `text.escape_json()` / `text.unescape_json()`
JSON escape sequence handling.

```rhai
e.escaped = e.text.escape_json()
e.unescaped = e.json_string.unescape_json()
```

#### `text.escape_html()` / `text.unescape_html()`
HTML entity escaping/unescaping.

```rhai
e.safe = e.user_input.escape_html()                   // "<script>" → "&lt;script&gt;"
e.text = e.html_entity.unescape_html()
```

#### `text.hash([algo])`
Hash with algorithm (default: sha256, also: xxh3).

```rhai
e.checksum = e.content.hash()                         // SHA-256
e.fast = e.data.hash("xxh3")                          // Fast non-crypto hash
```

#### `text.bucket()`
Fast hash for sampling/grouping (returns INT for modulo operations).

```rhai
// Sample 10% of events
if e.user_id.bucket() % 10 == 0 {
    e.sampled = true
}
```

### IP Address Functions

#### `text.is_ipv4()` / `text.is_ipv6()`
Check if text is a valid IP address.

```rhai
if e.addr.is_ipv4() {
    e.ip_version = 4
}
```

#### `text.is_private_ip()`
Check if IP is in private ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16).

```rhai
if e.ip.is_private_ip() {
    e.internal = true
}
```

#### `text.is_in_cidr(cidr)`
Check if IP address is in CIDR network.

```rhai
if e.ip.is_in_cidr("10.0.0.0/8") {
    e.corp_network = true
}
```

#### `text.mask_ip([octets])`
Mask IP address (default: last octet).

```rhai
e.masked_ip = e.client_ip.mask_ip()                   // "192.168.1.100" → "192.168.1.0"
e.partial = e.ip.mask_ip(2)                           // Mask last 2 octets
```

### Pattern Normalization

#### `text.normalized([patterns])`
Replace variable patterns with placeholders (e.g., `<ipv4>`, `<email>`).

Useful for identifying unique log patterns by normalizing variable data like IP addresses, UUIDs, and email addresses to fixed placeholders.

```rhai
// Default patterns (IPs, emails, UUIDs, hashes, etc.)
e.pattern = e.message.normalized()
// "User user@test.com from 192.168.1.5" → "User <email> from <ipv4>"

// CSV-style pattern list
e.simple = e.message.normalized("ipv4,email")

// Array-style pattern list
e.custom = e.message.normalized(["uuid", "sha256", "url"])
```

**Default patterns** (when no argument provided):
`ipv4_port`, `ipv4`, `ipv6`, `email`, `url`, `fqdn`, `uuid`, `mac`, `md5`, `sha1`, `sha256`, `path`, `oauth`, `function`, `hexcolor`, `version`

**Available patterns** (opt-in):
`hexnum`, `duration`, `num`

**Common use case** - Pattern discovery:
```bash
# Recommended alias for easy pattern discovery
kelora --save-alias patterns \
  --exec 'track_unique("patterns", e.message.normalized())' \
  --metrics -q

# Usage
kelora -a patterns app.log
```

**Output with many patterns:**
```
patterns     (127 unique):
  User <email> from <ipv4>
  Request to <url> failed
  Error <uuid> occurred
  Connection <ipv4_port> established
  Processing <fqdn> with <sha256>
  [+122 more. Use --metrics-file or --end script for full list]
```

For custom analysis, access full data in `--end` scripts or `--metrics-file`.

### String Manipulation

#### `text.strip([chars])` / `text.lstrip([chars])` / `text.rstrip([chars])`
Remove whitespace or specified characters.

```rhai
e.clean = e.text.strip()                              // Remove leading/trailing whitespace
e.trimmed = e.line.lstrip("# ")                       // Remove "# " from left
e.path = e.filename.rstrip("/")                       // Remove trailing slashes
```

#### `text.clip()` / `text.lclip()` / `text.rclip()`
Remove non-alphanumeric characters from edges.

```rhai
e.word = "'hello!'".clip()                            // → "hello"
e.left = "...start".lclip()                           // → "start"
e.right = "end...".rclip()                            // → "end"
```

#### `text.upper()` / `text.lower()`
Case conversion. **Note:** Both `upper()`/`lower()` and `to_upper()`/`to_lower()` are available - use whichever you prefer (Rhai builtins vs Python-style).

```rhai
e.normalized = e.country_code.upper()                 // "us" → "US"
e.also_upper = e.code.to_upper()                      // Same as upper()
e.lowercase = e.name.lower()                          // "Hello" → "hello"
e.also_lower = e.name.to_lower()                      // Same as lower()
```

#### `text.replace(pattern, replacement)`
Replace all occurrences of pattern.

```rhai
e.cleaned = e.text.replace("ERROR", "WARN")
```

#### `text.split(separator)` / `text.split_re(pattern)`
Split string into array.

```rhai
e.parts = e.path.split("/")
e.tokens = e.line.split_re(r"\s+")                    // Split on whitespace
```

### String Testing

#### `text.contains(pattern)`
Check if text contains pattern.

```rhai
if e.message.contains("timeout") {
    e.timeout_error = true
}
```

#### `text.like(pattern)`
Glob match (anchored) with `*` and `?`.

```rhai
if e.message.like("ERROR * timeout") {
    e.timeout_error = true
}
```

#### `text.ilike(pattern)`
Case-insensitive glob match with Unicode folding.

```rhai
if e.message.ilike("*straße*") {
    e.locale = "de"
}
```

#### `text.matches(pattern)`
Regex search with cached compilation. Invalid patterns raise errors.

```rhai
if e.path.matches(r"^/api/[^/]+/details$") {
    e.route = "details"
}
```

#### Text Matching Functions Comparison

| Function | Anchored | Errors on invalid pattern | Case handling | Use case |
|----------|----------|---------------------------|---------------|----------|
| `like()` | Yes      | N/A (glob syntax)         | Exact         | Simple wildcard matching |
| `ilike()`| Yes      | N/A                       | Unicode fold  | Case-insensitive glob |
| `matches()` | No   | Yes                       | Regex-driven  | Full regex search with caching |

> ⚠️ Regex performance tips: avoid nested quantifiers like `(.*)*`, prefer anchored patterns when possible, and reuse patterns to benefit from the per-thread cache.

#### `text.is_digit()`
Check if text contains only digits.

```rhai
if e.status.is_digit() {
    e.status_code = e.status.to_int()
}
```

#### `text.count(pattern)`
Count occurrences of pattern in text.

```rhai
e.error_count = e.log.count("ERROR")
```

#### `text.edit_distance(other)`
Compute Levenshtein edit distance between two strings.

```rhai
if e.message.edit_distance("connection reset") <= 3 {
    e.is_connection_issue = true
}
```

#### `text.index_of(substring [, start])`
Find 0-based position of literal substring (-1 if not found). Optional `start` parameter specifies where to begin searching.

```rhai
e.at_pos = e.url.index_of("?")                        // Find first "?"
e.second = e.text.index_of("test", 10)                // Search starting at position 10
```

---

## Array Functions

### Sorting and Filtering

#### `array.sorted()`
Return new sorted array (numeric/lexicographic).

```rhai
e.sorted_scores = sorted(e.scores)                    // [3, 1, 2] → [1, 2, 3]
e.sorted_names = sorted(e.names)                      // Alphabetical
```

#### `array.sorted_by(field)`
Sort array of objects by field name.

```rhai
let sorted_users = sorted_by(e.users, "age")
e.oldest = sorted_users[-1]
```

#### `array.reversed()`
Return new array in reverse order.

```rhai
e.reversed = reversed(e.items)
```

#### `array.slice(spec)`
Slice array using Python notation (e.g., `"1:5"`, `":3"`, `"-2:"`).

```rhai
e.top_three = e.values.slice(":3")                   // [9, 8, 7, 6] → [9, 8, 7]
e.tail = e.values.slice("-2:")                       // [9, 8, 7, 6] → [7, 6]
e.every_other = e.values.slice("0::2")               // [9, 8, 7, 6] → [9, 7]
```

#### `array.unique()`
Remove all duplicate elements (preserves first occurrence).

```rhai
e.unique_tags = unique(e.tags)                        // [1, 2, 1, 3] → [1, 2, 3]
```

#### `array.filter(|item| condition)`
Keep elements matching condition.

```rhai
e.errors = e.logs.filter(|log| log.level == "ERROR")
```

### Aggregation

#### `array.max()` / `array.min()`
Find maximum/minimum value in array.

```rhai
e.max_score = e.scores.max()
e.min_time = e.times.min()
```

#### `array.percentile(pct)`
Calculate percentile of numeric array.

```rhai
e.p95 = e.latencies.percentile(95)
e.median = e.values.percentile(50)
```

#### `array.reduce(|acc, item| expr, init)`
Aggregate array into single value.

```rhai
e.total = e.amounts.reduce(|sum, x| sum + x, 0)
```

### Transformation

#### `array.map(|item| expression)`
Transform each element.

```rhai
e.doubled = e.numbers.map(|n| n * 2)
e.names = e.users.map(|u| u.name)
```

#### `array.pluck(field)` / `array.pluck_as_nums(field)` {#arraypluckfield--arraypluck_as_numsfield}
Extract a single field from each element in an array of maps/objects, returning a new array of just those field values.

**`pluck(field)`** - Extract field values as-is, skipping elements where the field is missing or `()`.

**`pluck_as_nums(field)`** - Extract and convert field values to `f64` numbers, skipping elements where conversion fails or the field is missing.

```rhai
// Given array of event objects
let events = [
    #{status: 200, time: "1.5"},
    #{status: 404, time: "0.3"},
    #{status: 200, time: "2.1"}
]

// Extract field values
let statuses = events.pluck("status")        // [200, 404, 200]
let times = events.pluck_as_nums("time")     // [1.5, 0.3, 2.1] (converted to numbers)

// Compare to manual approach
let manual = events.map(|e| e.status)        // Same result, but errors if field missing
```

**Common use cases:**

```rhai
// Calculate average response time
let times = events.pluck_as_nums("response_time")
let avg = times.reduce(|sum, x| sum + x, 0) / times.len()

// Find most common status codes
let codes = events.pluck("status")
for code in codes {
    track_count(code)
}

// With window for rolling analysis (requires --window)
let recent_times = window.pluck_as_nums("response_time")
e.avg_recent = recent_times.reduce(|sum, x| sum + x, 0) / recent_times.len()
e.spike = recent_times.filter(|t| t > 1000).len()
```

**Why use `pluck()` vs `map()`:**

- Safe: Automatically skips missing fields instead of erroring
- Clear intent: Explicitly shows you're extracting one field
- Type conversion: `pluck_as_nums()` handles string-to-number conversion

#### `array.flattened([style [, max_depth]])`
Flatten nested arrays/objects.

```rhai
e.flat = [[1, 2], [3, 4]].flattened()                 // Returns flat map
e.fields = e.nested.flattened("dot", 2)               // Flatten to dot notation
```

### Testing

#### `array.contains(value)`
Check if array contains value.

```rhai
if e.roles.contains("admin") {
    e.is_admin = true
}
```

#### `array.contains_any(search_array)`
Check if array contains any search values.

```rhai
if e.tags.contains_any(["error", "critical"]) {
    e.alert = true
}
```

#### `array.starts_with_any(search_array)`
Check if array starts with any search values.

```rhai
if e.path_parts.starts_with_any(["/api", "/v1"]) {
    e.api_call = true
}
```

#### `array.all(|item| condition)` / `array.some(|item| condition)`
Check if all/any elements match condition.

```rhai
e.all_valid = e.scores.all(|s| s >= 0)
e.has_errors = e.logs.some(|l| l.level == "ERROR")
```

### Other Operations

#### `array.join(separator)`
Join array elements with separator.

```rhai
e.path = e.parts.join("/")
e.csv = e.values.join(",")
```

#### `array.push(item)` / `array.pop()`
Add/remove items from array.

```rhai
e.tags.push("new_tag")
let last = e.items.pop()
```

---

## Map/Object Functions

### Field Access

#### `map.get_path("field.path" [, default])`
Safe nested field access with fallback.

```rhai
e.user_name = e.get_path("user.profile.name", "unknown")
e.score = e.get_path("stats.score", 0)
```

#### `map.has_path("field.path")`
Check if nested field path exists.

```rhai
if e.has_path("error.details.code") {
    e.detailed_error = true
}
```

#### `map.path_equals("path", value)`
Safe nested field comparison.

```rhai
if path_equals(e, "user.role", "admin") {
    e.elevated = true
}
```

#### `map.has("key")`
Check if map contains key with non-unit value.

```rhai
if e.has("error_code") {
    // Field exists and has a value
}
```

### Field Manipulation

#### `map.rename_field("old", "new")`
Rename a field, returns true if successful.

```rhai
e.rename_field("old_name", "new_name")
```

#### `map.merge(other_map)`
Merge another map into this one (overwrites existing keys).

```rhai
e.merge(#{status: "ok", timestamp: now()})
```

#### `map.enrich(other_map)`
Merge another map, inserting only missing keys (does not overwrite).

```rhai
e.enrich(#{user: "default", level: "info"})  // Only adds if keys don't exist
```

#### `map.flattened([style [, max_depth]])`
Flatten nested object to dot notation.

```rhai
let flat = e.nested.flattened("dot")                  // {a: {b: 1}} → {"a.b": 1}
let flat = e.nested.flattened("dot", 2)               // With max depth
```

#### `map.flatten_field("field_name")`
Flatten just one specific field from the map.

```rhai
let flat = e.flatten_field("metadata")                // Flattens only e.metadata
```

#### `map.unflatten([separator])`
Reconstruct nested object from flat keys.

```rhai
let nested = e.flat.unflatten(".")                    // {"a.b": 1} → {a: {b: 1}}
```

### Format Conversion

#### `map.to_json([pretty])`
Convert map to JSON string.

```rhai
e.payload = e.data.to_json()
e.readable = e.data.to_json(true)                     // Pretty-printed
```

#### `map.to_logfmt()`
Convert map to logfmt format string.

```rhai
e.formatted = e.fields.to_logfmt()                    // {a: 1, b: 2} → "a=1 b=2"
```

#### `map.to_kv([sep [, kv_sep]])`
Convert map to key-value string with separators.

```rhai
e.query = e.params.to_kv("&", "=")                    // {a: 1, b: 2} → "a=1&b=2"
```

#### `map.to_syslog()` / `map.to_cef()` / `map.to_combined()`
Convert map to specific log format.

```rhai
e.syslog_line = e.fields.to_syslog()
e.cef_line = e.security_event.to_cef()
e.access_log = e.request.to_combined()
```

---

## DateTime Functions

### Creation

#### `now()`
Current timestamp (UTC).

```rhai
e.timestamp = now()
```

#### `to_datetime(text [, fmt [, tz]])`
Convert string into datetime value with optional hints.

```rhai
e.parsed = to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S", "UTC")
e.auto = to_datetime("2024-01-15T10:30:00Z")          // Auto-detect format
```

#### `to_duration("1h30m")`
Convert duration string into duration value.

```rhai
let timeout = to_duration("5m")
e.deadline = now() + timeout
```

#### `duration_from_seconds(n)`, `duration_from_minutes(n)`, etc.
Create duration from specific units.

```rhai
let hour = duration_from_hours(1)
let day = duration_from_days(1)
```

### Formatting

#### `dt.to_iso()`
Convert datetime to ISO 8601 string.

```rhai
e.iso_timestamp = e.timestamp.to_iso()                // "2024-01-15T10:30:00Z"
```

#### `dt.format("format_string")`
Format datetime using custom format string (see `--help-time`).

```rhai
e.date = e.timestamp.format("%Y-%m-%d")               // "2024-01-15"
e.time = e.timestamp.format("%H:%M:%S")               // "10:30:00"
```

### Component Extraction

#### `dt.year()`, `dt.month()`, `dt.day()`
Extract date components.

```rhai
e.year = e.timestamp.year()
e.month = e.timestamp.month()
e.day = e.timestamp.day()
```

#### `dt.hour()`, `dt.minute()`, `dt.second()`
Extract time components.

```rhai
e.hour = e.timestamp.hour()
```

### Timezone Conversion

#### `dt.to_utc()` / `dt.to_local()`
Convert timezone.

```rhai
e.utc_time = e.local_timestamp.to_utc()
e.local_time = e.utc_timestamp.to_local()
```

#### `dt.to_timezone("tz_name")`
Convert to named timezone.

```rhai
e.ny_time = e.timestamp.to_timezone("America/New_York")
```

#### `dt.timezone_name()`
Get timezone name as string.

```rhai
e.tz = e.timestamp.timezone_name()                    // "UTC"
```

### Time Bucketing

#### `dt.round_to("interval")`
Round timestamp down to the nearest interval. Useful for grouping events into time buckets for histograms and time-series analysis.

Accepts duration strings like `"5m"`, `"1h"`, `"1d"`, etc.

```rhai
// Group events into 5-minute buckets
let timestamp = to_datetime(e.timestamp);
e.bucket = timestamp.round_to("5m").to_iso();
track_bucket("requests_per_5min", e.bucket);

// Hourly buckets
e.hour_bucket = to_datetime(e.time).round_to("1h").format("%Y-%m-%d %H:00");

// Daily buckets
e.day = timestamp.round_to("1d").format("%Y-%m-%d");
```

**Common intervals:**
- `"1m"`, `"5m"`, `"15m"` - Minute-level bucketing
- `"1h"`, `"6h"`, `"12h"` - Hour-level bucketing
- `"1d"`, `"7d"` - Day/week-level bucketing

### Arithmetic and Comparison

#### `dt + duration`, `dt - duration`
Add/subtract duration from datetime.

```rhai
e.future = now() + duration_from_hours(1)
e.past = now() - duration_from_days(7)
```

#### `dt1 - dt2`
Get duration between datetimes.

```rhai
let elapsed = now() - e.start_time
e.duration_ms = elapsed.as_milliseconds()
```

#### `dt1 == dt2`, `dt1 > dt2`, etc.
Compare datetimes.

```rhai
if e.timestamp > to_datetime("2024-01-01") {
    e.this_year = true
}
```

### Duration Operations

#### `duration.as_seconds()`, `duration.as_milliseconds()`, etc.
Convert duration to specific units.

```rhai
e.seconds = duration.as_seconds()
e.ms = duration.as_milliseconds()
e.hours = duration.as_hours()
```

#### `duration.to_string()` / `humanize_duration(ms)`
Format duration as human-readable string.

```rhai
e.readable = duration.to_string()                     // "1h 30m"
e.humanized = humanize_duration(5400000)              // "1h 30m"
```

#### `duration.to_debug()`
Format duration with full precision for debugging. Useful for inspecting exact duration values.

```rhai
e.debug_duration = duration.to_debug()                // Full precision debug output
```

---

## Math Functions

#### `abs(x)`
Absolute value of number.

```rhai
e.magnitude = abs(e.value)
```

#### `clamp(value, min, max)`
Constrain value to be within min/max range.

```rhai
e.bounded = clamp(e.score, 0, 100)
```

#### `floor(x)` / `round(x)`
Rounding operations.

```rhai
e.floored = floor(e.value)
e.rounded = round(e.value)
```

#### `mod(a, b)` / `a % b`
Modulo operation with division-by-zero protection.

```rhai
e.bucket = e.id % 10
```

#### `rand()` / `rand_int(min, max)`
Random number generation.

```rhai
e.random_id = rand_int(1000, 9999)                    // Random ID assignment

// For sampling, prefer sample_every() instead:
// if sample_every(10) { e.sampled = true }           // Better: counter-based
```

#### `sample_every(n)`
Sample every Nth event - returns `true` on calls N, 2N, 3N, etc.

Fast counter-based sampling (thread-local, approximate in parallel mode). Each unique N value maintains its own counter. For deterministic sampling across parallel threads, use `bucket()` instead.

```rhai
// Keep only every 100th event (1% sampling)
if !sample_every(100) { skip() }

// Keep every 10th event (10% sampling)
if sample_every(10) {
    e.sampled = true
}

// Different N values have independent counters
sample_every(10)    // Returns true on calls 10, 20, 30...
sample_every(100)   // Returns true on calls 100, 200, 300...
```

**Comparison with `bucket()`:**
- `sample_every(n)` - Fast counter, approximate in parallel mode, non-deterministic
- `e.field.bucket() % n == 0` - Hash-based, deterministic across runs/threads, slightly slower

---

## Type Conversion Functions

#### `to_int(value)` / `to_float(value)` / `to_bool(value)`
Convert value to type (returns `()` on error).

```rhai
e.status = to_int(e.status_string)
e.score = to_float(e.score_string)
```

#### `to_int(value, thousands_sep)` / `to_float(value, thousands_sep, decimal_sep)`
Parse formatted numbers with explicit separators.

**Parameters:**
- `thousands_sep` - The thousands/grouping separator (single char or empty string)
- `decimal_sep` - The decimal separator (single char or empty string)

**Examples:**

```rhai
// US format (comma thousands, dot decimal)
e.price = "1,234.56".to_float(',', '.')     // → 1234.56
e.count = "1,234,567".to_int(',')           // → 1234567

// EU format (dot thousands, comma decimal)
e.price = "1.234,56".to_float('.', ',')     // → 1234.56
e.count = "1.234.567".to_int('.')           // → 1234567

// French format (space thousands, comma decimal)
e.price = "1 234,56".to_float(' ', ',')     // → 1234.56
e.count = "2 000 000".to_int(' ')           // → 2000000

// No thousands separator (empty string)
e.price = "1234.56".to_float("", '.')       // → 1234.56
```

#### `to_int_or(value, default)` / `to_float_or(value, default)` / `to_bool_or(value, default)`
Convert value to type with fallback.

```rhai
e.status = e.status_string.to_int_or(0)
e.score = e.score_string.to_float_or(0.0)
```

#### `to_int_or(value, thousands_sep, default)` / `to_float_or(value, thousands_sep, decimal_sep, default)`
Parse formatted numbers with separators and fallback.

```rhai
// With error handling
e.amount = e.value.to_float_or(',', '.', 0.0)   // Default to 0.0 if invalid
e.count = e.total.to_int_or(',', 0)             // Default to 0 if invalid
```

#### `value.or_empty()`
Convert empty values to Unit `()` for removal/filtering.

Converts conceptually "empty" values to Unit, which:

- Removes the field when assigned (e.g., `e.field = value.or_empty()`)
- Gets skipped by `track_*()` functions
- Works with missing fields (passes Unit through unchanged)

**Supported empty values:**

- Empty string: `""` → `()`
- Empty array: `[]` → `()`
- Empty map: `#{}` → `()`
- Unit itself: `()` → `()` (pass-through)

**String extraction:**
```rhai
// Extract only when prefix exists, otherwise remove field
e.name = e.message.after("prefix:").or_empty()

// Track only non-empty values
track_unique("names", e.extracted.or_empty())
```

**Array filtering:**
```rhai
// Only assign tags if array is non-empty
e.tags = e.tags.or_empty()  // [] becomes (), field removed

// Track only events with items
track_bucket("item_count", e.items.len())
if e.items.len() == 0 {
    e.items = e.items.or_empty()  // Remove empty array
}
```

**Map filtering:**
```rhai
// Only keep non-empty metadata
e.metadata = e.parse_json().or_empty()  // {} becomes (), field removed

// Safe chaining with missing fields
e.optional = e.maybe_field.or_empty()  // Works even if maybe_field is ()
```

**Common pattern - conditional extraction and tracking:**
```rhai
e.extracted = e.message.after("User:").or_empty()
track_unique("users", e.extracted)  // Only tracks when extraction succeeds

// Filter events with no data
e.results = e.search_results.or_empty()
track_unique("result_sets", e.results)  // Skips empty arrays and ()
```

---

## Utility Functions

#### `get_env(var [, default])`
Get environment variable with optional default.

```rhai
e.branch = get_env("CI_BRANCH", "main")
e.build_id = get_env("BUILD_ID")
```

#### `pseudonym(value, domain)`
Generate domain-separated pseudonym (requires `KELORA_SECRET`).

```rhai
e.user_alias = pseudonym(e.username, "users")
e.ip_alias = pseudonym(e.client_ip, "ips")
```

#### `read_file(path)` / `read_lines(path)`
Read file contents.

```rhai
e.config = read_file("config.json")
e.lines = read_lines("data.txt")
```

#### `drain_template(text [, options])`
Add a line to the Drain template model and return `{template, count, is_new}`. Sequential mode only.

```rhai
let r = drain_template(e.message);
e.template = r.template;
```

Default token filters normalize: ipv4_port, ipv4, ipv6, email, url, fqdn, uuid, mac,
md5, sha1, sha256, path, oauth, function, hexcolor, version, hexnum, duration,
timestamp, date, time, num.

Optional `options` map keys:

- `depth` (int)
- `max_children` (int)
- `similarity` (float)
- `filters` (string CSV or array of grok patterns)

#### `drain_templates()`
Return array of `{template, count}` from the current Drain model. Sequential mode only.

```rhai
let templates = drain_templates();
```

#### `print(message)` / `eprint(message)`
Print to stdout/stderr (suppressed with `--no-script-output` or data-only modes).

```rhai
print("Processing event: " + e.id)
eprint("Warning: " + e.error)
```

#### `exit(code)`
Exit kelora with given exit code.

```rhai
if e.critical {
    exit(1)
}
```

#### `skip()`
Skip the current event, mark it as filtered, and continue with the next one. Downstream stages and output for the skipped event do not run.

```rhai
if e.endpoint == "/health" {
    skip();
}
```

#### `status_class(status_code)`
Convert HTTP status code to class string ("1xx", "2xx", "3xx", "4xx", "5xx", or "unknown").

```rhai
e.status_category = status_class(e.status)            // 404 → "4xx", 200 → "2xx"
e.is_error = status_class(e.code) == "5xx"

// Track errors by class
track_count(status_class(e.status))

// Group status codes for analysis
e.status_group = status_class(e.response_code)        // 503 → "5xx"
```

#### `type_of(value)`
Get type name as string.

```rhai
e.value_type = type_of(e.value)                       // "string", "int", "array", etc.
```

#### `window.pluck(field)` / `window.pluck_as_nums(field)`
Extract field values from the sliding window array (requires `--window`). See [`array.pluck()`](#arraypluckfield--arraypluck_as_numsfield) for detailed documentation.

The `window` variable is an array containing the N most recent events, making `pluck()` especially useful for rolling calculations and burst detection.

```rhai
// Rolling average of response times
let recent_times = window.pluck_as_nums("response_time")
e.avg_recent = recent_times.reduce(|sum, x| sum + x, 0) / recent_times.len()

// Detect error bursts
let recent_statuses = window.pluck("status")
e.error_burst = recent_statuses.filter(|s| s >= 500).len() >= 3

// Compare current vs recent average
let recent_vals = window.pluck_as_nums("value")
e.spike = e.value > (recent_vals.reduce(|s, x| s + x, 0) / recent_vals.len()) * 2
```

---

## State Management Functions

The global `state` object provides a mutable map for tracking information across events. **Only available in sequential mode** - accessing `state` in `--parallel` mode will raise an error.

!!! warning "Parallel Mode"
    State management is **not available** when using `--parallel`. All state operations will raise errors. Use `--metrics` tracking functions for parallel-safe aggregation.

### Basic Operations

#### `state["key"]` / `state[key] = value`
Get or set values using indexer syntax.

```rhai
// Initialize counter
state["count"] = 0

// Increment counter
state["count"] = state["count"] + 1

// Track unique IPs
if !state.contains("seen_ips") {
    state["seen_ips"] = []
}
state["seen_ips"].push(e.ip)
```

#### `state.get(key)` / `state.set(key, value)`
Get or set values using method syntax. `get()` returns `()` if key doesn't exist.

```rhai
let count = state.get("count")                        // Returns () if not found
state.set("total_bytes", 0)

// Safer pattern with default
let current = state.get("count") ?? 0
state.set("count", current + 1)
```

#### `state.contains(key)`
Check if a key exists in state.

```rhai
if !state.contains("initialized") {
    state["initialized"] = true
    state["start_time"] = now()
}
```

### Map Operations

#### `state.keys()` / `state.values()`
Get arrays of all keys or values.

```rhai
let all_keys = state.keys()                           // ["count", "total", "seen_ips"]
let all_values = state.values()                       // [42, 1024, [...]]

// Iterate over all state entries
for key in state.keys() {
    print(key + ": " + state[key].to_string())
}
```

#### `state.len()` / `state.is_empty()`
Get number of entries or check if empty.

```rhai
if state.is_empty() {
    state["initialized"] = true
}

let num_keys = state.len()                            // Number of entries
```

#### `state.remove(key)`
Remove a key from state and return its value (or `()` if not found).

```rhai
let old_value = state.remove("temp_data")             // Remove and get value
state.remove("cache")                                 // Just remove
```

#### `state.clear()`
Remove all entries from state.

```rhai
// Reset state
state.clear()
```

### Bulk Operations

#### `state.mixin(map)`
Merge a map into state, overwriting existing keys.

```rhai
// Initialize multiple values
state.mixin(#{
    count: 0,
    total_bytes: 0,
    seen_users: []
})

// Merge new data
state.mixin(e.metadata)                               // Add all metadata fields
```

#### `state.fill_with(map)`
Replace entire state with a new map.

```rhai
// Reset state with new values
state.fill_with(#{
    count: 0,
    start_time: now()
})
```

#### `state += map`
Operator form of `mixin()` - merge map into state.

```rhai
state += #{ new_field: 42, another: "value" }
```

### Conversion

#### `state.to_map()`
Convert state to a regular map for use with other functions.

```rhai
// Export state as JSON
let state_json = state.to_map().to_json()
print(state_json)

// Export as logfmt
let state_logfmt = state.to_map().to_logfmt()

// Use in conditions
let snapshot = state.to_map()
if snapshot.contains("error_count") && snapshot["error_count"] > 100 {
    exit(1)
}
```

### Practical Examples

**Counter Pattern:**
```rhai
// Initialize on first event
if state.is_empty() {
    state["event_count"] = 0
    state["error_count"] = 0
}

// Increment counters
state["event_count"] = state["event_count"] + 1
if e.level == "ERROR" {
    state["error_count"] = state["error_count"] + 1
}

// Output summary at end
--end 'print("Events: " + state["event_count"] + ", Errors: " + state["error_count"])'
```

**Deduplication Pattern:**
```rhai
// Initialize seen set
if !state.contains("seen_ids") {
    state["seen_ids"] = #{}  // Use map as set
}

// Skip duplicates
if state["seen_ids"].contains(e.request_id) {
    skip()
}
state["seen_ids"][e.request_id] = true
```

**Session Tracking:**
```rhai
// Track active sessions
if !state.contains("sessions") {
    state["sessions"] = #{}
}

let session_id = e.session_id
if !state["sessions"].contains(session_id) {
    state["sessions"][session_id] = #{
        start: e.timestamp,
        events: 0
    }
}

// Update session
let session = state["sessions"][session_id]
session["events"] = session["events"] + 1
session["last_seen"] = e.timestamp
```

---

## Tracking/Metrics Functions

All tracking functions require the `--metrics` flag.

!!! tip "Unit Value Handling"
    All `track_*()` functions that accept values silently skip Unit `()` values. This enables safe tracking of optional or extracted fields without needing conditional checks.

### Tracking Functions {#tracking-functions}

#### `track_avg(key, value)`
Track average of numeric values for key. Automatically computes the average during output. Skips Unit `()` values. Works correctly in parallel mode.

```rhai
track_avg("avg_latency", e.response_time)
track_avg(e.endpoint, e.duration_ms)

// Safe with conversions that may fail
let latency = e.latency_str.to_float()  // Returns () on error
track_avg("avg_ms", latency)            // Skips () values
```

#### `track_count(key)`
Increment counter for key by 1.

```rhai
track_count(e.service)                                // Count by service
track_count("total")                                  // Global counter
```

#### `track_sum(key, value)`
Accumulate numeric values for key. Skips Unit `()` values.

```rhai
track_sum("total_bytes", e.bytes)
track_sum(e.endpoint, e.response_time)

// Safe with conversions that may fail
let score = e.score_str.to_int()  // Returns () on error
track_sum("total_score", score)   // Skips () values
```

#### `track_min(key, value)` / `track_max(key, value)`
Track minimum/maximum value for key. Skips Unit `()` values.

```rhai
track_min("fastest", e.response_time)
track_max("slowest", e.response_time)
```

#### `track_unique(key, value)`
Track unique values for key. Skips Unit `()` values.

```rhai
track_unique("users", e.user_id)
track_unique("ips", e.client_ip)

// Combined with .or_empty() for conditional tracking
track_unique("names", e.message.after("User:").or_empty())
```

#### `track_bucket(key, bucket)`
Track values in buckets for histograms. Skips Unit `()` values.

```rhai
let bucket = floor(e.response_time / 100) * 100
track_bucket("latency", bucket)

// Safe with optional fields
track_bucket("user_types", e.user_type.or_empty())  // Skips empty/missing
```

#### `track_top(key, item, n)` / `track_top(key, item, n, value)`
Track top N most frequent items (count mode) or highest-valued items (weighted mode). Skips Unit `()` values.

**Count mode** tracks the N items that appear most frequently:

```rhai
// Track top 10 most common errors
track_top("common_errors", e.error_type, 10)

// Track top 5 most active users
track_top("active_users", e.user_id, 5)
```

**Weighted mode** tracks the N items with the highest custom values:

```rhai
// Track top 10 slowest endpoints by latency
track_top("slowest_endpoints", e.endpoint, 10, e.latency_ms)

// Track top 5 biggest requests by bytes
track_top("heavy_requests", e.request_id, 5, e.bytes)

// Handles missing values gracefully
track_top("cpu_hogs", e.process, 10, e.cpu_time.or_empty())  // Skips ()
```

**Output format:**
- Count mode: `[{key: "item", count: 42}, ...]`
- Weighted mode: `[{key: "item", value: 123.4}, ...]`
- Results are sorted by value descending, then alphabetically by key

#### `track_bottom(key, item, n)` / `track_bottom(key, item, n, value)`
Track bottom N least frequent items (count mode) or lowest-valued items (weighted mode). Skips Unit `()` values.

**Count mode** tracks the N items that appear least frequently:

```rhai
// Track bottom 5 rarest errors
track_bottom("rare_errors", e.error_type, 5)

// Track least active users
track_bottom("inactive_users", e.user_id, 10)
```

**Weighted mode** tracks the N items with the lowest custom values:

```rhai
// Track 10 fastest endpoints by latency
track_bottom("fastest_endpoints", e.endpoint, 10, e.latency_ms)

// Track smallest requests
track_bottom("tiny_requests", e.request_id, 5, e.bytes)
```

**Output format:**
- Count mode: `[{key: "item", count: 1}, ...]`
- Weighted mode: `[{key: "item", value: 0.5}, ...]`
- Results are sorted by value ascending, then alphabetically by key

!!! tip "Memory Efficiency"
    `track_top()` and `track_bottom()` use bounded memory (O(N) per key) unlike `track_bucket()` which stores all unique values. For high-cardinality fields, prefer top/bottom tracking over bucketing.

!!! note "Parallel Mode Behavior"
    In parallel mode, each worker maintains its own top/bottom N. During merge, the lists are combined, re-sorted, and trimmed to N. Final results are deterministic.

#### `track_percentiles(key, value [, [percentiles]])`
Track streaming percentiles using the t-digest algorithm for memory-efficient percentile estimation. Automatically creates suffixed metrics for each percentile (e.g., `latency_p50`, `latency_p95`, `latency_p99.9`). **This is the only `track_*()` function that auto-suffixes** because percentiles are inherently multi-valued. Skips Unit `()` values. Works correctly in parallel mode.

**Default percentiles:** `[0.50, 0.95, 0.99]` when no array provided.

**Percentile notation:** Use 0.0-1.0 range (quantile notation):
- `0.50` = 50th percentile (median) → creates `key_p50`
- `0.95` = 95th percentile → creates `key_p95`
- `0.999` = 99.9th percentile → creates `key_p99.9`

**Memory efficiency:** Uses ~4KB per metric regardless of event count (vs. storing all values). Suitable for millions of events.

**Accuracy:** ~1-2% relative error, suitable for operational monitoring.

```rhai
// Default percentiles [0.50, 0.95, 0.99]
track_percentiles("api_latency", e.response_time)
// Creates: api_latency_p50, api_latency_p95, api_latency_p99

// Custom percentiles
track_percentiles("latency", e.duration_ms, [0.50, 0.95, 0.99])
// Creates: latency_p50, latency_p95, latency_p99

// High-precision percentiles
track_percentiles("latency", e.duration_ms, [0.999, 0.9999])
// Creates: latency_p99.9, latency_p99.99

// Per-endpoint tracking
track_percentiles("latency_" + e.endpoint, e.response_time, [0.95, 0.99])

// Safe with conversions that may fail
let latency = e.latency_str.to_float()  // Returns () on error
track_percentiles("api_p95", latency)   // Skips () values
```

!!! tip "When to Use Percentiles vs. Average"
    Use `track_percentiles()` instead of `track_avg()` when:

    - You need tail latency metrics (p95, p99) for SLO monitoring
    - Data has outliers that would skew the average
    - You need multiple percentile values (median, p95, p99)
    - Working with latency, response time, or duration metrics

!!! note "Parallel Mode Behavior"
    In parallel mode, each worker maintains its own t-digest. During merge, digests are combined using the t-digest merge algorithm, preserving accuracy. Final percentile values are deterministic.

---

#### `track_stats(key, value [, [percentiles]])`
**Convenience function** that tracks comprehensive statistics in a single call: min, max, avg, count, sum, and percentiles. Automatically creates suffixed metrics for each statistic. Ideal for getting the complete statistical picture of a metric without calling multiple `track_*()` functions. Skips Unit `()` values. Works correctly in parallel mode.

**Auto-created metrics:**
- `{key}_min` - Minimum value
- `{key}_max` - Maximum value
- `{key}_avg` - Average (stored as sum+count for parallel merging)
- `{key}_count` - Total count
- `{key}_sum` - Total sum
- `{key}_p50`, `{key}_p95`, `{key}_p99` - Percentiles (default)

**Default percentiles:** `[0.50, 0.95, 0.99]` when no array provided.

**Percentile notation:** Same as `track_percentiles()` - use 0.0-1.0 range (quantile notation).

```rhai
// Default percentiles [0.50, 0.95, 0.99]
track_stats("response_time", e.duration_ms)
// Creates: response_time_min, response_time_max, response_time_avg,
//          response_time_count, response_time_sum,
//          response_time_p50, response_time_p95, response_time_p99

// Custom percentiles
track_stats("latency", e.duration, [0.50, 0.90, 0.99, 0.999])
// Creates all basic stats plus: latency_p50, latency_p90, latency_p99, latency_p99.9

// Per-endpoint comprehensive tracking
track_stats("api_" + e.endpoint, e.response_time)

// Safe with conversions that may fail
let duration = e.duration_str.to_float()  // Returns () on error
track_stats("request_ms", duration)        // Skips () values
```

!!! tip "When to Use track_stats() vs. Individual Functions"
    **Use `track_stats()`** when:

    - You want the complete statistical picture (min, max, avg, percentiles)
    - Analyzing latency, response time, or duration metrics
    - Building dashboards that need multiple statistical views
    - Prototyping or exploring data characteristics

    **Use individual `track_min/max/avg/percentiles`** when:

    - You only need specific statistics (performance optimization)
    - Fine-grained control over which metrics are tracked
    - Minimizing memory usage (percentiles use ~4KB per metric)

!!! note "Performance Considerations"
    `track_stats()` internally calls the same logic as individual tracking functions, so it has the same performance characteristics. The main overhead is from percentile tracking (~4KB memory per metric). If you don't need percentiles, use `track_min()`, `track_max()`, and `track_avg()` instead.

!!! note "Parallel Mode Behavior"
    All generated metrics use existing merge operations (min, max, avg, count, sum, percentiles), so `track_stats()` works correctly in parallel mode with no special handling required.

---

## File Output Functions

All file output functions require the `--allow-fs-writes` flag.

#### `append_file(path, text_or_array)`
Append line(s) to file; arrays append one line per element.

```rhai
append_file("errors.log", e.message)
append_file("batch.log", [e.line1, e.line2, e.line3])
```

#### `truncate_file(path)`
Create or zero-length a file for fresh output.

```rhai
truncate_file("output.log")
```

#### `mkdir(path [, recursive])`
Create directory (set recursive=true to create parents).

```rhai
mkdir("logs")
mkdir("deep/nested/path", true)
```

---

## Event Manipulation

#### `emit_each(array [, base_map])`
Fan out array elements as separate events (returns emitted count).

```rhai
emit_each(e.users)                                    // Each user becomes an event
emit_each(e.items, #{batch_id: e.batch_id})           // Add batch_id to each

// Use return value to track emission count
let count = emit_each(e.batch_items, #{batch_id: e.id})
track_sum("items_emitted", count)
```

#### `e = ()`
Clear entire event (remove all fields).

```rhai
if e.should_drop {
    e = ()  // Event is filtered out
}
```

#### `e.field = ()`
Remove individual field from event.

```rhai
e.password = ()                                       // Remove sensitive field
e.temp_data = ()                                      // Clean up temporary field
```

#### `e.absorb_kv(field [, options])`
Parse inline `key=value` tokens from a string field, merge the pairs into the event, and get a status report back. Returns a map with `status`, `data`, `written`, `remainder`, `removed_source`, and `error` so scripts can branch without guessing.

```rhai
let res = e.absorb_kv("msg", #{ sep: ",", kv_sep: "=", keep_source: true });
if res.status == "applied" {
    e.cleaned_msg = res.remainder ?? "";
    // Parsed keys now live on the event; res.data mirrors the inserted pairs
}
```

Options:

- `sep`: string or `()` (default whitespace) – token separator; `()` normalizes whitespace.
- `kv_sep`: string (default `"="`) – separator between key and value.
- `keep_source`: bool (default `false`) – leave the original field untouched; use `remainder` for cleaned text.
- `overwrite`: bool (default `true`) – allow parsed keys to overwrite existing event fields; set `false` to skip conflicts.

Unknown option keys set `status = "invalid_option"`; in `--strict` mode this aborts the pipeline.

#### `e.absorb_json(field [, options])`
Parse a JSON object from a string field, merge its keys into the event, and return the same status map as `absorb_kv()`. On success the source field is deleted unless `keep_source` is true, and `remainder` is always `()`.

```rhai
let res = e.absorb_json("payload");
if res.status == "applied" {
    e.actor = e.actor ?? e.user;      // merged from payload
} else if res.status == "parse_error" {
    warn(`bad payload: ${res.error}`);
}
```

Options:

- `keep_source`: bool (default `false`) – keep the original JSON string instead of deleting the field.
- `overwrite`: bool (default `true`) – allow parsed keys to replace existing event fields (`false` skips conflicts).

Other absorb options (like `sep`) are accepted for consistency but ignored. JSON parsing is all-or-nothing: invalid JSON or non-object payloads set `status = "parse_error"` and leave the event untouched.

#### `e.absorb_regex(field, pattern [, options])`
Extract named capture groups from a string field using a regular expression pattern, merge the extracted values into the event, and return a status map (same structure as `absorb_kv()` and `absorb_json()`).

The pattern must use **named capture groups** (`(?P<name>...)`) to define which parts of the text to extract. Only named captures become event fields; numbered groups are ignored.

```rhai
// Extract user and IP from log message
let res = e.absorb_regex("msg", r"User (?P<user>\w+) logged in from (?P<ip>[\d.]+)");
if res.status == "applied" {
    print(`${e.user} from ${e.ip}`);  // Extracted fields now on event
}

// Parse structured log line with multiple fields
let pattern = r"(?P<date>[\d-]+) (?P<level>\w+) (?P<file>[\w.]+):(?P<line>\d+) (?P<message>.+)";
e.absorb_regex("line", pattern);
// Now e.date, e.level, e.file, e.line, e.message are all populated
```

**Options:**

- `keep_source`: bool (default `false`) – preserve the original field instead of removing it after extraction
- `overwrite`: bool (default `true`) – allow extracted fields to overwrite existing event fields (`false` skips conflicts)

**Status values:**

- `"applied"` – pattern matched and fields were extracted
- `"empty"` – pattern didn't match (no captures)
- `"parse_error"` – invalid regex pattern
- `"missing_field"` – source field doesn't exist
- `"not_string"` – source field is not a string
- `"invalid_option"` – unknown option key (aborts in `--strict` mode)

**When to use:**

- **absorb_regex()** – Extract structured data from unstructured text with custom patterns
- **absorb_kv()** – Parse `key=value` pairs (simpler, faster)
- **absorb_json()** – Parse JSON objects (type-aware)
- **Regex input format** (`-f regex`) – Use for whole-log parsing at input time

```rhai
// Complex example: parse Apache access log format
let apache_pattern = r#"(?P<ip>\S+) \S+ \S+ \[(?P<timestamp>[^\]]+)\] "(?P<method>\S+) (?P<path>\S+)[^"]*" (?P<status>\d+) (?P<bytes>\d+)"#;
e.absorb_regex("line", apache_pattern);

// Keep source for debugging
e.absorb_regex("raw_message", r"ERROR: (?P<error_code>\d+) - (?P<error_msg>.+)",
               #{ keep_source: true });
```

## Span Context – `--span-close` Only

A read-only `span` object is injected into scope whenever a `--span-close` script runs. Use it to emit per-span rollups after Kelora closes a count- or time-based window.

### Span Identity

`span.id` returns the current span identifier. Count-based spans use `#<index>` (zero-based). Time-based spans use `ISO_START/DURATION` (e.g. `2024-05-19T12:00:00Z/5m`).

```rhai
let id = span.id;  // "#0" or "2024-05-19T12:05:00Z/5m"
```

### Span Boundaries

`span.start` and `span.end` expose the half-open window bounds as `DateTime` values. Count-based spans return `()` for both fields.

```rhai
if span.start != () {
    print(`Window: ${span.start} → ${span.end}`);
}
```

### Span Size and Events

`span.size` reports how many events survived filters and were buffered in the span. `span.events` returns those events in arrival order. Each map includes span metadata fields (`span_status`, `span_id`, `span_start`, `span_end`) alongside the original event data.

```rhai
let included = span.events
    .filter(|evt| evt.span_status == "included")
    .len();
```

### Metrics Snapshot

`span.metrics` contains per-span deltas from `track_*` calls. Values reset automatically after each span closes, so you can emit per-span summaries without manual bookkeeping.

```rhai
let metrics = span.metrics;
let hits = metrics["events"];          // from track_count("events")
let failures = metrics["failures"];    // from track_count("failures")
let ratio = if hits > 0 { failures * 100 / hits } else { 0 };
print(span.id + ": " + ratio.to_string() + "% failure rate");
```

---

## Quick Reference by Use Case

**Error Extraction:**
```rhai
e.error_code = e.message.extract_regex(r"ERR-(\d+)", 1)
```

**IP Anonymization:**
```rhai
e.masked_ip = e.client_ip.mask_ip()
e.ip_alias = pseudonym(e.client_ip, "ips")
```

**Time Filtering:**
```rhai
if e.timestamp > to_datetime("2024-01-01") {
    // Process recent events
}
```

**Metrics Tracking:**
```rhai
track_count(e.service)
track_sum("bytes", e.response_size)
track_unique("users", e.user_id)
```

**Array Fan-Out:**
```rhai
emit_each(e.users, #{batch_id: e.batch_id})
```

**Safe Field Access:**
```rhai
e.user_name = e.get_path("user.profile.name", "unknown")
if e.has_path("error.details.code") {
    e.detailed = true
}
```

---

## See Also

- [CLI Reference](cli-reference.md) - Command-line flags and options
- [Rhai Cheatsheet](rhai-cheatsheet.md) - Rhai language syntax
- [Advanced Scripting Tutorial](../tutorials/advanced-scripting.md) - Learn advanced scripting
- [How-To: Sanitize Logs Before Sharing](../how-to/extract-and-mask-sensitive-data.md) - Practical examples

For more details, run:
```bash
kelora --help-functions    # This reference in CLI form
kelora --help-rhai         # Rhai language guide
kelora --help-examples     # Common usage patterns
```
