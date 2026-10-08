# Format Reference

Input format: `-f, --input-format <format>`. Output format: `-F, --output-format <format>`. `kelora --help-formats` prints a condensed version of this page.

## Input Formats

| Format | Input | Fields |
|--------|-------|--------|
| `auto` | Default. Detects one format (see [Auto-Detection](#auto-detection)) | depends |
| `json` / `-j` | JSON Lines, one object per line | all keys, types kept |
| `line` | Plain text | `line` |
| `raw` | Plain text | `raw` |
| `logfmt` | `key=value` pairs | all keys |
| `csv`, `tsv`, `csvnh`, `tsvnh` | Delimited, with or without header row | header names or `c1`, `c2`, … |
| `syslog` | RFC 5424 and RFC 3164 | `pri`, `facility`, `severity`, `level`, `ts`, `host`, `prog`, `pid`, `msg`, … |
| `combined` | Apache/Nginx access logs | `ip`, `ts`, `method`, `path`, `status`, … |
| `cef` | ArcSight Common Event Format | header fields + extensions |
| `cri` | Kubernetes CRI/containerd container logs | `ts`, `stream`, `tag`, `msg` |
| `apache-error` | Apache httpd error log | `ts`, `level`, `msg`, `module`, `pid`, `tid`, `client` |
| `glog` | Go glog and Kubernetes klog | `ts`, `level`, `msg`, `pid`, `source` |
| `haproxy` | HAProxy HTTP/TCP traffic log | `client_ip`, `frontend`, `backend`, `server`, `status`, timers, … |
| `iso8601-level` | ISO 8601 timestamp, level, message | `ts`, `level`, `msg` |
| `log4j` | log4j / Java logging | `ts`, `level`, `msg`, `thread`, `logger` |
| `monolog` | PHP Monolog (Laravel, Symfony) | `ts`, `channel`, `level`, `msg`, `context`, `extra` |
| `mysql-error` | MySQL and MariaDB error log | `ts`, `level`, `msg`, `thread`, `err_code`, `subsystem` |
| `nginx-error` | nginx error log | `ts`, `level`, `msg`, `pid`, `tid` |
| `postgres` | PostgreSQL log, default or Debian prefix | `ts`, `level`, `msg`, `pid`, `log_tz`, `user`, `db` |
| `python-logging` | Python `logging` (`asctime - name - levelname - message`) | `ts`, `level`, `msg`, `logger` |
| `redis` | Redis server log | `ts`, `level`, `msg`, `pid`, `role` |
| `s3` | AWS S3 server access log | `bucket`, `op`, `key`, `status`, … |
| `spring-boot` | Spring Boot default log | `ts`, `level`, `msg`, `pid`, `app`, `thread`, `logger` |
| `tomcat` | Tomcat catalina log | `ts`, `level`, `msg`, `thread`, `source` |
| `cols:<spec>` | Whitespace- or separator-delimited columns | named in the spec |
| `regex:<pattern>` | Anything a regex with named groups can match | named groups |
| `auto-per-file` | Detects separately for each file | depends |
| `<fmt1>,<fmt2>,…` | [Cascade](#cascade-mode): first parser that succeeds wins, per line | + `_format` |

A line that a parser rejects is a parse error: it is counted, reported on stderr and skipped. `--strict` aborts on it instead.

### JSON Format

One JSON object per line. All keys become fields with their JSON types (string, int, float, bool, null, nested map, array). A top-level array is a parse error; for a file that holds one JSON array, see [A file that is one JSON array](../cookbook/parsing.md#a-file-that-is-one-json-array).

For pretty-printed objects that span several lines, join them first: `-j -M 'regex:match=^\{'` (each object starts with `{` in column 1), or `-j -M all` for a single document. See [multiline.md](multiline.md).

### Line and Raw Formats

| Format | Field | Blank lines | Trimming |
|--------|-------|-------------|----------|
| `line` | `line` | kept, as `line=""` | trailing `\n`/`\r` removed |
| `raw` | `raw` | skipped | none by the parser |

Leading and trailing spaces, tabs and backslashes are kept by both. `line` is the `auto` fallback when nothing else matches. Both accept every line, so in a cascade they must come last.

### Logfmt Format

```
timestamp=2024-01-15T10:30:00Z level=ERROR status=500 message="Connection failed"
```

- Every pair becomes a top-level field. Keys may contain any non-space characters except `=` (`a.b-c`, `k:x` are fine).
- Unquoted values are typed: `500` → int, `1.5` and `1e3` → float, `true`/`True` → bool. `007` stays a string.
- Quoted values may contain spaces and `\"`. `key=` gives an empty string.
- A bare word without `=` is a parse error.

### CSV / TSV Formats

| Format | Delimiter | Header |
|--------|-----------|--------|
| `csv` | `,` | first row |
| `tsv` | tab | first row |
| `csvnh` | `,` | none; fields `c1`, `c2`, … |
| `tsvnh` | tab | none; fields `c1`, `c2`, … |

Values are strings unless you annotate them (see [Type Annotations](#type-annotations)):

```bash
kelora -f 'csv status:int bytes:int response_time:float' access.csv
```

- RFC 4180 quoting: `"a, b"`, `"say ""hi"""`, and quoted fields containing newlines (also under `-P`). An unclosed quote at end of input is an `Unterminated quoted field` error.
- Ragged rows are kept. Columns beyond the header (or beyond the first row with `csvnh`/`tsvnh`) get positional names (`c4`, `c5`, … counted from 1). Short rows leave the missing fields absent. A hint reports both counts. `--strict` makes a ragged row a fatal error.

### Syslog Format

```
<165>1 2024-01-15T10:30:00.000Z myhost myapp 1234 ID47 - Connection failed
<34>Jan 15 10:30:00 myhost myapp[1234]: Connection failed
2024-01-15T10:30:00.123456+00:00 myhost myapp[1234]: Connection failed
```

The third line is the RFC 3164 layout with an RFC 3339 timestamp, as rsyslog writes `/var/log/syslog` by default on Debian and Ubuntu; it is parsed as RFC 3164 and its timestamp carries the year and offset.

| Field | Type | RFC 5424 | RFC 3164 | Description |
|-------|------|----------|----------|-------------|
| `pri` | int | ✓ | if `<N>` present | priority = facility × 8 + severity |
| `facility` | int | ✓ | if `<N>` present | |
| `severity` | int | ✓ | if `<N>` present | 0–7 |
| `level` | string | ✓ | if `<N>` present | `EMERG` `ALERT` `CRIT` `ERROR` `WARN` `NOTICE` `INFO` `DEBUG` (severity 0–7) |
| `ts` | string | ✓ | ✓ | timestamp as written |
| `host` | string | ✓ | ✓ | |
| `prog` | string | ✓ | ✓ | |
| `pid` | int | ✓ | if `[pid]` present | RFC 5424: string if not numeric |
| `msgid` | string | ✓ | – | |
| `version` | int | ✓ | – | |
| `msg` | string | ✓ | ✓ | |

RFC 5424 fields whose value is `-` are omitted. Structured data is not extracted. RFC 3164 timestamps have no year; see [time-reference.md](time-reference.md#year-and-timezone).

### Combined Log Format

Handles Common Log Format, Combined, and Nginx Combined with a trailing request time (quoted or bare, as nginx's `$request_time`), detected per line:

```
192.168.1.1 - user [15/Jan/2024:10:30:00 +0000] "GET /index.html HTTP/1.0" 200 1234
192.168.1.1 - - [15/Jan/2024:10:30:00 +0000] "GET /api HTTP/1.1" 200 1234 "http://example.com/" "Mozilla/5.0"
192.168.1.1 - - [15/Jan/2024:10:30:00 +0000] "GET /api HTTP/1.1" 200 1234 "-" "curl/7.68.0" "0.123"
192.168.1.1 - - [15/Jan/2024:10:30:00 +0000] "GET /api HTTP/1.1" 200 1234 "-" "curl/7.68.0" 0.123
```

| Field | Type | Common | Combined | Nginx | Description |
|-------|------|--------|----------|-------|-------------|
| `ip` | string | ✓ | ✓ | ✓ | client address |
| `identity` | string | ✓ | ✓ | ✓ | RFC 1413 identity |
| `user` | string | ✓ | ✓ | ✓ | auth user |
| `ts` | string | ✓ | ✓ | ✓ | `15/Jan/2024:10:30:00 +0000` |
| `request` | string | ✓ | ✓ | ✓ | full request line |
| `method` | string | ✓ | ✓ | ✓ | from `request` |
| `path` | string | ✓ | ✓ | ✓ | from `request` |
| `protocol` | string | ✓ | ✓ | ✓ | from `request` |
| `status` | int | ✓ | ✓ | ✓ | |
| `bytes` | int | ✓ | ✓ | ✓ | `0` is kept |
| `referer` | string | – | ✓ | ✓ | |
| `user_agent` | string | – | ✓ | ✓ | |
| `request_time` | float | – | – | ✓ | seconds |

Any field whose value is `-` is omitted.

A line with anything after `user_agent` and the optional request time (an nginx `log_format` with extra fields such as `rt=` or `upstream=`) is a parse error, not silently truncated. The error names the column and the unexpected text, e.g. `unexpected trailing text after user_agent at column 122: 'trace=abc123'`; parse such logs with a `regex:` format that captures the extra fields.

### CEF Format

```
CEF:0|Security|threatmanager|1.0|100|worm successfully stopped|10|src=10.0.0.1 dst=2.1.2.2 spt=1232
```

| Field | Type | Description |
|-------|------|-------------|
| `ts` | string | from an optional syslog prefix (`Jan 15 10:30:00 host CEF:…`) |
| `host` | string | from the optional syslog prefix |
| `cefver` | string | CEF version |
| `vendor` | string | device vendor |
| `product` | string | device product |
| `version` | string | device version |
| `eventid` | string | signature ID |
| `event` | string | event name |
| `severity` | string | `0`–`10` |

Each extension pair becomes a top-level field. Extension values are typed: `1232` → int, `1.5` → float, `true` → bool. Everything else, including IP addresses, stays a string.

### CRI Format

The Kubernetes CRI/containerd log layout `<RFC3339Nano> <stream> <tag> <message>`: files under `/var/log/pods/`, and `kubectl logs --timestamps` output.

```
2024-07-17T12:12:05.123456789Z stdout F {"level":"info","msg":"started"}
2024-07-17T12:12:06.223456789Z stderr P panic: runtime error: nil pointer
```

| Field | Type | Description |
|-------|------|-------------|
| `ts` | string | runtime timestamp |
| `stream` | string | `stdout` or `stderr` |
| `tag` | string | `F` full line, `P` partial (the runtime split a long line; the following lines up to the next `F` continue it) |
| `msg` | string | the container's line, verbatim |

```bash
kelora pod.log -f cri --filter 'e.stream == "stderr"' -k ts,msg
kelora pod.log -f cri --exec 'e.absorb_json("msg")' --filter 'e.level == "error"'
```

Auto-detection tries `cri` before logfmt and CSV, so a JSON or logfmt payload does not hide it. Docker's `json-file` driver writes one JSON object per line (`log`, `stream`, `time`); use `-f json` for that.

### Server and Application Logs

Example lines for the formats from `apache-error` to `tomcat` in the table above. Select one with `-f <name>` or in a cascade (`-f log4j,line`). Most definitions are adapted from [lnav](https://lnav.org) (BSD-3-Clause).

| Format | Example line |
|--------|--------------|
| `apache-error` | `[Fri Oct 11 14:32:52 2024] [core:error] [pid 1234:tid 5678] [client 10.0.0.1] File does not exist` |
| `glog` | `I0102 15:04:05.123456 1 main.go:42] started` |
| `haproxy` | `Feb 06 12:14:14 lb haproxy[14389]: 10.0.1.2:33317 [06/Feb/2024:12:14:14.655] http-in static/srv1 10/0/30/69/109 200 2750 - - ---- 1/1/1/1/0 0/0 "GET / HTTP/1.1"` |
| `iso8601-level` | `2024-01-02T15:04:05Z INFO started` (space instead of `T`, `[…]` brackets, and `,` fractions also match) |
| `log4j` | `2024-01-02 15:04:05,123 INFO [main] com.example.App - started` |
| `monolog` | `[2026-10-08 10:30:51] local.WARNING: Unusually large cart {"items":25,"threshold":10}` |
| `mysql-error` | `2026-10-08T10:20:09.168138Z 6 [Warning] [MY-010453] [Server] root@localhost is created with an empty password !` (MariaDB: `2026-10-08 10:23:44 7 [Warning] Access denied for user …`) |
| `nginx-error` | `2024/01/02 15:04:05 [error] 29#29: open() failed` |
| `postgres` | `2024-01-02 15:04:05.123 UTC [1234] LOG:  ready` |
| `python-logging` | `2024-01-02 15:04:05,123 - myapp.db - INFO - connected` |
| `redis` | `12345:M 06 Feb 2024 12:00:00.123 * Ready` |
| `s3` | `79a59df9… mybucket [06/Feb/2024:00:00:38 +0000] 192.0.2.3 79a59df9… 3E57427F33A59F07 REST.GET.OBJECT photos/cat.jpg "GET /photos/cat.jpg HTTP/1.1" 200 - 2662 2662 14 12 "-" "aws-cli/2.0" -` |
| `spring-boot` | `2026-10-08T10:22:17.721Z  WARN 1 --- [demo] [nio-8080-exec-7] com.example.demo.DemoController : Order 4711 not found` |
| `tomcat` | `08-Oct-2026 10:27:32.174 SEVERE [Catalina-utility-2] org.apache.tomcat.util.digester.Digester.fatalError Parse fatal error at line [12] column [3]` |

`apache-error` fields besides `ts`, `level`, `msg` are each optional. `glog` `level` is `I`/`W`/`E`/`F`; `redis` `level` is the marker `.` `-` `*` `#`.

`s3` fields: `owner`, `bucket`, `client`, `requester`, `req_id`, `op`, `key`, `method`, `uri`, `query`, `httpver`, `status`, `error_code`, `bytes_sent`, `obj_size`, `total_time`, `turnaround_time`, `referer`, `user_agent`, then (newer logs) `version_id`, `host_id`, `sig_version`, `cipher_suite`, `auth_type`, `host_header`, `tls_version`. It has no `level` or `msg`.

`haproxy` fields: `host`, `proc`, `pid`, `client_ip`, `client_port`, `accept_date`, `frontend`, `backend`, `server`, timers `tq`/`tw`/`tc`/`tr`/`tt` (HTTP) or `tw`/`tc`/`tt` (TCP), `status` (HTTP), `bytes_read`, `termination_state`, `actconn`, `feconn`, `beconn`, `srv_conn`, `retries`, `srv_queue`, `backend_queue`, `req_headers`/`resp_headers` (when captured), and the request line in `msg` (HTTP). It has no `level`. HAProxy lines are syslog lines, so `-f auto` detects them as `syslog`: pass `-f haproxy`.

Types: fields named `pid`, `tid`, `client_port`, `bytes_read` and the haproxy connection counters are ints. Everything else is a string, including the `s3` and `haproxy` `status`, sizes and timers. Convert in a script if you need numbers: `--exec 'e.status = to_int(e.status)'`.

`s3` and `haproxy` keep a curated set of columns. The unparsed line is still available as `meta.line`, so a missing column can be extracted in a script:

```bash
kelora -f s3 access.log --exec 'e.last_col = meta.line.extract_regex("(\\S+)$", 1)'
```

Format notes:

- `glog` timestamps have no year; see [Year and timezone](time-reference.md#year-and-timezone).
- `postgres` matches the default `log_line_prefix = '%m [%p] '` and the Debian/Ubuntu package's `'%m [%p] %q%u@%d '`, whose session lines add `user` and `db` (`… [5871] app@shop ERROR:  …`). For another prefix use `-f regex:`. Multi-line `STATEMENT`s (tab-indented continuation lines) need `-M indent`; without it the continuation lines are parse errors. `-f postgres,line` keeps them as separate `line` events instead.
- `postgres` `ts` is naive. The logged zone abbreviation is kept in `log_tz` but not applied, because abbreviations like `CST` or `IST` are ambiguous. The timestamp is read in `--input-tz` (default UTC). For a server whose `log_timezone` is not UTC, pass e.g. `--input-tz Europe/Berlin`.
- `monolog` keeps `context` and `extra` as the JSON text Monolog wrote; read them with `e.context.parse_json()`. Laravel writes an exception's stack trace into `context` across many lines: group them with `-M 'regex:match=^\[\d{4}-'`.
- `mysql-error` covers MySQL 5.7/8 and MariaDB; `err_code` and `subsystem` exist only in MySQL 8 lines. `thread` is the connection id, 0 for the server itself.
- `spring-boot` matches Spring Boot's default layout, with or without the `[app]` group of 3.x and later. Lines with an application group or a tracing id fall through to `iso8601-level`. Stack traces need `-M timestamp`.
- `tomcat` stack traces need `-M java` (`-M timestamp` does not recognize Tomcat's `08-Oct-2026` date).

### Column Format

`-f 'cols:<spec>'` splits on whitespace (runs of spaces count as one separator), or on the string given with `--cols-sep`.

| Token | Meaning |
|-------|---------|
| `name` | one column |
| `name(N)` | N columns joined with a space |
| `-` / `-(N)` | skip one / N columns |
| `*name` | rest of the line; must be last |
| `name:type` | apply a type: `int`, `float`, `bool`, `string` |

```bash
# ERROR api "Connection failed"     → level, service, msg='"Connection failed"' (quotes kept)
kelora -f 'cols:level service *msg' app.log

# 2024-01-15 10:30:00 INFO Started  → ts='2024-01-15 10:30:00' (detected as timestamp)
kelora -f 'cols:ts(2) level *msg' app.log

# bob|42|Berlin
kelora -f 'cols:name age:int city' --cols-sep '|' data.txt
```

A line with too few columns sets the missing fields to `()`. Extra columns are dropped unless the spec ends in `*name`. Neither is an error, even under `--strict`. With `--cols-sep`, empty columns (`a||b`) become empty strings.

### Regex Format

`-f 'regex:<pattern>'`. The pattern is anchored automatically: the whole line must match. Each named group becomes a field.

| Syntax | Result |
|--------|--------|
| `(?P<name>…)` or `(?<name>…)` | string field |
| `(?P<name:int>…)`, `:float`, `:bool` | typed field (lowercase type names only) |
| `(?:…)` | non-capturing group |

```bash
# 404 Not found
kelora -f 'regex:(?P<code:int>\d+) (?P<msg>.*)' app.log

# 2025-01-15T10:00:00Z [ERROR] Database connection failed
kelora -f 'regex:(?P<ts>\S+) \[(?P<level>\w+)\] (?P<msg>.+)' app.log

# 192.168.1.1 - - [15/Jan/2025:10:00:00 +0000] "GET /api/users HTTP/1.1" 200 1234
kelora -f 'regex:(?P<ip>\S+) - - \[(?P<ts>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+) HTTP/[\d.]+" (?P<status:int>\d+) (?P<bytes:int>\d+)' access.log
```

- A group that captures an empty string is omitted.
- A line that does not match is a parse error.
- Nested named groups and the names `original_line`, `parsed_ts` and `fields` are rejected when the pattern is compiled.
- Single-quote the whole `-f` argument so the shell leaves `\`, `$` and `[` alone.

### Type Annotations

Used by `csv`/`tsv`, `cols:` and `regex:`. A value that cannot be converted (`abc` for `:int`) becomes `()`; the rest of the event is kept. With `--strict` the run aborts instead. To choose a fallback yourself, leave out the annotation and convert in a script: `--exec 'e.status = to_int_or(e.status, 0)'`.

### Auto-Detection

`-f auto` is the default. It picks one format:

- **stdin:** from the first non-empty line. A live pipe never waits for more input.
- **Files:** from up to the first 64 non-empty lines (max 256 KiB) of the first file that has content. Uncompressed files of 32 KiB or more are also sampled at 1/4, 1/2, 3/4 and the tail. Gzip/zstd files are sampled from the head only.

Each line is tested in this order, first match wins:

1. `json`: the line is a JSON object
2. `cef`: parses as CEF, with or without a syslog prefix
3. `syslog`: RFC 5424 or RFC 3164
4. `combined`
5. `cri`
6. `logfmt`
7. `tsv`/`tsvnh`: at least two tabs, with the same header check as `csv` below. Tab-separated lines are tried here, before the application-log formats, since tabs are a deliberate delimiter.
8. `glog`, `nginx-error`, `apache-error`, `log4j`, `python-logging`, `postgres`, `redis`, `s3`, `spring-boot`, `tomcat`, `mysql-error`, `monolog`, `iso8601-level` (`haproxy` lines are taken by `syslog` in step 3). These come before comma-separated `csv` because their layouts are specific, while `csv` takes any line with two commas.
9. `csv`: at least two commas. If the first field contains no letters, `csvnh`; otherwise the first line must read as a header row (a field holding prose or a full datetime marks it as data, so a log message with commas is not mistaken for CSV). An explicit `-f csv` skips this check.
10. `line`

**Mixed files:** if the file sample contains more than one format, kelora parses with `<dominant format>,line`, exactly like an explicit [cascade](#cascade-mode), and each event gets `_format`; a hint says so, since the extra field changes the output's shape (`--exclude-keys _format` drops it, an explicit `-f json,line` silences the hint). A format needs at least two matching sampled lines to be chosen (in samples of four or more lines). Further structured formats in the sample are not added; their lines become `line` events and a hint prints the explicit `-f` (e.g. `-f json,syslog,line`) that would parse them. CSV/TSV never joins a cascade. On stdin, mixed input is parsed with the first line's format.

**JSON arrays:** a sample that starts like a JSON array of objects (`[` on its own line followed by `{`, or `[{`) is read as `line`, and a hint names the command that reads it. On stdin only `[{` on the first line is recognized.

`-v` prints the decision, e.g. `Auto-detected format: cascade(json,line) (mixed formats in first 6 lines)`.

### Auto-Detection Per File

`-f auto-per-file` runs the file detection above separately for each file, so JSON and logfmt files can be read in one run:

```bash
kelora -f auto-per-file -J logs/api/*.log logs/workers/*.log
```

On stdin it behaves like `auto`. It cannot be combined with `--parallel` or `--merge-sorted`.

### Cascade Mode

Every line is tried against each format in order. The first that parses it wins, and the event gets a `_format` field naming that format. Typical use: structured logs with interleaved plain text (panics, banners, stack traces), or files that interleave several services.

```bash
kelora -f json,line app.log                              # noisy JSON
kelora -f json,syslog,line mixed.log
kelora -f json,line app.log --filter 'e._format == "line"'
kelora -f json,line app.log -s                           # adds "Cascade formats: json=9812, line=23"
```

| Members | How to list them |
|---------|------------------|
| every other named format (`json`, `line`, `raw`, `logfmt`, `syslog`, `cef`, `combined`, `cri`, `log4j`, …) | comma list or repeated `-f` |
| `cols:<spec>`, `regex:<pattern>` | repeated `-f` only (a pattern may contain commas) |
| `auto`, `auto-per-file`, `csv`, `tsv`, `csvnh`, `tsvnh` | not allowed |

```bash
kelora -f json -f 'cols:ts(2) level *msg' app.log
kelora -f json -f 'regex:(?P<ts>\S+ \S+) (?P<level>\w+) (?P<msg>.*)' -f line app.log
```

Repeated `-f` and comma lists can be mixed; members are tried in the order given.

- **Order:** `line`, `raw` and `cols:` accept every line, so they must be last; kelora rejects anything after them. `regex:` rejects non-matching lines, so it can appear earlier. Put liberal grammars such as `logfmt` after stricter ones.
- **`_format` collisions:** if a record already has its own `_format` field, it is kept and not tagged; a warning reports how many events this affected.
- **Multiline:** `-M` grouping runs before parsing and follows the first format's strategy.
- Works with `-P/--parallel`.

## Output Formats

| Format | Output | Requirements |
|--------|--------|--------------|
| `default` | colored `key='value'` pairs | |
| `json` / `-J` | JSON Lines | |
| `logfmt` | `key=value` pairs | |
| `inspect` | one field per line with its type | |
| `levelmap` | timeline of one glyph per event (first letter of the level) | no `-P` |
| `keymap` | timeline of the first character of one field; `.` when missing or empty | `-k` with exactly one field; no `-P` |
| `tailmap` | timeline of one numeric field by percentile: `_` below p90, `1` p90–p95, `2` p95–p99, `3` above p99, `.` missing | `-k` with exactly one numeric field; no `-P` |
| `csv`, `tsv` | delimited, header row | `-k` (sets the columns) |
| `csvnh`, `tsvnh` | delimited, no header | `-k` |

```bash
kelora -j app.log -F csv -k ts,level,msg
kelora access.log -F keymap -k status
kelora api.log -F tailmap -k response_time
```

**CSV/TSV:** columns appear in `-k` order; a field missing from an event is an empty cell. Nested maps and arrays are flattened into a single cell, e.g. `{"x":[1,2],"y":"q"}` becomes `x_0:1,x_1:2,y:q`. Values with quotes or delimiters are quoted CSV-style (also in TSV).

**Nested values in other formats:** `default` prints them as JSON, `logfmt` flattens them like CSV into one quoted value, `json` and `inspect` keep the structure.

**Maps** (`levelmap`, `keymap`, `tailmap`):

![Levelmap output](../screenshots/levelmap.gif)

- Each row starts with the timestamp of its first event, even if `-k` does not select the timestamp field. Without a timestamp (none detected, or removed with `--exclude-keys`) the row starts with `line N`, the number padded to a fixed width so the glyph columns stay aligned.
- A row holds a fixed number of events, not a fixed span of time: it can cover seconds or hours depending on event density. Compare row labels to see how much time a row covers.
- A one-line legend lists only the glyphs that appeared and the values behind them, e.g. `2 = 200,204 | 4 = 404 | 5 = 500,503`. `tailmap` adds a line with count, range and the p90/p95/p99 thresholds. The legend is shown only when stdout is a terminal; `--legend` forces it, `--no-legend` hides it.

`-q/--quiet` suppresses events; `-s/--stats` and `-m/--metrics` imply it.

See also: [guide/parse.md](../guide/parse.md), [guide/output.md](../guide/output.md), [multiline.md](multiline.md), [time-reference.md](time-reference.md).
