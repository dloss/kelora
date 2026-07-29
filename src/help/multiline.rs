/// Print multiline strategy help
pub fn print_multiline_help() {
    let help_text = r#"
Multiline Strategy Reference for --multiline:

Quick usage:
  kelora app.log --multiline timestamp
  kelora stack.log --multiline indent
  kelora report.txt --multiline blank
  kelora trace.log --multiline regex:match=^TRACE
  kelora payload.json --multiline all
  kelora stderr.log --multiline python     (also: java, go)

MODES:

timestamp
  A line beginning with a timestamp starts a new event. Detection locks onto
  the first timestamp format it sees, so a continuation line that merely
  starts with something time-like ("17:03 was the window") cannot split an
  event. Options:
    timestamp:format='%b %e %H:%M:%S'  detect ONLY this chrono format
    timestamp:loose                    accept any recognizable timestamp
                                       (mixed-format files; pre-2.1 behavior)

indent
  A non-indented line starts a new event; indented and blank lines continue
  the current one.

blank
  Blank lines separate events (paragraph mode). The blank separator itself
  belongs to no event.

regex:match=REGEX[:end=REGEX]
  Define record headers (and optional terminators) yourself.
  Example: --multiline regex:match=^BEGIN:end=^END
  Patterns may contain ':' (e.g. match=^\d{2}:\d{2}); only ':match=' /
  ':end=' / ':format=' act as option separators.

all
  Buffer the entire input as a single event (reads everything into memory).

java | python | go
  Language presets for stack traces in output WITHOUT reliable timestamps
  (raw stderr, container stdout, CI logs). Each line is its own event unless
  it is a recognized trace line: a trace start (java.lang...Exception:,
  "Traceback (most recent call last):", panic:/fatal error:/goroutine dumps)
  attaches to the line that logged it, and continuations (at ..., Caused by:,
  File "...", goroutine blocks -- interior blank lines included for
  python/go) extend the event. Default join: newline.
  If lines DO start with timestamps, a locked timestamp header always starts
  a new event, so a preset stays safe on timestamped files -- but prefer
  `timestamp` there: it also keeps non-stacktrace continuations (wrapped
  messages, embedded payloads) with their events; kelora hints once when a
  preset meets such input. Known limits: a multi-line exception *message*
  is not recognized past its first line, and a Java exception class named
  without Exception/Error/Throwable is only caught at its first frame.

RELATED FLAGS:
  --multiline-join=space|newline|empty
      How buffered lines are joined. Default: space (newline for `all` and
      the language presets). Use newline to keep stack traces readable and
      splittable.
  --multiline-timeout DURATION
      Flush a buffered partial event after this much input inactivity
      (e.g. 400ms, 2s; 0 = never). Default: off for regular files (file
      runs stay deterministic), 400ms when reading a stream (stdin, FIFO).
      A defaulted stream flush prints a one-time hint when it fires.
  --multiline-max-lines N
      Split events that exceed N buffered lines and warn once (safety cap
      against a never-matching pattern buffering the whole input).
      Default: 10000; 0 = unlimited. Ignored by `all`.

NOTES:
- Multiline stays off unless you set -M/--multiline.
- Events never span input files; the buffer is flushed at file boundaries.
- meta.line_num and meta.filename point at each event's FIRST line.
- --multiline-join=newline keeps stack traces readable and still parses: in
  regex/syslog line formats the trailing message capture spans the newlines.
- Detection runs before parsing; pick -f raw/json/etc. as needed.
- --keep-lines/--ignore-lines filter physical lines BEFORE assembly (useful
  for dropping interleaved noise). To filter whole assembled events, use
  --filter on the result instead.
- Trailing blank lines are trimmed from events (except under `all`).

TROUBLESHOOTING:
- Use --stats or --metrics to watch buffered event counts.
- Events merging that should split: with `timestamp`, the lock-in may have
  latched onto the wrong format if the file's first line is not a real
  header — pin it with timestamp:format=... or use timestamp:loose.
- Events splitting that should merge on a live stream: raise
  --multiline-timeout (or set 0 to never flush early).
- Inspect boundaries while tuning: -f raw -F json --take 5.

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
