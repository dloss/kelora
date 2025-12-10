# Auto-Suffix Examples Review

Real examples from Kelora docs/examples showing how auto-suffixing would look.

## Pattern 1: Simple Counting (Most Common)

### Current
```rhai
track_count("total")
track_count("errors")
track_count(e.service)
track_count("level_" + e.level)
```

### With Auto-Suffix
```rhai
track_count("total")          // → total_count ⚠️ AWKWARD
track_count("errors")         // → errors_count ⚠️ AWKWARD
track_count(e.service)        // → auth-service_count ⚠️ VERY AWKWARD
track_count("level_" + e.level) // → level_ERROR_count ⚠️ AWKWARD
```

**Verdict: This is the biggest problem.** When `track_count()` is used alone, the `_count` suffix is redundant and awkward.

---

## Pattern 2: Namespace/Grouping with Pipes

### Current
```rhai
track_sum("latency_total_ms|" + e.service, latency)
track_max("latency_p99|" + e.service, latency)
track_count("latency_samples|" + e.service)
track_max("memory_peak|" + e.service, e.memory_percent)
```

Output:
```
latency_total_ms|auth-service = 1234
latency_p99|auth-service = 567
latency_samples|auth-service = 42
memory_peak|auth-service = 78.5
```

### With Auto-Suffix
```rhai
track_sum("latency_total_ms|" + e.service, latency)
track_max("latency_p99|" + e.service, latency)
track_count("latency_samples|" + e.service)
track_max("memory_peak|" + e.service, e.memory_percent)
```

Output:
```
latency_total_ms|auth-service_sum = 1234   ⚠️ WEIRD - suffix after pipe
latency_p99|auth-service_max = 567         ⚠️ "p99_max" is confusing
latency_samples|auth-service_count = 42    ⚠️ "samples_count" redundant
memory_peak|auth-service_max = 78.5        ✅ OK but "peak_max" redundant
```

**Verdict: Awkward.** The pipe pattern is meant for human-readable grouping, and auto-suffixing breaks it.

---

## Pattern 3: Multiple Operations on Same Metric (The Fix!)

### Current (BROKEN - last operation wins)
```rhai
track_min("latency", e.duration)
track_max("latency", e.duration)
track_avg("latency", e.duration)
// Result: latency = 30 (avg value, min/max lost!) ❌
```

**Users must manually suffix:**
```rhai
track_min("latency_min", e.duration)
track_max("latency_max", e.duration)
track_avg("latency_avg", e.duration)
// Result: latency_min = 10, latency_max = 50, latency_avg = 30 ✅
```

### With Auto-Suffix (FIXED!)
```rhai
track_min("latency", e.duration)  // → latency_min
track_max("latency", e.duration)  // → latency_max
track_avg("latency", e.duration)  // → latency_avg
// Result: latency_min = 10, latency_max = 50, latency_avg = 30 ✅
```

**Verdict: This is what we're trying to fix!** Auto-suffix solves the conflict problem elegantly.

---

## Pattern 4: Bucketing

### Current
```rhai
track_bucket("level", e.level)
track_bucket("status_codes", e.status)
```

Output (map):
```
level = {ERROR: 10, WARN: 5, INFO: 100}
status_codes = {200: 50, 404: 10, 500: 2}
```

### With Auto-Suffix
```rhai
track_bucket("level", e.level)          // → level_bucket
track_bucket("status_codes", e.status)  // → status_codes_bucket
```

Output:
```
level_bucket = {ERROR: 10, WARN: 5, INFO: 100}  ⚠️ AWKWARD
status_codes_bucket = {200: 50, 404: 10, 500: 2}  ⚠️ AWKWARD
```

**Verdict: Awkward but not terrible.** The `_bucket` suffix makes it clearer it's a distribution, but feels verbose.

---

## Pattern 5: Top/Bottom

### Current
```rhai
track_top("endpoints", e.path, 10)
track_bottom("errors", e.code, 5)
```

Output (array of tuples):
```
endpoints = [("/api/users", 1234), ("/api/orders", 890), ...]
errors = [("TIMEOUT", 5), ("CONNECTION_REFUSED", 3), ...]
```

### With Auto-Suffix
```rhai
track_top("endpoints", e.path, 10)  // → endpoints_top
track_bottom("errors", e.code, 5)   // → errors_bottom
```

Output:
```
endpoints_top = [("/api/users", 1234), ...]  ✅ Actually clearer!
errors_bottom = [("TIMEOUT", 5), ...]        ✅ Makes sense
```

**Verdict: Good!** The suffix clarifies it's a "top N" result, not a count or sum.

---

## Pattern 6: Unique/Cardinality

### Current
```rhai
track_unique("users", e.user_id)
track_unique("hashes", e.user_hash)
```

Output:
```
users = 1234   (unique count)
hashes = 890
```

### With Auto-Suffix
```rhai
track_unique("users", e.user_id)    // → users_unique
track_unique("hashes", e.user_hash) // → hashes_unique
```

Output:
```
users_unique = 1234  ⚠️ AWKWARD - "unique users" reads better than "users unique"
hashes_unique = 890  ⚠️ Same issue
```

**Verdict: Awkward.** In English, "unique users" is natural, but "users_unique" isn't.

---

## Real Documentation Examples - Before/After

### Example 1: Service Health Snapshot
**Current:**
```rhai
track_count(e.service)                              // api-gateway = 100
track_count("level_" + e.level)                     // level_ERROR = 5
track_sum("latency_total_ms|" + e.service, latency) // latency_total_ms|api-gateway = 5000
track_max("latency_p99|" + e.service, latency)      // latency_p99|api-gateway = 250
```

**With Auto-Suffix:**
```rhai
track_count(e.service)                              // api-gateway_count = 100 ⚠️
track_count("level_" + e.level)                     // level_ERROR_count = 5 ⚠️
track_sum("latency_total_ms|" + e.service, latency) // latency_total_ms|api-gateway_sum = 5000 ⚠️
track_max("latency_p99|" + e.service, latency)      // latency_p99|api-gateway_max = 250 ⚠️
```

**Problems:**
- `api-gateway_count` - when counting services, you want `api-gateway: 100`, not `api-gateway_count: 100`
- `level_ERROR_count` - redundant, `level_ERROR: 5` is clearer
- Pipe patterns break: `latency_p99|api-gateway_max` has suffix in wrong place

---

### Example 2: Error Triage
**Current:**
```rhai
track_count(e.service)                         // auth-service = 42
track_count(e.get_path("error.code", "unknown")) // TIMEOUT = 10
```

**With Auto-Suffix:**
```rhai
track_count(e.service)                         // auth-service_count = 42 ⚠️
track_count(e.get_path("error.code", "unknown")) // TIMEOUT_count = 10 ⚠️
```

**Problem:** Suffixing dynamic keys from event fields is awkward.

---

### Example 3: Histogram Bucketing
**Current:**
```rhai
track_bucket("level", e.level)
let bucket = floor(e.response_time / 100) * 100
track_bucket("latency_ms", bucket)
```

Output:
```
level = {ERROR: 10, WARN: 5}
latency_ms = {0: 50, 100: 30, 200: 15, 300: 5}
```

**With Auto-Suffix:**
```rhai
track_bucket("level", e.level)           // → level_bucket
track_bucket("latency_ms", bucket)       // → latency_ms_bucket
```

Output:
```
level_bucket = {ERROR: 10, WARN: 5}  ⚠️
latency_ms_bucket = {0: 50, 100: 30, ...}  ⚠️
```

**Problem:** `_bucket` suffix makes names verbose and less natural.

---

## The Core Issues with Auto-Suffixing

### 1. **Count is the biggest problem**
When counting is the ONLY operation (90% of use cases), `_count` suffix is pure noise:
- `track_count("requests")` → `requests_count` ❌
- `track_count(e.service)` → `auth-service_count` ❌

Users want: `requests: 1234`, not `requests_count: 1234`

### 2. **Pipe patterns break**
The `|` namespace pattern is already solving the grouping problem:
- `latency_total_ms|service-a` is clear
- `latency_total_ms|service-a_sum` is confused

### 3. **Operation is often redundant in the name**
Users already embed semantics:
- `track_sum("latency_total_ms|" + svc, lat)` - "total" implies sum
- `track_max("latency_p99|" + svc, lat)` - "p99" implies max
- `track_count("latency_samples|" + svc)` - "samples" implies count

Auto-suffixing creates: `latency_total_ms|service_sum`, `latency_p99|service_max` 😖

### 4. **Dynamic keys from event fields are awkward**
```rhai
track_count(e.service)  // → "auth-service_count"
track_count(e.status)   // → "404_count"
```

When the key IS the thing being counted, suffix is redundant.

---

## Frequency Analysis

Looking at all examples in docs:

| Pattern | Count | Auto-Suffix Quality |
|---------|-------|---------------------|
| `track_count(dynamic_key)` | ~40 | ❌ Awkward |
| `track_count("prefix_" + field)` | ~15 | ❌ Awkward |
| `track_sum/max with pipe` | ~20 | ⚠️ Breaks pattern |
| Multiple ops on same base | ~3 | ✅ FIXES BUG |
| `track_bucket()` | ~5 | ⚠️ Verbose |
| `track_top/bottom()` | ~2 | ✅ Clear |
| `track_unique()` | ~3 | ⚠️ Awkward |

**~55 examples get worse, ~5 get better.**

---

## Alternative: Don't Auto-Suffix, Detect Conflicts at Runtime

Keep current API, but add validation:

```rhai
track_min("latency", e.value)
track_max("latency", e.value)
// → ⚠️  kelora: Metric 'latency' used with conflicting operations: min, max
//       Use separate keys to track multiple stats:
//         track_min("latency_min", e.value)
//         track_max("latency_max", e.value)
```

**Pros:**
- ✅ No breaking changes
- ✅ Educates users about the problem
- ✅ Doesn't make common patterns awkward

**Cons:**
- ❌ Requires detection logic (track operation per key)
- ❌ Warning-only (doesn't prevent the bug)

---

## Conclusion

**Auto-suffixing breaks more than it fixes** because:

1. **90% of usage is single operation** - `track_count()` alone, where suffix is noise
2. **Users already encode semantics** - "total", "p99", "samples" in names
3. **Pipe patterns break** - suffix goes in wrong place
4. **Dynamic keys are awkward** - `service-name_count` is unnatural

**Better solution: Runtime conflict detection + clear error messages**

Let users keep their current patterns but warn them when they shoot themselves:
```
track_min("latency", e.dur); track_max("latency", e.dur)
→ Error: Conflicting operations on metric 'latency' (min, max).
  Metrics must have unique names. Use: latency_min, latency_max
```

This educates without breaking the 95% of code that's fine.
