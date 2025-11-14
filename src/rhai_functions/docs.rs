/// Generate comprehensive function reference documentation
pub fn generate_help_text() -> &'static str {
    r#"
Available Rhai Functions for Kelora:

STRING FUNCTIONS:
text.after(delimiter [,nth])         Text after occurrence of delimiter (nth: 1=first, -1=last)
text.before(delimiter [,nth])        Text before occurrence of delimiter (nth: 1=first, -1=last)
text.between(start, end)             Text between start and end delimiters
text.bucket()                        Fast hash for sampling/grouping (returns INT for modulo operations)
text.clip()                          Remove leading/trailing non-alphanumeric characters
text.col(spec [,separator])          Extract columns by index/range/list (e.g., '1', '1,3,5', '1:4')
text.contains(pattern)               Check if text contains pattern (builtin)
text.like(pattern)                   Glob match (*, ?) against entire string
text.ilike(pattern)                  Glob match with Unicode case folding (*, ?)
text.count(pattern)                  Count occurrences of pattern in text
text.decode_b64()                    Decode base64 string to text
text.decode_hex()                    Decode hexadecimal string to text
text.decode_url()                    Decode URL-encoded string
text.edit_distance(other)            Compute Levenshtein edit distance between two strings
text.encode_b64()                    Encode text to base64 string
text.encode_hex()                    Encode text to hexadecimal string
text.encode_url()                    URL-encode text (percent encoding)
text.ending_with(suffix [,nth])      Return substring from start to end of suffix (nth: 1=first, -1=last)
text.escape_html()                   Escape HTML special characters (&, <, >, ", ')
text.escape_json()                   Escape JSON special characters
text.extract_all_re(pattern [,group]) Extract all regex matches as array
text.extract_domain()                Extract domain from URL or email address
text.extract_ip([nth])               Extract IP address from text (nth: 1=first, -1=last)
text.extract_ips()                   Extract all IP addresses as array
text.extract_json([nth])             Extract JSON object/array from text (nth: 0=first, 1=second, etc.)
text.extract_jsons()                 Extract all JSON objects/arrays from text as array of strings
text.extract_re_maps(pattern, field) Extract regex matches as array of maps for fan-out
text.extract_re(pattern [,group])    Extract regex match or capture group
text.extract_url([nth])              Extract URL from text (nth: 1=first, -1=last)
text.has_matches(pattern)            Regex search (legacy helper; invalid pattern returns false)
text.matches(pattern)                Regex search (cached; invalid pattern raises error)
text.hash([algo])                    Hash with algorithm (default: sha256, also: sha1, md5, xxh3, blake3)
text.index_of(pattern)               Find position of substring (-1 if not found) (builtin)
text.is_digit()                      Check if text contains only digits
text.is_in_cidr(cidr)                Check if IP address is in CIDR network (e.g., "10.0.0.0/8")
text.is_ipv4()                       Check if text is a valid IPv4 address
text.is_ipv6()                       Check if text is a valid IPv6 address
text.is_private_ip()                 Check if IP is in private ranges
text.lclip()                         Remove leading non-alphanumeric characters (left side only)
text.len                             Get string length (builtin)
text.lower()                         Convert text to lowercase
text.lstrip([chars])                 Remove leading whitespace or specified characters
text.mask_ip([octets])               Mask IP address (default: last octet)
text.normalized([patterns])          Replace variable patterns with placeholders (e.g., <ipv4>, <email>)
text.parse_cef()                     Parse Common Event Format line into fields
text.parse_cols(spec [,sep])         Parse columns according to spec
text.parse_combined()                Parse Apache/Nginx combined log line
text.parse_content_disposition()     Parse Content-Disposition header parameters
text.parse_email()                   Parse email address into parts
text.parse_json()                    Parse JSON string into map/array
text.parse_jwt()                     Parse JWT header/payload without verification
text.parse_kv([sep [,kv_sep]])       Parse key-value pairs from text (skips tokens without separator)
text.parse_logfmt()                  Parse logfmt line into structured fields
text.parse_media_type()              Parse media type tokens and parameters
text.parse_path()                    Parse filesystem path into components
text.parse_query_params()            Parse URL query string into map
text.parse_syslog()                  Parse syslog line into structured fields
text.parse_url()                     Parse URL into structured components
text.parse_user_agent()              Parse common user-agent strings into components
text.rclip()                         Remove trailing non-alphanumeric characters (right side only)
text.replace(pattern, replacement)   Replace all occurrences of pattern (builtin)
text.rstrip([chars])                 Remove trailing whitespace or specified characters
text.slice(spec)                     Slice text using Python notation (e.g., "1:5", ":3", "-2:")
text.split_re(pattern)               Split text by regex pattern
text.split(separator)                Split string into array by delimiter (builtin)
text.starting_with(prefix [,nth])    Return substring from prefix to end (nth: 1=first, -1=last)
text.strip([chars])                  Remove whitespace or specified characters
text.sub_string(start [,length])     Extract substring from position (builtin)
text.to_float()                      Convert text to float (returns () on error)
text.to_int()                        Convert text to integer (returns () on error)
text.or_empty()                      Convert empty string/array/map to () for removal/filtering
text.to_lower()                      Convert to lowercase (builtin)
text.to_upper()                      Convert to uppercase (builtin)
text.trim()                          Remove whitespace from start and end (builtin)
text.unescape_html()                 Unescape HTML entities to text
text.unescape_json()                 Unescape JSON escape sequences

ARRAY FUNCTIONS:
array.all(|item| condition)          Check if all elements match condition (builtin)
array.contains_any(search_array)     Check if array contains any search values
array.contains(value)                Check if array contains value (builtin)
array.filter(|item| condition)       Keep elements matching condition (builtin)
array.flattened([style [,max_depth]]) Return new flattened map from nested arrays/objects
array.join(separator)                Join array elements with separator
array.len                            Get array length (builtin)
array.map(|item| expression)         Transform each element (builtin)
array.pluck(field)                   Extract field from array of maps (skips missing/() values)
array.pluck_as_nums(field)           Extract field as f64 from array of maps (skips invalid)
array.max()                          Find maximum value in array (no auto string-to-number coercion)
array.min()                          Find minimum value in array (no auto string-to-number coercion)
array.parse_cols(spec [,sep])        Apply column spec to pre-split values
array.percentile(pct)                Calculate percentile of numeric array
array.pop()                          Remove and return last item (builtin)
array.push(item)                     Add item to end of array (builtin)
array.reduce(|acc, item| expr, init) Aggregate array into single value (builtin)
array.reversed()                     Return new array in reverse order
array.slice(spec)                    Slice array using Python notation (e.g., "1:5", ":3", "-2:")
array.some(|item| condition)         Check if any element matches condition (builtin)
array.sort()                         Sort array in place (builtin)
array.sorted_by(field)               Sort array of objects by field name
array.sorted()                       Return new sorted array (numeric/lexicographic)
array.starts_with_any(search_array)  Check if array starts with any search values
array.unique()                       Remove all duplicate elements (preserves first occurrence)
  
MAP/OBJECT FUNCTIONS:
map.contains("key")                  Check if map contains key (ignores value) (builtin)
map.flattened([style [,max_depth]])  Return new flattened map from nested object
map.get_path("field.path" [,default]) Safe nested field access with fallback
map.has("key")                 Check if map contains key with non-unit value
map.has_path("field.path")           Check if nested field path exists
map.merge(other_map)                 Merge another map into this one
map.normalized([patterns])           Return new map with all string fields normalized
map.path_equals("path", value)       Safe nested field comparison
map.rename_field("old", "new")       Rename a field, returns true if successful
map.to_cef()                         Convert map to Common Event Format (CEF) string
map.to_combined()                    Convert map to Apache/Nginx combined log format
map.to_json([pretty])                Convert map to JSON string
map.to_kv([sep [,kv_sep]])           Convert map to key-value string with separators
map.to_logfmt()                      Convert map to logfmt format string
map.to_syslog()                      Convert map to syslog format string
map.unflatten([separator])           Reconstruct nested object from flat keys
  
DATETIME FUNCTIONS:
now()                                Current UTC timestamp (DateTimeWrapper)
to_datetime(text [,fmt [,tz]])       Convert string into DateTimeWrapper with optional hints
to_duration("1h30m")                 Convert duration string into DurationWrapper
duration_from_<unit>(n)              Create duration from seconds/minutes/hours/days/ms/ns
humanize_duration(ms)                Convert milliseconds to human-readable format (e.g., "1h 30m")
dt.to_iso()                          Convert datetime to ISO 8601 string
dt.format("format_string")           Format datetime using custom format string (see --help-time)
dt.year(), dt.month(), dt.day()      Extract date components
dt.hour(), dt.minute(), dt.second()  Extract time components
dt.to_utc(), dt.to_local()           Convert timezone
dt.to_timezone("tz_name")            Convert to named timezone
dt.timezone_name()                   Get timezone name as string
dt.ts_nanos()                        Get timestamp as nanoseconds
dt + duration, dt - duration         Add/subtract duration from datetime
dt1 - dt2                            Get duration between datetimes (returns DurationWrapper)
dt1 == dt2, dt1 != dt2               Compare datetimes for equality
dt1 > dt2, dt1 < dt2                 Compare datetimes (greater/less than)
dt1 >= dt2, dt1 <= dt2               Compare datetimes (greater/less or equal)
duration.as_seconds()                Convert duration to seconds
duration.as_milliseconds()           Convert duration to milliseconds
duration.as_nanoseconds()            Convert duration to nanoseconds
duration.as_minutes()                Convert duration to minutes
duration.as_hours()                  Convert duration to hours
duration.as_days()                   Convert duration to days
duration.to_string()                 Format duration as human-readable string (e.g., "1h 30m")
duration1 + duration2                Add durations
duration1 - duration2                Subtract durations (always returns positive result)
duration1 == duration2               Compare durations for equality
duration1 > duration2, duration1 < duration2  Compare durations (greater/less than)
duration1 >= duration2, duration1 <= duration2  Compare durations (greater/less or equal)

MATH FUNCTIONS:
abs(x)                               Absolute value of number
clamp(value, min, max)               Constrain value to be within min/max range
floor(x)                             Round down to nearest integer
mod(a, b) / a % b                    Modulo operation with division-by-zero protection
rand()                               Random float between 0 and 1
rand_int(min, max)                   Random integer between min and max (inclusive)
round(x)                             Round to nearest integer

TYPE CONVERSION FUNCTIONS:
to_int(value)                        Convert value to integer (returns () on error)
to_float(value)                      Convert value to float (returns () on error)
to_bool(value)                       Convert value to boolean (returns () on error)
to_int_or(value, default)            Convert value to integer with fallback
to_float_or(value, default)          Convert value to float with fallback
to_bool_or(value, default)           Convert value to boolean with fallback

UTILITY FUNCTIONS:
eprint(message)                      Print to stderr (suppressed with -qqq)
exit(code)                           Exit kelora with given exit code
get_env(var [,default])              Get environment variable with optional default
print(message)                       Print to stdout (suppressed with -qqq)
pseudonym(value, domain)             Generate domain-separated pseudonym (requires KELORA_SECRET)
read_file(path)                      Read file contents as string
read_lines(path)                     Read file as array of lines
type_of(value)                       Get type name as string (builtin)
window.pluck(field)                  Extract field values from current window/window array
window.pluck_as_nums(field)          Extract numeric field values from current window/window array

TRACKING/METRICS FUNCTIONS (requires --metrics):
track_bucket(key, bucket)            Track values in buckets for histograms (skips () values)
track_count(key)                     Increment counter for key by 1
track_max(key, value)                Track maximum value for key (skips () values)
track_min(key, value)                Track minimum value for key (skips () values)
track_sum(key, value)                Accumulate numeric values for key (skips () values)
track_unique(key, value)             Track unique values for key (skips () values)

FILE OUTPUT (requires --allow-fs-writes):
append_file(path, text_or_array)     Append line(s) to file; arrays append one line per element
mkdir(path [,recursive])             Create directory (set recursive=true to create parents)
truncate_file(path)                  Create or zero-length a file for fresh output

SPAN CONTEXT (available inside --span-close):
span.id                              Span identifier ('#index' for count, 'ISO/DURATION' for time)
span.start                           Span start as DateTime (time spans) or () for count spans
span.end                             Span end as DateTime (time spans) or () for count spans
span.size                            Number of events that survived the span
span.events                          Array of event maps for the span in arrival order
span.metrics                         Per-span metric deltas from track_* calls (read-only map)

EVENT MANIPULATION:
emit_each(array [,base_map])         Fan out array elements as separate events (returns emitted count)
e.absorb_kv(field [,options])        Parse key=value tokens from field, merge pairs, return status map
e.absorb_json(field [,options])      Parse JSON object from field, merge keys, return status map
e = ()                               Clear entire event (remove all fields)
e.field = ()                         Remove individual field from event
e.has("key")                         Check if key exists and value is not ()
e.rename_field("old", "new")         Rename field, returns true if successful

Rhai lets you call functions as either `value.method(args)` or `function(value, args)`.
"#
}

/// Generate practical examples for common log analysis patterns
pub fn generate_examples_text() -> &'static str {
    r###"
Common Log Analysis Patterns:

MICRO SEARCH HELPERS:
# Case-insensitive wildcard search
kelora -f json --filter 'e.message.ilike("*timeout*")'

# Regex search with cached compilation
kelora -f json --filter 'e.message.matches(#"[Uu]ser\s+not\s+found"#)'

# Field existence that ignores () sentinel
kelora -f logfmt --filter 'e.has("user") && e.user.like("alice*")'

WEB LOG ANALYSIS:
# Extract HTTP details from combined log
kelora -f combined --exec 'e.large_response = e.bytes > 50000' --filter 'e.large_response'

# Parse request path parameters
kelora -f combined --exec '
  let params = e.path.after("?").parse_query_params();
  e.utm_source = params.utm_source ?? "";
  e.user_id = (params.user_id ?? "").to_int()
'

# Mask IPs and extract domains from referers
kelora -f json --exec 'e.ip = e.client_ip.mask_ip(2)' \
  --exec 'e.referer_domain = (e.referer ?? "").extract_domain()'

# Detect suspicious user agents
kelora -f combined --filter 'e.user_agent.has_matches("(?i)(bot|crawler|scanner)")'

ERROR TRACKING:
# Extract stack traces and error types
kelora -f json --filter 'e.level == "ERROR"' --exec '
  e.error_type = e.error.before(":");
  e.line_number = e.stack_trace.extract_re(#":(\d+)"#, 1).to_int()
'

# Group errors by hash for deduplication
kelora -f json -l error --exec 'e.error_hash = e.message.hash("xxh3")' \
  --metrics --exec 'track_unique("errors", e.error_hash)'

# Pattern discovery with reusable alias
kelora --save-alias patterns \
  --exec 'track_unique("patterns", e.message.normalized())' \
  --metrics -qq
kelora -a patterns app.log

# Normalize events for pattern identification
kelora -f json --exec 'e = e.normalized(["ipv4", "email", "uuid"])'

# Time-based error clustering (5min windows)
kelora -f json -l error --window 100 --exec '
  let recent = window.pluck("timestamp").map(|ts| to_datetime(ts));
  let time_span = (recent[-1] - recent[0]).as_minutes();
  e.error_burst = time_span < 5 && recent.len() > 10
' --filter 'e.error_burst'

SECURITY & AUDIT:
# Check for private IPs in external traffic
kelora -f json --exec 'e.is_public = e.source_ip.is_private_ip() == false' --filter 'e.is_public'

# JWT token analysis without verification
kelora -f json --exec 'let jwt = e.token.parse_jwt(); e.jwt_user = jwt.claims.sub; e.jwt_role = jwt.claims.role'

# Detect CIDR-based access patterns
kelora -f json --exec 'e.internal = e.client_ip.is_in_cidr("10.0.0.0/8")' --filter 'e.internal == false'

# Hash sensitive fields with domain separation
export KELORA_SECRET="your-secret-key"
kelora -f json --exec 'e.user_hash = pseudonym(e.email, "users")' --exec 'e.email = ()'

DATA TRANSFORMATION:
# Parse nested JSON strings
kelora -f json --exec 'e.metadata = e.json_payload.parse_json()' \
  --exec 'e.user_tier = e.get_path("metadata.subscription.tier", "free")'

# Column extraction from structured text
kelora -f line --exec 'e.cols = e.line.col("1,3,5", " ")' \
  --exec 'e.timestamp = e.cols[0]; e.level = e.cols[1]; e.msg = e.cols[2]'

# Parse key-value logs (multiple formats)
kelora -f line --exec 'e = e.line.parse_logfmt()'  # logfmt: key=value
kelora -f line --exec 'e = e.line.parse_kv(" ", "=")'  # custom separators, extracts only key=value pairs

# Absorb inline key=value tails without losing the original message
kelora -f line --exec '
  let res = e.absorb_kv("msg", #{ keep_source: true });
  if res.status == "applied" {
    e.cleaned_msg = res.remainder ?? ""
  }
'

# Email header parsing
kelora -f line --exec 'let email = e.from.parse_email(); e.domain = email.domain; e.user = email.local'

METRICS & AGGREGATION:
# Response time percentiles (requires --window)
kelora -f json --window 1000 --metrics --end '
  let times = window.pluck_as_nums("response_time");
  print("p50: " + times.percentile(50));
  print("p95: " + times.percentile(95));
  print("p99: " + times.percentile(99))
'

# Status code distribution
kelora -f combined --metrics --exec 'track_bucket("status", e.status / 100 * 100)' \
  --end 'print(metrics.status)'

# Unique users per endpoint
kelora -f json --metrics --exec 'track_unique(e.path, e.user_id)' \
  --end 'for path in metrics.keys() { let users = metrics[path]; print(path + ": " + users.len() + " users") }'

ARRAY & FAN-OUT PROCESSING:
# Process nested arrays - fan out items
kelora -f json --exec 'emit_each(e.items)' --filter 'e.status == "active"'

# Multi-level fan-out with context preservation
kelora -f json --exec 'emit_each(e.batches)' \
  --exec 'let ctx = #{batch: e.id}; emit_each(e.items, ctx)' \
  --filter 'e.priority == "high"'

# Array transformations and sorting
kelora -f json --exec 'let s = sorted(e.scores); let len = s.len(); e.top_3 = [s[len-3], s[len-2], s[len-1]]'

# Remove duplicate elements
kelora -f json --exec 'e.unique_tags = unique(e.tags)'

DATETIME & FILTERING:
# Recent events (last 2 hours)
kelora -f json --since -2h --until now

# Business hours filter (9-5 local time)
kelora -f json --exec 'let dt = to_datetime(e.timestamp).to_local(); e.hour = dt.hour()' \
  --filter 'e.hour >= 9 && e.hour < 17'

# Calculate durations and SLA violations
kelora -f json --exec '
  let start = to_datetime(e.start_time);
  let end = to_datetime(e.end_time);
  let duration = end - start;
  e.duration_ms = duration.as_milliseconds();
  e.sla_breach = duration.as_seconds() > 5
' --filter 'e.sla_breach'

ADVANCED PATTERNS:
# Sampling - process 10% of events
kelora -f json --filter 'e.request_id.bucket() % 10 == 0'

# Conditional field removal based on sensitivity
kelora -f json --exec 'if e.level != "DEBUG" { e.stack_trace = (); e.locals = () }'

# Dynamic field creation from arrays
kelora -f json --exec 'for (idx, tag) in e.tags { e["tag_" + idx] = tag }'

# Format conversion pipeline
kelora -f json --exec 'e.syslog_compat = e.to_syslog()' -F json > output.jsonl

# CI/CD integration with exit codes
kelora -f json -qq -l error logs/*.json && echo "✓ No errors" || echo "✗ Errors found"

# Export results to file from script
kelora -f json --allow-fs-writes --exec '
  if e.severity == "critical" {
    append_file("alerts.log", e.to_json())
  }
'

MULTI-FILE & METADATA:
# Track errors by source file (meta.filename)
kelora -f json logs/*.log --metrics --exec '
  if e.level == "ERROR" {
    track_count(meta.filename)
  }
'

# Add source context to each event
kelora -f json server1.log server2.log --exec 'e.source = meta.filename'

# Debugging with line numbers (meta.line_num)
kelora -f json --filter 'e.status >= 500' --exec '
  eprint("⚠️  Server error at " + meta.filename + ":" + meta.line_num)
'

# Conditional processing based on filename
kelora -f json prod-*.log staging-*.log --exec '
  e.environment = if meta.filename.contains("prod") { "production" } else { "staging" }
' --filter 'e.environment == "production" && e.level == "ERROR"'

# Access raw line for re-parsing (meta.line)
kelora -f json --exec '
  if e.message.contains("CUSTOM:") {
    let custom = meta.line.after("CUSTOM:").parse_json();
    e.custom_data = custom
  }
'

# Track unique files with errors
kelora -f json logs/**/*.log --metrics --exec '
  if e.level == "ERROR" {
    track_unique("error_files", meta.filename)
  }
' --end 'print("Files with errors: " + metrics.error_files.len())'

# Create audit trail with source location
kelora -f json --allow-fs-writes --exec '
  if e.action == "admin_action" {
    let audit = "File: " + meta.filename + " Line: " + meta.line_num + " Event: " + e.to_json();
    append_file("audit.log", audit)
  }
'

COMMON IDIOMS:
# Method chaining              → e.domain = e.url.extract_domain().to_lower().strip()
# Top-level default value      → e.referer ?? "unknown"
# Nested field with default    → e.get_path("user.role", "guest")
# Safe type conversion         → to_int_or(e.port, 8080)
# Check top-level field        → e.has("user")
# Check nested field exists    → e.has_path("user.id")
# Only process if field exists → --filter 'e.has("referer")' --exec '...'
# Remove sensitive fields      → e.password = (); e.ssn = ()
# Hash for grouping/sampling   → e.session_id.bucket() % 100
# Parse then extract           → e.url.parse_url().path
# Regex capture groups         → e.log.extract_re(r"duration: (\d+)", 1)
# Array bounds safety          → if e.items.len() > 0 { e.first = e.items[0] }
# Negative indexing            → e.last_score = e.scores[-1]
# Clamp values to range        → e.normalized = clamp(e.value, 0, 100)
# Remove duplicate elements    → unique([1,2,2,3,2,1]) = [1,2,3]
# Pattern normalization        → e.message.normalized("ipv4,email,uuid")

See --help-functions for complete function reference. Visit https://rhai.rs for Rhai language details.
"###
}
