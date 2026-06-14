# Rhai Cheatsheet

Quick reference for Rhai scripting in Kelora. For detailed tutorials, see [Advanced Scripting](../tutorials/advanced-scripting.md). For practical examples: `kelora --help-examples`. For function reference: `kelora --help-functions`.

## Variables & Types

```rhai
let x = 42;                          // Integer (i64)
let price = 19.99;                   // Float (f64)
let name = "alice";                  // String (double quotes only!)
let active = true;                   // Boolean (true/false)
let tags = [1, "two", 3.0];          // Array (mixed types ok)
let user = #{name: "bob", age: 30};  // Map/object literal
let empty = ();                      // Unit type (Rhai's "nothing")

type_of(x)                           // Returns: "i64", "string", "array", "map", "()"
x = "hello";                         // Dynamic typing: can change type
```

**Key Points:**

- `let` required for new variables (no implicit declaration)
- Double quotes only for strings (`"text"` not `'text'`)
- Unit type `()` represents "nothing" (not null/undefined)
- Arrays and maps are reference types (modifying copies affects original)

## Operators

```rhai
// Arithmetic
a + b    a - b    a * b    a / b    a % b    a ** b  // power: 2**3 == 8

// Comparison
a == b   a != b   a < b    a > b    a <= b   a >= b

// Logical
a && b   a || b   !a

// Bitwise
a & b    a | b    a ^ b    a << b   a >> b

// Assignment
a = b    a += b   a -= b   a *= b   a /= b   a %= b
a &= b   a |= b   a ^= b   a <<= b  a >>= b

// Ranges (for loops only)
1..5     // Exclusive: 1, 2, 3, 4
1..=5    // Inclusive: 1, 2, 3, 4, 5

// Membership
"key" in map                         // Check if key exists
```

## String Interpolation

Rhai supports string interpolation using `${...}` syntax within backtick strings:

```rhai
let name = "Alice";
let age = 30;
let message = `Hello, ${name}! You are ${age} years old.`;
// Result: "Hello, Alice! You are 30 years old."

// Complex expressions in interpolation
let x = 10;
let y = 20;
let result = `Sum: ${x + y}, Product: ${x * y}`;
// Result: "Sum: 30, Product: 200"

// Nested interpolations
let status = "active";
let msg = `User ${name} is ${`currently ${status}`}`;
// Result: "User Alice is currently active"

// Multi-line interpolated strings
let report = `
  User: ${e.user.name}
  Status: ${e.status}
  Count: ${e.items.len()}
`;
```

**Key Points:**

- Interpolation only works with backtick strings (`` `text` ``), not double-quote strings (`"text"`)
- Use `${expression}` to embed any Rhai expression
- The expression can be a variable, function call, or complex statement block
- Cannot escape `${` in interpolated strings; build such strings in pieces instead

## Raw Strings

Disable escape sequences with `#"..."#` (ideal for regexes and file paths):

```rhai
let regex = #"\d{3}-\d{2}-\d{4}"#;        // vs "\\d{3}-\\d{2}-\\d{4}"
let path = #"C:\Users\data"#;             // Windows paths
let s = ##"Has "quotes" inside"##;        // Multiple # to include "
```

## Control Flow

### If-Else

```rhai
if x > 10 {
    print("big");
} else if x > 5 {
    print("medium");
} else {
    print("small");
}

// Ternary-style (if is an expression)
let category = if x > 10 { "big" } else { "small" };
```

### Switch

```rhai
let category = switch x {
    1 => "one",
    2 | 3 => "two or three",          // Multiple cases
    4..=6 => "four to six",            // Range matching
    _ => "other"                       // Default (underscore)
};
```

### Loops

```rhai
// Range loops
for i in 0..10 { print(i); }          // 0 to 9
for i in 0..=10 { print(i); }         // 0 to 10

// Array iteration
for item in array { print(item); }

// Map iteration (maps aren't directly iterable; iterate keys, then index)
for key in map.keys() {
    print(`${key} = ${map[key]}`);
}

// While loop
while condition {
    if done { break; }
    if skip { continue; }
}

// Infinite loop
loop {
    if should_stop { break; }
}
```

## Functions & Closures

```rhai
// Function definition
fn add(a, b) {
    a + b                             // Last expr is return value
}

fn greet(name) {
    return "Hello, " + name;          // Explicit return
}

// Closures
let double = |x| x * 2;
let add = |a, b| a + b;

// Closures in array methods
[1, 2, 3].map(|x| x * 2)              // [2, 4, 6]
[1, 2, 3].filter(|x| x > 1)           // [2, 3]
```

## Rhai Special Feature: Function-as-Method

Rhai allows calling any function as a method on its first argument:

```rhai
// These are equivalent:
extract_regex(e.line, r"\d+")            // Function call style
e.line.extract_regex(r"\d+")             // Method call style

// Use method style for chaining:
e.domain = e.url
    .extract_domain()
    .to_lower()
    .strip();

// Both styles work for all functions:
to_int(e.port)                        // Function style
e.port.to_int()                       // Method style (more readable)
```

## Kelora Event Access

The global variable `e` represents the current event in `--filter` and `--exec` stages:

```rhai
// Direct field access
e.level                               // Top-level field
e.user.name                           // Nested field (maps)
e.scores[1]                           // Array indexing (0-based)
e.scores[-1]                          // Negative indexing (last element)
e.headers["user-agent"]               // Bracket notation for special chars

// Field existence checking
"field" in e                          // Check top-level field exists
e.has("field")                        // True only if value not ()
e.has_path("user.role")               // Check nested path exists

// Safe field access with defaults
e.get_path("user.role", "guest")      // Get nested with fallback
e.get_path("scores[0]", 0)            // Works with array paths

// Field removal
e.password = ()                       // Remove field (unit assignment)
e.ssn = ()                            // Remove another field
e = ()                                // Remove entire event (filtered out)
```

## Array & Map Operations

JSON arrays become native Rhai arrays with full functionality:

```rhai
// Array transformations
sorted(e.scores)                      // Sort numerically/lexicographically
reversed(e.items)                     // Reverse order
unique(e.tags)                        // Remove duplicates
dedup(e.values)                       // Remove consecutive duplicates
sorted_by(e.users, "age")             // Sort objects by field

// Array methods
e.tags.len()                          // Length
e.tags.is_empty()                     // Check if empty
e.tags.join(", ")                     // Join to string
e.scores.sum()                        // Sum numbers
e.scores.min()                        // Minimum value
e.scores.max()                        // Maximum value

// Array access patterns
if e.items.len() > 0 {
    e.first = e.items[0];
    e.last = e.items[-1];
}

// Fan-out: convert array elements to separate events
emit_each(e.items)                    // Each element becomes an event
emit_each(e.items, #{ctx: "value"})   // Add base fields to each

// Map operations (maps aren't directly iterable; iterate keys, then index)
for key in e.keys() {
    print(`${key} = ${e[key]}`);
}
```

## Type Conversions

```rhai
// Strict conversions (return () on error)
to_int(e.port)                        // String → integer
to_float(e.price)                     // String → float
to_bool(e.active)                     // String → boolean

// Safe conversions with defaults
e.port.to_int_or(8080)                // Use default if conversion fails
e.price.to_float_or(0.0)
e.active.to_bool_or(false)

// String conversions
to_string(42)                         // Any → string
e.value.to_int()                      // Method style

// Type checking
type_of(e.field)                      // Get type as string
type_of(e.field) != "()"              // Check if field has value
```

## Common Patterns

### Safe Nested Access

```rhai
// With default fallback
let role = e.get_path("user.role", "guest");
let port = e.port.to_int_or(8080);

// With existence check
if e.has_path("user.profile.avatar") {
    e.avatar = e.user.profile.avatar;
}

// Safe array access
if e.items.len() > 0 {
    e.first_item = e.items[0];
}
```

### Conditional Field Removal

```rhai
// Remove debug fields in production
if e.level != "DEBUG" {
    e.stack_trace = ();
    e.debug_info = ();
}

// Remove entire event conditionally
if e.status < 400 { e = (); }         // Only keep errors
```

### Method Chaining

```rhai
// Extract and normalize domain
e.domain = e.url
    .extract_domain()
    .to_lower()
    .strip();

// Parse and extract from structured text
e.error_line = e.stack_trace
    .extract_regex(r"line (\d+)", 1)
    .to_int_or(0);
```

### Array Processing

```rhai
// Get top N scores
e.top_3 = sorted(e.scores)[-3:];

// Extract names from sorted users
e.winners = sorted_by(e.users, "score")
    .reverse()
    .map(|u| u.name);

// Filter and count
e.active_items = e.items.filter(|i| i.status == "active");
e.active_count = e.active_items.len();
```

### Multi-Level Fan-Out

```rhai
# First exec: batches → separate events
--exec 'emit_each(e.batches)'

# Second exec: items → separate events with context
--exec 'let ctx = #{batch_id: e.id}; emit_each(e.items, ctx)'

# Filter the final events
--filter 'e.status == "active"'
```

## Global Context

```rhai
conf                                  // Global config map (read-only after --begin)
metrics                               // Global metrics map (from track_* calls)
meta                                  // Event metadata (filename, line numbers, raw line)
get_env("VAR", "default")             // Environment variable access

// meta attributes:
meta.line                             // Original raw line (always available)
meta.line_num                         // Line number, 1-based (available with files)
meta.filename                         // Source filename (multi-file processing)
meta.parsed_ts                        // Parsed UTC timestamp before scripts (or () if missing)

// Example usage:
--begin 'conf.env = get_env("ENVIRONMENT", "dev")'
--filter 'conf.env == "prod" || e.level == "ERROR"'

// Multi-file tracking
--exec 'if e.level == "ERROR" { track_freq("file", meta.filename) }'

// Debugging with line numbers
--exec 'eprint("Error at " + meta.filename + ":" + meta.line_num)'
```

## Error Handling Modes

**Default (resilient):**

- Parse errors → skip line, continue
- Filter errors → treat as false, drop event
- Exec errors → rollback, keep original event
- Recovered filter/exec errors are warnings and exit 0

**Strict mode (`--strict`):**

- Any error → abort immediately with exit code 1

## Rhai Quirks & Gotchas

| Coming from | Watch out for |
|------------|---------------|
| **JavaScript** | No `null`/`undefined` (use `()`), no single quotes, `let` required |
| **Python** | Braces required, no `:` after if/for, `!=` not `<>`, double quotes only |
| **Rust** | More permissive syntax, semicolons mostly optional, dynamic typing |

**Common mistakes:**

```rhai
// ❌ Wrong
x = 1                                 // Error: x not declared
let name = 'alice';                   // Error: single quotes not allowed
if x > 5: print("big")                // Error: colon not allowed, braces required
"5" + 3                               // Error: no implicit conversion

// ✅ Correct
let x = 1;                            // Declare with let
let name = "alice";                   // Double quotes
if x > 5 { print("big"); }            // Braces required
"5".to_int() + 3                      // Explicit conversion
```

**Special behaviors:**

- Last expression in block is return value (no `return` needed)
- Semicolons recommended but often optional
- Function calls without args: `e.len` same as `e.len()`
- No implicit type conversion (use `to_int()`, `to_float()`, etc.)

## Quick Reference

```rhai
// Event manipulation
e.field = value                       // Set field
e.field = ()                          // Remove field
e = ()                                // Remove event

// Type checking
type_of(e.field)                      // Get type
"field" in e                          // Field exists

// Safe access
e.get_path("a.b.c", default)          // Nested with fallback
e.has_path("a.b.c")                   // Check nested exists

// Conversions
val.to_int_or(0)                      // Safe int conversion
val.to_float_or(0.0)                  // Safe float conversion
val.to_bool_or(false)                 // Safe bool conversion
try { risky_call(); } catch (err) {   // Catch runtime errors (type mismatch, missing fields); slower than guards
  eprint(err);                        // Prefer to_int_or/has_path for common cases
}

// Arrays
e.items.len()                         // Length
e.items.is_empty()                    // Check empty
sorted(e.items)                       // Sort
unique(e.items)                       // Deduplicate
emit_each(e.items)                    // Fan out to events

// Strings
e.text.to_lower()                     // Lowercase
e.text.to_upper()                     // Uppercase
e.text.strip()                        // Trim whitespace
e.text.contains("word")               // Substring check
e.text.extract_regex(r"(\d+)", 1)        // Regex extraction

// Environment & Context
get_env("VAR", "default")             // Get env var
conf.key                              // Read config (from --begin)
metrics.key                           // Read metrics (in --end)
meta.filename                         // Current source filename
meta.line_num                         // Current line number (1-based)
meta.line                             // Original raw line
```

## See Also

- [Advanced Scripting Tutorial](../tutorials/advanced-scripting.md) - Detailed walkthrough with examples
- `kelora --help-functions` - Complete function catalogue (add a KEYWORD to search, e.g. `kelora --help-functions ip`)
- `kelora --help-examples` - Practical log analysis patterns
- `kelora --help-rhai` - Language guide (this cheatsheet's source)
- [Rhai Documentation](https://rhai.rs) - Full Rhai language reference
