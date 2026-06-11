/// Print format reference help
pub fn print_formats_help() {
    let help_text = r#"
Format Reference:

INPUT FORMATS:

Specify with -f, --input-format <format>

json (-j)
  JSON Lines format, one object per line
  Fields: All JSON keys preserved with types

line
  Plain text, one line per event
  Fields: line

logfmt
  Heroku-style key=value pairs
  Fields: All parsed keys

syslog
  RFC5424/RFC3164 system logs
  Fields: pri, facility, severity, level, ts, host, prog, pid, msg
          [msgid, version - RFC5424 only]

combined
  Apache/Nginx access logs (CLF, Combined, Nginx+request_time)
  Fields: ip, ts, method, path, protocol, status
          [identity, user, bytes, referer, agent, request_time]
  Note: Fields in brackets are optional (omitted if value is "-")

cef
  ArcSight Common Event Format
  Fields: cefver, vendor, product, version, eventid, event, severity
          [ts, host - from optional syslog prefix]
          + all extension key=value pairs become top-level fields

csv / tsv / csvnh / tsvnh
  Comma/tab-separated values, with/without headers
  Fields: Header names or c1, c2, c3...
  Type annotations: 'csv status:int bytes:int response_time:float'
  Supported types: int, float, bool

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

regex:<pattern>
  Regular expression with named capture groups
  Fields: Named groups (?P<name>...) with optional type annotations
  Examples: 'regex:(?P<code:int>\d+) (?P<msg>.*)'
            'regex:(?P<ip>\S+) - - \[(?P<ts>[^\]]+)\] "(?P<method>\w+) (?P<path>\S+)'
  Types: (?P<name:int>...), (?P<name:float>...), (?P<name:bool>...)
  Note: Pattern automatically anchored with ^...$

Type annotations (csv/tsv/cols/regex)
  A type annotation declares the field's type. A value that cannot satisfy it
  becomes () (explicitly absent), and the rest of the row is kept; with --strict
  the run aborts instead. For tolerant coercion with a fallback you choose, drop
  the annotation and convert in a script stage, e.g.
    -f csv --exec 'e.status = to_int_or(e.status, 0)'

Named application-log formats (auto-detect only)
  When no structured format matches, auto-detection also recognises a small set
  of common application-log layouts and parses them with the regex engine:
    glog/klog, nginx-error, log4j, python-logging, iso8601-level
  These are tried just before the 'line' fallback, so they never override a
  format detected earlier. Adapted from lnav (see THIRD_PARTY_LICENSES.md).

auto (default)
  Auto-detect format from first non-empty line
  Detection order: json → syslog → cef → combined → logfmt → csv
                   → named app-log formats (regex) → line
  Note: Detects once and applies to all lines

auto-per-file
  Auto-detect format separately for each input file
  Detection order: json → syslog → cef → combined → logfmt → csv
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
  Allowed in cascade: json, line, raw, logfmt, syslog, cef, combined
  NOT allowed: auto, csv/tsv/csvnh/tsvnh, cols:, regex: (schema-based or spec-based)
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
csv       - Comma-separated with header row
tsv       - Tab-separated with header row
csvnh     - CSV without header
tsvnh     - TSV without header

Use -q/--quiet to suppress output (implied by -s/--stats and -m/--metrics).

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
