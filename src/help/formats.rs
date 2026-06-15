/// Print format reference help
pub fn print_formats_help() {
    let help_text = r#"
Format Reference:

INPUT FORMATS:

Specify with -f, --input-format <format>

Concrete formats (parse input directly; listed alphabetically):

cef
  ArcSight Common Event Format
  Fields: cefver, vendor, product, version, eventid, event, severity
          [ts, host - from optional syslog prefix]
          + all extension key=value pairs become top-level fields

cols:<spec>
  Custom column-based parsing with whitespace or custom separator
  Fields: User-defined via spec
  Examples: 'cols:ts level *msg'
            'cols:ts(2) level *msg'  (ts consumes 2 tokens)
            'cols:name age:int city' --cols-sep '|'
  Tokens: field       - consume one column
          field(N)    - consume N columns and join
          -           - skip one column
          -(N)        - skip N columns
          *field      - capture rest of line (must be last)
          field:type  - apply type (int, float, bool, string)

combined
  Apache/Nginx access logs (CLF, Combined, Nginx+request_time)
  Fields: ip, ts, method, path, protocol, status
          [identity, user, bytes, referer, agent, request_time]
  Note: Fields in brackets are optional (omitted if value is "-")

csv / tsv / csvnh / tsvnh
  Comma/tab-separated values, with/without headers
  Fields: Header names or c1, c2, c3...
  Type annotations: 'csv status:int bytes:int response_time:float'
  Supported types: int, float, bool
  Ragged rows: extra columns are kept under positional names (cN, counted
  from 1); rows with fewer columns leave the trailing fields absent. Both
  are counted and reported as a hint; --strict rejects ragged rows instead.
  Quoted fields may contain embedded newlines (RFC 4180); such records are
  reassembled before parsing in both sequential and -P/--parallel mode.

json (-j)
  JSON Lines format, one object per line
  Fields: All JSON keys preserved with types

line
  Plain text, one event per line (trailing newline/CR trimmed)
  Fields: line

logfmt
  Heroku-style key=value pairs
  Fields: All parsed keys

raw
  Plain text, one event per line, preserved verbatim — unlike 'line', no
  trailing newline/CR is trimmed and backslashes and other artifacts are
  kept exactly as read
  Fields: raw

regex:<pattern>
  Regular expression with named capture groups
  Fields: Named groups (?P<name>...) with optional type annotations
  Examples: 'regex:(?P<code:int>\d+) (?P<msg>.*)'
            'regex:(?P<ip>\S+) - - \[(?P<ts>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+)'
  Types: (?P<name:int>...), (?P<name:float>...), (?P<name:bool>...)
  Note: Pattern automatically anchored with ^...$

syslog
  RFC5424/RFC3164 system logs
  Fields: pri, facility, severity, level, ts, host, prog, pid, msg
          [msgid, version - RFC5424 only]

Named application-log formats
  A small set of common application-log layouts, parsed with the regex engine:
    apache-error    Apache error log ("[Fri Oct 11 14:32:52 2024] [core:error] ... msg")
    cri             Kubernetes CRI/containerd log (2024-07-17T12:12:05.0Z stdout F msg)
    glog            Go/glog and Kubernetes klog (I0102 15:04:05.123 1 f.go:42] msg)
    haproxy         HAProxy http/tcp traffic log (via syslog); use -f haproxy
    iso8601-level   ISO-8601 timestamp + level + message (2024-01-02T15:04:05Z INFO msg)
    log4j           log4j / Java (2024-01-02 15:04:05,123 INFO [main] logger - msg)
    nginx-error     nginx error log (2024/01/02 15:04:05 [error] 29#29: msg)
    python-logging  Python logging default (... ,123 - logger - INFO - msg)
    redis           Redis 3+ (12345:M 06 Feb 2024 12:00:00.123 * msg)
    s3              AWS S3 server access log (owner bucket [date] ip ... "GET ..." 200 ...)
  Select explicitly with -f <name> (e.g. -f log4j), or in a cascade list
  (e.g. -f log4j,line). Most are also tried during auto-detection, just before
  the 'line' fallback, so they never override a format detected earlier; when
  one matches, it emits 'ts' (timestamp), 'level', 'msg', and format-specific
  extras (thread, logger, pid, ...).
  Notes: glog/redis omit the year, so 'ts' assumes the current year (like
  syslog). haproxy lines are syslog-wrapped, so under -f auto they are detected
  as 'syslog' — pass -f haproxy to extract the structured fields. 'cri' is the
  exception to the "tried last" rule: because a CRI message is often itself JSON
  or logfmt, it is detected early (before logfmt/csv) so auto-detect works
  regardless of the payload; its fields are 'ts', 'stream' (stdout/stderr),
  'tag' (F full / P partial), and 'msg'.
  Most definitions are adapted from lnav (BSD-3-Clause; see
  THIRD_PARTY_LICENSES.md); 'cri' is Kelora-original.

Type annotations (csv/tsv/cols/regex)
  A type annotation declares the field's type. A value that cannot satisfy it
  becomes () (explicitly absent), and the rest of the row is kept; with --strict
  the run aborts instead. For tolerant coercion with a fallback you choose, drop
  the annotation and convert in a script stage, e.g.
    -f csv --exec 'e.status = to_int_or(e.status, 0)'

Meta formats (select or combine the concrete formats above):

auto (default)
  Auto-detect format from first non-empty line
  Detection order: json → syslog → cef → combined → cri → logfmt → csv
                   → named app-log formats (regex) → line
  Note: Detects once and applies to all lines

auto-per-file
  Auto-detect format separately for each input file
  Detection order: json → syslog → cef → combined → cri → logfmt → csv
                   → named app-log formats (regex) → line
  Note: Detects once per file and applies to that file's lines
  stdin: behaves like 'auto' (single input stream)

<fmt1>,<fmt2>[,...]   (cascade mode)
  Try each format in order, first success wins (per line)
  Examples: -f json,line          (noisy JSON with plain-text fallback)
            -f json,logfmt,line   (structured streams with fallback)
  Put catch-all fallbacks like 'line' or 'raw' last so stricter parsers get first shot
  Adds an '_format' field to each event with the winning format name
  Stats (--stats) include per-format event counts
  Allowed in a comma list: json, line, raw, logfmt, syslog, cef, combined
  NOT in a comma list: auto, csv/tsv/csvnh/tsvnh (schema-based)

  Repeated -f   (cascade including spec-based parsers)
  Build the same cascade with one -f per format; this is the only way to put
  cols:/regex: in a cascade (commas can't delimit a regex pattern safely):
  Examples: -f json -f 'cols:ts(2) level *msg'          (JSON lines + app-log text)
            -f json -f 'regex:(?P<ts>\S+) (?P<msg>.*)' -f line
  Ordering rule: 'line', 'raw', and 'cols:' match every line, so they must be
  last. 'regex:' is selective (it declines non-matching lines), so it may sit
  earlier and fall through to a later catch-all.
  Multiline: uses the first listed format's strategy

OUTPUT FORMATS:

Specify with -F, --output-format <format>

default   - Colored key-value format
json      - JSON Lines (one object per line)
logfmt    - Key-value pairs
inspect   - Debug format with type information
levelmap  - Compact visual with timestamps and level indicators
keymap    - Compact visual showing first character of specified field (-k/--keys required, exactly one field)
tailmap   - Visualizes numeric field distribution with percentile thresholds (-k/--keys required, exactly one numeric field)

Map legends (levelmap/keymap/tailmap)
  Map formats append a one-line, data-driven legend decoding their glyphs
  (e.g. 'E = ERROR | I = INFO' or '2 = 200,204 | 4 = 404'). Shown only on a
  terminal by default; use --legend to force it when piping, --no-legend to hide.
csv       - Comma-separated with header row
tsv       - Tab-separated with header row
csvnh     - CSV without header
tsvnh     - TSV without header

Use -q/--quiet to suppress output (implied by -s/--stats and -m/--metrics).

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
