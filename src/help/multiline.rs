/// Print multiline strategy help
pub fn print_multiline_help() {
    let help_text = r#"
Multiline Strategy Reference for --multiline:

Quick usage:
  kelora access.log --multiline timestamp
  kelora stack.log --multiline indent
  kelora trace.log --multiline regex:match=^TRACE
  kelora payload.json --multiline all

MODES:

timestamp
  Detect leading timestamps with Kelora's adaptive parser.
  Optional hint: --multiline timestamp:format='%b %e %H-%M-%S'

indent
  Treat any line that begins with indentation as a continuation.

regex:match=REGEX[:end=REGEX]
  Define record headers (and optional terminators) yourself.
  Example: --multiline regex:match=^BEGIN:end=^END

all
  Buffer the entire input as a single event.

NOTES:
- Multiline stays off unless you set -M/--multiline.
- Detection runs before parsing; pick -f raw/json/etc. as needed.
- Buffering continues until the next detected start or end arrives.
- With --parallel, tune --batch-size/--batch-timeout to keep memory bounded.
- Literal ':' characters are not supported inside the value today. Encode them in regex patterns (e.g. '\x3A') or normalise timestamp headers before parsing.

TROUBLESHOOTING:
- Use --stats or --metrics to watch buffered event counts.
- If buffers grow unbounded, tighten the regex or disable multiline temporarily.
- Remember that `--multiline all` reads the entire stream into memory.

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
