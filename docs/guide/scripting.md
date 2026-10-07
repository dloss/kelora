# Transform with Scripts

When options run out, `--filter` and `--exec` (`-e`) run small
[Rhai](https://rhai.rs) scripts on each event. Rhai reads like a mix of
JavaScript and Rust; this page covers what log work needs. The
[Rhai cheatsheet](../reference/rhai-cheatsheet.md) has the full syntax, and
`kelora --help-functions KEYWORD` searches the 150+ built-in functions.

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error \
  -e 'e.duration_s = e.get("duration_ms", 0) / 1000.0; e.service = e.service.to_upper()' \
  -k timestamp,service,duration_s,message
```

## The event, `e`

Each event is a map of fields, available as `e`.

| Do | Write |
|---|---|
| read a field | `e.status`, `e["user-agent"]` (names with `-`, `@`, `.`) |
| read nested data | `e.user.id`, `e.tags[0]`, `e.tags[-1]` |
| add or change a field | `e.duration_s = e.duration_ms / 1000.0` |
| remove a field | `e.password = ()` |
| drop the whole event | `e = ()` |
| copy into a new field | `e.path_only = e.path.before("?")` |

Alongside `e`, scripts see `meta`: `meta.line` (the raw input line),
`meta.line_num`, `meta.filename`, and `meta.parsed_ts` (the parsed timestamp,
see [Work with Time](time.md)). The [script variables
reference](../reference/script-variables.md) lists everything.

## Types

JSON and logfmt values keep their types: numbers are numbers, `true` is a
boolean. Text formats — `line`, `cols:` and `csv` without type annotations,
most `regex:` groups — give you strings. `-F inspect` shows the type of every
field:

```bash exec="on" source="above" result="ansi"
kelora examples/errors_json_types.jsonl -F inspect -n 2
```

Comparing a string with a number never matches (`"500" > 400` is false), so
convert first: `to_int()` / `to_float()` return `()` when conversion fails, and
`to_int_or(0)` / `to_float_or(0.0)` return a default instead. Integer division
truncates: `e.ms / 1000` is an integer, `e.ms / 1000.0` a float.

## Missing fields

Events in the same file often have different fields. A missing field reads as
`()` — **accessing it never fails by itself.** What happens next depends on the
operation:

| Expression, when `dur` is missing | Result |
|---|---|
| `e.dur == "x"`, `e.dur > 5` | `false` — comparisons accept `()` |
| `e.dur + "ms"` | `"ms"` — string concatenation accepts `()` |
| `e.dur + 1` | error — no arithmetic on `()` |
| `e.dur.to_upper()` | error — no methods on `()` |
| `e.user.role`, when `user` is missing | error — can't look inside `()` |

Two idioms cover every case:

```rhai
if e.has("dur") { e.dur_s = e.dur / 1000.0 }   // guard first
e.dur_s = e.get("dur", 0) / 1000.0              // or read with a default
e.role = e.get_path("user.role", "guest")       // same for nested paths
```

When a script fails on an event, Kelora rolls that `--exec` back (the event
keeps its previous fields), counts the error, and continues. The summary on
stderr names the line and the problem; `--strict` stops at the first error
instead. See [errors](../how-it-works.md#when-something-goes-wrong).

## Strings

| Want | Use |
|---|---|
| test for text | `.contains("x")`, `.starts_with("x")`, `.matches("re")` |
| change case | `.to_lower()`, `.to_upper()` |
| cut around delimiters | `.before(" ")`, `.after("=")`, `.between("[", "]")` |
| split | `.split(",")` (array), `.col(2)` (third whitespace column) |
| regex capture | `.extract_regex("id=(\\d+)", 1)` |
| replace | `.replace_regex("\\d+", "N")` |
| trim | `.strip()`, `.clip()` (also strips punctuation) |
| length | `.len()` |

!!! warning "`trim` and `replace` return nothing"
    Rhai's built-in `trim()` and `replace()` change a string variable in place and
    return `()`, so `e.y = e.x.trim()` leaves `y` unset. Use `strip()` and
    `replace_regex()`, which return the new string.

`before`, `after`, `between`, and `extract_regex` return `""` when there is
nothing to return; add `.or_empty()` to get `()` instead, which skips the
assignment: `e.user = e.msg.after("user=").or_empty()` creates `user` only on
lines that have one. [Get Logs into Shape](parse.md#6-finish-the-job-in-a-script)
shows the extraction functions at work.

## Logic

```rhai
let ms = e.get("duration_ms", 0);                 // local variable
e.speed = if ms > 1000 { "slow" } else { "ok" };  // if is an expression
if e.status >= 500 { e.alert = true; } else if e.status == 404 { e.alert = false; }
e.class = switch e.status / 100 { 2 => "ok", 4 => "client", 5 => "server", _ => "other" };
for tag in e.tags { if tag == "pii" { e.redact = true; } }
```

Separate statements with `;`. Strings take double quotes, so wrap the whole
script in single quotes on the shell; `#"…"#` is a raw string, handy for
regexes with backslashes.

## Stages run in order

Every `--filter` and `--exec` is a stage, and stages run in the order you
write them. Field changes carry from one stage to the next; `let` variables
don't — each stage has its own scope:

```bash
kelora app.jsonl \
  -e 'e.ms = e.duration_ms.to_int_or(0)' \
  --filter 'e.ms > 1000' \
  -e 'e.secs = e.ms / 1000.0'       # sees e.ms, only for events that passed the filter
```

Put statements that share a `let` in one `--exec`. Splitting work across
several stages has one advantage: if a later stage fails, the fields set by
earlier stages survive.

## Nested JSON

`get_path` and `has_path` walk dotted paths with array indexes, returning a
default instead of failing:

```bash exec="on" source="above" result="ansi"
kelora examples/fan_out_batches.jsonl \
  -e 'e.first_sku = e.get_path("orders[0].items[0].sku", "none"); e.n_orders = e.get("orders", []).len()' \
  -k batch_id,n_orders,first_sku
```

`-k` selects top-level fields only, so copy nested values up first, as above.
`e.flattened()` returns the whole event as one level of keys like
`orders[0].order_id`, ready for CSV export.

### One event per array element

`emit_each(array)` replaces the current event with one event per element. The
optional second argument adds fields to each, typically the parent's IDs:

```bash exec="on" source="above" result="ansi"
kelora examples/fan_out_batches.jsonl \
  -e 'emit_each(e.orders, #{batch_id: e.batch_id})' \
  -e 'emit_each(e.items, #{batch_id: e.batch_id, order_id: e.order_id})' \
  -k batch_id,order_id,sku,qty,price -n 4
```

Each `emit_each` fans out one level, and later stages see the new events.

## Lookups

`--begin` runs once before any event; whatever it stores in `conf` is
read-only afterwards. Use it to load lookup tables:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error,critical \
  --begin 'conf.owner = #{database: "dba-team", auth: "identity", disk: "infra"}' \
  -e 'e.owner = conf.owner.get(e.service) ?? "unassigned"' \
  -k service,owner,message
```

In `--begin`, `read_lines("blocked.txt")` returns a file's lines as an array
and `read_file("map.json").parse_json()` loads a JSON map. Memory *across*
events (counters, deduplication, sessions) is different: see
[Cross-Event Logic](state.md).

## Redact before sharing

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=team-key kelora examples/web_access_large.log.gz \
  -e 'e.ip = e.ip.mask_ip(2); if e.has("user") { e.user = pseudonym(e.user, "users") }' \
  -k ip,user,path -n 3
```

`mask_ip(n)` zeroes the last n parts of an address. `pseudonym(value, domain)`
replaces a value with a keyed alias: the same input and `KELORA_SECRET` always
give the same alias, so you can still count and join on it. `hash()` gives a
plain SHA-256 digest, and `normalized()` replaces IPs, emails, UUIDs and the
like inside free text. The [cookbook](../cookbook.md#privacy) has complete recipes.

## Reuse scripts

| Option | Use for |
|---|---|
| `-E transform.rhai` | an `--exec` stage read from a file |
| `-I helpers.rhai` | function definitions for the next stage: `-I helpers.rhai --filter 'is_noise(e)'` |
| `--save-alias NAME` | save the whole command line; rerun with `-a NAME` ([Configuration](config.md)) |

## Debug a script

- `-F inspect` shows each field's type.
- `-n 5` while you iterate.
- `print(...)` and `eprint(...)` write to stdout/stderr from inside a script.
- `-v` prints every error with its line; `--strict` stops at the first one.
- `--assert 'e.has("user_id")'` reports events that break an expectation
  without filtering them.
