# Exploration: Number Format Parsing for Kelora

## Problem Statement

Logs from different locales and systems may contain numbers formatted in various ways:

**Examples:**
- European format: `1.234.567,34` (dots for thousands, comma for decimal)
- US format: `1,234,567.34` (commas for thousands, dot for decimal)
- Space separators: `2 000 000` or `2 000 000,50`
- Underscore separators: `3_222_444.67` (common in some programming logs)
- Mixed: `1 234.56` (space thousands, dot decimal)

Currently, `to_float()` and `to_int()` only handle standard formats that Rust's `parse()` accepts (e.g., `123456.78` or `123456`). They fail silently on formatted numbers, returning `()`.

**Use Case Examples:**
```rhai
// Current behavior (fails):
"1,234.56".to_float()      // Returns () - can't parse
"1.234,56".to_float()      // Returns () - can't parse
"2 000 000".to_int()       // Returns () - can't parse

// Desired behavior:
parse_number("1,234.56", format)   // Should work!
```

---

## How Other Tools Solve This

### 1. **Python / Pandas**

**Approach:** Explicit parameters for decimal and thousands separators

```python
# Reading CSV with European format
pd.read_csv('data.csv', decimal=',', thousands='.')

# Locale-aware parsing
import locale
locale.setlocale(locale.LC_ALL, 'de_DE.UTF-8')
locale.atof('1.234.567,89')  # Returns 1234567.89
```

**Pros:**
- Explicit and clear
- No guessing/ambiguity
- Works per-operation

**Cons:**
- Verbose for repeated operations
- Need to know format ahead of time

**Sources:**
- [pandas.read_csv() documentation](https://pandas.pydata.org/docs/reference/api/pandas.read_csv.html)
- [How to deal with international data formats in Python](https://herrmann.tech/en/blog/2021/02/05/how-to-deal-with-international-data-formats-in-python.html)

---

### 2. **JavaScript**

**Approach:** Locale-based formatting (primarily for output)

```javascript
// Format with locale
(1234567.89).toLocaleString("de-DE")  // "1.234.567,89"
new Intl.NumberFormat("de-DE").format(1234567.89)

// Parse requires manual cleanup
parseFloat("1.234.567,89".replace(/\./g, '').replace(',', '.'))
```

**Pros:**
- Standards-based (ECMA-402 Internationalization API)
- Rich locale support

**Cons:**
- Parsing requires manual string manipulation
- No built-in parse from locale format

**Sources:**
- [Intl.NumberFormat - MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/NumberFormat)
- [Number.prototype.toLocaleString() - MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Number/toLocaleString)

---

### 3. **Rust Ecosystem**

**Available Crates:**

#### `num-format` (Most comprehensive)
```rust
use num_format::{Locale, ToFormattedString};
let formatted = 1234567.to_formatted_string(&Locale::de);
// "1.234.567"
```

#### `numfmt`
```rust
// Custom format spec: "[,1n/_]"
// Comma signals period separator, underscore decimal marker
```

**Key Finding:** Rust has **good formatting** crates but **limited parsing** support for localized number strings. Most forum discussions show manual string manipulation as the solution.

**Sources:**
- [num_format crate](https://docs.rs/num-format)
- [numfmt crate](https://docs.rs/numfmt)
- [Rust Forum: How to parse a localized number string](https://users.rust-lang.org/t/how-to-parse-a-localized-number-string/50272)

---

### 4. **AWK**

**Approach:** Locale-based (global setting)

```bash
# Parse European format
LC_ALL=de_DE.UTF-8 awk '{print $1 + 0}' file.txt

# Format with thousands separator
LC_ALL=en_US.UTF-8 awk '{printf("%'d\n", $1)}'
```

**Pros:**
- Automatic based on locale
- No explicit conversion needed

**Cons:**
- Global setting affects entire script
- Hard to mix formats in one script
- Implicit behavior can be confusing

**Sources:**
- [GNU AWK: Locale influences conversions](https://www.gnu.org/software/gawk/manual/html_node/Locale-influences-conversions.html)
- [GNU AWK: Format Modifiers](https://www.gnu.org/software/gawk/manual/html_node/Format-Modifiers.html)

---

### 5. **jq (JSON processor)**

**Approach:** Manual string cleanup before parsing

```bash
echo '"1,234.56"' | jq 'gsub(","; "") | tonumber'
# Output: 1234.56
```

**Pros:**
- Explicit and transparent
- No magic/guessing

**Cons:**
- Verbose
- User must handle all formats manually

**Sources:**
- [jq Manual](https://jqlang.org/manual/)

---

### 6. **PostgreSQL / MySQL**

**PostgreSQL:**
```sql
-- Format with locale-aware separators
SELECT to_char(1234567.89, '999G999G999D99');
-- Uses lc_numeric setting for G (group) and D (decimal)

-- Parsing requires regex cleanup
SELECT regexp_replace('1.234,56', '\.', '', 'g')::numeric;
```

**MySQL:**
```sql
-- Format with optional locale
SELECT FORMAT(1234567.89, 2, 'de_DE');
-- Output: "1.234.567,89"
```

**Pros:**
- Integrated with database locale settings
- Powerful formatting

**Cons:**
- Parsing often requires manual regex
- Complex to mix formats

**Sources:**
- [PostgreSQL: Data Type Formatting Functions](https://www.postgresql.org/docs/current/functions-formatting.html)
- [Format Numbers with Commas in PostgreSQL](https://database.guide/format-numbers-with-commas-in-postgresql/)
- [How to Format Numbers in MySQL](https://database.guide/how-to-format-numbers-in-mysql/)

---

## API Design Options for Kelora

### Option 1: Extended `to_float()` / `to_int()` with Auto-Detection

**Approach:** Make current functions smarter by detecting and handling common formats automatically.

```rhai
// Auto-detect and parse
"1,234.56".to_float()      // → 1234.56 (US format)
"1.234,56".to_float()      // → 1234.56 (EU format)
"1 234.56".to_float()      // → 1234.56 (space separator)
"3_222_444".to_int()       // → 3222444 (underscore separator)
"1,234,567".to_int()       // → 1234567 (US thousands)
```

**Auto-Detection Logic:**
1. Count occurrences of `.`, `,`, space, `_`
2. Identify last separator as decimal (if it appears once and has 2-3 digits after)
3. Treat all preceding separators as thousands separators
4. Strip thousands separators, replace decimal with `.`, parse

**Examples of ambiguous cases:**
- `"1,234"` - Could be 1.234 (EU) or 1234 (US thousands) → **Assume integer 1234**
- `"1.234"` - Could be 1234 (EU thousands) or 1.234 (US decimal) → **Assume integer 1234**
- `"1,234.56"` - Clear: US format → 1234.56
- `"1.234,56"` - Clear: EU format → 1234.56

**Pros:**
- ✅ Zero API changes - backward compatible
- ✅ Works automatically for most common cases
- ✅ Fits Kelora's "just works" philosophy
- ✅ No extra functions to learn

**Cons:**
- ❌ Ambiguous for some numbers (e.g., "1,234" could be 1.234 or 1234)
- ❌ Might surprise users who expect strict behavior
- ❌ Could incorrectly parse edge cases

**Recommendation:** Assume integers unless clear decimal separator pattern detected.

---

### Option 2: Explicit Format Parameter Functions

**Approach:** New functions that accept explicit format specification.

```rhai
// Explicit decimal and thousands separator
"1.234,56".parse_float(',', '.')     // → 1234.56
"1,234.56".parse_float('.', ',')     // → 1234.56
"2 000 000".parse_int(' ')           // → 2000000
"3_222_444".parse_int('_')           // → 3222444

// Or with format string
"1.234,56".parse_float("EU")         // → 1234.56
"1,234.56".parse_float("US")         // → 1234.56
"1 234,56".parse_float("FR")         // → 1234.56
```

**Pros:**
- ✅ Explicit and unambiguous
- ✅ User controls behavior
- ✅ Handles all edge cases correctly

**Cons:**
- ❌ More verbose
- ❌ Additional API surface
- ❌ User must know format ahead of time
- ❌ Redundant when format is obvious

---

### Option 3: Hybrid Approach - `to_float()` Auto + Optional Explicit

**Approach:** Make `to_float()` / `to_int()` smarter with auto-detection, but add explicit functions for edge cases.

```rhai
// Most common cases - auto-detect (extended behavior)
"1,234.56".to_float()         // → 1234.56 (auto-detect US)
"1.234,56".to_float()         // → 1234.56 (auto-detect EU)
"2 000 000".to_int()          // → 2000000 (auto-detect space)

// Edge cases or explicit control
"1,234".parse_number(',', '') // → 1.234 (force EU decimal)
"1,234".to_int()              // → 1234 (auto-detect as integer)
"1.234,56".parse_float(',', '.') // → 1234.56 (explicit)
```

**Pros:**
- ✅ Best of both worlds
- ✅ Auto works for 90% of cases
- ✅ Explicit available for edge cases
- ✅ Backward compatible

**Cons:**
- ❌ More API surface (but optional)
- ❌ Two ways to do same thing

---

### Option 4: Cleanup Helper + Current Functions

**Approach:** Provide helper to clean formatted strings, then use current `to_float()` / `to_int()`.

```rhai
// Chain operations
"1,234.56".remove_separators(',').to_float()     // → 1234.56
"1.234,56".clean_number(',', '.').to_float()     // → 1234.56

// Or combined
"1.234,56".normalize_number("EU").to_float()     // → 1234.56
```

**Pros:**
- ✅ Separation of concerns
- ✅ Composable
- ✅ Reusable for other purposes

**Cons:**
- ❌ Verbose for simple cases
- ❌ Multiple function calls
- ❌ Still needs format knowledge

---

## Recommendation for Kelora

### **Option 1 (Auto-Detection) is the best fit** for Kelora's design philosophy:

**Why it fits Kelora:**
1. **Minimal API changes** - Leverages existing `to_float()` / `to_int()` functions
2. **"Just works" philosophy** - Users don't need to think about formats
3. **Log analysis context** - Logs typically have consistent formatting within a file
4. **Backward compatible** - Current valid inputs still work
5. **Common case optimization** - Handles 90% of real-world cases automatically

**Implementation Strategy:**

```rust
// Enhanced to_float_strict() logic
pub fn to_float_strict(value: Dynamic) -> Dynamic {
    // Try standard parse first (fast path)
    if let Some(s) = value.read_lock::<ImmutableString>() {
        if let Ok(num) = s.parse::<f64>() {
            return Dynamic::from(num);
        }

        // Auto-detect and clean formatted numbers
        if let Some(cleaned) = auto_clean_number(s.as_str()) {
            if let Ok(num) = cleaned.parse::<f64>() {
                return Dynamic::from(num);
            }
        }
    }

    Dynamic::UNIT
}

fn auto_clean_number(s: &str) -> Option<String> {
    // 1. Strip whitespace
    // 2. Detect separator pattern
    // 3. Identify decimal vs thousands separator
    // 4. Clean and normalize to "123456.78" format
}
```

**Auto-Detection Rules:**

| Input | Interpretation | Output |
|-------|---------------|--------|
| `"1,234,567.89"` | US format (comma thousands, dot decimal) | `1234567.89` |
| `"1.234.567,89"` | EU format (dot thousands, comma decimal) | `1234567.89` |
| `"1 234 567.89"` | Space thousands, dot decimal | `1234567.89` |
| `"1_234_567.89"` | Underscore thousands, dot decimal | `1234567.89` |
| `"1,234"` | Ambiguous → Assume integer | `1234` |
| `"1.234"` | Ambiguous → Assume integer | `1234` |
| `"1,23"` | Two decimal places → EU decimal | `1.23` |
| `"1.23"` | Two decimal places → US decimal | `1.23` |

**Heuristic Logic:**
1. Last separator with 2-3 digits after = decimal separator
2. All other separators = thousands separators
3. If only one separator with 3+ digits after = thousands separator (integer)
4. If only one separator with 1-3 digits after = could be either → check digit count after separator

**Alternative Enhancement (Optional Future Addition):**

Add explicit functions if users report ambiguous cases:

```rhai
// If auto-detection fails, explicit override available
"1,234".parse_number(',', '')    // Force comma as decimal
"1.234".parse_number('.', '')    // Force dot as decimal
```

---

## Testing Strategy

If implementing Option 1, comprehensive tests needed:

```rust
// Clear cases (should all work)
assert_eq!(to_float("1,234.56"), 1234.56);
assert_eq!(to_float("1.234,56"), 1234.56);
assert_eq!(to_float("1 234.56"), 1234.56);
assert_eq!(to_int("2 000 000"), 2000000);
assert_eq!(to_int("3_222_444"), 3222444);

// Ambiguous cases (define expected behavior)
assert_eq!(to_int("1,234"), 1234);      // Assume integer
assert_eq!(to_int("1.234"), 1234);      // Assume integer
assert_eq!(to_float("1,23"), 1.23);     // Two digits → decimal
assert_eq!(to_float("1.23"), 1.23);     // Two digits → decimal

// Edge cases
assert_eq!(to_float("1,234,567,890.12"), 1234567890.12);
assert_eq!(to_float(".50"), 0.50);
assert_eq!(to_float("-.50"), -0.50);
assert!(to_float("invalid").is_unit());
```

---

## Documentation Impact

Update `--help-functions` with enhanced description:

```
text.to_float()                      Convert text to float (returns () on error)
                                     Auto-detects common number formats:
                                     - US: 1,234.56 (comma thousands, dot decimal)
                                     - EU: 1.234,56 (dot thousands, comma decimal)
                                     - Space: 1 234.56 (space thousands)
                                     - Underscore: 1_234.56 (underscore thousands)

text.to_int()                        Convert text to integer (returns () on error)
                                     Auto-detects thousands separators: , . _ space
                                     Examples: "1,234" → 1234, "2_000_000" → 2000000
```

Add example to `--help-examples`:

```rhai
// Parse numbers from different locales
e.us_amount = e.us_price.to_float()     // "1,234.56" → 1234.56
e.eu_amount = e.eu_price.to_float()     // "1.234,56" → 1234.56
e.count = e.metric.to_int()             // "2 000 000" → 2000000
```

---

## Performance Considerations

**Impact:** Minimal overhead for most cases

1. **Fast path:** Standard format numbers parse immediately (no change)
2. **Slow path:** Only triggered on parse failure
3. **Optimization:** Cache separator detection pattern per unique format

**Micro-benchmark suggestion:**
```rust
// Before: "123456.78" → parse() → 20ns
// After:  "123456.78" → parse() → 20ns (same)
// After:  "1,234.56"  → detect + clean + parse() → ~100ns (acceptable)
```

---

## Potential Issues and Mitigations

### Issue 1: Breaking Changes
**Risk:** Users relying on `"1,234".to_float()` returning `()` might see different behavior

**Mitigation:**
- Very unlikely - most users would expect it to parse
- If needed, add config flag: `--strict-number-parsing`

### Issue 2: Ambiguous Numbers
**Risk:** `"1,234"` could be 1.234 or 1234

**Mitigation:**
- Define clear heuristic (prefer integer interpretation)
- Document behavior clearly
- Add explicit `parse_number()` if users need override

### Issue 3: Performance
**Risk:** Auto-detection adds overhead

**Mitigation:**
- Fast path for standard formats (no overhead)
- Only parse formatted numbers on demand
- Complexity is O(n) where n = string length (very fast)

---

## Summary

| Aspect | Recommendation |
|--------|---------------|
| **Best Option** | Option 1: Auto-detection in existing `to_float()` / `to_int()` |
| **Why** | Fits Kelora's "just works" philosophy, zero API changes |
| **Implementation** | Add auto-cleaning logic before parse, with smart heuristics |
| **Fallback** | Can add explicit `parse_number()` later if needed |
| **Testing** | Comprehensive test suite for edge cases |
| **Documentation** | Update `--help-functions` with format examples |
| **Performance** | Negligible impact (fast path for standard formats) |

**Next Steps:**
1. ✅ Research complete (this document)
2. ⏸️ Get user/maintainer feedback on Option 1 approach
3. ⏸️ Implement auto-detection logic if approved
4. ⏸️ Add comprehensive tests
5. ⏸️ Update documentation
6. ⏸️ Consider explicit functions if edge cases arise

---

## References

**Research Sources:**
- [pandas.read_csv() documentation](https://pandas.pydata.org/docs/reference/api/pandas.read_csv.html)
- [Python international data formats](https://herrmann.tech/en/blog/2021/02/05/how-to-deal-with-international-data-formats-in-python.html)
- [Intl.NumberFormat - MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/NumberFormat)
- [Number.prototype.toLocaleString() - MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Number/toLocaleString)
- [Rust num_format crate](https://docs.rs/num-format)
- [Rust numfmt crate](https://docs.rs/numfmt)
- [Rust Forum: Localized number parsing](https://users.rust-lang.org/t/how-to-parse-a-localized-number-string/50272)
- [GNU AWK: Locale influences conversions](https://www.gnu.org/software/gawk/manual/html_node/Locale-influences-conversions.html)
- [GNU AWK: Format Modifiers](https://www.gnu.org/software/gawk/manual/html_node/Format-Modifiers.html)
- [jq Manual](https://jqlang.org/manual/)
- [PostgreSQL: Data Type Formatting Functions](https://www.postgresql.org/docs/current/functions-formatting.html)
- [Format Numbers with Commas in PostgreSQL](https://database.guide/format-numbers-with-commas-in-postgresql/)
- [How to Format Numbers in MySQL](https://database.guide/how-to-format-numbers-in-mysql/)
