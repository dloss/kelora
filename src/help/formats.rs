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

auto (default)
  Auto-detect format from first non-empty line
  Detection order: json → syslog → cef → combined → logfmt → csv → line
  Note: Detects once and applies to all lines

OUTPUT FORMATS:

Specify with -F, --output-format <format>

default   - Colored key-value format
json      - JSON Lines (one object per line)
logfmt    - Key-value pairs
inspect   - Debug format with type information
levelmap  - Compact visual with timestamps and level indicators
keymap    - Compact visual showing first character of specified field (-k/--keys required, exactly one field)
csv       - Comma-separated with header row
tsv       - Tab-separated with header row
csvnh     - CSV without header
tsvnh     - TSV without header

Use -q/--quiet to suppress output (implied by -s/--stats and -m/--metrics).

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
