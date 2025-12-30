/// Print Rhai scripting guide
pub fn print_rhai_help() {
    let help_text = r###"
Rhai Language Guide:

This guide covers Rhai language fundamentals for programmers familiar with Python, JavaScript, or Bash.
For Rhai language details: https://rhai.rs

VARIABLES & TYPES:
  let x = 42;                          Variable declaration (required for new vars)
  let name = "alice";                  String (double quotes only)
  let active = true;                   Boolean (true/false)
  let tags = [1, 2, 3];                Array (dynamic, mixed types ok)
  let user = #{name: "bob", age: 30};  Map/object literal
  let empty = ();                      Unit type (Rhai's "nothing", not null/undefined)

  type_of(x)                           Returns type as string: "i64", "string", "array", "map", "()"
  x = "hello";                         Dynamic typing: variables can change type

OPERATORS:
  Arithmetic:  +  -  *  /  %  **       (power: 2**3 == 8)
  Comparison:  ==  !=  <  >  <=  >=
  Logical:     &&  ||  !
  Bitwise:     &  |  ^  <<  >>
  Assignment:  =  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=
  Range:       1..5  1..=5            (exclusive/inclusive, for loops only)
  Membership:  "key" in map            (check map key existence)

STRING INTERPOLATION:
  Rhai supports string interpolation using ${...} syntax within backtick strings:

  let name = "Alice";
  let age = 30;
  let msg = `Hello, ${name}! You are ${age} years old.`;

  Complex expressions:
  let x = 10, y = 20;
  let result = `Sum: ${x + y}, Product: ${x * y}`;

  Nested interpolations allowed:
  let status = "active";
  let msg = `User ${name} is ${`currently ${status}`}`;

  Note: Interpolation only works with backtick strings (`text`), not double quotes ("text")

RAW STRINGS:
  Wrap strings with #"..."# to disable escape sequences (perfect for regexes):

  let regex = #"\d{3}-\d{2}-\d{4}"#;       No escaping needed (vs "\\d{3}-\\d{2}-\\d{4}")
  let path = #"C:\Users\data"#;            Windows paths work naturally
  let s = ##"Contains "quotes""##;         Use multiple # to include " inside

CONTROL FLOW:
  if x > 10 {                          If-else (braces required)
      print("big");
  } else if x > 5 {
      print("medium");
  } else {
      print("small");
  }

  switch x {                           Switch expression (returns value)
      1 => "one",
      2 | 3 => "two or three",
      4..=6 => "four to six",
      _ => "other"                     (underscore = default)
  }

LOOPS:
  for i in 0..10 { print(i); }         Range loop (0..10 excludes 10, 0..=10 includes)
  for item in array { print(item); }   Array iteration
  for (key, value) in map { ... }      Map iteration

  while condition { ... }              While loop
  loop { if done { break; } }          Infinite loop (use break/continue)

FUNCTIONS & CLOSURES:
  fn add(a, b) { a + b }               Function definition (last expr is return value)
  fn greet(name) {                     Explicit return
      return "Hello, " + name;
  }

  let double = |x| x * 2;              Closure syntax
  [1,2,3].map(|x| x * 2)               Common in array methods
  [1,2,3].filter(|x| x > 1)            Predicate closures

FUNCTION-AS-METHOD SYNTAX (Rhai special feature):
  extract_regex(e.line, "\d+")            Function call style
  e.line.extract_regex("\d+")             Method call style (same thing!)

  Rhai allows calling any function as a method on its first argument.
  Use method style for chaining: e.url.extract_domain().lower().strip()

RHAI QUIRKS & GOTCHAS:
  • Strings use double quotes only: "hello" (not 'hello')
  • Semicolons recommended (optional at end of blocks, required for multiple statements)
  • No null/undefined: use unit type () to represent "nothing"
  • No implicit type conversion: "5" + 3 is error (use "5".to_int() + 3)
  • try/catch available: try { ... } catch (err) { ... } catches runtime errors (type/type-mismatch, missing fields); compile errors still abort; prefer guards/to_int_or over exceptions for speed
  • let required for new variables (x = 1 errors if x not declared)
  • Arrays/maps are reference types: modifying copies affects original
  • Last expression in block is return value (no return needed)
  • Single-line comments: // ... (multi-line: /* ... */)
  • Function calls without parens ok if no args: e.len (same as e.len())

KELORA PIPELINE STAGES:
  --begin         Pre-run once before parsing; populate global `conf` map (becomes read-only)
  --filter        Boolean gate per event (true keeps, false drops); repeatable, ordered
  --exec / -e     Transform per event; repeatable, ordered
  --exec-file     Same as --exec, reads script from file
  --end           Post-run once after processing; access global `metrics` map for reports

Prerequisites: --allow-fs-writes (file I/O), --window N (windowing), --metrics (tracking)

VARIABLE SCOPE BETWEEN STAGES:
  Each --exec stage runs in ISOLATION. Local variables (let) do NOT persist:

  WRONG:  kelora -e 'let ctx = e.id' -e 'e.context = ctx'     # ERROR: ctx undefined!
  RIGHT:  kelora -e 'let ctx = e.id; e.context = ctx'         # Use semicolons for shared vars

  What persists:   e.field modifications, conf, metrics, window
  What doesn't:    let variables, function definitions (unless from --include)

RESILIENT MODE SNAPSHOTTING:
  Each successful stage creates a snapshot. On error, event reverts to last good state:

  kelora --resilient -e 'e.safe = "ok"' -e 'e.risky = parse(e.raw)' -e 'e.done = true'
  → If parse fails, event keeps 'safe' but not 'risky', continues with 'safe' field

  Why use multiple stages:
    - Error isolation (failures don't corrupt earlier work)
    - Progressive checkpoints (partial success possible)
  Why use semicolons in one stage:
    - Share local variables
    - All-or-nothing execution (no partial results)

KELORA EVENT ACCESS:
  e                                    Current event (global variable in --filter/--exec)
  e.field                              Direct field access
  e.nested.field                       Nested field traversal (maps)
  e.scores[1]                          Array indexing (0-based, negative ok: -1 = last)
  e.headers["user-agent"]              Bracket notation for special chars in keys

  "field" in e                         Check top-level field exists
  e.has_path("user.role")              Check nested path exists (safe)
  e.get_path("user.role", "guest")     Get nested with default fallback

  e.field = ()                         Remove field (unit assignment)
  e = ()                               Remove entire event (becomes empty, filtered out)

EVENT METADATA:
  meta                                 Event metadata (global variable in --filter/--exec)
  meta.line                            Original raw line from input (always available)
  meta.line_num                        Line number (1-based, available with files)
  meta.filename                        Source filename (available when processing multiple files)
  meta.parsed_ts                       Parsed UTC timestamp before scripts (or () if missing)

  # Example: Track errors by filename
  --exec 'if e.level == "ERROR" { track_count(meta.filename) }'

  # Example: Debug with line numbers
  --filter 'e.status >= 500' --exec 'eprint("Error at line " + meta.line_num)'

ARRAY & MAP OPERATIONS:
  JSON arrays → native Rhai arrays (full functionality)
  sorted(e.scores)                     Sort numerically/lexicographically
  reversed(e.items)                    Reverse order
  unique(e.tags)                       Remove duplicates
  sorted_by(e.users, "age")            Sort objects by field
  e.tags.join(", ")                    Join to string

  emit_each(e.items)                   Fan out: each array element → separate event
  emit_each(e.items, #{ctx: "x"})      Fan out with base fields added to each

COMMON PATTERNS:
  # Safe nested access
  let role = e.get_path("user.role", "guest");

  # Type conversion with fallback
  let port = to_int_or(e.port, 8080);

  # Array safety
  if e.items.len() > 0 { e.first = e.items[0]; }

  # Conditional field removal
  if e.level != "DEBUG" { e.stack_trace = (); }

  # Method chaining
  e.domain = e.url.extract_domain().to_lower().strip();

  # Map iteration
  for (key, val) in e { print(key + " = " + val); }

GLOBAL CONTEXT:
  state                                Mutable global map for complex state tracking (sequential mode only)
                                       Use for: deduplication, storing complex objects, cross-event logic
                                       For simple counting/metrics, prefer track_*() (works in parallel too)
                                       Supports: state["key"], contains(), get(), set(), len(), is_empty(),
                                       keys(), values(), clear(), remove(), +=, mixin(), fill_with()
                                       Use state.to_map() to convert to regular map for other operations
                                       (e.g., state.to_map().to_logfmt(), state.to_map().to_kv())
                                       Note: Accessing state in --parallel mode will cause a runtime error
  conf                                 Global config map (read-only after --begin)
  metrics                              Global metrics map (from track_* calls, read in --end)
  get_env("VAR", "default")            Environment variable access

ERROR HANDLING MODES:
  Default (resilient):
    • Parse errors → skip line, continue
    • Filter errors → treat as false, drop event
    • Exec errors → rollback, keep original event
  --strict mode:
    • Any error → abort with exit code 1

SCRIPT OUTPUT (print/eprint):
  print("msg")                        Write to stdout (visible by default)
  eprint("err")                       Write to stderr (visible by default)

  Suppression: --no-script-output, -s, -m suppress print/eprint
               --silent does NOT suppress print/eprint (they still work)

  File operations (always work, requires --allow-fs-writes):
    append_file(path, content), write_file(path, content), --metrics-file

For other help topics: kelora -h
"###;
    println!("{}", help_text);
}
