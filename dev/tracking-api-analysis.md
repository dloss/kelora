# Tracking API Analysis: Auto-Suffixing Discussion

## Current Behavior

### How It Works Internally

Each metric has TWO pieces of state:
1. **Value**: The actual metric value stored at `key`
2. **Operation**: The operation type stored at `__op_{key}` (internal metadata)

Operation types: `"count"`, `"sum"`, `"min"`, `"max"`, `"avg"`, `"unique"`, `"bucket"`, `"top"`, `"bottom"`

In parallel mode, the operation metadata tells the merge logic how to combine values from different workers.

### The Problem: Key Conflicts

**When you use the same key with different operations, the last operation wins, and results are WRONG:**

```rhai
// Input: values [10, 50, 30]
track_min("latency", e.value);
track_max("latency", e.value);

// Result: latency = 30 ❌ WRONG!
// Should be either min=10 or max=50
```

**Why:** Each call overwrites:
1. First call: `latency = 10`, `__op_latency = "min"`
2. Second call: `latency = 50`, `__op_latency = "max"` (overwrites min)
3. Third call: `latency = 30`, `__op_latency = "max"`
4. Final: `latency = 30` (last value with max operation)

### Current Workaround: Manual Suffixes

Users MUST manually suffix their keys:

```rhai
track_min("latency_min", e.value);  // ✅ Works
track_max("latency_max", e.value);  // ✅ Works
track_avg("latency_avg", e.value);  // ✅ Works
track_count("latency_count");        // ✅ Works
```

Results:
```
latency_avg  = 30
latency_count = 3
latency_max  = 50
latency_min  = 10
```

## Experimental Results

### Test 1: Same Key Conflict
```bash
# Input: [10, 50, 30]
track_min("latency", e.value); track_max("latency", e.value)
# Result: latency = 30  ❌ WRONG!
```

### Test 2: Different Keys (Current Best Practice)
```bash
track_min("latency_min", e.value); track_max("latency_max", e.value)
# Result: latency_min = 10, latency_max = 50  ✅ CORRECT
```

### Test 3: Parallel Mode
Same behavior - parallel merge respects the operation type, but conflicting operations on the same key still produce wrong results.

## Options for Auto-Suffixing

### Option 1: Keep Status Quo (No Breaking Changes)

**API stays the same:**
```rhai
track_min("latency_min", e.value)
track_max("latency_max", e.value)
track_percentile("latency_p95", e.value, 95)
```

**Pros:**
- ✅ No breaking changes
- ✅ Maximum control over names
- ✅ Simple mental model

**Cons:**
- ❌ Verbose and repetitive
- ❌ Easy to make mistakes (forget suffix → wrong results)
- ❌ Inconsistent if percentile auto-suffixes but others don't

---

### Option 2: Auto-Suffix All Functions (Breaking Change)

**New API:**
```rhai
track_min("latency", e.value)      // → latency_min
track_max("latency", e.value)      // → latency_max
track_avg("latency", e.value)      // → latency_avg
track_count("latency")             // → latency_count
track_percentile("latency", e.value, 95) // → latency_p95

// Grouped metrics in output:
// latency_avg  = 30
// latency_count = 3
// latency_max  = 50
// latency_min  = 10
// latency_p95  = 48
```

**Pros:**
- ✅ Prevents key conflicts automatically
- ✅ Less verbose, cleaner code
- ✅ Natural grouping of related metrics
- ✅ Consistent across all functions
- ✅ Multiple operations on same logical metric "just work"

**Cons:**
- ❌ Breaking change for existing users
- ❌ Less control over exact names (can't use bare name)
- ❌ `track_count("requests")` → `requests_count` is awkward

**Special case for count:**
```rhai
// Could special-case count to NOT suffix if it's standalone:
track_count("requests")  // → requests (no suffix)

// But suffix if used with other operations:
track_min("requests", e.count)   // → requests_min
track_count("requests")           // → requests_count (to avoid conflict)
```

---

### Option 3: Hybrid - Only Auto-Suffix When Needed

**Smart detection:**
- If you only use ONE operation on a key → no suffix
- If you use MULTIPLE operations → auto-suffix all

```rhai
// Single operation - no suffix
track_count("requests")  // → requests

// Multiple operations - auto-suffix
track_min("latency", e.value)
track_max("latency", e.value)
// → latency_min, latency_max
```

**Pros:**
- ✅ Best of both worlds
- ✅ Backwards compatible for single-operation use
- ✅ Prevents conflicts automatically

**Cons:**
- ❌ Magic behavior - hard to predict
- ❌ Output changes based on what operations you use
- ❌ Complex to implement and explain

---

### Option 4: New Grouped API + Keep Old API

**Add new convenience functions:**
```rhai
// Old API - still works, exact control
track_min("my_custom_name", value)

// New API - grouped stats
track_stats("latency", e.value, ["min", "max", "avg", "p95"])
// → latency_min, latency_max, latency_avg, latency_p95

// Or individual
track_stat("latency", "min", e.value)  // → latency_min
```

**Pros:**
- ✅ No breaking changes
- ✅ Convenience for common patterns
- ✅ Power users keep control

**Cons:**
- ❌ Two ways to do the same thing
- ❌ More API surface to document
- ❌ Which one do beginners use?

---

## Recommendation: Option 2 (Auto-Suffix All)

Since Kelora is pre-1.0 and breaking changes are acceptable, auto-suffixing provides:

1. **Correctness by default** - prevents silent bugs from key conflicts
2. **Better DX** - less repetition, cleaner code
3. **Natural grouping** - `latency_*` metrics appear together
4. **Consistency** - all functions behave the same way

### Migration Path

**v0.x → v1.0 breaking change:**

Before (v0.x):
```rhai
track_min("api_latency_min", e.duration)
track_max("api_latency_max", e.duration)
```

After (v1.0):
```rhai
track_min("api_latency", e.duration)  // → api_latency_min
track_max("api_latency", e.duration)  // → api_latency_max
```

Users who want exact names from v0.x: Strip the suffix from their key name.

### Edge Cases to Handle

**1. Count as standalone metric:**
```rhai
track_count("requests")  // → requests_count
// Slightly awkward, but consistent
```

**2. Top/Bottom arrays:**
```rhai
track_top("endpoints", e.path, 10)  // → endpoints_top = ["path1", "path2", ...]
track_bottom("errors", e.code, 5)   // → errors_bottom = ["404", "500", ...]
```

**3. Bucket histograms:**
```rhai
track_bucket("status_codes", e.status)  // → status_codes_bucket = {200: 50, 404: 10}
// Or: status_codes_hist? status_codes_dist?
```

**4. Unique/cardinality:**
```rhai
track_unique("users", e.user_id)  // → users_unique = 1234
// Or: users_count (but conflicts with track_count semantics)
```

## Alternative Consideration: Don't Auto-Suffix, Just Warn

Keep current API but add runtime validation:

```rhai
track_min("latency", e.value);
track_max("latency", e.value);
// → Runtime warning: "Metric 'latency' used with multiple operations (min, max).
//    Consider using different keys: latency_min, latency_max"
```

**Pros:**
- ✅ No breaking changes
- ✅ Educates users about the issue

**Cons:**
- ❌ Still allows wrong behavior
- ❌ Warnings easy to ignore
- ❌ Doesn't solve the UX problem

## Parallel Mode Implications

Auto-suffixing doesn't change parallel merge behavior - each suffixed metric is independent:

```rust
// Worker 1: latency_min = 5, latency_max = 100
// Worker 2: latency_min = 3, latency_max = 150
// Merged:   latency_min = 3, latency_max = 150
```

Works exactly as expected.

## Implementation Complexity

**Low:** ~50 lines of code changes
- Modify each `record_operation_metadata(key, op)` call
- Add suffix: `record_operation_metadata(&format!("{}_{}", key, op), op)`
- Update metrics output formatting (already shows suffixed names)

## User Impact

**Breaking change checklist:**
- [ ] Update documentation and examples
- [ ] Add migration guide in CHANGELOG
- [ ] Consider adding a v0.x→v1.0 script to update user configs
- [ ] Announce in release notes prominently

## Final Verdict

**Implement Option 2 (Auto-Suffix All) for v1.0** because:
1. Pre-1.0 status allows breaking changes
2. Prevents subtle bugs (key conflicts)
3. Improves ergonomics significantly
4. Makes related metrics naturally group together
5. Consistent behavior across all tracking functions

The awkwardness of `requests_count` is outweighed by the benefits of consistency and correctness.
