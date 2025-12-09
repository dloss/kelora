# Missing Feature Ideas for Kelora

This document contains creative, useful features that fit Kelora's design principles and architecture but haven't been implemented yet. All suggestions are:

- ✅ Simple to implement (30-200 lines each)
- ✅ Fit the streaming pipeline architecture
- ✅ Actually missing (not currently possible without verbose workarounds)
- ✅ Broadly useful for common log analysis patterns
- ✅ Follow Kelora's existing conventions

## Datetime & Time-Based Features

### 1. Time Bucketing/Rounding for Timestamps (`dt.round_to()`)

**What:** Round timestamps to nearest time interval for easy grouping and aggregation.

**Why useful:** When analyzing logs, you often want to group events by hour, minute, or 5-minute intervals. Currently you'd need manual arithmetic with timestamps. This would be a simple datetime method.

**Example:**
```rhai
// Round to nearest hour for grouping
e.ts = to_datetime(e.timestamp).round_to("1h")

// Round to 5-minute buckets for rate analysis
bucket_time = to_datetime(e.ts).round_to("5m")
track_count(bucket_time.to_string())
```

**Implementation:** Simple datetime manipulation using existing Duration types. ~50 lines in `datetime.rs`.

---

### 9. Burst/Session Detection (`track_burst()`)

**What:** Detect when events cluster within a time window - identify attack bursts, user sessions, batch processing windows.

**Why useful:** Common security/monitoring pattern. "Alert when >50 failed logins in 5 minutes" is currently complex. This makes it trivial.

**Example:**
```rhai
// Detect authentication attacks
burst_size = track_burst("failed_login_" + e.ip, "5m")
if burst_size > 50 {
    e.alert = "POSSIBLE_BRUTE_FORCE"
    e.burst_size = burst_size
}

// Identify batch processing windows
batch_events = track_burst("job_" + e.job_id, "10s")
e.batch_position = batch_events

// Auto-reset after idle period
user_session_size = track_burst("user_" + e.user_id, "30m")
```

**Implementation:** Time-windowed counter with expiration, ~100 lines in `tracking.rs`.

---

## Tracking & State Management

### 2. Event Deduplication (`track_seen()`)

**What:** Track whether a value has been seen before, perfect for deduplicating events based on request IDs, hashes, or combinations of fields.

**Why useful:** Log shippers often produce duplicates. Currently you'd need to manually maintain a hash set in the state map. A built-in function would make this trivial.

**Example:**
```rhai
// Skip duplicate request IDs
if track_seen("request_ids", e.request_id) {
    skip()
}

// Dedupe by content hash
content_hash = e.to_json().hash()
if !track_seen("events", content_hash) {
    e  // Only emit first occurrence
}
```

**Implementation:** Uses internal HashSet in tracking state. ~80 lines in `tracking.rs`.

---

### 6. First/Last Value Tracking (`track_first()` / `track_last()`)

**What:** Remember the first or last seen value for a key - useful for baseline comparisons, time range detection, and status changes.

**Why useful:** Common pattern in monitoring: "what was the initial value?" or "what's the latest status?" Currently requires manual state management.

**Example:**
```rhai
// Track when each service first appeared
first_seen = track_first("service_start_" + e.service, e.timestamp)
e.uptime_since = first_seen

// Compare current vs initial memory
baseline_mem = track_first("baseline_memory", e.memory_mb)
e.mem_growth = e.memory_mb - baseline_mem

// Track latest status per host
latest_status = track_last("host_" + e.hostname, e.status)
if latest_status == "degraded" && e.status == "healthy" {
    print("✅ " + e.hostname + " recovered")
}
```

**Implementation:** Simple key-value store in tracking state, ~40 lines.

---

### 8. Auto-Incrementing Sequence Numbers (`track_sequence()`)

**What:** Add monotonic sequence IDs to events - critical for detecting missing events, ordering guarantees, or adding synthetic keys.

**Why useful:** When correlating across systems or detecting gaps in event streams. Dead simple feature that's surprisingly useful.

**Example:**
```rhai
// Add sequence number to every event
e.seq = track_sequence("global")

// Per-service sequences
e.service_seq = track_sequence("seq_" + e.service_name)

// Detect gaps: if seq jumped by >1, events were lost
prev = track_last("prev_seq", e.seq)
if e.seq - prev > 1 {
    print("⚠️ Missing " + (e.seq - prev - 1) + " events!")
}
```

**Implementation:** Simple counter in tracking state, ~25 lines.

---

### 13. Event Correlation Tracker (`track_correlation()`)

**What:** Match paired events (request/response, start/end) and emit when both arrive.

**Why useful:** Distributed systems generate paired events. Correlating them reveals latency, completion rates, and failures. Currently requires complex state management.

**Example:**
```rhai
// Match HTTP requests with responses
if e.event_type == "request" {
    track_correlation("http_" + e.request_id, e, "request")
} else if e.event_type == "response" {
    pair = track_correlation("http_" + e.request_id, e, "response")
    if pair != () {
        // Emit combined event with both request and response
        #{
            request_id: e.request_id,
            latency: e.timestamp - pair.timestamp,
            status: e.status,
            endpoint: pair.endpoint
        }
    }
}
```

**Implementation:** State storage with timeout/cleanup, ~120 lines in `tracking.rs`.

---

### 15. Threshold-Based Heavy Hitters (`track_if_above()`)

**What:** Only track items that appear above a threshold - automatic filtering of rare values.

**Why useful:** When tracking top items, you don't want to waste memory on singletons. This auto-prunes low-frequency values.

**Example:**
```rhai
// Only track IPs with >10 requests (ignore one-offs)
track_if_above("frequent_ips", e.ip, 10)

// Track only popular endpoints
track_if_above("hot_paths", e.path, 100)

// Auto-filter noise, metrics shows only significant items
```

**Implementation:** Delayed insertion until threshold met, ~70 lines in `tracking.rs`.

---

## Statistical & Metrics Functions

### 3. Rate/Throughput Calculation (`track_rate()`)

**What:** Automatically calculate events per second/minute/hour for performance monitoring.

**Why useful:** Understanding throughput is critical for monitoring. Currently you'd need complex window logic or manual timestamp tracking. This would make it one-liner.

**Example:**
```rhai
// Track overall throughput
track_rate("total_eps")  // events per second

// Track per-endpoint rates
track_rate("endpoint_" + e.path, "1m")  // per minute

// At end, metrics shows: endpoint_/api/users: 145.2/min
```

**Implementation:** Tracks timestamps internally, calculates rolling rate. ~100 lines in `tracking.rs`.

---

### 5. Field Change Detection (`track_delta()`)

**What:** Track numeric changes between consecutive events - perfect for monitoring counters, detecting anomalies, or calculating derivatives.

**Why useful:** Many systems log cumulative counters (bytes processed, requests served). You often want the delta between readings. Currently requires manual state tracking.

**Example:**
```rhai
// Track request count increases
delta = track_delta("api_requests", e.total_requests)
if delta > 1000 {
    print("🔥 Spike detected: " + delta + " requests")
}

// Monitor memory growth
mem_delta = track_delta("memory_" + e.service, e.memory_mb)
if mem_delta > 500 {
    e.mem_increase = mem_delta
    e  // Flag events with big memory jumps
}
```

**Implementation:** Stores previous value in state, returns difference. ~60 lines in `tracking.rs`.

---

### 7. Streaming Approximate Percentiles (`track_p50()`, `track_p95()`, `track_p99()`)

**What:** Calculate percentiles on-the-fly without storing all values - essential for latency monitoring at scale.

**Why useful:** Current `percentile()` requires collecting all values in an array. For millions of events, this is memory-prohibitive. Streaming approximation (using t-digest or similar) solves this.

**Example:**
```rhai
// Track API latency percentiles
track_p95("api_latency", e.duration_ms)
track_p99("api_latency", e.duration_ms)

// Per-endpoint tracking
track_p50("endpoint_" + e.path, e.response_time)

// Metrics output shows: api_latency_p95: 245ms, api_latency_p99: 890ms
```

**Implementation:** t-digest algorithm or simpler histogram-based approach, ~150-200 lines in `tracking.rs`.

---

### 11. Moving Average for Smoothing (`track_moving_avg()`)

**What:** Calculate moving average over the last N values - smooth out spikes and see trends.

**Why useful:** Raw metrics are noisy. Moving averages reveal patterns. Great for dashboards and anomaly detection.

**Example:**
```rhai
// Smooth response times over last 100 requests
smooth_latency = track_moving_avg("api_latency", e.duration_ms, 100)
if e.duration_ms > smooth_latency * 3 {
    e.anomaly = "LATENCY_SPIKE"
}

// Track rolling error rate
errors = track_moving_avg("error_rate", e.is_error ? 1 : 0, 1000)
if errors > 0.05 {
    print("⚠️ Error rate above 5%: " + (errors * 100) + "%")
}
```

**Implementation:** Circular buffer in tracking state, ~80 lines in `tracking.rs`.

---

## Sampling & Filtering

### 4. Simple Counter-Based Sampling (`sample_every()`)

**What:** Sample every Nth event - simpler alternative to hash-based bucket sampling for when you just want "every 10th event."

**Why useful:** `bucket()` does deterministic sampling (great for consistent sampling across runs), but sometimes you just want "give me 10% of events" or "every 100th line." This complements the existing sampling.

**Example:**
```rhai
// Keep only every 100th event for high-volume logs
if !sample_every(100) { skip() }

// 1% sampling for metrics
if sample_every(100) {
    track_count("sampled_errors")
}
```

**Implementation:** Simple counter in state. ~30 lines in `rhai_functions/random.rs`.

---

## String & Parsing Functions

### 10. Regex Multi-Field Extraction (`extract_fields()`)

**What:** Extract multiple named capture groups from a regex into event fields in one operation.

**Why useful:** Common pattern: parse unstructured text into structured fields. Currently needs multiple `extract_regex()` calls. This does it in one shot.

**Example:**
```rhai
// Parse custom log format in one go
e.extract_fields(
    r"User (?P<user>\w+) from (?P<ip>[\d.]+) (?P<action>\w+) (?P<resource>.*)",
    ["user", "ip", "action", "resource"]
)
// Now e.user, e.ip, e.action, e.resource are all populated

// Parse error messages
e.extract_fields(
    r"Error (?P<code>\d+): (?P<message>.*) at (?P<location>.*)",
    ["error_code", "error_msg", "location"]
)
```

**Implementation:** Use existing regex infrastructure, ~60 lines in `strings.rs`.

---

### 12. Smart Text Truncation (`truncate_words()`)

**What:** Truncate strings but preserve word boundaries - avoid cutting mid-word for cleaner output.

**Why useful:** When limiting message length for display or storage, cutting mid-word looks broken. This keeps it readable.

**Example:**
```rhai
// Truncate long messages cleanly
e.summary = e.message.truncate_words(50)  // Max 50 chars, break at word boundary

// Preserve formatting
e.preview = e.description.truncate_words(100, "...")

// Compare:
// Bad:  "The quick brown fox jum..."
// Good: "The quick brown fox..."
```

**Implementation:** String splitting with UTF-8 awareness, ~40 lines in `strings.rs`.

---

## Array & Collection Functions

### 14. Array Set Operations (`intersect()`, `difference()`, `union()`)

**What:** Set operations on arrays - find common elements, differences, combinations.

**Why useful:** Common when comparing tags, roles, permissions, or features between events. Currently requires verbose filtering.

**Example:**
```rhai
// Find common tags between events
common_tags = current_tags.intersect(previous_tags)

// Find newly added permissions
new_perms = after.permissions.difference(before.permissions)

// Combine feature flags
all_features = baseline_features.union(experimental_features)

// Alert on tag changes
added = e.tags.difference(baseline_tags)
if added.len() > 0 {
    e.tags_added = added
}
```

**Implementation:** HashSet operations, ~50 lines in `arrays.rs`.

---

## Summary by Category

### High Priority (Simple + High Impact)
- **Time Bucketing** - Essential for grouping by time intervals
- **Deduplication** - Critical for real-world log processing
- **Sequence Numbers** - Simple but surprisingly useful
- **Counter Sampling** - Complements existing bucket sampling
- **First/Last Tracking** - Common pattern, easy to implement

### Medium Priority (More Complex but Very Useful)
- **Burst Detection** - Security and monitoring use cases
- **Rate Calculation** - Performance monitoring essential
- **Delta Tracking** - Counter monitoring and anomaly detection
- **Moving Average** - Noise reduction and trend detection
- **Multi-Field Extraction** - Parsing efficiency

### Advanced Features (Require More Design)
- **Streaming Percentiles** - Memory-efficient metrics at scale
- **Event Correlation** - Distributed tracing patterns
- **Threshold Tracking** - Heavy hitter detection
- **Set Operations** - Collection comparison utilities
- **Smart Truncation** - Polish for text handling

## Implementation Notes

All features:
- Use existing infrastructure (tracking state, datetime wrappers, etc.)
- Follow Kelora's conventions (method-style calls, error handling)
- Require no external dependencies
- Fit within the streaming pipeline model
- Work in both sequential and parallel modes (where applicable)

Features that require `--metrics` flag:
- All `track_*` functions (consistent with existing tracking functions)

Features that work only in sequential mode:
- `track_correlation()` (requires consistent state)
- `track_burst()` (time-window tracking needs ordering)

Features compatible with `--parallel`:
- `dt.round_to()` (stateless transformation)
- `sample_every()` (with thread-local counters)
- `extract_fields()` (stateless parsing)
- `truncate_words()` (stateless string operation)
- Array set operations (stateless)
