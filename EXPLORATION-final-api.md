# Final API Design: Overloaded to_float() and to_int()

## Approved Design: Function Overloads

Add optional separator parameters to existing `to_float()` and `to_int()` functions.

---

## API Signature

```rhai
// FLOATS
text.to_float()                              // Existing: strict parsing
text.to_float(thousands_sep, decimal_sep)    // NEW: explicit format
text.to_float_or(default)                    // Existing: with default
text.to_float_or(thousands_sep, decimal_sep, default)  // NEW: format + default

// INTEGERS
text.to_int()                                // Existing: strict parsing
text.to_int(thousands_sep)                   // NEW: explicit format
text.to_int_or(default)                      // Existing: with default
text.to_int_or(thousands_sep, default)       // NEW: format + default
```

**Parameter names:**
- `thousands_sep` - The thousands/grouping separator (any string, including empty `""`)
- `decimal_sep` - The decimal separator (any string, including empty `""`)

**Parameter behavior:**
- Any string length accepted (single-char, multi-char, or empty)
- Treated as literal string for find/replace operations
- Empty string `""` means "no separator" (skip that operation)

**Parameter order rationale:** Matches left-to-right reading of formatted numbers.
In `"1,234.56"` you encounter thousands separator first, then decimal separator.

---

## Behavior

**Processing steps:**
1. **Remove all occurrences** of `thousands_sep` (if not empty string)
2. **Replace all occurrences** of `decimal_sep` with `'.'` (if not empty string)
3. **Parse** the cleaned string as float/int
4. **Return** number or `()` on error (or default if using `_or` variant)

**Separator handling:**
- Any string length accepted (literal string replacement)
- Empty string `""` → skip that operation
- Multi-char separators work: `"1,,234".to_int(",,")` → `1234`

**Examples:**
- `"1,234.56"` with `(',', '.')` → remove `,`, replace `.` with `.` → `"1234.56"` → `1234.56`
- `"1::234;;56"` with `('::', ';;')` → remove `::`, replace `;;` with `.` → `"1234.56"` → `1234.56`

---

## Examples

### US Format (comma thousands, dot decimal)

```rhai
"1,234.56".to_float(',', '.')        // → 1234.56
"1,234,567.89".to_float(',', '.')    // → 1234567.89
"1,234".to_int(',')                  // → 1234
"999,999,999".to_int(',')            // → 999999999

// With defaults
"1,234.56".to_float_or(',', '.', 0.0)      // → 1234.56
"invalid".to_float_or(',', '.', 0.0)       // → 0.0
```

### EU Format (dot thousands, comma decimal)

```rhai
"1.234,56".to_float('.', ',')        // → 1234.56
"1.234.567,89".to_float('.', ',')    // → 1234567.89
"1.234".to_int('.')                  // → 1234
"999.999.999".to_int('.')            // → 999999999

// With defaults
"1.234,56".to_float_or('.', ',', 0.0)      // → 1234.56
```

### French/Swiss Format (space thousands, comma decimal)

```rhai
"1 234,56".to_float(' ', ',')        // → 1234.56
"1 234 567,89".to_float(' ', ',')    // → 1234567.89
"2 000 000".to_int(' ')              // → 2000000

// With defaults
"1 234,56".to_float_or(' ', ',', 0.0)      // → 1234.56
```

### Programming Logs (underscore separator)

```rhai
"3_222_444.67".to_float('_', '.')    // → 3222444.67
"3_222_444".to_int('_')              // → 3222444
"1_000_000.5".to_float('_', '.')     // → 1000000.5
```

### Custom/Weird Formats

```rhai
// Single-char separators
"1:234;56".to_float(':', ';')        // → 1234.56

// Multi-char separators (now supported!)
"1,,234..56".to_float(",,", "..")    // → 1234.56
"1:::234;;;56".to_float(":::", ";;;") // → 1234.56
"1__234__567".to_int("__")           // → 1234567

// No thousands separator (empty string)
"1234,56".to_float("", ',')          // → 1234.56 (empty = skip removal)
"1234.56".to_float("", '.')          // → 1234.56

// No decimal separator (already standard)
"1,234".to_int(',')                  // → 1234
"1,234.0".to_float(',', "")          // → 1234.0 (empty = skip replacement)
```

### Invalid Input (Returns `()`)

```rhai
// Malformed numbers
"invalid".to_float(',', '.')         // → () (can't parse)
"".to_float(',', '.')                // → () (empty string)
"abc123".to_int(',')                 // → () (not a number)
```

---

## Real-World Usage

```rhai
// US financial logs
e.price = e.price_str.to_float(',', '.')
e.quantity = e.qty_str.to_int(',')

// EU server logs
e.response_time = e.time_ms.to_float('.', ',')
e.request_count = e.requests.to_int('.')

// Mixed format handling
if e.locale == "US" {
    e.amount = e.value.to_float(',', '.')
} else if e.locale == "EU" {
    e.amount = e.value.to_float('.', ',')
} else {
    e.amount = e.value.to_float()  // Standard format
}

// With error handling
e.total = e.messy_value.to_float_or(',', '.', 0.0)
e.count = e.user_count.to_int_or(',', 0)
```

---

## Parameter Order Rationale

### For to_float(): `(thousands_sep, decimal_sep)`

**Why thousands first?**
- Matches left-to-right reading of the number string
- In `"1,234.56"` you encounter `,` before `.`
- Natural visual order: `to_float(',', '.')` mirrors `"1,234.56"`
- Intuitive: parameters appear in the same sequence as in the input

**Examples:**
```rhai
"1,234.56".to_float(',', '.')   // comma first, dot second (matches string order!)
"1.234,56".to_float('.', ',')   // dot first, comma second (matches string order!)
"1 234,56".to_float(' ', ',')   // space first, comma second (matches string order!)
```

### For to_int(): `(thousands_sep)` only

**Why only one parameter?**
- Integers don't have decimal separators
- Simpler API for the common case
- Clear and unambiguous

**Examples:**
```rhai
.to_int(',')      // "Remove commas" = US
.to_int('.')      // "Remove dots" = EU
.to_int(' ')      // "Remove spaces" = FR
.to_int('_')      // "Remove underscores" = programming
```

---

## Alternative Parameter Names Considered

| Option | Pros | Cons | Verdict |
|--------|------|------|---------|
| `decimal_sep`, `thousands_sep` | Very explicit | Longer | ✅ **RECOMMENDED** |
| `decimal`, `thousands` | Shorter | Less clear | ⚠️ Acceptable |
| `dec_sep`, `thou_sep` | Abbreviated | Uglier | ❌ |
| `radix`, `grouping` | Technical | Not intuitive | ❌ |
| `point`, `separator` | Confusing | Ambiguous | ❌ |

**Choice: `decimal_sep` and `thousands_sep`**
- Clear and unambiguous
- Consistent with pandas and other tools
- Self-documenting in code

---

## Implementation Notes

```rust
// src/rhai_functions/safety.rs

/// Convert value to float with explicit format
/// Usage: to_float(',', '.') for US format
pub fn to_float_with_format(
    value: Dynamic,
    thousands_sep: ImmutableString,
    decimal_sep: ImmutableString,
) -> Dynamic {
    // Try existing conversion first (for already-numeric values)
    if let Ok(num) = value.as_float() {
        return Dynamic::from(num);
    }
    if let Ok(num) = value.as_int() {
        return Dynamic::from(num as f64);
    }

    // Clean and parse string
    if let Some(s) = value.read_lock::<ImmutableString>() {
        let cleaned = clean_number_string_float(
            s.as_str(),
            thousands_sep.as_str(),
            decimal_sep.as_str()
        );
        if let Ok(num) = cleaned.parse::<f64>() {
            return Dynamic::from(num);
        }
    }

    Dynamic::UNIT
}

/// Helper to clean number string for float parsing
fn clean_number_string_float(s: &str, thousands_sep: &str, decimal_sep: &str) -> String {
    let mut result = s.to_string();

    // Remove thousands separator (if not empty)
    if !thousands_sep.is_empty() {
        result = result.replace(thousands_sep, "");
    }

    // Replace decimal separator with standard dot (if not empty and not already '.')
    if !decimal_sep.is_empty() && decimal_sep != "." {
        result = result.replace(decimal_sep, ".");
    }

    result
}

// Register overloads
pub fn register_functions(engine: &mut Engine) {
    // Existing registrations
    engine.register_fn("to_float", to_float_strict);
    engine.register_fn("to_int", to_int_strict);

    // NEW: Format overloads
    engine.register_fn("to_float", to_float_with_format);
    engine.register_fn("to_int", to_int_with_format);
    engine.register_fn("to_float_or", to_float_or_with_format);
    engine.register_fn("to_int_or", to_int_or_with_format);
}
```

---

## Documentation Updates

### --help-functions

```
CONVERSION FUNCTIONS:
text.to_float()                      Convert text to float (returns () on error)
text.to_float(thousands_sep, decimal_sep)
                                     Parse with explicit separators
                                     Examples:
                                       "1,234.56".to_float(',', '.')   → 1234.56 (US)
                                       "1.234,56".to_float('.', ',')   → 1234.56 (EU)
                                       "1 234,56".to_float(' ', ',')   → 1234.56 (FR)

text.to_int()                        Convert text to integer (returns () on error)
text.to_int(thousands_sep)           Parse with explicit thousands separator
                                     Examples:
                                       "1,234,567".to_int(',')   → 1234567 (US)
                                       "1.234.567".to_int('.')   → 1234567 (EU)
                                       "2 000 000".to_int(' ')   → 2000000 (FR)

text.to_float_or(default)            Convert to float with default fallback
text.to_float_or(thousands_sep, decimal_sep, default)
                                     Parse with separators and default

text.to_int_or(default)              Convert to int with default fallback
text.to_int_or(thousands_sep, default)
                                     Parse with separator and default
```

### --help-examples

```rhai
// Parse formatted numbers
e.us_price = e.price.to_float(',', '.')        // US: "1,234.56" → 1234.56
e.eu_price = e.preis.to_float('.', ',')        // EU: "1.234,56" → 1234.56
e.fr_count = e.nombre.to_int(' ')              // FR: "2 000 000" → 2000000

// With error handling
e.amount = e.value.to_float_or(',', '.', 0.0)  // Default to 0.0 if invalid
e.total = e.count.to_int_or(',', 0)            // Default to 0 if invalid

// Conditional format handling
if e.locale == "US" {
    e.value = e.amount.to_float(',', '.')
} else {
    e.value = e.amount.to_float('.', ',')
}
```

---

## Testing Strategy

```rust
#[test]
fn test_to_float_with_format() {
    // US format
    assert_eq!(to_float_with_format("1,234.56", ",", "."), 1234.56);
    assert_eq!(to_float_with_format("1,234,567.89", ",", "."), 1234567.89);

    // EU format
    assert_eq!(to_float_with_format("1.234,56", ".", ","), 1234.56);
    assert_eq!(to_float_with_format("1.234.567,89", ".", ","), 1234567.89);

    // French format
    assert_eq!(to_float_with_format("1 234,56", " ", ","), 1234.56);

    // Underscore
    assert_eq!(to_float_with_format("3_222_444.67", "_", "."), 3222444.67);

    // Empty string (no separator)
    assert_eq!(to_float_with_format("1234.56", "", "."), 1234.56);
    assert_eq!(to_float_with_format("1234,56", "", ","), 1234.56);
    assert_eq!(to_float_with_format("1,234.0", ",", ""), 1234.0);

    // Multi-char separators (now supported!)
    assert_eq!(to_float_with_format("1,,234..56", ",,", ".."), 1234.56);
    assert_eq!(to_float_with_format("1:::234;;;56", ":::", ";;;"), 1234.56);

    // Invalid: malformed numbers
    assert!(to_float_with_format("invalid", ",", ".").is_unit());
    assert!(to_float_with_format("", ",", ".").is_unit());
}

#[test]
fn test_to_int_with_format() {
    // US format
    assert_eq!(to_int_with_format("1,234", ","), 1234);
    assert_eq!(to_int_with_format("1,234,567", ","), 1234567);

    // EU format
    assert_eq!(to_int_with_format("1.234.567", "."), 1234567);

    // French format
    assert_eq!(to_int_with_format("2 000 000", " "), 2000000);

    // Underscore
    assert_eq!(to_int_with_format("3_222_444", "_"), 3222444);

    // Empty string (no separator)
    assert_eq!(to_int_with_format("1234567", ""), 1234567);

    // Multi-char separator (now supported!)
    assert_eq!(to_int_with_format("1__234__567", "__"), 1234567);
    assert_eq!(to_int_with_format("1,,234", ",,"), 1234);

    // Invalid: malformed numbers
    assert!(to_int_with_format("invalid", ",").is_unit());
    assert!(to_int_with_format("", ",").is_unit());
}
```

---

## Summary

**Final API (4 new overloads):**

```rhai
// NEW overloads
text.to_float(thousands_sep, decimal_sep)
text.to_int(thousands_sep)
text.to_float_or(thousands_sep, decimal_sep, default)
text.to_int_or(thousands_sep, default)

// Existing (unchanged)
text.to_float()
text.to_int()
text.to_float_or(default)
text.to_int_or(default)
```

**Benefits:**
- ✅ Minimal API - just overloads of existing functions
- ✅ One-step conversion (not two like cleanup helpers)
- ✅ Zero ambiguity - explicit separator specification
- ✅ Zero corruption risk - no auto-detection
- ✅ Handles all formats - any separator combination
- ✅ Backward compatible - existing code unchanged
- ✅ Self-documenting - separators visible in code

**This is clean, minimal, and safe!** ✨
