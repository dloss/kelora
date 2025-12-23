# Minimal API Design for Explicit Number Format Parsing

## Goal

Design the **simplest, clearest API** for parsing formatted numbers without auto-detection magic. Prioritize:
- **Zero ambiguity** - User explicitly states what they want
- **Minimal functions** - Small API surface
- **Ergonomic** - Not too verbose for common cases
- **Composable** - Works well with existing Kelora patterns

---

## API Option 1: Separate Functions Per Format

**Approach:** Dedicated function for each common format.

```rhai
// US format (comma thousands, dot decimal)
"1,234.56".parse_float_us()      // → 1234.56
"1,234,567".parse_int_us()       // → 1234567

// EU format (dot thousands, comma decimal)
"1.234,56".parse_float_eu()      // → 1234.56
"1.234.567".parse_int_eu()       // → 1234567

// Space separator (common in FR, SE, etc.)
"1 234.56".parse_float_space()   // → 1234.56
"2 000 000".parse_int_space()    // → 2000000

// Underscore (programming logs)
"3_222_444.67".parse_float_underscore()  // → 3222444.67
"3_222_444".parse_int_underscore()       // → 3222444
```

**Pros:**
- ✅ Crystal clear intent
- ✅ Fast (no format parsing)
- ✅ Autocomplete-friendly
- ✅ Zero parameters to remember

**Cons:**
- ❌ Many functions (API bloat)
- ❌ Doesn't handle mixed formats (e.g., space thousands + comma decimal)
- ❌ What about `_or()` variants? Even more functions?

**Function count:** 8-16 functions (us/eu/space/underscore × int/float × strict/or)

**Verdict:** Too many functions. Not scalable.

---

## API Option 2: Format String Parameter

**Approach:** Single function with format string parameter.

```rhai
// Format codes: "US", "EU", "FR", etc.
"1,234.56".parse_float("US")     // → 1234.56
"1.234,56".parse_float("EU")     // → 1234.56
"1 234,56".parse_float("FR")     // → 1234.56 (space thousands, comma decimal)

"1,234,567".parse_int("US")      // → 1234567
"2 000 000".parse_int("SPACE")   // → 2000000

// With defaults
"1,234.56".parse_float_or("US", 0.0)
```

**Alternative: More explicit format strings**
```rhai
"1,234.56".parse_float(",.") // comma thousands, dot decimal
"1.234,56".parse_float(".,") // dot thousands, comma decimal
"1 234.56".parse_float(" .") // space thousands, dot decimal
```

**Pros:**
- ✅ Only 4 functions total: `parse_float`, `parse_int`, `parse_float_or`, `parse_int_or`
- ✅ Extensible (add more format codes easily)
- ✅ Clear intent

**Cons:**
- ❌ String parameter (typos possible: "US" vs "us" vs "USA")
- ❌ Need to remember format codes
- ❌ Error if invalid format string?

**Function count:** 4 functions

**Verdict:** Good balance, but string parameters are error-prone.

---

## API Option 3: Separator Parameters (Most Flexible)

**Approach:** Explicitly specify decimal and thousands separators.

```rhai
// parse_float(text, decimal_sep, thousands_sep)
"1,234.56".parse_float('.', ',')     // → 1234.56 (US)
"1.234,56".parse_float(',', '.')     // → 1234.56 (EU)
"1 234,56".parse_float(',', ' ')     // → 1234.56 (FR)
"3_222_444.67".parse_float('.', '_') // → 3222444.67

// parse_int(text, thousands_sep)
"1,234,567".parse_int(',')           // → 1234567
"2 000 000".parse_int(' ')           // → 2000000
"3_222_444".parse_int('_')           // → 3222444

// With defaults
"1,234.56".parse_float_or('.', ',', 0.0)
```

**Alternative ordering (decimal first makes more sense):**
```rhai
// Maybe: parse_float(text, decimal, thousands)
"1,234.56".parse_float('.', ',')
```

**Pros:**
- ✅ Maximum flexibility
- ✅ Handles any separator combination
- ✅ Only 4 functions
- ✅ Self-documenting (separators visible in code)
- ✅ No magic strings to remember

**Cons:**
- ❌ Slightly verbose (2-3 parameters)
- ❌ Need to remember parameter order
- ❌ More typing than format codes

**Function count:** 4 functions

**Verdict:** Most flexible, clear, and minimal. Best for power users.

---

## API Option 4: Helper Functions + Chaining

**Approach:** Provide cleanup helpers that work with existing `to_float()` / `to_int()`.

```rhai
// Remove specific separator, then parse
"1,234.56".remove(',').to_float()        // → 1234.56
"2 000 000".remove(' ').to_int()         // → 2000000

// Replace decimal separator
"1.234,56".replace(',', '.').remove('.').to_float()  // Hmm, this is broken
"1.234,56".swap_decimal(',').to_float()  // → 1234.56 (removes thousands, swaps decimal)

// Generic normalize
"1,234.56".normalize_us().to_float()     // → 1234.56
"1.234,56".normalize_eu().to_float()     // → 1234.56
```

**Better version:**
```rhai
// One-step normalize for each format
"1,234.56".clean_us().to_float()     // → 1234.56 (removes commas)
"1.234,56".clean_eu().to_float()     // → 1234.56 (removes dots, comma→dot)
"1 234,56".clean_fr().to_float()     // → 1234.56 (removes spaces, comma→dot)
```

**Pros:**
- ✅ Reuses existing `to_float()` / `to_int()`
- ✅ Separation of concerns
- ✅ Composable
- ✅ Clear data flow

**Cons:**
- ❌ Two-step process (verbose)
- ❌ Still need format-specific helpers
- ❌ Easy to mess up (wrong helper for format)

**Function count:** 3-6 helper functions + existing to_float/to_int

**Verdict:** Interesting but verbose. Good for transparency.

---

## API Option 5: Hybrid - Clean + Optional Explicit

**Approach:** Add simple cleanup function for common cases, explicit parse for complex ones.

```rhai
// Simple: Just remove separators (for unambiguous cases)
"1,234,567".strip_thousands(',').to_int()    // → 1234567
"2 000 000".strip_thousands(' ').to_int()    // → 2000000
"1,234,567.89".strip_thousands(',').to_float() // → 1234567.89

// Full control when needed
"1.234,56".parse_float(',', '.')    // → 1234.56 (decimal, thousands)
"1 234,56".parse_float(',', ' ')    // → 1234.56
```

**Alternative naming:**
```rhai
// Even simpler - just "remove"
"1,234,567".remove(',').to_int()
"2 000 000".remove(' ').to_int()
"1,234,567.89".remove(',').to_float()

// Explicit when ambiguous
"1.234,56".parse_float(',', '.')
```

**Pros:**
- ✅ Simple cases are very simple
- ✅ Complex cases still possible
- ✅ Minimal functions (2-3 total)
- ✅ Natural progression (simple → complex)

**Cons:**
- ❌ Two ways to do similar things
- ❌ User needs to know when to use which

**Function count:** 3-4 functions

**Verdict:** Good balance of simplicity and power.

---

## Recommendation: Option 3 (Separator Parameters)

**Minimal, explicit, flexible API:**

```rhai
// Core functions (4 total)
parse_float(text, decimal, thousands)     // Returns () on error
parse_int(text, thousands)                 // Returns () on error
parse_float_or(text, decimal, thousands, default)
parse_int_or(text, thousands, default)
```

**Examples:**
```rhai
// US format
e.price = e.price_str.parse_float('.', ',')     // "1,234.56" → 1234.56
e.count = e.count_str.parse_int(',')            // "1,234,567" → 1234567

// EU format
e.price = e.price_str.parse_float(',', '.')     // "1.234,56" → 1234.56
e.count = e.count_str.parse_int('.')            // "1.234.567" → 1234567

// French (space thousands, comma decimal)
e.price = e.price_str.parse_float(',', ' ')     // "1 234,56" → 1234.56

// Programming logs (underscore)
e.bytes = e.size_str.parse_int('_')             // "3_222_444" → 3222444

// With defaults
e.amount = e.val.parse_float_or('.', ',', 0.0)
```

**Why this is best:**

1. **Minimal API surface** - Only 4 functions
2. **Zero ambiguity** - Separators are explicit
3. **Maximum flexibility** - Handles any combination
4. **Self-documenting** - Code shows exactly what's expected
5. **No magic strings** - Characters, not codes to remember
6. **Consistent pattern** - Same as `to_float()` / `to_int()` design

---

## Even More Minimal: Option 5B (Just One Cleanup Function)

**The absolutely minimal approach:**

```rhai
// Single cleanup function
clean_number(text, decimal, thousands)  // Returns cleaned string

// Usage
"1,234.56".clean_number('.', ',').to_float()     // → 1234.56
"1.234,56".clean_number(',', '.').to_float()     // → 1234.56
"1 234,56".clean_number(',', ' ').to_float()     // → 1234.56

// With defaults
"1,234.56".clean_number('.', ',').to_float_or(0.0)
```

**Even simpler - shorthand for common formats:**
```rhai
// Shorthand helpers that return cleaned strings
"1,234.56".clean_us().to_float()     // clean_number('.', ',')
"1.234,56".clean_eu().to_float()     // clean_number(',', '.')
"1 234,56".clean_fr().to_float()     // clean_number(',', ' ')

// Or fully explicit
"1:234;56".clean_number(';', ':').to_float()  // Weird custom format
```

**Pros:**
- ✅ Absolute minimal: 1 core function + optional shortcuts
- ✅ Reuses existing `to_float()` / `to_int()`
- ✅ Clear data transformation pipeline
- ✅ Handles integers and floats with same function

**Cons:**
- ❌ Two-step (cleanup then convert)
- ❌ Returns string (intermediate step visible)

**Function count:** 1 core + 3-4 optional shortcuts = 4-5 total

**Verdict:** Most minimal, very clear data flow.

---

## Side-by-Side Comparison

| Approach | Functions | Clarity | Verbosity | Flexibility |
|----------|-----------|---------|-----------|-------------|
| **Option 1: Separate functions** | 8-16 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ |
| **Option 2: Format strings** | 4 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |
| **Option 3: Separator params** | 4 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **Option 4: Helpers + chain** | 6 | ⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ |
| **Option 5B: clean_number()** | 1-5 | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |

---

## Final Recommendation: Two-Tier Design

**Tier 1: Simple helpers for common formats (90% of cases)**
```rhai
clean_us(text)      // Remove commas: "1,234.56" → "1234.56"
clean_eu(text)      // Swap separators: "1.234,56" → "1234.56"
clean_space(text)   // Remove spaces: "1 234.56" → "1234.56"
```

Usage:
```rhai
e.price = e.price_str.clean_us().to_float()
e.count = e.count_str.clean_eu().to_int()
```

**Tier 2: Generic function for edge cases (10% of cases)**
```rhai
clean_number(text, decimal, thousands)
```

Usage:
```rhai
// Weird custom format
e.value = e.data.clean_number(';', ':').to_float()
```

**Total API surface:**
- 3 shorthand helpers
- 1 generic function
- **4 functions total**
- Reuses existing `to_float()`, `to_int()`, `to_float_or()`, `to_int_or()`

**Why this is optimal:**

| Aspect | Benefit |
|--------|---------|
| **Simple cases** | One clear function: `.clean_us()` |
| **Minimal typing** | Shorthand for common formats |
| **Edge cases** | Generic `clean_number()` handles everything |
| **Self-documenting** | `clean_us()` is obvious, `clean_number(',', '.')` is explicit |
| **Composable** | Works with existing to_float/to_int |
| **No ambiguity** | User chooses format explicitly |
| **Zero corruption risk** | No auto-detection |

---

## Implementation Sketch

```rust
// src/rhai_functions/strings.rs

/// Clean number with US format (comma thousands, dot decimal)
/// Example: "1,234.56" → "1234.56"
pub fn clean_us(text: ImmutableString) -> ImmutableString {
    text.replace(',', "").into()
}

/// Clean number with EU format (dot thousands, comma decimal)
/// Example: "1.234,56" → "1234.56"
pub fn clean_eu(text: ImmutableString) -> ImmutableString {
    text.replace('.', "").replace(',', ".").into()
}

/// Clean number with space thousands separator
/// Example: "1 234.56" → "1234.56" or "1 234,56" → "1234.56"
pub fn clean_space(text: ImmutableString) -> ImmutableString {
    text.replace(' ', "").replace(',', ".").into()
}

/// Clean number with custom decimal and thousands separators
/// Example: clean_number("1.234,56", ',', '.') → "1234.56"
pub fn clean_number(
    text: ImmutableString,
    decimal: char,
    thousands: char,
) -> ImmutableString {
    let s = text.as_str();

    // Remove thousands separator
    let without_thousands = s.replace(thousands, "");

    // Replace decimal separator with standard dot
    if decimal != '.' {
        without_thousands.replace(decimal, ".").into()
    } else {
        without_thousands.into()
    }
}
```

**Usage examples:**
```rhai
// US logs
e.amount = e.price.clean_us().to_float()              // "1,234.56" → 1234.56
e.count = e.requests.clean_us().to_int()              // "1,234,567" → 1234567

// EU logs
e.amount = e.price.clean_eu().to_float()              // "1.234,56" → 1234.56
e.count = e.users.clean_eu().to_int()                 // "1.234.567" → 1234567

// French logs
e.amount = e.metric.clean_space().to_float()          // "1 234,56" → 1234.56

// Programming logs with underscores
e.bytes = e.size.clean_number('.', '_').to_int()      // "3_222_444" → 3222444

// With error handling
e.total = e.value.clean_us().to_float_or(0.0)
```

---

## Documentation Impact

### Update `--help-functions`:

```
NUMBER PARSING HELPERS:
text.clean_us()                      Remove US thousands separators (commas)
                                     Example: "1,234.56" → "1234.56"

text.clean_eu()                      Clean EU format (dots→removed, comma→dot)
                                     Example: "1.234,56" → "1234.56"

text.clean_space()                   Remove space thousands separators
                                     Example: "1 234,56" → "1234.56"

text.clean_number(decimal, thousands) Clean custom number format
                                     Example: text.clean_number(',', '.') for EU

Then chain with: .to_float(), .to_int(), .to_float_or(default), .to_int_or(default)
```

### Add to examples:

```rhai
// Parse formatted numbers
e.us_price = e.price.clean_us().to_float()        // "1,234.56" → 1234.56
e.eu_price = e.preis.clean_eu().to_float()        // "1.234,56" → 1234.56
e.fr_count = e.nombre.clean_space().to_int()      // "2 000 000" → 2000000

// Custom formats
e.bytes = e.size.clean_number('.', '_').to_int()  // "3_222_444" → 3222444

// With defaults
e.amount = e.value.clean_us().to_float_or(0.0)
```

---

## Comparison to Auto-Detection

| Aspect | Auto-Detection | Explicit Helpers |
|--------|---------------|------------------|
| **API surface** | 0 new functions | 4 new functions |
| **Corruption risk** | 0.1-5% | **0%** |
| **Verbosity** | Low | Medium |
| **Clarity** | Implicit | **Explicit** |
| **Flexibility** | Limited | **Full** |
| **Learning curve** | None | Minimal |
| **Best for** | Quick scripts | Production logs |

**Explicit helpers win on:**
- ✅ Zero corruption risk
- ✅ Clear intent
- ✅ Predictable behavior
- ✅ Handles all edge cases

**Auto-detection wins on:**
- ✅ Less typing
- ✅ "Just works" for simple cases

---

## Summary

**Recommended minimal API:**

```rhai
// Tier 1: Common formats (shorthand)
clean_us(text)        // US: "1,234.56" → "1234.56"
clean_eu(text)        // EU: "1.234,56" → "1234.56"
clean_space(text)     // Space: "1 234.56" → "1234.56"

// Tier 2: Custom formats (explicit)
clean_number(text, decimal, thousands)

// Always chain with existing converters
.to_float()
.to_int()
.to_float_or(default)
.to_int_or(default)
```

**Total:** 4 new functions, zero ambiguity, maximum clarity.

**Trade-off:** Slightly more verbose than auto-detection, but:
- Zero corruption risk
- Explicit intent
- Self-documenting code
- Handles all formats
- Composable with existing functions

This is the sweet spot for a log analysis tool where **correctness > convenience**.
