# Rhai Cheatsheet

Rhai syntax and idioms for Kelora scripts. Which variables (`e`, `meta`, `conf`, `state`, `window`, `metrics`, `span`) exist in which stage: [Script Variables](script-variables.md). All functions: [Functions](functions.md) or `kelora --help-functions [KEYWORD]`.

## Values

```rhai
let n = 42;                          // i64
let f = 19.99;                       // f64
let s = "alice";                     // string: double quotes ('a' is a single char)
let re = #"\d+\.\d+"#;               // raw string: no escapes, use for regexes
let q = ##"has "quotes""##;          // more # to allow " inside
let msg = `user ${s} has ${n + 1}`;  // interpolation: backticks only, \${ for a literal ${
let ok = true;
let tags = [1, "two", 3.0];          // array, mixed types ok
let user = #{name: "bob", age: 30};  // map
let none = ();                       // unit: Rhai's "nothing" (no null/undefined)

type_of(n)                           // "i64", "f64", "string", "bool", "array", "map", "()"
```

- `let` is required to create a variable; plain `x = 1` fails with `Variable not found`.
- Variables are dynamically typed: `let x = 1; x = "s";` is fine.
- Arrays and maps are copied on assignment: `let b = a; b.push(2);` leaves `a` unchanged.
- Comments: `// line`, `/* block */`.

## Operators

```text
Arithmetic   + - * / %   ** (power)
Comparison   == != < > <= >=
Logical      && || !
Bitwise      & | ^ << >>
Assignment   = += -= *= /= %= &= |= ^= <<= >>=
Default      a ?? b       a unless it is (); b is evaluated only if needed
Membership   "key" in map, x in array
Ranges       1..5 (1–4), 1..=5 (1–5), in for loops and switch arms
```

`+` with a string on either side concatenates: `"5" + 3` is `"53"`, `"n=" + ()` is `"n="`. To add numbers, convert first: `"5".to_int() + 3`.

Assignment is a statement, not an expression: `let n = (x = 1);` does not compile.

## Control flow

```rhai
if x > 10 { "big" } else if x > 5 { "medium" } else { "small" }   // braces required
let size = if x > 10 { "big" } else { "small" };                  // if is an expression

let kind = switch e.status {
    200 | 204 => "ok",
    400..=499 => "client error",
    _ => "other"
};

for i in 0..10 { ... }
for item in e.items { ... }
for key in e.keys() { print(`${key}=${e[key]}`); }   // maps aren't iterable: use keys()/values()
while cond { if done { break; } if skip { continue; } }
loop { if stop { break; } }
```

Semicolons separate statements; they are optional only after a block `}` and after the last statement.

## Functions, closures, method calls

```rhai
fn add(a, b) { a + b }               // last expression is the return value
fn greet(name) { return "hi " + name; }

let double = |x| x * 2;
[1, 2, 3].map(|x| x * 2)             // [2, 4, 6]
[1, 2, 3].filter(|x| x > 1)          // [2, 3]
[1, 2, 3].reduce(|acc, x| acc + x, 0)
```

Any built-in can be called as a method on its first argument, which allows chaining:

```rhai
to_int(e.port)  ==  e.port.to_int()
e.domain = e.url.extract_domain().to_lower().strip();
```

Your own functions (inline `fn` or `--include`) must be called function-style: `is_problem(e)`, not `e.is_problem()` (`Function not found`). A `fn` defined in one `--exec` is not visible in the next; use `--include` to share it.

## Strings

```rhai
e.msg.to_lower()  e.msg.to_upper()   // new string
e.msg.strip()                        // new string, whitespace trimmed
e.msg.contains("timeout")
e.msg.starts_with("GET")  e.msg.ends_with(".json")
e.msg.split(" ")                     // array
e.msg.replace_regex(#"\d+"#, "N")    // new string
e.msg.extract_regex(#"user=(\w+)"#, 1)   // "" if no match
e.msg.extract_regex(#"user=(\w+)"#, 1).or_empty()   // () if no match, so the field is not set
```

!!! warning "`trim()`, `replace()`, `sort()`, `reverse()` modify in place and return `()`"
    These Rhai built-ins change the variable they are called on and return nothing.
    `e.y = e.msg.trim()` leaves `y` unset, and `e.msg.trim().to_upper()` fails.
    Use the value-returning versions: `strip()`, `replace_regex()`, `sorted()`, `reversed()`.

## Event fields

```rhai
e.level                              // top-level field
e.user.name                          // nested map
e.scores[0]   e.scores[-1]           // array index, negative counts from the end
e["user-agent"]   e[name]            // non-identifier or dynamic key (e.user-agent is subtraction)

e.status = 500;                      // set
e.password = ();                     // remove field
e = ();                              // drop the event
```

`m.key` and `m["key"]` are the same for identifier keys. On a map, bare `e.len` reads a field called `len`; use `e.len()` or `len(e)` for the field count.

### Missing fields

A missing field reads as `()`. What happens next depends on the operation:

| Expression | Result |
|---|---|
| `e.missing == "x"`, `e.missing > 5` | `false` |
| `"took " + e.missing` | `"took "` |
| `e.missing + 1` | error |
| `e.missing.to_upper()` | error |
| `e.user.role` with `user` absent | error |

Safe forms:

```rhai
e.dur ?? 0                           // default for a top-level field
e.get("dur", 0)                      // same, method style
e.get_path("user.role", "guest")     // dotted path, also "items[0].id"
e.has("dur")                         // true if present and not ()
"dur" in e                           // true if present
e.has_path("user.role")
```

## Arrays and maps

```rhai
e.tags.len()   e.tags.is_empty()   e.tags.contains("x")
e.tags.join(", ")
sorted(e.scores)                     // new array, numeric or lexicographic
reversed(e.items)
unique(e.tags)                       // keeps first occurrence
sorted_by(e.users, "age")            // array of maps, by field
e.users.pluck("name")                // field from each map, skips missing
e.scores.slice("-3:")                // Python-style slice spec
e.scores.sum()   e.scores.min()   e.scores.max()   e.scores.mean()

// highest scorer first, names only
e.ranking = sorted_by(e.users, "score").reversed().pluck("name");

emit_each(e.items)                   // one event per element; the original is dropped
emit_each(e.items, #{batch: e.id})   // add fields to each

e.keys()   e.values()   e.contains("k")
```

Nested fan-out takes one `--exec` per level:

```bash
kelora -j batches.json \
  -e 'emit_each(e.batches)' \
  -e 'emit_each(e.items, #{batch_id: e.id})' \
  --filter 'e.status == "active"'
```

## Conversions

```rhai
to_int("42")   to_float("1.5")   to_bool("true")   // () if conversion fails
e.port.to_int_or(8080)                            // default instead of ()
e.price.to_float_or(0.0)
e.active.to_bool_or(false)
to_string(42)   42.to_string()
```

## Errors

```rhai
try {
    e.n = e.raw.to_int() + 1;
} catch (err) {
    eprint(err);                     // err is a map with message, line, position
}
```

Guards (`??`, `to_int_or`, `has`) are cheaper than `try`. Without `--strict`, a failing `--filter` counts as false and a failing `--exec` is rolled back (the event continues unchanged from before that stage); both are reported as warnings and the exit code stays 0. With `--strict` the first error aborts with exit code 1. Details: [When something goes wrong](../how-it-works.md#when-something-goes-wrong).

## Coming from other languages

| Habit | In Rhai |
|---|---|
| `null`, `None`, `undefined` | `()` |
| `'text'` | `"text"` (`'t'` is a single character) |
| `r"\d+"` | `#"\d+"#` |
| `if x > 5:` | `if x > 5 { ... }` |
| `x = 1` to declare | `let x = 1;` |
| `"5" + 3 == 8` | `"53"`; convert with `to_int()` |
| `arr[-3:]` | `arr.slice("-3:")` |
| `for k in dict` | `for k in map.keys()` |
| `s.trim()` returns a string | `s.strip()` |

More: [Scripting guide](../guide/scripting.md), `kelora --help-rhai`, [rhai.rs](https://rhai.rs/book/).
