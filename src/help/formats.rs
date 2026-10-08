/// Print format reference help
pub fn print_formats_help() {
    let help_text = r#"
Format Reference:

INPUT FORMATS:

Specify with -f, --input-format <format>

Concrete formats (parse input directly; listed alphabetically):

apache-error
  Apache httpd error log
  ([Fri Oct 11 14:32:52 2024] [core:error] [pid 1234:tid 5678] [client ...] msg)
  Fields: ts, level, msg [module, pid, tid, client]

cef
  ArcSight Common Event Format
  Fields: cefver, vendor, product, version, eventid, event, severity
          [ts, host - from optional syslog prefix]
          + all extension key=value pairs become top-level fields

cols:<spec>
  Custom column-based parsing with whitespace or custom separator
  Fields: User-defined via spec
  Examples: 'cols:ts level *msg'
            'cols:ts(2) level *msg'  (ts joins 2 tokens: 2024-01-15 10:00:00)
            'cols:ts(3) level *msg'  (syslog-style Jan 15 10:00:00 is 3 tokens)
            'cols:name age:int city' --cols-sep '|'
  Tokens: field       - consume one column
          field(N)    - consume N columns and join
          -           - skip one column
          -(N)        - skip N columns
          *field      - capture rest of line (must be last)
          field:type  - apply type (int, float, bool, string)

combined
  Apache/Nginx access logs (CLF, Combined, Nginx + request_time quoted or bare)
  Fields: ip, ts, request, method, path, protocol, status
          [identity, user, bytes, referer, user_agent, request_time]
  Note: Fields in brackets are optional (omitted if value is "-")

cri
  Kubernetes CRI/containerd container log (2024-07-17T12:12:05.0Z stdout F msg)
  Fields: ts, stream (stdout/stderr), tag (F full / P partial), msg
  Note: Auto-detected before logfmt/csv, so a JSON or logfmt payload in msg
        does not hide it

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

glog
  Go glog and Kubernetes klog (I0102 15:04:05.123456 1 main.go:42] msg)
  Fields: ts, level (I/W/E/F), msg, pid, source
  Note: No year in the timestamp; 'ts' is dated near the current year (like
        syslog). Pass --input-year YYYY for an archived log

haproxy
  HAProxy HTTP/TCP traffic log, as written through syslog
  Fields: host, proc, pid, client_ip, client_port, accept_date, frontend,
          backend, server, timers, status, bytes_read, termination_state,
          connection counters, msg (HTTP request line); no level
  Note: Its lines are syslog lines, so -f auto detects them as 'syslog';
        pass -f haproxy
  Note: Keeps a curated set of columns; the full line is in 'meta.line'

iso8601-level
  ISO-8601 timestamp + level + message (2024-01-02T15:04:05Z INFO msg)
  Also: space instead of T, [...] around the timestamp, ',' fractions
  Fields: ts, level, msg

json (-j)
  JSON Lines format, one object per line
  Fields: All JSON keys preserved with types

line
  Plain text, one event per line (trailing newline/CR trimmed)
  Fields: line

log4j
  log4j / Java logging (2024-01-02 15:04:05,123 INFO [main] logger - msg)
  Fields: ts, level, msg, thread, logger

logfmt
  Heroku-style key=value pairs
  Fields: All parsed keys

nginx-error
  nginx error log (2024/01/02 15:04:05 [error] 29#29: msg)
  Fields: ts, level, msg, pid, tid

postgres
  PostgreSQL log with the default log_line_prefix '%m [%p] '
  (2024-01-02 15:04:05.123 UTC [1234] LOG:  msg)
  Fields: ts, level, msg, pid, log_tz
  Note: A custom prefix (user@db, app name, ...) won't match; use -f regex:
  Note: Multi-line statements (tab-indented continuation lines) need
        -M indent; without it those lines are parse errors. -f postgres,line
        keeps them as separate 'line' events instead
  Note: 'ts' is read in --input-tz (default UTC); the logged zone
        abbreviation is kept in 'log_tz' but not applied (abbreviations are
        ambiguous). For a non-UTC server pass e.g. --input-tz Europe/Berlin

python-logging
  Python logging, '%(asctime)s - %(name)s - %(levelname)s - %(message)s'
  (2024-01-02 15:04:05,123 - myapp.db - INFO - msg)
  Fields: ts, level, msg, logger

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

redis
  Redis 3+ server log (12345:M 06 Feb 2024 12:00:00.123 * msg)
  Fields: ts, level (marker . - * #), msg, pid, role

s3
  AWS S3 server access log (owner bucket [date] ip ... "GET ..." 200 ...)
  Fields: owner, bucket, client, requester, req_id, op, key, method, uri,
          query, httpver, status, error_code, bytes_sent, obj_size,
          total_time, turnaround_time, referer, user_agent
          [version_id, host_id, sig_version, cipher_suite, auth_type,
           host_header, tls_version - newer logs]; no level or msg
  Note: Keeps a curated set of columns; the full line is in 'meta.line' for
        a second-stage parse, e.g.:
    kelora -f s3 access.log --exec 'e.tail = meta.line.extract_regex("\"[^\"]*\"\\s*$", 0)'

syslog
  RFC5424/RFC3164 system logs
  Fields: pri, facility, severity, level, ts, host, prog, pid, msg
          [msgid, version - RFC5424 only]

Several of the log layouts above are adapted from lnav (BSD-3-Clause; see
THIRD_PARTY_LICENSES.md).

Type annotations (csv/tsv/cols/regex)
  A type annotation declares the field's type. A value that cannot satisfy it
  becomes () (explicitly absent), and the rest of the row is kept; with --strict
  the run aborts instead. For tolerant coercion with a fallback you choose, drop
  the annotation and convert in a script stage, e.g.
    -f csv --exec 'e.status = to_int_or(e.status, 0)'

Meta formats (select or combine the concrete formats above):

auto (default)
  Auto-detect format: from the first non-empty line on stdin (a live pipe
  never waits for more input), from a sample of the file head — up to the
  first 64 non-empty lines, capped at 256 KiB — for file input. Plain
  (uncompressed) files of 32 KiB or more are additionally probed at a few
  deeper offsets (1/4, 1/2, 3/4, tail), so a format change partway through
  the file — concatenated rotations, say — is still caught; gzip/zstd files
  sample the head only (compressed streams aren't seekable)
  Detection order: json → cef → syslog → combined → cri → logfmt → csv
                   → glog → nginx-error → apache-error → log4j
                   → python-logging → postgres → redis → s3 → iso8601-level
                   → line
  Note: Detects once and applies to all lines
  Note: File input only: if the sampled head mixes formats, kelora parses
        with a two-member cascade of the dominant structured format plus the
        'line' catch-all (as if you had passed -f json,line) and adds an
        '_format' field per event; see cascade mode below. Auto-detection
        never builds wider cascades: a format needs at least two sampled
        lines to qualify, and any further structured formats in the sample
        parse as 'line' with a hint naming the explicit -f that would parse
        them. CSV/TSV never joins a cascade: a file starting as csv/tsv is
        parsed entirely as such, and a csv-looking line later in a non-csv
        file counts as 'line'. Mixed stdin still pins to the first line's
        format — pass an explicit cascade for mixed streams.
  Note: The csv/tsv step only claims the input if the first line reads as a
        header row, so a comma in a log message can't turn field names into
        message fragments. A field reads as data if it holds prose, a full ISO
        datetime, or — next to other words in the same field — a bare level
        (INFO/WARN/…) or a date/clock-shaped token. Column names that simply
        *are* one (TIMESTAMP,ERROR_COUNT / region,2024-01-01) still work, and an
        explicit -f csv skips the check entirely.

auto-per-file
  Auto-detect format separately for each input file
  Detection order: json → cef → syslog → combined → cri → logfmt → csv
                   → glog → nginx-error → apache-error → log4j
                   → python-logging → postgres → redis → s3 → iso8601-level
                   → line
  Note: Detects once per file and applies to that file's lines, sampling each
        file's head like 'auto' — a file that mixes formats gets a per-file
        cascade
  Note: The same csv/tsv header-plausibility check as 'auto' applies per file
  stdin: behaves like 'auto' (single input stream)

<fmt1>,<fmt2>[,...]   (cascade mode)
  Try each format in order, first success wins (per line)
  Examples: -f json,line          (noisy JSON with plain-text fallback)
            -f json,logfmt,line   (structured streams with fallback)
  Put catch-all fallbacks like 'line' or 'raw' last so stricter parsers get first shot
  Adds an '_format' field to each event with the winning format name
  A record that already has its own '_format' keeps that value: the tag is
    skipped for it (with a warning) rather than overwriting your data
  Stats (--stats) include per-format event counts
  Allowed in a comma list: every named format except those below (json,
    line, raw, logfmt, syslog, cef, combined, cri, log4j, ...)
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
csv       - Comma-separated with header row
tsv       - Tab-separated with header row
csvnh     - CSV without header
tsvnh     - TSV without header

Map legends (levelmap/keymap/tailmap)
  Map formats append a one-line, data-driven legend decoding their glyphs
  (e.g. 'E = ERROR | I = INFO' or '2 = 200,204 | 4 = 404'). Shown only on a
  terminal by default; use --legend to force it when piping, --no-legend to hide.
  Each row is labeled with its first event's timestamp ('line N' when the event
  has none). Rows hold a fixed number of events, not a fixed span of time: a
  row can cover seconds or hours depending on event density.

Use -q/--quiet to suppress output (implied by -s/--stats and -m/--metrics).

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
