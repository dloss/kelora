# Function Reference

Every built-in Rhai function, grouped by category. `kelora --help-functions` prints the same catalogue in the terminal; `kelora --help-functions <word>` searches it.

!!! tip "Call syntax"
    Rhai accepts `value.method(args)` and `function(value, args)` interchangeably: `e.msg.after(":")` is `after(e.msg, ":")`.

## Quick Navigation

- [String Functions](#string-functions) - extraction, parsing, encoding, manipulation
- [Array Functions](#array-functions) - sorting, aggregation, transformation
- [Map/Object Functions](#mapobject-functions) - field access, reshaping, serialization
- [DateTime Functions](#datetime-functions) - timestamps, durations, bucketing
- [Math Functions](#math-functions) - numeric helpers, sampling
- [Output Formatting](#output-formatting-functions) - human-readable numbers, padding, colors, bars
- [Type Conversion](#type-conversion-functions) - safe conversions
- [Utility Functions](#utility-functions) - environment, files, pseudonyms, Drain
- [State Management](#state-management-functions) - cross-event state
- [Tracking/Metrics](#trackingmetrics-functions) - counters and aggregations
- [File Output](#file-output-functions) - writing files
- [Event Manipulation](#event-manipulation) - fan-out, field removal, `absorb_*`
- [Span Context](#span-context-span-close-only) - per-span metadata and rollups

---

## String Functions

### Extraction and Searching

!!! note "Nothing found is `""`, not `()`"
    Every function in this section returns an empty string when there is no match
    (an empty array for the plural forms), not `()`. That is the opposite of the
    parser [type annotations](formats.md#regex-format), where a value that cannot
    satisfy its declared type becomes `()`.

    Chain [`or_empty()`](#valueor_empty) for the absent convention: it turns `""`
    into `()`, which removes the field on assignment, so `!= ()` filters and
    `--freq` skip those events instead of counting an empty value.

    ```rhai
    e.ip = e.msg.extract_regex(#"rhost=(\S+)"#, 1);                      // "" if no match
    e.ip = e.msg.extract_regex(#"rhost=(\S+)"#, 1).or_empty();           // no ip field at all
    e.ip = e.msg.extract_regex(#"rhost=(\S+)"#, 1).or_empty() ?? "unknown";
    ```

#### `text.extract_regex(pattern [, group])`
First regex match, or capture group `group` of it. Returns `""` if the pattern does not match or the group did not participate.

```rhai
e.error_code = e.message.extract_regex(#"ERR-(\d+)"#, 1);     // "ERR-404" → "404"
e.first_num = e.line.extract_regex(#"\d{3}"#);                // whole match
```

#### `text.extract_regexes(pattern [, group])`
All matches as an array. With `group`, one string per match; without it and with capture groups in the pattern, one array of groups per match.

```rhai
e.numbers = e.line.extract_regexes(#"\d+"#);                  // ["123", "4567"]
e.codes = e.message.extract_regexes(#"ERR-(\d+)"#, 1);        // ["404", "500"]
```

#### `text.extract_regex_maps(pattern, field)`
Matches as an array of **single-field** maps for `emit_each()`. The field is named `field` and holds capture group 1 (or the whole match if the pattern has no groups). Named groups do not become fields.

```rhai
emit_each(e.log.extract_regex_maps(#"(ERR-\d+)"#, "error_code"));
// → {"error_code": "ERR-404"}, {"error_code": "ERR-500"}
```

For several fields per match, build the maps from `extract_regexes()`:

```rhai
emit_each(e.log.extract_regexes(#"(ERR-\d+): ([^\n]+)"#).map(|g| #{code: g[0], msg: g[1]}));
// → {"code": "ERR-404", "msg": "not found"}, …
```

To turn named groups into fields of the *current* event, use [`absorb_regex()`](#eabsorb_regexfield-pattern-options).

#### `text.extract_ip([nth])` / `text.extract_ips()`
IPv4 address(es) found in text (`nth`: 1=first, -1=last). IPv6 addresses are not matched.

```rhai
e.client_ip = e.headers.extract_ip();            // first
e.origin_ip = e.forwarded.extract_ip(-1);        // last
e.all_ips = e.headers.extract_ips();             // ["10.0.0.1", "192.168.1.1"]
```

#### `text.extract_url([nth])`
URL found in text (`nth`: 1=first, -1=last).

```rhai
e.link = e.message.extract_url();                // "https://api.example.com/path?x=1"
```

#### `text.extract_email([nth])` / `text.extract_emails()`
Email address(es) found in text (`nth`: 1=first, -1=last).

```rhai
e.sender = e.log.extract_email();                // first
e.recipient = e.log.extract_email(-1);           // last
e.all = e.log.extract_emails();                  // ["alice@example.com", "bob@test.org"]
```

#### `text.extract_domain()`
Host part of a URL, or domain part of an email address.

```rhai
e.host = "https://api.example.com/path".extract_domain();   // "api.example.com"
e.mail_domain = "user@corp.example.com".extract_domain();   // "corp.example.com"
```

#### `text.extract_json([nth])` / `text.extract_jsons()`
Find JSON objects or arrays embedded in text. `extract_json()` returns the `nth` one (1=first, -1=last) already **parsed** into a map or array (`""` if none); `extract_jsons()` returns all of them as an array of JSON **strings**.

```rhai
// 'pre {"a":1} mid [1,2] end {"b":2}'
e.data = e.msg.extract_json();                   // #{a: 1}
e.last = e.msg.extract_json(-1);                 // #{b: 2}
e.raw = e.msg.extract_jsons();                   // ["{\"a\":1}", "[1,2]", "{\"b\":2}"]
```

### String Slicing and Position

#### `text.before(delimiter [, nth])` / `text.after(delimiter [, nth])`
Text before/after an occurrence of `delimiter` (`nth`: 1=first, -1=last).

```rhai
e.user = e.email.before("@");                    // "user@host.com" → "user"
e.path = e.url.before("?");                      // strip query string
e.ext = e.file.after(".", -1);                   // "app.tar.gz" → "gz"
```

#### `text.between(start, end [, nth])`
Text between two delimiters; same as `text.after(start, nth).before(end)`.

```rhai
e.quoted = e.line.between("\"", "\"");           // first quoted string
e.second = "[a][b][c]".between("[", "]", 2);     // "b"
```

#### `text.starting_with(prefix [, nth])` / `text.ending_with(suffix [, nth])`
Substring from `prefix` to the end, or from the start through `suffix`.

```rhai
e.err = e.log.starting_with("ERROR:");           // "INFO: ok ERROR: bad" → "ERROR: bad"
e.file = e.log.ending_with(".txt");              // "file.txt more" → "file.txt"
```

#### `text.slice(spec)`
Python-style slice: `"1:5"`, `":3"`, `"-2:"`, `"::2"`.

```rhai
e.head = e.code.slice(":3");                     // "ABCDEF" → "ABC"
e.tail = e.code.slice("-2:");                    // "EF"
e.mid = e.code.slice("2:5");                     // "CDE"
```

#### `text.sub_string(start [, length])`
Rhai builtin: substring from 0-based character position `start`.

```rhai
e.rest = e.code.sub_string(2);                   // "ABCDEF" → "CDEF"
e.part = e.code.sub_string(1, 3);                // "BCD"
```

### Column Extraction

#### `text.col(spec [, separator [, out_separator]])`
Select whitespace-separated (or `separator`-separated) columns by 0-based index, list or range (`"0"`, `"0,2,4"`, `"1:4"`, `":2"`). Multiple columns are joined with a space, or with `out_separator`.

```rhai
e.first = e.line.col("0");
e.picked = e.line.col("0,2,4");                  // "a c e"
e.range = e.line.col("1:4", "\t");               // columns 1-3 of a tab-separated line
e.csvish = e.line.col("0,2", "|", ",");          // "a|b|c" → "a,c"
```

#### `text.cols(col1, col2 [, ...] [, separator])`
Up to six 0-based column indices, returned as an array.

```rhai
let parts = e.line.cols(0, 2, 4);                // ["a", "c", "e"]
e.user = parts[0];
let tabbed = e.line.cols(1, 3, "\t");
```

#### `text.parse_cols(spec [, separator])` / `array.parse_cols(spec [, join_sep])`
Map columns to named fields with the same spec language as `-f 'cols:<spec>'` (see [Column Format](formats.md#column-format)): `name` takes one column, `name(N)` joins N columns, `-` / `-(N)` skips, and a final `*name` keeps the rest of the line verbatim. Missing columns become `()` (an error under `--strict`). The array form applies the spec to values you already split; `join_sep` joins multi-column fields (default space). Type suffixes like `age:int` are not supported here.

```rhai
// "2025-09-22 12:33:44 -- INFO hello   world"
e += e.line.parse_cols("ts(2) - level *msg");    // ts="2025-09-22 12:33:44", level="INFO", msg="hello   world"
let m = e.raw.parse_cols("host - status *rest", "|");
let n = ["a", "b"].parse_cols("x y z");          // #{x: "a", y: "b", z: ()}
```

### Parsing Functions

Each `parse_*` returns a map you can index or merge into the event (`e += text.parse_logfmt()`). On input they cannot parse they return an empty map — except `parse_json()`, which raises an error. To parse a field and merge it into the event in one step, with a status report, see the [`absorb_*` functions](#event-manipulation).

#### `text.parse_json()`
Parse a JSON string into a map or array. Invalid JSON is a runtime error.

```rhai
e.data = e.payload.parse_json();
e.value = e.data["key"];
```

#### `text.parse_logfmt()`
Parse a logfmt line; quote-aware, and numbers and booleans are typed.

```rhai
let f = e.line.parse_logfmt();                   // 'level=info msg="hi there" n=3'
e.level = f["level"];                            // n is the integer 3
```

#### `text.parse_syslog()`
Parse an RFC 3164/5424 syslog line. Keys: `pri`, `facility`, `severity`, `level`, `ts`, `host`, `prog`, `pid`, `msg` (5424 adds `version`, `msgid`).

```rhai
let s = e.line.parse_syslog();
e.prog = s["prog"];                              // "su"
e.message = s["msg"];
```

#### `text.parse_combined()`
Parse an Apache/Nginx combined log line. Keys: `ip`, `user`, `ts`, `request`, `method`, `path`, `protocol`, `status` (int), `bytes` (int), `referer`, `user_agent`.

```rhai
let a = e.line.parse_combined();
e.ip = a["ip"];
e.status = a["status"];
```

#### `text.parse_cef()`
Parse a Common Event Format line. Header keys: `cefver`, `vendor`, `product`, `version`, `eventid`, `event`, `severity`; extension keys are added as-is.

```rhai
let cef = e.line.parse_cef();
e.severity = cef["severity"];
```

#### `text.parse_kv([sep [, kv_sep]])`
Split `key=value` pairs (defaults: whitespace and `=`). Tokens without `kv_sep` are skipped. **Not quote-aware**: quotes stay on values and a separator inside quotes splits the value; use [`parse_logfmt()`](#textparse_logfmt) for `key="value with spaces"`.

```rhai
e.params = e.query.parse_kv("&", "=");           // "a=1&b=2" → {a: "1", b: "2"}
e.fields = e.msg.parse_kv();                     // "Payment timeout order=1234" → {order: "1234"}
```

#### `text.parse_url()`
Parse a URL. Keys: `scheme`, `user`, `host`, `port`, `path`, `query`, `query_map`, `fragment` (present when the URL has them).

```rhai
let u = e.request.parse_url();
e.host = u["host"];
e.id = u["query_map"]["id"];
```

#### `text.parse_query_params()`
Parse a query string (leading `?` optional) into a map.

```rhai
e.params = e.query_string.parse_query_params();  // "a=1&b=2" → {a: "1", b: "2"}
```

#### `text.parse_email()`
Split a bare address into `local` and `domain`. Display-name forms like `Name <addr>` return an empty map.

```rhai
let m = "user@example.com".parse_email();
e.local = m["local"];                            // "user"
e.domain = m["domain"];                          // "example.com"
```

#### `text.parse_user_agent()`
Parse common user-agent strings. Keys: `agent_family`, `agent_version`, `os_family`, `os_version`, `device`.

```rhai
let ua = e.user_agent.parse_user_agent();
e.browser = ua["agent_family"];                  // "Chrome"
e.os = ua["os_family"];                          // "Windows"
```

#### `text.parse_jwt()`
Decode a JWT **without verifying** it. Returns `header`, `claims` (the payload), `signature_b64u`, plus `alg`/`kid`/`typ` when the header has them. The NumericDate claims are also exposed as datetime values:

| Datetime field | Source claim | Meaning |
| --- | --- | --- |
| `expires_at` | `claims.exp` | Expiration time |
| `issued_at` | `claims.iat` | Issued-at time |
| `not_before` | `claims.nbf` | Not-valid-before time |

Each is present only when its claim is a valid numeric date. They compare chronologically and subtract to durations; the raw integers stay under `claims`. Render with `.to_iso()` or `.format()` rather than comparing as strings.

```rhai
let jwt = e.token.parse_jwt();
e.user_id = jwt["claims"]["sub"];
e.expired = jwt.expires_at < now();                         // bool
e.lifetime = (jwt.expires_at - jwt.issued_at).to_string();  // "1h"
e.exp_iso = jwt.expires_at.to_iso();                        // "2025-01-01T00:00:00+00:00"
e.exp_raw = jwt.claims.exp;                                 // 1735689600
```

#### `text.parse_path()`
Parse a filesystem path. Keys: `input`, `root`, `parent`, `file_name`, `stem`, `extension`, `components`, `is_absolute`, `is_relative`, `has_root`.

```rhai
let p = "/var/log/app.log".parse_path();
e.dir = p["parent"];                             // "/var/log"
e.file = p["file_name"];                         // "app.log"
```

#### `text.parse_media_type()`
Parse a media type into `type`, `subtype` and `params`.

```rhai
let mt = "text/html; charset=utf-8".parse_media_type();
e.subtype = mt["subtype"];                       // "html"
e.charset = mt["params"]["charset"];             // "utf-8"
```

#### `text.parse_content_disposition()`
Parse a Content-Disposition header into `disposition`, `params`, and `filename` when present.

```rhai
let cd = e.header.parse_content_disposition();   // 'attachment; filename="report.pdf"'
e.filename = cd["filename"];                     // "report.pdf"
```

### Encoding and Hashing

#### `text.encode_b64()` / `text.decode_b64()`
Base64. Decoding invalid input is a runtime error.

```rhai
e.encoded = e.data.encode_b64();                 // "hello world" → "aGVsbG8gd29ybGQ="
e.decoded = e.payload.decode_b64();
```

#### `text.encode_hex()` / `text.decode_hex()`
Hexadecimal.

```rhai
e.hex = e.data.encode_hex();                     // "hello" → "68656c6c6f"
e.text = e.hex_string.decode_hex();
```

#### `text.encode_url()` / `text.decode_url()`
URL percent-encoding.

```rhai
e.encoded = e.param.encode_url();                // "hello world" → "hello%20world"
e.decoded = e.url_param.decode_url();
```

#### `text.escape_json()` / `text.unescape_json()`
JSON string escapes (`\"`, `\n`, …).

```rhai
e.escaped = e.text.escape_json();
e.unescaped = e.json_string.unescape_json();
```

#### `text.escape_html()` / `text.unescape_html()`
HTML entities for `&`, `<`, `>`, `"`, `'`.

```rhai
e.safe = e.user_input.escape_html();             // "<script>" → "&lt;script&gt;"
e.text = e.html_entity.unescape_html();
```

#### `text.hash([algo])`
Hex digest; `algo` is `"sha256"` (default) or `"xxh3"` (fast, non-cryptographic). Any other name is an error. The hash is unkeyed, so low-entropy values (IPs, user names) can be recovered by hashing candidates; [`pseudonym()`](#pseudonymvalue-domain) is keyed.

```rhai
e.checksum = e.content.hash();                   // SHA-256, 64 hex chars
e.fast = e.data.hash("xxh3");                    // 16 hex chars
```

#### `text.bucket()`
Fast integer hash for deterministic sampling and sharding. The result can be negative, so compare `% n` against `0` rather than expecting `0..n-1`.

```rhai
if e.user_id.bucket() % 10 == 0 {               // same ~10% of users on every run
    e.sampled = true;
}
```

### IP Address Functions

#### `text.is_ipv4()` / `text.is_ipv6()`
Whether text is a valid IPv4/IPv6 address.

```rhai
if e.addr.is_ipv4() { e.ip_version = 4 }
```

#### `text.is_private_ip()`
True for RFC 1918 IPv4, loopback, IPv6 unique local (`fc00::/7`) and link-local (`fe80::/10`). IPv4 link-local (`169.254.0.0/16`) is not counted.

```rhai
if e.ip.is_private_ip() { e.internal = true }
```

#### `text.is_in_cidr(cidr)`
Whether the address is inside a CIDR network.

```rhai
if e.ip.is_in_cidr("10.0.0.0/8") { e.corp_network = true }
```

#### `text.mask_ip([octets])`
Zero the last `octets` IPv4 octets (default 1) or the last `octets` IPv6 groups. Non-IP text is returned unchanged.

```rhai
e.masked = e.client_ip.mask_ip();                // "192.168.1.100" → "192.168.1.0"
e.net16 = e.client_ip.mask_ip(2);                // "192.168.1.100" → "192.168.0.0"
e.v6 = e.ip6.mask_ip(2);                         // "2001:db8:1:2:3:4:5:6" → "2001:db8:1:2:3:4::"
```

### Pattern Normalization

#### `text.normalized([patterns])` / `map.normalized([patterns])`
Replace variable data with placeholders such as `<ipv4>` or `<email>`, to group messages by shape. `patterns` is a CSV string or an array. On a map, every string value is normalized and other values are left alone.

```rhai
e.pattern = e.message.normalized();
// "User user@test.com from 192.168.1.5" → "User <email> from <ipv4>"
e.simple = e.message.normalized("ipv4,email");
e.pii = e.message.normalized(["credit_card", "ssn", "phone"]);
let clean = e.normalized();                      // every string field
```

**Default patterns:** `ipv4_port`, `ipv4`, `ipv6`, `email`, `url`, `fqdn`, `uuid`, `mac`, `md5`, `sha1`, `sha256`, `path`, `oauth`, `function`, `hexcolor`, `version`

**Opt-in patterns:** `hexnum`, `duration`, `num`, `credit_card`, `ssn`, `phone`. The PII ones are off by default on purpose:

- `credit_card` - Luhn-validated card numbers
- `ssn` - US SSNs in strict `XXX-XX-XXXX` form (hyphens required); rejects area 000, 666, 900–999, group 00 and serial 0000
- `phone` - NANP-aware for US/CA numbers, permissive for other international numbers

Count message shapes with `kelora -j app.jsonl -q -e 'track_freq("pattern", e.message.normalized())' --metrics`, or let `--drain` mine templates.

### String Manipulation

!!! warning "`replace()` and `trim()` change the string in place and return `()`"
    These two Rhai builtins mutate the variable or field they are called on and
    return nothing. `e.clean = e.msg.trim()` rewrites `e.msg` and leaves `e.clean`
    unset. Call them as statements (`e.msg.trim();`), or use the functions below
    that return a new string: [`strip()`](#textstripchars-textlstripchars-textrstripchars)
    instead of `trim()`, [`replace_regex()`](#textreplace_regexpattern-replacement)
    instead of `replace()`.

#### `text.strip([chars])` / `text.lstrip([chars])` / `text.rstrip([chars])`
New string with whitespace, or any of the characters in `chars`, removed from both ends / the left / the right.

```rhai
e.clean = e.text.strip();                        // "  hi  " → "hi"
e.body = e.line.lstrip("# ");                    // "# # comment" → "comment"
e.dir = e.path.rstrip("/");                      // "/a/b//" → "/a/b"
```

#### `text.clip()` / `text.lclip()` / `text.rclip()`
Remove non-alphanumeric characters from both ends / the left / the right.

```rhai
e.word = "'hello!'".clip();                      // "hello"
e.left = "...start".lclip();                     // "start"
e.right = "end...".rclip();                      // "end"
```

#### `text.to_upper()` / `text.to_lower()` / `text.upper()` / `text.lower()`
Case conversion; returns a new string. `upper()`/`lower()` are aliases.

```rhai
e.cc = e.country_code.to_upper();                // "us" → "US"
e.name = e.name.lower();                         // "Hello" → "hello"
```

#### `text.replace_regex(pattern, replacement)`
New string with every regex match replaced. `$1` or `${name}` in `replacement` refer to capture groups.

```rhai
e.masked = e.msg.replace_regex(#"\d+"#, "#");            // "a1b22" → "a#b#"
e.tagged = e.msg.replace_regex(#"(\d+)"#, "<$1>");       // "a1b22" → "a<1>b<22>"
e.level = e.level.replace_regex("ERROR", "WARN");        // plain words work as patterns
```

#### `text.replace(find, replacement)` / `text.trim()`
Rhai builtins that modify the string **in place** (literal replace of all occurrences; trim surrounding whitespace) and return `()`. See the warning above.

```rhai
e.msg.replace("ERROR", "WARN");                 // rewrites e.msg
e.msg.trim();
```

#### `text.split(separator)` / `text.split_regex(pattern)`
Split into an array on a literal separator or a regex. (The old `split_re`, `replace_re` and `extract_re_maps` names were removed.)

```rhai
e.parts = e.path.split("/");
e.tokens = e.line.split_regex(#"\s+"#);          // "a  b\tc" → ["a", "b", "c"]
```

### String Testing

#### `text.contains(substring)`
Literal substring test.

```rhai
if e.message.contains("timeout") { e.timeout_error = true }
```

#### `text.like(pattern)` / `text.ilike(pattern)`
Glob match against the **whole** string with `*` and `?`. `ilike()` is case-insensitive with Unicode folding (`"STRASSE".ilike("*straße*")` is true).

```rhai
if e.message.like("ERROR * timeout") { e.timeout_error = true }
if e.city.ilike("*straße*") { e.locale = "de" }
```

#### `text.matches(pattern)`
Unanchored regex search; compiled patterns are cached per thread. An invalid pattern is an error.

```rhai
if e.path.matches(#"^/api/[^/]+/details$"#) { e.route = "details" }
```

| Function | Anchored | Invalid pattern | Case handling |
|----------|----------|-----------------|---------------|
| `like()` | Yes | n/a (glob) | Exact |
| `ilike()`| Yes | n/a (glob) | Unicode fold |
| `matches()` | No | Error | Regex-driven (`(?i)` for insensitive) |

Avoid nested quantifiers like `(.*)*` in hot paths.

#### `text.is_digit()`
True if text is non-empty and all ASCII digits (`"-1"` and `""` are false).

```rhai
if e.status.is_digit() { e.status_code = e.status.to_int() }
```

#### `text.count(substring)`
Number of non-overlapping literal occurrences.

```rhai
e.error_count = e.log.count("ERROR");
```

#### `text.edit_distance(other)`
Levenshtein distance.

```rhai
if e.message.edit_distance("connection reset") <= 3 { e.is_connection_issue = true }
```

#### `text.index_of(substring [, start])`
0-based position of a literal substring, or -1. `start` sets where the search begins.

```rhai
e.q = e.url.index_of("?");
e.next = e.text.index_of("test", 10);
```

#### `text.len`
Length in characters (a property, no parentheses).

```rhai
if e.msg.len > 200 { e.long = true }
```

---

## Array Functions

### Sorting and Filtering

#### `array.sorted()` / `array.sort()`
`sorted()` returns a new array sorted numerically or lexicographically. The builtin `sort()` sorts **in place** and returns `()`.

```rhai
e.ordered = e.scores.sorted();                  // [3, 1, 2] → [1, 2, 3]
e.scores.sort();                                // e.scores is now sorted
```

#### `array.sorted_by(field)`
New array of maps sorted ascending by `field`.

```rhai
e.oldest = e.users.sorted_by("age")[-1];
```

#### `array.reversed()`
New array in reverse order.

```rhai
e.newest_first = e.items.reversed();
```

#### `array.slice(spec)`
Python-style slice (`"1:5"`, `":3"`, `"-2:"`, `"0::2"`).

```rhai
e.top3 = e.values.slice(":3");                   // [9, 8, 7, 6] → [9, 8, 7]
e.tail = e.values.slice("-2:");                  // [7, 6]
e.every_other = e.values.slice("0::2");          // [9, 7]
```

#### `array.unique()`
Remove duplicates, keeping first occurrences.

```rhai
e.tags = e.tags.unique();                        // [1, 2, 1, 3] → [1, 2, 3]
```

#### `array.filter(|item| condition)`
Keep elements for which the closure returns true.

```rhai
e.errors = e.logs.filter(|log| log.level == "ERROR");
```

### Aggregation

`max`, `min`, `sum`, `mean`, `variance` and `stddev` reject mixed-type arrays and do not convert strings to numbers; `pluck_as_nums()` gives clean numbers.

#### `array.max()` / `array.min()`
Largest/smallest value; `()` for an empty or mixed-type array.

```rhai
e.max_score = e.scores.max();
```

#### `array.percentile(pct)`
Interpolated percentile, `pct` on a 0–100 scale (unlike `track_percentiles()`, which takes 0–1). Numeric strings are converted. An empty array is an error.

```rhai
e.p95 = e.latencies.percentile(95);
e.median = e.latencies.percentile(50);
```

#### `array.sum()`
Sum as a float; `()` for an empty or mixed-type array.

```rhai
e.total = [10, 20.5, 30].sum();                  // 60.5
e.bad = [10, "20"].sum();                        // () — field not set
```

#### `array.mean()` / `array.variance()` / `array.stddev()`
Arithmetic mean, population variance, population standard deviation. Unlike `sum()`, an empty or mixed-type array is a **runtime error**.

```rhai
e.avg = [10, 20, 30].mean();                     // 20.0
e.sd = e.latencies.stddev();
```

#### `array.reduce(|acc, item| expr, init)`
Fold into one value.

```rhai
e.total = e.amounts.reduce(|sum, x| sum + x, 0);
```

### Transformation

#### `array.map(|item| expression)`
Transform each element.

```rhai
e.doubled = e.numbers.map(|n| n * 2);
```

#### `array.pluck(field)` / `array.pluck_as_nums(field)` {#arraypluckfield--arraypluck_as_numsfield}
Pull one field out of each map. `pluck()` skips elements where it is missing or `()`; `pluck_as_nums()` also converts to float and skips values that do not convert. (`map(|x| x.field)` keeps `()` placeholders instead.)

```rhai
let events = [#{status: 200, time: "1.5"}, #{status: 404, time: "0.3"}, #{time: "x"}];
let statuses = events.pluck("status");           // [200, 404]
let times = events.pluck_as_nums("time");        // [1.5, 0.3]
e.avg_time = times.mean();
```

#### `array.flattened([style [, max_depth]])`
Flatten nested arrays/maps into a single-level map. See [`map.flattened()`](#mapflattenedstyle-max_depth) for styles.

```rhai
e.flat = [[1, 2], [3, 4]].flattened();           // {"[0][0]": 1, "[0][1]": 2, "[1][0]": 3, "[1][1]": 4}
```

### Testing

#### `array.contains(value)`
Whether the array holds `value`.

```rhai
if e.roles.contains("admin") { e.is_admin = true }
```

#### `array.contains_any(values)`
Whether any of `values` is in the array.

```rhai
if e.tags.contains_any(["error", "critical"]) { e.alert = true }
```

#### `array.starts_with_any(values)`
Whether the **first element** equals any of `values`.

```rhai
if e.path_parts.starts_with_any(["api", "v1"]) { e.api_call = true }
```

#### `array.all(|item| condition)` / `array.some(|item| condition)`
Whether every / at least one element matches.

```rhai
e.all_valid = e.scores.all(|s| s >= 0);
e.has_errors = e.logs.some(|l| l.level == "ERROR");
```

### Other Operations

#### `array.join(separator)`
Join string elements. **Non-string elements are dropped**, so convert first.

```rhai
e.path = e.parts.join("/");
e.csv = e.codes.map(|c| c.to_string()).join(",");  // [1, 2] → "1,2"
```

#### `array.push(item)` / `array.pop()` / `array.len`
Append in place; remove and return the last item; length.

```rhai
e.tags.push("new_tag");
let last = e.items.pop();
e.n = e.items.len;
```

---

## Map/Object Functions

### Field Access

#### `map.get("key" [, default])`
Top-level field, or `default` when it is missing or `()` (`()` without a default).

```rhai
e.user = e.get("user", "anonymous");
```

#### `map.get_path("field.path" [, default])`
Nested access with a fallback. Array elements use brackets: `"items[0].id"`.

```rhai
e.user_name = e.get_path("user.profile.name", "unknown");
e.second = e.get_path("metadata.tags[1]");
```

#### `map.has_path("field.path")`
Whether a nested path exists.

```rhai
if e.has_path("error.details.code") { e.detailed_error = true }
```

#### `map.path_equals("path", value)`
Nested comparison that is false (not an error) when the path is missing.

```rhai
if e.path_equals("user.role", "admin") { e.elevated = true }
```

#### `map.has("key")` / `map.contains("key")`
`has()` is true when the key exists and its value is not `()`; the builtin `contains()` ignores the value.

```rhai
if e.has("error_code") { e.failed = true }
```

### Field Manipulation

#### `map.keep(["field1", ...])` / `map.drop(["field1", ...])`
New map with only / without the listed top-level fields. Names match exactly (no paths or wildcards); missing names are ignored; `e` itself is not changed.

```rhai
e = e.keep(["service", "level", "msg"]);
let trimmed = e.drop(["password", "token"]);
```

#### `map.rename_field("old", "new")`
Rename in place; returns `true` if `old` existed.

```rhai
e.rename_field("old_name", "new_name");
```

#### `map.merge(other)` / `map.enrich(other)`
Copy keys from `other` into the map in place. `merge()` overwrites existing keys; `enrich()` only adds missing ones. `e += other` is the same as `merge()`.

```rhai
e.merge(#{status: "ok"});
e.enrich(#{user: "default", level: "info"});
```

#### `map.flattened([style [, max_depth]])`
New single-level map. `style` is `"bracket"` (default: `a.b` for maps, `a[0]` for arrays), `"dot"` (`a.b.0`) or `"underscore"` (`a_b_0`). `max_depth` caps how many key levels are joined (0 = unlimited); deeper values stay nested.

```rhai
let flat = e.nested.flattened();                 // {a: {b: [1]}} → {"a.b[0]": 1}
let two = e.nested.flattened("dot", 2);          // {a: {b: {c: 1}}} → {"a.b": {c: 1}}
```

#### `map.flatten_field("field_name")`
Flatten one nested field into dotted keys prefixed with the field name; the rest of the map is not included.

```rhai
e.flat_user = e.flatten_field("user");          // {user: {a: {b: 1}, c: 2}} → {"user.a.b": 1, "user.c": 2}
```

#### `map.unflatten([separator])`
Rebuild nesting from flat keys (default separator `"_"`).

```rhai
e.nested = e.flat.unflatten(".");                // {"a.b": 1, "a.c": 2} → {a: {b: 1, c: 2}}
```

### Format Conversion

#### `map.to_json([indent])`
JSON string; `indent` > 0 pretty-prints with that many spaces.

```rhai
e.json = e.to_json();
e.readable = e.details.to_json(2);
```

#### `map.to_logfmt()`
Logfmt string; values with spaces are quoted.

```rhai
e.line = e.fields.to_logfmt();                   // {a: 1, b: "x y"} → 'a=1 b="x y"'
```

#### `map.to_kv([sep [, kv_sep]])`
`key=value` string (defaults: space and `=`).

```rhai
e.query = e.params.to_kv("&", "=");              // {a: 1, b: 2} → "a=1&b=2"
```

#### `map.to_syslog()` / `map.to_cef()` / `map.to_combined()`
Render a map as a log line, with defaults for missing parts. The key names they read differ from what the matching `parse_*` functions produce:

- `to_syslog()`: `priority`, `timestamp`, `hostname`/`host`, `tag`/`program`/`ident`, `message`/`msg`/`content`
- `to_cef()`: `deviceVendor`/`device_vendor`, `deviceProduct`, `deviceVersion`, `signatureId`/`event_id`, `name`/`event_name`/`message`, `severity`/`level`; all other keys become extensions
- `to_combined()`: `ip`/`remote_addr`/`client_ip`, `user`, `timestamp`, `request` or `method`+`path`+`protocol`, `status`, `bytes`, `referer`, `user_agent`, `request_time`

```rhai
e.syslog_line = e.to_syslog();                   // "<13>Oct 07 08:21:57 h kelora: hello"
e.access_log = e.to_combined();
```

---

## DateTime Functions

The event's own timestamp is already parsed: **`meta.parsed_ts`** is a UTC datetime (or `()` if the event has none). Event fields like `e.timestamp` are plain strings; convert other fields with `to_datetime()`. Assigning a datetime to a field stores it as an ISO 8601 string. `type_of()` returns a Rust type path ending in `DateTimeWrapper` / `DurationWrapper`.

### Creation

#### `now()`
Current time (UTC).

```rhai
e.processed_at = now();
```

#### `to_datetime(text [, fmt [, tz]])`
Parse a string into a datetime. Without `fmt` the format is auto-detected; with `fmt` (chrono syntax, see [Time Reference](time-reference.md)) only that format is tried. Unparseable text is a runtime error. `tz` is the zone a time without an offset was recorded in (default UTC): `to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S", "Europe/Berlin")` is 10:30 Berlin time, 09:30 UTC. A time with an offset keeps it.

```rhai
e.start = to_datetime(e.start_time);                       // auto-detect
e.parsed = to_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S");
```

#### `to_duration(text)`
Parse a duration such as `"5m"`, `"1h30m"`, `"2d"` or `"1 hour 30 minutes"`.

```rhai
e.deadline = now() + to_duration("5m");
```

#### `duration_from_seconds(n)` / `_minutes` / `_hours` / `_days` / `_milliseconds` / `_nanoseconds`
Duration from an integer count of that unit (`duration_from_seconds(90)`, …). Floats are not accepted.

```rhai
let hour = duration_from_hours(1);
let ms = duration_from_milliseconds(1500);
```

### Formatting

#### `dt.to_iso()`
ISO 8601 / RFC 3339 string with a numeric offset.

```rhai
e.iso = meta.parsed_ts.to_iso();                 // "2024-01-15T10:34:56+00:00"
```

#### `dt.format("format_string")`
Format with chrono `%` codes (see `--help-time`).

```rhai
e.date = meta.parsed_ts.format("%Y-%m-%d");      // "2024-01-15"
e.time = meta.parsed_ts.format("%H:%M:%S");      // "10:34:56"
```

#### `dt.ts_nanos()`
Unix timestamp in nanoseconds (int).

```rhai
e.ns = meta.parsed_ts.ts_nanos();                // 1705314896000000000
```

### Component Extraction

#### `dt.year()`, `dt.month()`, `dt.day()`, `dt.hour()`, `dt.minute()`, `dt.second()`
Integer components in the datetime's own timezone.

```rhai
e.hour = meta.parsed_ts.hour();
e.local_hour = meta.parsed_ts.to_timezone("America/New_York").hour();
```

### Timezone Conversion

#### `dt.to_utc()` / `dt.to_local()` / `dt.to_timezone("tz_name")`
Same instant in UTC, the system's local zone, or a named IANA zone.

```rhai
e.ny_time = meta.parsed_ts.to_timezone("America/New_York");  // "2024-01-15T05:34:56-05:00"
e.local = meta.parsed_ts.to_local();
```

#### `dt.timezone_name()`
Zone name, e.g. `"UTC"` or `"America/New_York"`.

```rhai
e.tz = meta.parsed_ts.timezone_name();
```

### Time Bucketing

#### `dt.round_to("interval")` / `dt.ceil_to("interval")`
Round down / up to an interval boundary (`"5m"`, `"1h"`, `"1d"`, …). A timestamp already on a boundary stays unchanged.

```rhai
e.bucket = meta.parsed_ts.round_to("5m");                      // 10:34:56 → 10:30:00
e.bucket_end = meta.parsed_ts.ceil_to("1h");                   // 10:34:56 → 11:00:00
track_freq("per_hour", meta.parsed_ts.round_to("1h"));
e.day = to_datetime(e.created).round_to("1d").format("%Y-%m-%d");
```

### Arithmetic and Comparison

#### `dt + duration`, `dt - duration`, `dt1 - dt2`
Shift a datetime, or get the duration between two. `dt1 - dt2` is always non-negative regardless of order.

```rhai
e.expires = meta.parsed_ts + duration_from_hours(1);
e.elapsed_ms = (meta.parsed_ts - to_datetime(e.start_time)).as_milliseconds();
```

#### `==`, `!=`, `<`, `>`, `<=`, `>=`
Compare datetimes with datetimes, durations with durations.

```rhai
if meta.parsed_ts > to_datetime("2024-01-01") { e.recent = true }
if to_duration("90m") > to_duration("1h") { e.long = true }
```

### Duration Operations

#### `duration.as_seconds()` / `as_milliseconds()` / `as_nanoseconds()` / `as_minutes()` / `as_hours()` / `as_days()`
Whole units as an integer (truncated: 90 minutes `.as_hours()` is 1).

```rhai
let d = to_duration("1h30m");
e.secs = d.as_seconds();                         // 5400
e.mins = d.as_minutes();                         // 90
```

#### `duration + duration`, `duration - duration`
Add or subtract; subtraction always returns a non-negative duration.

```rhai
e.total = (to_duration("1h30m") + to_duration("30m")).to_string();  // "2h"
```

#### `duration.to_string()` / `humanize_duration(ms)`
Compact text with at most two units, truncated to whole seconds (`"1h 30m"`, `"1m 30s"`, `"0s"` for 250 ms). `humanize_duration()` takes milliseconds.

```rhai
e.readable = to_duration("90m").to_string();     // "1h 30m"
e.humanized = humanize_duration(5400000);        // "1h 30m"
```

#### `duration.to_debug()`
Currently identical to `to_string()`. Use `as_milliseconds()` or `as_nanoseconds()` for exact values.

---

## Math Functions

#### `abs(x)`
Absolute value (int or float).

```rhai
e.magnitude = abs(e.delta);
```

#### `clamp(value, min, max)`
Constrain to a range. All three arguments must be ints, or all floats.

```rhai
e.bounded = clamp(e.score, 0, 100);
e.ratio = clamp(e.r, 0.0, 1.0);
```

#### `floor(x)` / `round(x)`
Rhai builtins for **floats only**; they return a float. Integer input is an error ("Function not found"); add `.to_int()` to get an integer back.

```rhai
e.floored = floor(e.latency);                    // -3.7 → -4.0
e.rounded = round(e.latency).to_int();           // 2.5 → 3
```

#### `mod(a, b)` / `a % b`
Integer remainder. `mod(a, 0)` returns 0; the `%` operator raises an error on zero.

```rhai
e.shard = mod(e.id, 10);
```

#### `rand()` / `rand_int(min, max)`
Random float in [0, 1), or integer in [min, max] inclusive.

```rhai
e.random_id = rand_int(1000, 9999);
```

Set `KELORA_SEED` to a non-negative integer to make `rand()`, `rand_int()` and `sample_prob()` reproducible. This holds in sequential mode; under `--parallel`, scheduling still decides which worker draws which value.

#### `sample_every(n)`
True on the Nth, 2Nth, 3Nth… call. Each distinct `n` has its own counter; counters are per thread, so the rate is approximate under `--parallel`.

```rhai
if !sample_every(100) { skip() }                // keep every 100th event
```

#### `sample_prob(p)`
True with probability `p` (0.0–1.0).

```rhai
if !sample_prob(0.01) { skip() }                // keep ~1%
```

| Method | Kept fraction | Same events on every run? |
|---|---|---|
| `sample_every(n)` | exactly 1/n (approximate in parallel) | yes, sequentially |
| `sample_prob(p)` | about p | only with `KELORA_SEED` |
| `e.field.bucket() % n == 0` | about 1/n, per field value | yes, also in parallel |

---

## Output Formatting Functions

String-returning helpers for `print`, `eprint`, inline fields and `--end` reports.

#### `human_bytes(n)` / `human_bytes_si(n)`
Byte counts with binary units (1024: `KiB`, `MiB`, …) or SI units (1000: `KB`, `MB`, …).

```rhai
e.size = human_bytes(1536);                      // "1.5 KiB"
e.size_si = human_bytes_si(1_500_000_000);       // "1.5 GB"
```

#### `format_decimals(value, decimals)`
Exactly N decimals. Negative `decimals` counts as 0; values above 20 are clamped.

```rhai
e.third = format_decimals(1.0 / 3.0, 3);         // "0.333"
e.whole = format_decimals(42.987, 0);            // "43"
```

#### `format_percent(ratio, decimals)`
Ratio × 100 with N decimals and `%`. Divide as floats: `3 / 100` is integer division and gives `0`.

```rhai
e.rate = format_percent(0.042, 1);                              // "4.2%"
e.err_rate = format_percent(e.errors.to_float() / e.total, 2);  // 3 of 100 → "3.00%"
```

### Padding & Alignment

Widths are display columns: wide (CJK) characters count 2, combining marks 0. `fill` and `marker` are strings, so use double quotes (`"."`, not `'.'`).

#### `text.ljust(n [, fill])` / `text.rjust(n [, fill])` / `text.center(n [, fill])`
Pad to width `n` (default fill: space). Longer text is returned unchanged. `center()` puts the odd extra column on the right.

```rhai
e.a = "ERROR".ljust(8, ".");                     // "ERROR..."
e.b = "42".rjust(5, "0");                        // "00042"
e.c = " TITLE ".center(20, "=");                 // "====== TITLE ======="
```

#### `text.shorten(n [, marker])` / `text.shorten_middle(n [, marker])`
If wider than `n`, keep the start (or both ends) and insert `marker` (default `"…"`). `""` truncates hard.

```rhai
e.a = "hello world".shorten(8);                  // "hello w…"
e.b = "hello world".shorten(8, "...");           // "hello..."
e.c = e.path.shorten_middle(30);                 // "/home/user/proj…/formatting.rs"
```

### Colors & Styles

#### `text.red()` / `.green()` / `.yellow()` / `.blue()` / `.cyan()` / `.magenta()` / `.bold()` / `.dim()`
Wrap text in an ANSI color or style. When colors are off (non-TTY output, `NO_COLOR`, `--no-color`) the text is returned unchanged, so scripts need no checks. Calls chain.

```rhai
print("CRITICAL".bold().red());
print(`${"OK".green()} ${e.msg}`);
```

### Charts & Sparklines

#### `bar(value, max, width)`
A bar exactly `width` columns wide showing `value / max`, with eighth-block resolution. Values outside `0..max` are clamped; `max <= 0` gives spaces. For ratios use `max = 1`.

```rhai
print(bar(7, 10, 10));                          // "███████   "
print(bar(0.42, 1, 10));                        // "████▎     "
```

#### `sparkline(array)`
One-line chart with `▁▂▃▄▅▆▇█`, scaled to `0..max(array)`. Negative and non-numeric values render as spaces; `[]` gives `""`.

```rhai
print(sparkline([1, 4, 2, 8, 5, 7]));           // "▁▄▂█▅▇"
```

---

## Type Conversion Functions

#### `to_int(value)` / `to_float(value)` / `to_bool(value)`
Convert, or return `()` on failure (so the assigned field is removed and `track_*` skips it). `to_int(3.9)` truncates to 3, but `to_int("3.9")` fails. `to_bool` accepts `true/false`, `yes/no`, `on/off`, `1/0` (any case) and numbers (non-zero is true).

```rhai
e.status = to_int(e.status_str);
e.score = e.score_str.to_float();
```

#### `to_int(value, thousands)` / `to_float(value, thousands, decimal)`
Parse formatted numbers. Every character in `thousands` is removed; `decimal` is one character or `""`. Pass strings in double quotes; single-quoted chars are not accepted.

```rhai
e.price = "1,234.56".to_float(",", ".");         // 1234.56
e.price = "1.234,56".to_float(".", ",");         // 1234.56 (EU)
e.count = "2 000 000".to_int(" ");               // 2000000
```

#### `to_int_or(value, default)` / `to_float_or(value, default)` / `to_bool_or(value, default)`
Same conversions with a fallback instead of `()`. Separator forms: `to_int_or(value, thousands, default)`, `to_float_or(value, thousands, decimal, default)`.

```rhai
e.status = e.status_str.to_int_or(0);
e.amount = e.value.to_float_or(",", ".", 0.0);
```

#### `value.or_empty()`
Turn `""`, `[]` and `#{}` into `()`; `()` passes through. Assigning the result removes the field, and `track_*()` skips it. Numbers and booleans are not accepted (error).

```rhai
e.name = e.message.after("User:").or_empty();    // no name field when "User:" is absent
e.tags = e.tags.or_empty();                      // drop an empty array
track_unique("users", e.message.after("User:").or_empty());
```

---

## Utility Functions

#### `get_env(var [, default])`
Environment variable; `default` (or `""`) when unset.

```rhai
e.branch = get_env("CI_BRANCH", "main");
```

#### `pseudonym(value, domain)`
Stable, keyed alias for a value, separated by `domain` (the same value in two domains gives unrelated aliases). Set `KELORA_SECRET` to get the same aliases across runs; without it kelora uses an ephemeral per-run key and prints a one-time notice (hidden by `--silent` or `--no-diagnostics`).

```rhai
e.user_alias = pseudonym(e.username, "users");   // e.g. "0809mKUnbCRvoshgu3NZZCeH"
e.ip_alias = pseudonym(e.client_ip, "ips");
```

Related: `hash()` (one-way digest), `mask_ip()`, `normalized()`. Recipes: [Cookbook → Security and Privacy](../cookbook/security-privacy.md).

#### `read_file(path)` / `read_lines(path)`
File contents as one string, or as an array of lines. **Only allowed in `--begin`**; keep the result in `state` for later stages.

```rhai
// --begin
state.blocklist = read_lines("blocked_ips.txt");
```

#### `drain_template(text [, options])`
Add a line to the Drain template model and return `{template, template_id, count, is_new, sample}` (plus `first_line`/`last_line` when `line_num` is given). Sequential mode only.

```rhai
let r = drain_template(e.message, #{line_num: meta.line_num});
e.template = r.template;
```

Options:

- `depth` (int, default 2) — leading tokens used as clustering keys. This is the count itself, not the Drain paper's `depth` (whose 4 means one keyed token). Never more than one below a message's token count.
- `max_children` (int, default 100) — distinct keys per tree node before further values share a wildcard branch.
- `similarity` (float, default 0.8) — fraction of positions that must match for a line to join a template.
- `filters` (CSV string or array of patterns like `%{IPV4:ip}`, Logstash grok syntax) — replaces the default masking set; an explicit list masks exactly those patterns, without the multi-token collapses below.
- `line_num` (int) — record line numbers.

The defaults were measured on the 16 [loghub](https://github.com/logpai/loghub) `_2k` datasets (`just drain-accuracy`, baseline in `dev/drain-accuracy-baseline.json`). Prefer changing the mined field (or pre-masking with `normalized()`) over tuning them.

Default filters: `ipv4_port`, `ipv4`, `ipv6`, `email`, `url`, `fqdn`, `uuid`, `mac`, `md5`, `sha1`, `sha256`, `path`, `oauth`, `hexcolor`, `version`, `hexnum`, `duration`, `timestamp`, `date`, `time`, `num`. Masking behaviour:

- `function` is off (it masked the identifier too, so `Intel(R)` and `packet(s)` became `<function>`); opt in with `filters: ["%{KELORA_FUNCTION:function}", ...]`. `normalized()` keeps its own `function` pattern on.
- `timestamp` also covers multi-token dates: ctime/asctime (`Mon Jun 13 03:55:15 2005`, year optional) and syslog (`Jun 13 03:55:15`).
- Sizes with units (`18.4 KB`, `10MB`) mask as one `<size_kb>`-style token, spaced durations (`took 5 seconds`) as `<duration>`.
- Only the matched span is masked: `uid=0` → `uid=<num>`, `worker-3` → `worker-<num>`, `GET /api/v1/users?id=5` → `GET <path>?id=<num>`. Digits inside words stay (`ssh2`, `utf8`, `sha256`).
- No PII patterns; pre-mask with `normalized(["credit_card", "ssn", "phone"])`.

Positions where a template's events disagree become `<*>`, also across token counts, so an optional segment (`(1.13 KB)`) or a value of varying width folds into one template. Per-line results are provisional: templates are rewritten as clusters generalize and near-identical ones are merged at end of input, so `--drain` and `drain_templates()` report the final set.

#### `drain_templates()`
Array of templates from the current model, with the same fields as `drain_template()` minus `is_new`. Sequential mode only.

```rhai
// --end
for t in drain_templates() { print(`${t.count}\t${t.template}`) }
```

#### `print(message)` / `eprint(message)`
Write to stdout / stderr. Suppressed by `--no-script-output`, `--silent` and the data-only modes (`-s`, `-m`, `--freq`, …) unless `--script-output` is given.

```rhai
print("Processing event: " + e.id);
eprint("Warning: " + e.error);
```

#### `exit(code)`
Stop kelora with the given exit code. The current event is not output.

```rhai
if e.level == "FATAL" { exit(1) }
```

#### `skip()`
Drop the current event: later stages and output do not run for it, and it counts as filtered.

```rhai
if e.endpoint == "/health" { skip() }
```

#### `status_class(code)`
HTTP status class `"1xx"`…`"5xx"`, or `"unknown"` outside 100–599. Takes an **integer**; convert string fields first.

```rhai
e.class = status_class(e.status);                // 404 → "4xx"
track_freq("status_class", status_class(e.code.to_int()));
```

#### `type_of(value)`
Type name: `"i64"`, `"f64"`, `"string"`, `"bool"`, `"array"`, `"map"`, `"()"`.

```rhai
if type_of(e.status) == "string" { e.status = e.status.to_int() }
```

#### `window.pluck(field)` / `window.pluck_as_nums(field)`
With `--window N`, `window` is an array of the most recent events, newest first (`window[0]` is the current event). [`pluck()`](#arraypluckfield--arraypluck_as_numsfield) works on it like on any array.

```rhai
e.avg_recent = window.pluck_as_nums("response_time").mean();
e.error_burst = window.pluck("status").filter(|s| s >= 500).len() >= 3;
```

---

## State Management Functions

`state` is a mutable map shared across all events and stages in **sequential mode**. Under `--parallel`, any access to `state` is an error; use `track_*()` for parallel-safe aggregation. Background: [Script Variables → state](script-variables.md#state).

#### `state["key"]` / `state["key"] = value`
Indexer access. Nested values can be modified in place.

```rhai
if !state.contains("seen_ips") { state["seen_ips"] = [] }
state["seen_ips"].push(e.ip);
```

#### `state.get(key [, default])` / `state.set(key, value)`
`get(key)` returns `()` if missing; `get(key, default)` returns `default` when missing or `()`.

```rhai
state.set("count", state.get("count", 0) + 1);
```

#### `state.contains(key)`
Whether the key exists.

```rhai
if !state.contains("start") { state["start"] = meta.parsed_ts }
```

#### `state.keys()` / `state.values()` / `state.len()` / `state.is_empty()`
Keys, values, entry count, emptiness.

```rhai
for key in state.keys() { print(key + ": " + state[key]) }
```

#### `state.remove(key)` / `state.clear()`
Remove one key (returning its value, or `()`), or everything.

```rhai
let old = state.remove("temp");
```

#### `state.mixin(map)` / `state += map` / `state.fill_with(map)`
Merge a map in (overwriting keys), or replace the whole state.

```rhai
state.mixin(#{count: 0, total_bytes: 0});
state.fill_with(#{count: 0});
```

#### `state.to_map()`
Copy into a regular map, e.g. for `to_json()`.

```rhai
print(state.to_map().to_json());
```

**Deduplicate by ID:**

```rhai
let id = e.request_id.to_string();              // map keys must be strings
if !state.contains("seen") { state["seen"] = #{} }
if state["seen"].contains(id) { skip() }
state["seen"][id] = true;
```

**Per-session counters** — update through the full `state[...]` path; `let s = state["sessions"][sid]` is a copy, and changes to it are lost:

```rhai
let sid = e.session_id;
if !state.contains("sessions") { state["sessions"] = #{} }
if !state["sessions"].contains(sid) { state["sessions"][sid] = #{start: meta.parsed_ts, events: 0} }
state["sessions"][sid].events += 1;
```

---

## Tracking/Metrics Functions

`track_*()` calls record metrics in any stage. `--metrics` prints them at the end, `--metrics-file` writes them as JSON, and `--end` scripts read them from the `metrics` map.

Shared rules:

- **`()` is skipped.** Missing fields and failed conversions produce `()`, which every `track_*()` skips instead of erroring. Skips are counted per metric; a metric that never recorded a value triggers a hint, so a field-name typo is visible.
- **Categorical arguments accept any scalar.** Strings, numbers, bools and datetimes are stringified; a datetime keys as ISO 8601 (as `.to_iso()` renders it): `track_freq("hour", meta.parsed_ts.round_to("1h"))`.
- **One metric name, one function.** Using a name with two different `track_*()` functions is an error. Under `--parallel`, a conflict between `--begin` and the event stages is a warning at merge time. Known gap: `track_stats("lat", …)` suffix keys (`lat_min`, …) can collide silently with a standalone `track_min("lat_min", …)`.
- **`__kelora_*` and `__op_*` names are reserved** and hidden from output.

### Tracking Functions {#tracking-functions}

#### `track_freq(name, value)`
Frequency table: count each distinct value, stored as `{name: {value: count}}`.

```rhai
track_freq("level", e.level);                      // {level: {ERROR: 12, INFO: 3041}}
track_freq("status_class", e.status / 100 * 100);  // 200/400/500 (integer division)
track_freq("latency_bucket", (e.latency_ms / 100).to_int() * 100);
```

!!! note "Changed in kelora 2.0"
    `track_count` (and 1.x `track_bucket`) were removed; calling them errors with a migration hint. Use `track_freq(name, value)` for per-value counts and `track_inc(name)` for a plain counter.

#### `track_inc(name)`
Add 1 to a counter; same as `track_sum(name, 1)`.

```rhai
track_inc("events");
if e.level == "ERROR" { track_inc("errors") }
```

#### `track_sum(name, value)`
Running total.

```rhai
track_sum("total_bytes", e.bytes);
```

#### `track_avg(name, value)`
Mean of the recorded values (kept as sum and count, so it merges across `--parallel` workers).

```rhai
track_avg("avg_latency", e.response_time);
```

#### `track_min(name, value)` / `track_max(name, value)`
Smallest / largest value.

```rhai
track_min("fastest", e.response_time);
track_max("slowest", e.response_time);
```

#### `track_unique(name, value)`
Exact set of distinct values. Memory grows with the set; kelora warns past 100,000 values.

```rhai
track_unique("users", e.user_id);
```

#### `track_cardinality(name, value [, error_rate])`
Approximate distinct count with HyperLogLog: about 12 KB per metric, ~1% standard error by default. `error_rate` ranges 0.001–0.26 (lower costs more memory). Reported as a whole number, never above the number of values seen; the terminal output marks it with `≈`.

```rhai
track_cardinality("unique_ips", e.client_ip);
track_cardinality("unique_users", e.user_id, 0.005);
```

| | `track_unique()` | `track_cardinality()` |
|-|------------------|----------------------|
| Memory | grows with distinct values | fixed ~12 KB |
| Accuracy | exact | ~1% (configurable) |
| Values listed | yes | no, count only |

#### `track_top(name, item [, n])` / `track_bottom(name, item [, n])`
The `n` (default 10) most / least **frequent** items. Output: `[{key, count}, …]`, sorted by count, ties alphabetical.

```rhai
track_top("common_errors", e.error_type);
track_bottom("rare_errors", e.error_type, 5);
```

#### `track_top_by(name, item, score [, n])` / `track_bottom_by(name, item, score [, n])`
The `n` (default 10) items with the highest / lowest **score**; each item keeps its maximum (top) or minimum (bottom) score. Output: `[{key, value}, …]`.

```rhai
track_top_by("slowest_endpoints", e.endpoint, e.latency_ms);
track_bottom_by("fastest_endpoints", e.endpoint, e.latency_ms, 5);
```

!!! note "Changed in kelora 2.0"
    The 1.x form `track_top(key, item, n, value)` is now `track_top_by(name, item, score [, n])`.

Top/bottom lists use bounded memory per metric; under `--parallel` the worker lists are merged, re-sorted and trimmed, with deterministic results.

#### `track_percentiles(name, value [, percentiles])`
Streaming percentile estimates (t-digest). Creates one metric per percentile, named with a `_pNN` suffix; `percentiles` are quantiles in 0–1 (default `[0.50, 0.95, 0.99]`).

```rhai
track_percentiles("api_latency", e.response_time);        // api_latency_p50, _p95, _p99
track_percentiles("latency", e.duration_ms, [0.999]);     // latency_p99.9
```

The t-digest keeps at most a few hundred centroids, so memory is flat and the results are estimates — typically within 0.5% of the exact value, most accurate in the tails. Sequential runs are reproducible; `--parallel` runs merge per-worker digests and can differ slightly.

#### `track_stats(name, value [, percentiles])`
Shorthand for `name_min`, `name_max`, `name_avg`, `name_count`, `name_sum` and the percentile metrics (default p50/p95/p99) in one call.

```rhai
track_stats("response_time", e.duration_ms);
track_stats("latency", e.duration_ms, [0.5, 0.9, 0.999]);  // …, latency_p50, latency_p90, latency_p99.9
```

If you do not need percentiles, `track_min/max/avg` are cheaper.

---

## File Output Functions

All of these need `--allow-fs-writes`; without it the call is an error.

#### `append_file(path, text_or_array)`
Append one line, or one line per array element.

```rhai
append_file("errors.log", e.message);
append_file("batch.log", [e.id, e.message]);
```

#### `truncate_file(path)`
Create the file or empty it.

```rhai
truncate_file("output.log");
```

#### `mkdir(path [, recursive])`
Create a directory; `recursive = true` also creates parents (otherwise a missing parent is an error).

```rhai
mkdir("out/2024/01", true);
```

---

## Event Manipulation

#### `emit_each(array [, base_map])`
Emit each map in `array` as its own event and drop the original (even when nothing is emitted). Fields of `base_map` are added where the item lacks them. Non-map items are skipped with a warning. Returns the number emitted. Only in `--exec`/`--filter`, not `--begin`/`--end`.

```rhai
let n = emit_each(e.items, #{batch_id: e.batch_id});
track_sum("items_emitted", n);
```

#### `e = ()`
Remove every field; the event is not output.

```rhai
if e.should_drop { e = () }
```

#### `e.field = ()`
Remove one field.

```rhai
e.password = ();
```

### Absorbing fields

The `absorb_*` functions parse one string field, merge the result into the event, and return a status map:

| Key | Meaning |
|---|---|
| `status` | `"applied"`, `"empty"` (nothing extracted), `"missing_field"`, `"not_string"`, `"parse_error"` |
| `data` | every parsed pair (also those skipped by `overwrite: false`) |
| `written` | whether any field was written |
| `remainder` | `absorb_kv` only: the unparsed text, else `()` |
| `removed_source` | whether the source field was deleted |
| `error` | parse error message, else `()` |

Common options: `keep_source` (default `false`: the source field is consumed) and `overwrite` (default `true`; `false` keeps existing event fields). Options a function does not use (`sep` for JSON, say) are accepted and ignored. A parsed key with the same name as the source field survives: absorbing `msg` = `{"msg": "inner"}` leaves `e.msg == "inner"`.

An **unknown option key** is a script error, not a status: in `--exec` the event rolls back and stderr reports `Exec errors: N total, affecting every event` (exit 0, transforms are best-effort); `--strict` aborts. So `status = "invalid_option"` never reaches a script without `try`/`catch`.

#### `e.absorb_kv(field [, options])`
Merge `key=value` tokens. Extra options: `sep` (default whitespace; `()` also means whitespace) and `kv_sep` (default `"="`). Tokens without `kv_sep` form the `remainder`; unless `keep_source` is set, the field is replaced by the remainder, or deleted when nothing remains (a parsed key with the source's name is then overwritten by the remainder). **Not quote-aware**; use `absorb_logfmt()` for `err="connection refused"`.

```rhai
// msg = "Payment timeout order=1234 user=bob"
let res = e.absorb_kv("msg");
// e.order == "1234", e.user == "bob", e.msg == "Payment timeout"
let res2 = e.absorb_kv("params", #{sep: ",", kv_sep: ":", keep_source: true});
```

#### `e.absorb_logfmt(field [, options])`
Merge a logfmt string: quote-aware, with numbers and booleans typed. All-or-nothing: a bare token makes the whole field a `parse_error`.

```rhai
// msg = 'pod="kube-system/foo" err="connection refused" replicas=3'
let res = e.absorb_logfmt("msg");
if res.status == "parse_error" { eprint(`not logfmt: ${res.error}`) }
// e.pod == "kube-system/foo", e.replicas == 3 (int), msg deleted
```

#### `e.absorb_json(field [, options])`
Merge a JSON **object** (arrays and invalid JSON are a `parse_error` and leave the event untouched).

```rhai
let res = e.absorb_json("payload");
if res.status == "applied" { e.actor = e.actor ?? e.user }
```

#### `e.absorb_jwt(field [, options])`
Merge a JWT's claims (header and signature ignored, **no verification**). Time claims stay integers; use [`parse_jwt()`](#textparse_jwt) for datetime `exp`/`iat`/`nbf`.

```rhai
let res = e.absorb_jwt("token");
// e.sub == "alice", e.role == "admin", e.exp == 1735689600
```

#### `e.absorb_regex(field, pattern [, options])`
Merge the **named** capture groups (`(?P<name>...)`) of the first match. Numbered groups are ignored. A pattern that does not compile is a script error, like an unknown option. On no match the status is `"empty"` and the event is left unchanged.

Values are always **strings**, even all-digit ones, so convert before comparing numbers (`"503" >= 500` is quietly `false`):

```rhai
e.absorb_regex("line", #"(?P<status>\d{3}) (?P<bytes>\d+)$"#, #{keep_source: true});
e.status = e.status.to_int();
e.bytes = e.bytes.to_int();
```

For whole-line parsing at input time, `-f 'regex:...'` is usually simpler ([Regex Format](formats.md#regex-format)).

---

## Span Context – `--span-close` Only

A read-only `span` object exists while a `--span-close` script runs (with `--span` or `--span-idle`). See [Group into Spans](../guide/spans.md) and [Script Variables → span](script-variables.md#span).

### Span Identity

`span.id` identifies the span; `span.label` is what `--span-summary` prints, so a hook need not branch on the span mode.

| Span mode | `span.id` | `span.label` |
|---|---|---|
| count (`--span 100`) | `#0`, `#1`, … | same as id |
| time (`--span 5m`) | `2024-05-19T12:00:00Z/5m` | `2024-05-19T12:00:00Z` |
| field (`--span service`) | the field value, e.g. `api` | same as id |
| idle (`--span-idle 5m`) | `idle-#0-2024-05-19T12:01:00Z` | `2024-05-19T12:01:00Z` |

```rhai
print(span.label + ": " + span.size + " events");
```

### Span Boundaries

`span.start` and `span.end` are datetimes for time spans (half-open window) and idle spans (first and last event); `()` for count and field spans.

```rhai
if span.start != () { print(`${span.start} → ${span.end}`) }
```

`span.first_ts` and `span.last_ts` are the parsed timestamps of the first and last events in the span, in arrival order, for every span mode; `()` when no event in the span had a timestamp. They say when a count or field span actually ran, and when events actually arrived inside a time window.

```rhai
print(`${span.label}: ${span.first_ts} → ${span.last_ts}`)
```

### Span Size and Events

`span.size` is the number of events that passed the filters and entered the span. `span.events` holds them in arrival order, each with `line`, `line_num`, `filename`, `span_id`, `span_start`, `span_end` and `span_status` added.

```rhai
let rts = span.events.pluck_as_nums("rt");
let max_rt = if rts.is_empty() { () } else { rts.max() };
```

### Metrics Snapshot

`span.metrics` maps metric names to what `track_*()` recorded while this span was open. Only additive trackers appear: `track_freq`, `track_sum`, `track_inc`, `track_avg` and `track_unique` (for `track_unique`, the values first seen in this span), plus the count/sum/avg parts of `track_stats`. Zero values are omitted.

#### `span.metric(name)`

Returns one per-span metric value, or `0` when the span recorded none; dotted names reach into `track_freq` tables (`span.metric("level.ERROR")`). Prefer it over indexing `span.metrics`, which yields `()` for an omitted zero and breaks arithmetic. For a non-additive metric (see below) it returns `()`, not `0`: such a metric has no per-span value, and `0` would read as data.

```rhai
// -e 'track_inc("events"); if e.status >= 500 { track_inc("failures") }'
let hits = span.metric("events");
let ratio = if hits > 0 { span.metric("failures") * 100 / hits } else { 0 };
print(`${span.label}: ${ratio}% failures`);
```

!!! warning "Non-additive trackers are omitted"
    `track_min`, `track_max`, `track_percentiles`, `track_cardinality`,
    `track_top`/`track_bottom` and `track_top_by`/`track_bottom_by` (and those
    parts of `track_stats`) have no per-window value. They are left out of
    `span.metrics` with a one-time warning, and `span.metric()` returns `()` for
    them. Compute them from `span.events`, as in the example above.

---

## Quick Reference by Use Case

**Error extraction:**
```rhai
e.error_code = e.message.extract_regex(#"ERR-(\d+)"#, 1).or_empty();
```

**IP anonymization:**
```rhai
e.masked_ip = e.client_ip.mask_ip();
e.ip_alias = pseudonym(e.client_ip, "ips");
```

**Time filtering** (or use `--since`/`--until`):
```rhai
if meta.parsed_ts != () && meta.parsed_ts.hour() >= 22 { e.night = true }
if to_datetime(e.created_at) > to_datetime("2024-01-01") { e.recent = true }
```

**Metrics:**
```rhai
track_freq("service", e.service);
track_sum("bytes", e.response_size);
track_unique("users", e.user_id);
```

**Fan-out:**
```rhai
emit_each(e.users, #{batch_id: e.batch_id});
```

**Safe field access:**
```rhai
e.user_name = e.get_path("user.profile.name", "unknown");
```

See also: [Scripting guide](../guide/scripting.md), [Rhai Cheatsheet](rhai-cheatsheet.md), [Script Variables](script-variables.md), `kelora --help-rhai`.
