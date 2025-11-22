# Events and Fields

Understanding how Kelora represents log data as structured events and how to access their fields.

## What is an Event?

An **event** is Kelora's internal representation of a single log entry. After parsing, each log line becomes an event (a map/object) with fields that you can access and transform.

```rhai
// In Rhai scripts, the current event is available as 'e'
e.timestamp  // Access field directly
e.level      // Access another field
e.message    // And so on
```

## Event Structure

Events are maps (key-value pairs) where:

- **Keys** are field names (strings)
- **Values** can be any JSON-compatible type

```json
{
  "timestamp": "2024-01-15T10:30:00Z",
  "level": "ERROR",
  "service": "api",
  "user": {
    "id": 12345,
    "name": "alice"
  },
  "tags": ["authentication", "security"],
  "duration_ms": 1234
}
```

After parsing, this becomes an event with fields accessible as:

- `e.timestamp` → `"2024-01-15T10:30:00Z"`
- `e.level` → `"ERROR"`
- `e.service` → `"api"`
- `e.user` → Map with `id` and `name` fields
- `e.tags` → Array `["authentication", "security"]`
- `e.duration_ms` → `1234`

## Field Types

Kelora preserves JSON types after parsing:

| Type | Example Value | Rhai Access |
|------|---------------|-------------|
| String | `"error"` | `e.level` |
| Integer | `404` | `e.status` |
| Float | `1.234` | `e.duration` |
| Boolean | `true` | `e.success` |
| Null | `null` | `e.optional_field` → `()` |
| Object/Map | `{"key": "value"}` | `e.metadata` |
| Array | `[1, 2, 3]` | `e.scores` |

**Unit Type `()`:**
In Rhai, `null` from JSON becomes the unit type `()`, representing "no value" or "empty". Unit values have special behaviors:

- Assigning `()` to a field removes it from the event
- Missing fields return `()` when accessed
- `track_*()` functions silently skip `()` values
- Use `.or_empty()` to convert empty values (strings, arrays, maps) to `()` for conditional field assignment

## Field Access Patterns

### Direct Access

For simple field names (alphanumeric, no special characters):

```rhai
e.timestamp
e.level
e.service
e.user_id
```

### Nested Access

Access nested fields using dot notation:

```rhai
e.user.name          // Object field
e.user.id            // Another nested field
e.metadata.region    // Deeper nesting
```

**Important:** Direct nested access requires the field to exist. If the field might be missing, check first or use `get_path()`:

```rhai
// Safe: Check before access
if "user" in e && "name" in e.user {
    e.user.name
}

// Safer: Use get_path with default
e.get_path("user.name", "unknown")      // Returns "unknown" if missing
e.get_path("metadata.region", "us-west")  // Fallback value
```

### Array Access

Access array elements by index:

```rhai
e.tags[0]            // First element
e.tags[1]            // Second element
e.tags[-1]           // Last element (negative indexing)
e.tags[-2]           // Second-to-last element
```

### Bracket Notation

For field names with special characters or dynamic access:

```rhai
e["content-type"]           // Hyphens in field name
e["@timestamp"]             // @ symbol in name
e["user-agent"]             // Multiple special chars
e.headers["authorization"]  // Nested with special chars
```

### Deep Nested Access

Combine patterns for complex structures:

```rhai
e.user.addresses[0].city                    // Object → array → object
e.data.items[-1].metadata.tags[0]          // Multiple levels
e.response.headers["content-type"]          // Object → bracket notation
```

## Safe Field Access

### Check Field Existence

Before accessing fields, check if they exist:

```rhai
// Top-level field
if "field" in e {
    e.field
} else {
    "default"
}

// Nested field
if e.has_path("user.role") {
    e.user.role
} else {
    "guest"
}

// Check against Unit (missing fields return ())
if e.optional_field != () {
    // Field exists and has a value
    e.optional_field
}
```

### Array Bounds Checking

Check array length before accessing elements:

```rhai
if e.scores.len() > 0 {
    e.scores[0]
} else {
    0
}

// Last element safely
if e.items.len() > 0 {
    e.items[-1]
} else {
    #{}
}
```

### Safe Path Access

Use `get_path()` for safe nested access with defaults:

```rhai
// Returns default if path doesn't exist
e.user_role = e.get_path("user.role", "guest")
e.first_tag = e.get_path("tags[0]", "untagged")
e.response_code = e.get_path("response.status", 0)

// Complex nested paths
e.city = e.get_path("user.address.city", "unknown")
```

### Type Checking

Check if field has a value (not unit type):

```rhai
if type_of(e.field) != "()" {
    // Field exists and has a value
    e.field
}
```

## Modifying Events

### Add Fields

Assign values to new or existing fields:

```rhai
e.processed = true
e.category = "error"
e.duration_s = e.duration_ms / 1000
```

### Modify Existing Fields

Transform field values in place:

```rhai
e.level = e.level.to_upper()
e.message = e.message.trim()
e.tags = sorted(e.tags)
```

### Remove Fields

Assign unit `()` to remove fields:

```rhai
e.password = ()          // Remove sensitive field
e.internal_id = ()       // Remove another field
```

Removed fields won't appear in output.

### Conditional Field Assignment

Use `.or_empty()` to conditionally assign fields based on empty values. It works with strings, arrays, and maps:

**String extraction:**
```rhai
// Only assign field if extraction succeeds
e.user = e.message.after("User:").or_empty()
// If "User:" not found → empty string → Unit → field removed

e.code = e.error.extract_re(r"ERR-(\d+)", 1).or_empty()
// If regex doesn't match → empty string → Unit → field removed
```

**Array filtering:**
```rhai
// Remove field if array is empty
e.tags = e.tags.or_empty()
// If tags is [] → Unit → field removed

// Only track events with items
track_unique("has_items", e.items.or_empty())
// Skips events where items is [] or ()
```

**Map/object filtering:**
```rhai
// Remove field if map is empty
e.metadata = e.parse_json().or_empty()
// If parsed JSON is {} → Unit → field removed

// Only assign config if it has values
e.config = e.settings.or_empty()
// If settings is #{} → Unit → field removed
```

**Combined with tracking:**
```rhai
e.extracted = e.text.after("prefix:").or_empty()
track_unique("values", e.extracted)  // Only tracks non-empty values
```

The `.or_empty()` method converts empty values (`""`, `[]`, `#{}`) to `()`, enabling clean conditional field creation without explicit `if` statements.

**Pattern comparison:**

```rhai
// Without .or_empty() - verbose
let extracted = e.message.after("User:")
if extracted != "" {
    e.user = extracted
}

// With .or_empty() - concise
e.user = e.message.after("User:").or_empty()
```

### Remove Entire Event

Clear all fields to filter out the event:

```rhai
if e.level == "DEBUG" {
    e = ()  // Event becomes empty and is filtered out
}
```

Empty events are counted as "filtered" in statistics.

## Field Name Patterns

### Common Field Names

Kelora recognizes these standard field names across formats:

**Timestamps:**

- `timestamp`, `ts`, `time`, `@timestamp`

**Log Levels:**

- `level`, `severity`, `loglevel`

**Messages:**

- `message`, `msg`, `text`

**Identifiers:**

- `id`, `request_id`, `trace_id`, `span_id`

**Metadata:**

- `service`, `host`, `hostname`, `source`

### Format-Specific Fields

Different parsers add format-specific fields:

**JSON (`-f json`):**

- Preserves all original fields
- Nested structures maintained

**Syslog (`-f syslog`):**

- `hostname`, `appname`, `procid`, `msgid`
- `facility`, `severity`

**Combined/Apache (`-f combined`):**

- `ip`, `timestamp`, `request`, `status`, `bytes`
- `method`, `path`, `protocol`
- `referer`, `user_agent`
- `request_time` (NGINX only)

**CSV (`-f csv`):**

- Column names from header row
- Or `col_0`, `col_1`, `col_2`, etc. without header

**Logfmt (`-f logfmt`):**

- All key-value pairs as top-level fields

## Working with Nested Structures

### Objects/Maps

Access nested maps using dot notation:

=== "Command"

    ```bash
    kelora -f json examples/simple_json.jsonl \
      --filter 'e.service == "api"' \
      --exec 'e.req_method = e.get_path("request.method", "UNKNOWN")' \
      --keys timestamp,service,req_method \
      --take 3
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f json examples/simple_json.jsonl \
      --filter 'e.service == "api"' \
      --exec 'e.req_method = e.get_path("request.method", "UNKNOWN")' \
      --keys timestamp,service,req_method \
      --take 3
    ```

### Arrays

Process arrays with Rhai array functions:

```bash
kelora -f json input.log \
    --exec 'e.tag_count = e.tags.len()' \
    --exec 'e.first_tag = e.get_path("tags[0]", "none")' \
    --exec 'e.unique_tags = unique(e.tags)' \
    --keys timestamp,tag_count,first_tag,unique_tags
```

### Fan-Out Arrays

Convert array elements to individual events:

```bash
kelora -f json batch.log \
    --exec 'emit_each(e.items)' \
    --keys item_id,status
```

Each element in `e.items` becomes a separate event.

### Mixed Structures

Handle complex nested structures:

```json
{
  "user": {
    "id": 12345,
    "scores": [85, 92, 78],
    "metadata": {
      "tags": ["premium", "verified"]
    }
  }
}
```

Access patterns:

```rhai
e.user_id = e.user.id                           // Nested object
e.first_score = e.user.scores[0]                // Object → array
e.last_score = e.user.scores[-1]                // Negative index
e.avg_score = e.user.scores.sum() / e.user.scores.len()
e.first_tag = e.user.metadata.tags[0]           // Deep nesting
e.is_premium = "premium" in e.user.metadata.tags // Array membership
```

## Field Naming Conventions

### Output Format: Bracket Notation

The default formatter uses bracket notation for arrays:

```
scores[0]=85 scores[1]=92 scores[2]=78
user.name=alice user.scores[0]=85
items[0].id=1 items[0].status=active
```

This matches the path syntax used in `get_path()`:

```rhai
// Access and path syntax are consistent
e.get_path("scores[0]", 0)           // Access first score
e.get_path("items[1].status", "")    // Access nested array element
```

### Field Selection: Top-Level Only

The `--keys` parameter operates on **top-level fields only**:

```bash
# ✅ Supported: Select top-level fields
kelora -f json input.log --keys user,timestamp,message

# ❌ Not supported: Nested paths in --keys
kelora -f json input.log --keys user.name,scores[0]
```

To extract nested fields, use `--exec` to promote them to top-level:

```bash
kelora -f json input.log \
    --exec 'e.user_name = e.get_path("user.name", "")' \
    --exec 'e.first_score = e.get_path("scores[0]", 0)' \
    --keys user_name,first_score
```

## Common Patterns

### Extract Nested Fields

```bash
kelora -f json app.log \
    --exec 'e.user_name = e.get_path("user.name", "unknown")' \
    --exec 'e.user_role = e.get_path("user.role", "guest")' \
    --keys timestamp,user_name,user_role
```

### Flatten Structures

```bash
kelora -f json app.log \
    --exec 'e.request_method = e.request.method' \
    --exec 'e.request_path = e.request.path' \
    --exec 'e.request = ()' \
    --keys timestamp,request_method,request_path
```

### Conditional Extraction and Tracking

```bash
# Extract and track only when pattern exists
kelora -f json app.log \
    --exec 'e.user = e.message.after("User:").or_empty()' \
    --exec 'e.code = e.message.extract_re(r"ERR-(\d+)", 1).or_empty()' \
    --exec 'track_unique("users", e.user)' \
    --exec 'track_unique("error_codes", e.code)' \
    --metrics

# Safe conversion with automatic Unit skipping
kelora -f json app.log \
    --exec 'let score = e.score_str.to_int()' \
    --exec 'track_sum("total_score", score)' \
    --exec 'track_min("min_score", score)' \
    --exec 'track_max("max_score", score)' \
    --metrics
```

### Combine Fields

```bash
kelora -f json app.log \
    --exec 'e.full_name = e.first_name + " " + e.last_name' \
    --exec 'e.endpoint = e.method + " " + e.path' \
    --keys timestamp,full_name,endpoint
```

### Conditional Field Creation

```bash
kelora -f json app.log \
    --exec 'if e.status >= 500 { e.severity = "critical" } else if e.status >= 400 { e.severity = "warning" } else { e.severity = "normal" }' \
    --keys timestamp,status,severity
```

### Array Transformations

```bash
kelora -f json app.log \
    --exec 'e.tag_count = e.tags.len()' \
    --exec 'e.sorted_tags = sorted(e.tags)' \
    --exec 'e.unique_tags = unique(e.tags)' \
    --keys timestamp,tag_count,sorted_tags
```

### Safe Deep Access

```bash
kelora -f json app.log \
    --exec 'e.city = e.get_path("user.address.city", "N/A")' \
    --exec 'e.zip = e.get_path("user.address.zip", "00000")' \
    --exec 'e.country = e.get_path("user.address.country", "Unknown")' \
    --keys city,zip,country
```

## Type Conversions

### String to Number

```rhai
e.status_code = e.status.to_int()           // String to integer
e.duration = e.duration_str.to_float()      // String to float
```

**Note:** Conversion functions return `()` on failure:

```rhai
"123".to_int()      // → 123
"abc".to_int()      // → ()  (conversion failed)
"".to_int()         // → ()  (empty string)
```

This works seamlessly with `track_*()` functions that skip `()` values:

```rhai
// Safe: track_sum() skips Unit values from failed conversions
let score = e.score_str.to_int()
track_sum("total", score)      // Only tracks valid integers
```

With defaults if conversion fails:

```rhai
e.status_code = to_int_or(e.status, 0)
e.duration = to_float_or(e.duration_str, 0.0)
```

### Number to String

```rhai
e.status_str = e.status.to_string()
e.duration_str = e.duration.to_string()
```

### Boolean Conversions

```rhai
e.is_error = to_bool(e.error_flag)          // "true"/"false" to boolean
e.success = to_bool_or(e.status_ok, false)  // With default
```

### Type Checking

```rhai
e.field_type = type_of(e.field)

// Common checks
if type_of(e.field) == "i64" { /* integer */ }
if type_of(e.field) == "f64" { /* float */ }
if type_of(e.field) == "string" { /* string */ }
if type_of(e.field) == "array" { /* array */ }
if type_of(e.field) == "map" { /* object/map */ }
if type_of(e.field) == "()" { /* null/empty */ }
```

## Field Access Performance

### Direct vs Path Access

**Direct access** is fastest for known fields:
```rhai
e.level           // Fast: direct map lookup
e.user.name       // Fast: two map lookups
```

**Use direct access when:**

- Field names are known at script time
- Fields are guaranteed to exist (e.g., parser output)
- Performance is critical

**Path access** provides safety and flexibility:
```rhai
e.get_path("level", "INFO")              // Safe with default
e.get_path("user.name", "unknown")       // Handles missing fields
```

**Use `get_path()` when:**

- Fields might not exist (optional data)
- Working with inconsistent log formats
- You need default values for missing fields
- Path is dynamic or comes from configuration

**Hybrid approach for best results:**
```rhai
// Check existence, then use direct access for speed
if "user" in e {
    e.user_name = e.user.name              // Fast once verified
} else {
    e.user_name = "unknown"
}

// Or use get_path for one-liners
e.user_name = e.get_path("user.name", "unknown")  // Simpler but slower
```

### Field Existence Checks

**Fastest** - direct membership:
```rhai
"field" in e
```

**Flexible** - path checking:
```rhai
e.has_path("user.role")
e.has_path("items[0].status")
```

### Array Operations

Array functions create new arrays (not in-place):

```rhai
// Creates new sorted array
e.sorted_scores = sorted(e.scores)

// Original array unchanged
e.scores  // Still unsorted
```

For large arrays, avoid unnecessary transformations.

## Troubleshooting

### Field Not Found

**Problem:** Accessing non-existent field causes error.

**Solution:** Use safe access patterns:
```rhai
// Check before access
if "field" in e {
    e.field
}

// Use get_path with default
e.get_path("field", "default")
```

### Array Index Out of Bounds

**Problem:** Accessing array element beyond length.

**Solution:** Check array length first:
```rhai
if e.items.len() > 5 {
    e.items[5]
}

// Or use safe path access
e.get_path("items[5]", #{})
```

### Type Mismatch

**Problem:** Field has unexpected type.

**Solution:** Use type conversion with defaults:
```rhai
e.status_code = to_int_or(e.status, 0)
e.duration = to_float_or(e.duration_ms, 0.0)
```

### Nested Field Access Errors

**Problem:** Accessing non-existent nested fields causes errors.

**Solution:** Check field existence before access or use `get_path()`:
```rhai
// Error if user or name doesn't exist
e.user.name = "alice"

// Safe: Check first
if "user" in e {
    e.user.name = "alice"
}

// Safest: Use get_path for reading
let current = e.get_path("user.name", "default")
```

**Note:** Direct nested assignment (`e.user.name = "alice"`) works fine when the parent object exists.

### Field Name with Special Characters

**Problem:** Field names contain hyphens, dots, or other special chars.

**Solution:** Use bracket notation:
```rhai
e["content-type"]
e["user-agent"]
e["@timestamp"]
```

## See Also

- [Pipeline Model](pipeline-model.md) - How events flow through processing stages
- [Scripting Stages](scripting-stages.md) - Using --begin/--exec/--end with events
- [Function Reference](../reference/functions.md) - All field manipulation functions
- [Rhai Cheatsheet](../reference/rhai-cheatsheet.md) - Quick reference for Rhai syntax
