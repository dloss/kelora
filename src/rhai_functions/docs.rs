/// Generate comprehensive function reference documentation
pub fn generate_help_text() -> &'static str {
    r#"
Available Rhai Functions:

STRING FUNCTIONS:
text.after(delimiter [,nth])         Text after occurrence of delimiter (nth: 1=first, -1=last)
text.before(delimiter [,nth])        Text before occurrence of delimiter (nth: 1=first, -1=last)
text.between(start, end [,nth])      Text between start and end delimiters (nth: 1=first, -1=last)
                                     Equivalent to: text.after(start, nth).before(end)
text.bucket()                        Fast hash for sampling/grouping (returns INT for modulo operations)
text.clip()                          Remove leading/trailing non-alphanumeric characters
text.col(spec [,separator])          Extract columns by index/range/list (e.g., '1', '1,3,5', '1:4')
text.cols(col1, col2 [,...] [,sep])  Extract multiple columns as array (up to 6 columns)
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
text.extract_regexes(pattern [,group]) Extract all regex matches as array
text.extract_domain()                Extract domain from URL or email address
text.extract_email([nth])            Extract email address from text (nth: 1=first, -1=last)
text.extract_emails()                Extract all email addresses as array
text.extract_ip([nth])               Extract IP address from text (nth: 1=first, -1=last)
text.extract_ips()                   Extract all IP addresses as array
text.extract_json([nth])             Extract JSON object/array from text (nth: 1=first, -1=last)
text.extract_jsons()                 Extract all JSON objects/arrays from text as array of strings
text.extract_regex_maps(pattern, field) Extract regex matches as array of maps for fan-out
text.extract_re_maps(pattern, field)    Deprecated alias for extract_regex_maps
text.extract_regex(pattern [,group])    Extract regex match or capture group
text.extract_url([nth])              Extract URL from text (nth: 1=first, -1=last)
text.matches(pattern)                Regex search (cached; invalid pattern raises error)
text.hash([algo])                    Hash with algorithm (default: sha256, also: xxh3)
text.index_of(substring [,start])    Find position of literal substring (-1 if not found) (builtin)
text.is_digit()                      Check if text contains only digits
text.is_in_cidr(cidr)                Check if IP address is in CIDR network (e.g., "10.0.0.0/8")
text.is_ipv4()                       Check if text is a valid IPv4 address
text.is_ipv6()                       Check if text is a valid IPv6 address
text.is_private_ip()                 Check if IP is in private/internal ranges
text.lclip()                         Remove leading non-alphanumeric characters (left side only)
text.len                             Get string length (builtin)
text.lower()                         Convert text to lowercase
text.lstrip([chars])                 Remove leading whitespace or specified characters
text.mask_ip([octets])               Mask IP address by zeroing the suffix (IPv4/IPv6)
text.normalized([patterns])          Replace patterns with placeholders (<ipv4>, <email>, <credit_card>, etc.)
                                     Patterns: ipv4, ipv4_port, ipv6, email, url, fqdn, uuid, mac, md5,
                                     sha1, sha256, path, oauth, function, hexcolor, version, hexnum,
                                     duration, num, credit_card (Luhn), ssn (strict XXX-XX-XXXX), phone (NANP-aware for US/CA; permissive internationally)
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
text.replace_regex(pattern, replacement) Replace all regex matches
text.replace_re(pattern, replacement)    Deprecated alias for replace_regex
text.rstrip([chars])                 Remove trailing whitespace or specified characters
text.slice(spec)                     Slice text using Python notation (e.g., "1:5", ":3", "-2:")
text.split_regex(pattern)            Split text by regex pattern
text.split_re(pattern)               Deprecated alias for split_regex
text.split(separator)                Split string into array by delimiter (builtin)
text.starting_with(prefix [,nth])    Return substring from prefix to end (nth: 1=first, -1=last)
text.strip([chars])                  Remove whitespace or specified characters
text.sub_string(start [,length])     Extract substring from position (builtin)
text.to_float()                      Convert text to float (returns () on error)
text.to_float(thousands, decimal)    Parse with explicit separators
                                     - thousands: remove ANY char in string (e.g., ',', ',. ', ",.'")
                                     - decimal: single char or empty (multi-char returns error)
text.to_int()                        Convert text to integer (returns () on error)
text.to_int(thousands)               Parse with thousands separator removal
                                     - thousands: remove ANY char in string (e.g., ',', '. ', ",.'")
text.or_empty()                      Convert empty string/array/map to () for removal/filtering
text.to_lower()                      Convert to lowercase (builtin)
text.to_upper()                      Convert to uppercase (builtin; also available as upper())
text.lower()                         Convert to lowercase (alias for to_lower(); for Python users)
text.upper()                         Convert to uppercase (alias for to_upper(); for Python users)
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
array.pluck(field)                   Extract field from each map/object in array (skips missing/() values)
array.pluck_as_nums(field)           Extract field as f64 from each map in array (skips invalid/missing)
array.max()                          Find maximum value in array (rejects mixed types; no auto string-to-number coercion)
array.mean()                         Calculate arithmetic mean of numeric array (rejects mixed types)
array.min()                          Find minimum value in array (rejects mixed types; no auto string-to-number coercion)
array.parse_cols(spec [,sep])        Apply column spec to pre-split values
array.percentile(pct)                Calculate percentile of numeric array
array.stddev()                       Calculate standard deviation of numeric array (rejects mixed types)
array.sum()                          Calculate sum of numeric values in array (rejects mixed types)
array.variance()                     Calculate variance of numeric array (rejects mixed types)
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
map.enrich(other_map)                Merge another map, inserting only missing keys
map.flattened([style [,max_depth]])  Return new flattened map from nested object
map.flatten_field("field_name")      Flatten just one field from the map
map.get("key" [,default])            Safe top-level field access with fallback
map.get_path("field.path" [,default]) Safe nested field access with fallback
map.has("key")                       Check if map contains key with non-unit value
map.has_path("field.path")           Check if nested field path exists
map.keep(["field1", ...])            Return new map with only selected top-level fields
map.merge(other_map)                 Merge another map into this one (overwrites existing keys)
map.normalized([patterns])           Return new map with all string fields normalized
map.path_equals("path", value)       Safe nested field comparison
map.drop(["field1", ...])            Return new map without selected top-level fields
map.rename_field("old", "new")       Rename a field, returns true if successful
map.to_cef()                         Convert map to Common Event Format (CEF) string
map.to_combined()                    Convert map to Apache/Nginx combined log format
map.to_json([indent])                Convert map to JSON string (indent: spaces for pretty-printing, 0/omit for compact)
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
dt.round_to("interval")              Round timestamp down to interval (e.g., "5m", "1h", "1d")
dt.ceil_to("interval")               Round timestamp up to next interval boundary
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
duration.to_debug()                  Format duration with full precision for debugging
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
sample_every(n)                      Sample every Nth event (returns true on Nth, 2Nth, 3Nth calls)
                                     Fast counter-based sampling (thread-local, approximate in parallel mode)
                                     For deterministic sampling, use: text.bucket() % n == 0
sample_prob(p)                       Probabilistic sampling: returns true with probability p (0.0-1.0)

OUTPUT FORMATTING FUNCTIONS:
bar(value, max, width)               Render a horizontal bar of `width` cells showing value/max,
                                     using Unicode eighth-blocks (▏▎▍▌▋▊▉█) for sub-cell resolution.
                                     Pads with spaces so the result has exactly `width` display columns.
                                     Values outside 0..max are clamped; max<=0 renders empty.
                                     For ratios in 0.0–1.0 (error rate, CPU fraction, etc.), set max to 1.0.
format_decimals(value, decimals)     Format number with exactly N digits after the decimal point (returns string)
format_percent(ratio, decimals)      Format ratio as percentage string with N decimals and '%' suffix (returns string)
                                     Input is multiplied by 100, so pass 0.042 to render "4.2%".
human_bytes(n)                       Format byte count with binary/IEC units (1024-based): B, KiB, MiB, GiB, ...
human_bytes_si(n)                    Format byte count with decimal/SI units (1000-based): B, KB, MB, GB, ...
sparkline(array)                     Render an array of numbers as a single-line sparkline (▁▂▃▄▅▆▇█)
                                     scaled to 0..max(array). Negatives and non-numerics render as space.
                                     Empty arrays return "".
text.bold() / .dim()                 Chainable: "X".bold().red() renders as bold red.
text.blue() / .cyan() / .magenta()   Wrap text with ANSI color/style; resets at end. Returns text unchanged
                                     when colors are disabled (non-TTY output, NO_COLOR, --no-color).
text.ljust(n [,fill])                Left-justify: pad right to display width n (default fill: space). Unicode-width aware.
text.rjust(n [,fill])                Right-justify: pad left to display width n (default fill: space).
text.center(n [,fill])               Center within display width n (extra goes right on odd difference).
text.shorten(n [,marker])            If text exceeds width n, keep start and append marker (default "…").
text.shorten_middle(n [,marker])     If text exceeds width n, keep both ends, insert marker (default "…") in the middle.
                                     Useful for paths/URLs/IDs where both ends are informative.
text.red() / .green() / .yellow()    Wrap text with ANSI color/style; resets at end. Returns text unchanged
                                     when colors are disabled (non-TTY output, NO_COLOR, --no-color).

TYPE CONVERSION FUNCTIONS:
to_int(value)                        Convert value to integer (returns () on error)
to_int(value, thousands)             Parse integer, removing ANY char in thousands string
to_float(value)                      Convert value to float (returns () on error)
to_float(value, thousands, decimal)  Parse float with explicit separators
                                     - thousands: remove ANY char in string
                                     - decimal: single char or empty (multi-char → error)
to_bool(value)                       Convert value to boolean (returns () on error)
to_int_or(value, default)            Convert value to integer with fallback
to_int_or(value, thousands, default) Parse integer with thousands removal and fallback
to_float_or(value, default)          Convert value to float with fallback
to_float_or(value, thousands, decimal, default)
                                     Parse float with separators and fallback
to_bool_or(value, default)           Convert value to boolean with fallback

UTILITY FUNCTIONS:
eprint(message)                      Print to stderr (suppressed with --no-script-output or data-only modes)
exit(code)                           Exit kelora with given exit code
skip()                               Skip the current event and continue with the next one
get_env(var [,default])              Get environment variable with optional default
print(message)                       Print to stdout (suppressed with --no-script-output or data-only modes)
pseudonym(value, domain)             Generate domain-separated pseudonym (requires KELORA_SECRET)
read_file(path)                      Read file contents as string
read_lines(path)                     Read file as array of lines
status_class(status_code)            Convert HTTP status code to class string ("2xx", "4xx", etc.)
type_of(value)                       Get type name as string (builtin)
window.pluck(field)                  Extract field values from window array (requires --window)
window.pluck_as_nums(field)          Extract numeric field values from window array (requires --window)

DRAIN TEMPLATE MINING (sequential mode only; errors in --parallel mode):
drain_template(text [,options])      Add line to Drain model; returns {template, template_id, count,
                                     is_new, sample, first_line, last_line}
drain_templates()                    Return array of templates with same fields (except is_new)
                                     Default filters: ipv4_port, ipv4, ipv6, email, url, fqdn, uuid,
                                     mac, md5, sha1, sha256, path, oauth, function, hexcolor, version,
                                     hexnum, duration, timestamp, date, time, num
                                     PII filters (opt-in): credit_card (Luhn), ssn (strict XXX-XX-XXXX), phone (NANP-aware for US/CA; permissive internationally)
                                     Options: depth, max_children, similarity, filters, line_num

STATE MANAGEMENT (sequential mode only; errors in --parallel mode):
state["key"]                         Get/set state value via indexer (state["count"] = 0)
state.get(key)                       Get value from state (returns () if not found)
state.set(key, value)                Set value in state
state.contains(key)                  Check if key exists in state
state.remove(key)                    Remove key from state (returns removed value or ())
state.clear()                        Remove all entries from state
state.keys()                         Get array of all keys in state
state.values()                       Get array of all values in state
state.len()                          Get number of entries in state
state.is_empty()                     Check if state is empty
state.mixin(map)                     Merge map into state (overwrites existing keys)
state.fill_with(map)                 Replace entire state with new map
state.to_map()                       Convert state to regular map (for use with to_logfmt(), etc.)
state += map                         Merge map into state (operator form)

TRACKING/METRICS FUNCTIONS (requires --metrics):
All track_* functions skip Unit () values (missing fields); skips are counted
and surfaced via --diagnostics. Categorical arguments (category, item) accept
strings, numbers, and bools (stringified). One metric name = one track function
(mixing functions on the same name is an error).
track_avg(name, value)                Track average of numeric values
track_bottom(name, item [,n])         Track bottom N least frequent items (default n=10)
track_bottom_by(name, item, score [,n]) Track bottom N items by lowest score (default n=10)
track_cardinality(name, value [,err]) Estimate unique count using HyperLogLog (~1% error, ~12KB; err range: 0.001-0.26)
track_count(name, category)           Count occurrences per category: {name: {category: n}}; e.g. track_count("status", e.status)
track_max(name, value)                Track maximum numeric value
track_min(name, value)                Track minimum numeric value
track_percentiles(name, value [,[p]]) Track streaming percentiles using t-digest (default [0.50,0.95,0.99]; auto-suffixes)
track_stats(name, value [,[p]])       Track comprehensive stats: min, max, avg, count, sum, percentiles (auto-suffixes)
track_sum(name, value)                Accumulate numeric values; track_sum(name, 1) is a plain counter
track_top(name, item [,n])            Track top N most frequent items (default n=10)
track_top_by(name, item, score [,n])  Track top N items by highest score (default n=10)
track_unique(name, value)             Track exact set of distinct values (unbounded memory; warns past 100k values)

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
span.metrics                         Per-window metrics from additive track_* calls: track_count,
                                     track_sum, track_avg, track_unique (read-only map).
                                     Non-additive aggregators (min, max, percentiles, cardinality,
                                     top/bottom, top_by/bottom_by) have no per-window value and are
                                     omitted with a warning; iterate span.events to compute them
                                     per window.

EVENT MANIPULATION:
emit_each(array [,base_map])         Fan out array elements as separate events (returns emitted count)
e.absorb_kv(field [,options])        Parse key=value tokens from field, merge pairs, return status map
e.absorb_json(field [,options])      Parse JSON object from field, merge keys, return status map
e.absorb_regex(field, pattern [,opts]) Extract named captures from field using regex, return status map
e = ()                               Clear entire event (remove all fields)
e.field = ()                         Remove individual field from event
e.has("key")                         Check if key exists and value is not ()
e.rename_field("old", "new")         Rename field, returns true if successful

Rhai lets you call functions as either `value.method(args)` or `function(value, args)`.

For other help topics: kelora -h
"#
}

/// Filter the function catalogue by a case-insensitive keyword.
///
/// Returns only the sections and function entries that match `keyword`.
/// A function entry is matched when the keyword appears anywhere in its
/// signature, description, or any indented continuation lines, or when the
/// keyword appears in the section header (so e.g. "string" lists the whole
/// STRING FUNCTIONS section). Section headers are preserved above their
/// matching entries so the output keeps its context.
pub fn filter_help_text(keyword: &str) -> String {
    let full = generate_help_text();
    let needle = keyword.to_lowercase();
    let lines: Vec<&str> = full.lines().collect();

    // A section header sits at column 0 and ends with ':'. Function entries
    // also start at column 0; their continuation lines are indented.
    let is_header = |line: &str| {
        !line.is_empty() && !line.starts_with(char::is_whitespace) && line.trim_end().ends_with(':')
    };

    let mut out = String::new();
    let mut current_section: Option<&str> = None;
    let mut section_printed = false;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        if is_header(line) {
            // The leading title is not a real section; skip it.
            if line.trim() == "Available Rhai Functions:" {
                i += 1;
                continue;
            }
            current_section = Some(line);
            section_printed = false;
            i += 1;
            continue;
        }

        if line.is_empty() {
            i += 1;
            continue;
        }

        // Start of a function entry: this line plus any indented continuations.
        let entry_start = i;
        i += 1;
        while i < lines.len() && lines[i].starts_with(char::is_whitespace) && !lines[i].is_empty() {
            i += 1;
        }
        let entry = &lines[entry_start..i];

        let header_matches = current_section
            .map(|h| h.to_lowercase().contains(&needle))
            .unwrap_or(false);
        let entry_matches = entry.iter().any(|l| l.to_lowercase().contains(&needle));

        if header_matches || entry_matches {
            if !section_printed {
                if let Some(sec) = current_section {
                    out.push('\n');
                    out.push_str(sec);
                    out.push('\n');
                }
                section_printed = true;
            }
            for l in entry {
                out.push_str(l);
                out.push('\n');
            }
        }
    }

    out
}

/// Generate practical examples for common log analysis patterns
pub fn generate_examples_text() -> &'static str {
    r###"
Common Log Analysis Patterns:

GETTING STARTED:
# Preview first 100 lines to understand structure
kelora -f combined web_access.log --head 100 -F inspect

# Quick field discovery and parsing statistics
kelora -j api_logs.jsonl --stats

# Stream from stdin (tail -F, ssh, kubectl logs, etc.) and keep only error/warn
tail -F app.log | kelora -j -l error,warn
ssh loghost.example.net 'tail -F /var/log/app.jsonl' | kelora -j -l error,warn

# Filter by log level (works with any structured format)
kelora -f syslog syslog.log --levels error,critical

# Your first filter - exact match
kelora -f combined web_access.log --filter 'e.status >= 500'

FILTERING & SEARCHING:
# Case-insensitive wildcard search in JSON logs
kelora -j api_logs.jsonl --filter 'e.message.ilike("*timeout*")'

# Regex with Rhai raw string syntax (no escaping backslashes)
kelora -f line email_logs.log --filter 'e.line.matches(#"\d{3}-\d{2}-\d{4}"#)'

# Regex with regular string (requires escaping)
kelora -j api_logs.jsonl --filter 'e.url.matches("/api/v\\d+/users")'

# Field existence check on logfmt (ignores () sentinel)
kelora -f logfmt app.log --filter 'e.has("user_id") && e.user_id != "anonymous"'

# Combine multiple conditions on CSV data
kelora -f csv access_data.csv --filter 'e.has("method") && e.method == "POST" && e.status >= 400'

BOOLEAN LOGIC & COMPLEX FILTERS:
# Control precedence with parentheses (auth + gateway errors only)
kelora -j api_logs.jsonl --filter '(e.service == "auth-service" || e.service == "api-gateway") && e.get_path("status", 0) >= 500'

# Guard against missing fields before comparing (safe nested access)
kelora -j api_logs.jsonl --filter 'e.get_path("stack_trace") != () && e.level == "ERROR"'

# Mix OR/AND with negation and array membership
kelora -j api_logs.jsonl --filter '["POST","PUT"].contains(e.get_path("method")) && (e.get_path("status", 0) >= 500 || e.get_path("response_time", 0.0) > 1.5)'

# Chain filters for readability (evaluated in order)
kelora -j api_logs.jsonl \
  --filter 'e.get_path("status", 0) >= 400' \
  --filter 'e.service == "auth-service" || e.get_path("metadata.subscription.tier") == "premium"' \
  --filter 'e.get_path("response_time", 0.0) > 0.2'

DATA VALIDATION:
# Validate required fields exist (events still pass through, violations reported)
kelora -j api_logs.jsonl --assert 'e.has("user_id")'

# Validate field after transformation
kelora -j data.log --exec 'e.name = e.name.lower()' --assert 'e.name == e.name.lower()'

# Multiple validation rules (all checked)
kelora -j api_logs.jsonl \
  --assert 'e.has("timestamp")' \
  --assert 'e.has("level")' \
  --assert 'e.has("service")'

# Validate data ranges (only check events with status field)
kelora -j api_logs.jsonl --filter 'e.has("status")' --assert 'e.status >= 0 && e.status < 600'

# Strict validation: abort on first failure
kelora -j --strict api_logs.jsonl --assert 'e.has("user_id")'

# Check stats for assertion failure counts
kelora -j api_logs.jsonl --assert 'e.has("user_id")' --stats

PARSING & TRANSFORMATION:
# Parse nested JSON strings from a field
kelora -j api_logs.jsonl --exec 'e.metadata = e.json_payload.parse_json()' \
  --exec 'e.user_tier = e.get_path("metadata.subscription.tier", "free")'

# Extract data with regex from plain text logs (regex in Rhai's raw strings)
kelora -f line email_logs.log --exec 'e.duration = e.line.extract_regex(#"took (\d+)ms"#, 1).to_int()'
kelora -f line app.log --exec 'e.ip = e.line.extract_regex(#"ip=([\d.]+)"#, 1)'

# Fan out nested arrays into separate events
kelora -j fan_out_batches.jsonl --exec 'emit_each(e.items)' --filter 'e.status == "active"'

# Parse key=value pairs from unstructured text
kelora -f line incident_story.log --exec 'e.absorb_kv("line", #{ keep_source: true })'

# Extract fields using regex named captures
kelora -f line app.log --exec 'e.absorb_regex("line", r"User (?P<user>\w+) from (?P<ip>[\d.]+)")'

OUTPUT FORMATS & CLI OPTIONS:
# Output as JSON (from any input format)
kelora -f combined web_access.log -F json

# Output as logfmt (from JSON input)
kelora -j api_logs.jsonl -F logfmt

# Output as CSV with headers
kelora -j api_logs.jsonl -F csv

# Inspect format shows structure (useful for debugging)
kelora -f line email_logs.log --head 20 -F inspect

# Visualize numeric field distributions with tailmap (percentile-based)
kelora -j api_logs.jsonl -F tailmap --keys response_time
kelora -j database_logs.jsonl -F tailmap --keys query_time_ms --filter 'e.query_time_ms > 0'

# Visualize field patterns with keymap (shows first character of field)
kelora -j api_logs.jsonl -F keymap --keys method

# Select specific fields only (-k)
kelora -f combined web_access.log -k client_ip,status,path

# Discover fields: names, types, cardinality, and sample values
kelora -j api_logs.jsonl --discover
kelora -j api_logs.jsonl --discover=json
kelora -j api_logs.jsonl --discover-final --filter 'e.level == "ERROR"'

# Template mining summary (Drain)
kelora -j api_logs.jsonl --drain -k message

# Template mining with custom filters (Rhai)
kelora -j api_logs.jsonl --exec 'e.template = drain_template(e.message, #{ filters: "%{IPV4:ip},%{UUID:uuid}" }).template' -k template

# Brief output: field values only, no labels (-b)
kelora -j api_logs.jsonl -b -k timestamp,level,message

# Core fields only: exclude metadata (-c)
kelora -j api_logs.jsonl -c --filter 'e.level == "ERROR"'

# Convert format using Rhai methods
kelora -j api_logs.jsonl --exec 'print(e.to_logfmt())' -q
kelora -f logfmt app.log --exec 'print(e.to_json())' -q

OUTPUT CONTROL (suppressing different streams):
# Show only stats (automatically suppresses events; no need for -q)
kelora -j api_logs.jsonl -s
kelora -f combined web_access.log --filter 'e.status >= 500' --stats

# Show only metrics (automatically suppresses events)
kelora -j api_logs.jsonl --exec 'track_count("level", e.level)' -m

# Silent mode: suppress all terminal output, but print() still works & files still write
kelora -j api_logs.jsonl --exec 'track_count("error_type", e.error_type)' --silent --metrics-file errors.json

# Custom output format with print() (suppress default formatter with -q)
kelora -j api_logs.jsonl --exec 'print(`${e.timestamp} | ${e.message}`)' -q

# Custom alerts from live logs (tail -f pattern)
tail -f basics.jsonl | kelora -j --filter 'e.level == "ERROR"' --exec 'print(`Error: ${e.message}`)' -q

COMPRESSION:
# Transparent decompression of .gz files
kelora -f combined web_access_large.log.gz --filter 'e.status >= 400' --stats

# Compressed JSON logs
kelora -j sampling_hash.jsonl.gz -k session_id,event,timestamp

# Mix compressed and uncompressed files
kelora -j logs/*.log logs/*.log.gz --filter 'e.level == "ERROR"'

TIME HANDLING:
# Events from the last 2 hours
kelora -j duration_logs.jsonl --since 2h --until now

# Business hours filter (9-5 local time)
kelora -j api_logs.jsonl --exec 'e.hour = to_datetime(e.timestamp).to_local().hour()' \
  --filter 'e.hour >= 9 && e.hour < 17'

# Calculate duration and flag SLA violations
kelora -j duration_logs.jsonl --exec '
  let duration = to_datetime(e.end_time) - to_datetime(e.start_time);
  e.duration_ms = duration.as_milliseconds();
  e.sla_breach = duration.as_seconds() > 5
' --filter 'e.sla_breach'

# Group events by time buckets for histogram
kelora -j api_logs.jsonl --exec '
  let timestamp = to_datetime(e.timestamp);
  e.bucket = timestamp.round_to("5m").to_iso()
' | kelora -j - -m --exec 'track_count("time_buckets", e.bucket)'

# round_to / ceil_to for explicit bucket edges
kelora -j api_logs.jsonl --exec '
  let ts = to_datetime(e.timestamp);
  e.bucket_start = ts.round_to("1h").to_iso();
  e.bucket_end = ts.ceil_to("1h").to_iso()
'

# Show local timestamps
kelora -j api_logs.jsonl -z --since yesterday

METRICS & AGGREGATION:
# Count errors by type with metrics
kelora -j api_errors.jsonl -l error -m \
  --exec 'track_count("error_type", e.error_type)'

# Track unique users (compact output)
kelora -f combined web_access.log --metrics=short \
  --exec 'track_unique("users", e.user)'

# Estimate unique IPs with HyperLogLog (for high-cardinality data)
kelora -f combined web_access.log -m \
  --exec 'track_cardinality("unique_ips", e.client_ip)'

# Histogram of status codes by bucket (JSON output)
kelora web_access.log --metrics=json \
  --exec 'track_count("status", e.status / 100 * 100)'

# Save metrics to JSON file
kelora -j api_logs.jsonl --metrics --metrics-file stats.json \
  --exec 'track_count("level", e.level); track_sum("bytes", e.bytes)' --silent

# Track average response time
kelora -j api_logs.jsonl -m \
  --exec 'track_avg("avg_latency_ms", e.latency_ms)'

# Track percentiles (streaming, memory-efficient, parallel-safe)
# Default percentiles [0.50, 0.95, 0.99]:
kelora -j api_logs.jsonl -m \
  --exec 'track_percentiles("latency", e.response_time)'
# Creates: latency_p50, latency_p95, latency_p99

# Custom percentiles (use 0.0-1.0 range; 0.999 → p99.9):
kelora -j api_logs.jsonl -m \
  --exec 'track_percentiles("latency", e.response_time, [0.50, 0.95, 0.999])'
# Creates: latency_p50, latency_p95, latency_p99.9

# Comprehensive statistics (convenience function combining min/max/avg/percentiles)
# Default percentiles [0.50, 0.95, 0.99]:
kelora -j api_logs.jsonl -m \
  --exec 'track_stats("response_time", e.duration_ms)'
# Creates: response_time_min, response_time_max, response_time_avg,
#          response_time_count, response_time_sum,
#          response_time_p50, response_time_p95, response_time_p99

# Custom percentiles with track_stats:
kelora -j api_logs.jsonl -m \
  --exec 'track_stats("latency", e.duration, [0.50, 0.90, 0.99, 0.999])'
# Creates all basic stats plus: latency_p50, latency_p90, latency_p99, latency_p99.9

# Top/bottom tracking: frequency vs scored
# 3 params = count occurrences (most/least COMMON)
kelora -j api_logs.jsonl -m \
  --exec 'if e.level == "ERROR" { track_top("common_errors", e.error_type, 10) }'

# 4 params = rank by score (HIGHEST/LOWEST values)
kelora -f combined access.log --metrics \
  --exec 'track_top_by("slowest", e.endpoint, e.latency_ms)'

kelora -j db.log --metrics \
  --exec 'track_bottom_by("fastest", e.query_id, e.cpu_time, 5)'

# Custom calculations with print() for complex output (requires --metrics)
kelora -f combined web_access.log --metrics \
  --exec 'track_unique("users", e.user); track_stats("response_time", e.response_time)' \
  --end 'print("p95: " + metrics["response_time_p95"])'

MULTI-FILE PROCESSING:
# Add source filename to each event
kelora -j logs/*.jsonl --exec 'e.source = meta.filename'

# Count errors per file
kelora -f auto logs/*.{log,jsonl} --metrics --exec '
  if e.level == "ERROR" {
    track_count("file", meta.filename)
  }
' --end 'for file in metrics.keys() { print(file + ": " + metrics[file]) }'

# Debug with line numbers
kelora -j api_logs.jsonl --filter 'e.status >= 500' --exec '
  eprint("Error at " + meta.filename + ":" + meta.line_num)
'

SECURITY & DATA PRIVACY:
# Mask IP addresses (keep first 3 octets)
kelora -f combined web_access.log --exec 'e.client_ip = e.client_ip.mask_ip(1)'

# Check for private IPs in external traffic
kelora -j security_audit.jsonl --filter 'e.has("src_ip") && !e.src_ip.is_private_ip()'

# Parse JWT tokens (no verification)
kelora -j auth_burst.jsonl --exec 'let jwt = e.token.parse_jwt(); e.user = jwt.claims.sub'

# Hash sensitive fields with domain separation (requires KELORA_SECRET env var)
kelora -j audit_findings.jsonl --exec 'e.email_hash = pseudonym(e.email, "users"); e.email = ()'

PERFORMANCE PATTERNS:
# Quick preview with --head (stops reading early)
kelora -f line huge.log.gz --head 1000 -F inspect

# Sample every Nth event (fast counter-based, approximate in parallel mode)
kelora -j api_logs.jsonl --filter 'sample_every(100)'

# Sample ~10% of events probabilistically
kelora -j api_logs.jsonl --filter 'sample_prob(0.10)'

# Sample 10% of events for analysis (deterministic)
kelora -j api_logs.jsonl --filter 'e.request_id.bucket() % 10 == 0'

# Limit output events (reads entire file)
kelora -f combined web_access.log --filter 'e.status == 404' --take 50

COMMON IDIOMS:
# Method chaining              → e.domain = e.url.extract_domain().to_lower().strip()
# Default value if missing     → e.referer ?? "direct"
# Nested field with default    → e.get_path("user.profile.tier", "free")
# Safe type conversion         → to_int_or(e.port, 8080)
# Parse formatted integers     → e.count.to_int(",'")         (mixed: "1,234'567" → 1234567)
# Check field exists & not ()  → e.has("user_id")
# Check nested field exists    → e.has_path("response.body.status")
# Remove sensitive fields      → e.password = (); e.ssn = ()
# Hash for sampling/bucketing  → e.session_id.bucket() % 100
# Regex with raw strings       → e.log.extract_regex(#"duration: (\d+)ms"#, 1)
# Regex with regular strings   → e.log.extract_regex("took (\\d+)", 1)
# Array bounds safety          → if e.items.len() > 0 { e.first = e.items[0] }
# Negative array indexing      → e.last = e.items[-1]
# Remove array duplicates      → e.unique_tags = e.tags.unique()
# Pattern normalization        → e.normalized_msg = e.message.normalized("ipv4,email,uuid")

For other help topics: kelora -h
"###
}
