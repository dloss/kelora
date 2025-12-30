/// Print regex format parsing help
pub fn print_regex_help() {
    let help_text = r#"
Regex Format Parsing Reference for -f regex:PATTERN:

QUICK START:
  kelora app.log -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'
  kelora access.log -f 'regex:(?P<ip>\S+) - (?P<user>\S+) \[(?P<ts>[^\]]+)\]'
  kelora metrics.log -f 'regex:(?P<code:int>\d+) (?P<latency:float>[\d.]+)ms (?P<msg>.*)'

SYNTAX:

Pattern format:
  -f 'regex:PATTERN'

Named capture groups (REQUIRED):
  (?P<field_name>...)       Capture as string
  (?P<field:int>...)        Capture and convert to integer
  (?P<field:float>...)      Capture and convert to float
  (?P<field:bool>...)       Capture and convert to boolean

IMPORTANT NOTES:

Automatic anchoring:
  Kelora automatically adds ^ and $ anchors to your pattern.
  DON'T write:  -f 'regex:^pattern$'    (anchors will be doubled!)
  DO write:     -f 'regex:pattern'      (anchors added automatically)

Named groups required:
  All capture groups must be named with (?P<name>...).
  Regular unnamed groups (\d+) won't create fields.

Field names:
  Must contain only letters, numbers, and underscores.
  Reserved names: original_line, parsed_ts, fields

EXAMPLES:

Simple syslog-style log:
  kelora app.log -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'
  # Matches: Jan 15 10:00:00 INFO Application started

Apache combined log format:
  kelora access.log -f 'regex:(?P<ip>\S+) - (?P<user>\S+) \[(?P<ts>[^\]]+)\] "(?P<request>[^"]+)" (?P<status:int>\d+) (?P<bytes:int>\d+)'
  # Matches: 192.168.1.1 - alice [15/Jan/2025:10:00:00 +0000] "GET /api HTTP/1.1" 200 1234

Custom format with typed fields:
  kelora metrics.log -f 'regex:(?P<ts>\S+) \[(?P<level>\w+)\] (?P<code:int>\d+) (?P<duration:float>[\d.]+)ms (?P<msg>.+)'
  # Matches: 2025-01-15T10:00:00Z [ERROR] 500 123.45ms Internal error

Greedy vs. non-greedy matching:
  kelora data.log -f 'regex:(?P<date>\d{4}-\d{2}-\d{2}) (?P<msg>.*)'   # .* is greedy (matches to end)
  kelora data.log -f 'regex:(?P<key>\w+)=(?P<val>[^ ]+) (?P<rest>.*)'  # [^ ]+ stops at space

COMMON MISTAKES:

✗ Adding your own anchors:
  -f 'regex:^pattern$'                    # WRONG: Anchors doubled!
  -f 'regex:pattern'                      # CORRECT: Anchors added automatically

✗ Using unnamed groups:
  -f 'regex:(\d+) (\w+)'                  # WRONG: Groups must be named!
  -f 'regex:(?P<num>\d+) (?P<word>\w+)'   # CORRECT: Named groups required

✗ Wrong type annotation:
  -f 'regex:(?P<status:integer>\d+)'      # WRONG: Unknown type 'integer'
  -f 'regex:(?P<status:int>\d+)'          # CORRECT: Use 'int', 'float', or 'bool'

✗ Forgetting to escape special characters:
  -f 'regex:(?P<ip>\S+) [(?P<ts>.*)]'     # WRONG: [ needs escaping
  -f 'regex:(?P<ip>\S+) \[(?P<ts>.*)\]'   # CORRECT: Escape [ and ]

ALTERNATIVE: Use -f cols for simpler patterns!

For whitespace-delimited logs, cols: is often easier than regex:

Instead of regex:
  -f 'regex:(?P<month>\w+) (?P<day>\d+) (?P<time>\S+) (?P<level>\w+) (?P<msg>.*)'

Use cols:
  -f 'cols:month day time level *msg'

The cols: format:
  - Splits on whitespace automatically
  - *field captures remaining line (like .* in regex)
  - Supports custom separators: --cols-sep=','
  - No need to worry about escaping special characters

Learn more: kelora --help (see --input-format examples)

DEBUGGING:

When patterns don't match:
  1. Use -vv to see detailed error messages
  2. Check for trailing newlines in error output
  3. Test pattern incrementally (start simple, add complexity)
  4. Verify pattern works in a regex tester (remember Kelora adds ^$)
  5. Consider using -f cols for simpler whitespace-delimited logs

For other help topics: kelora -h
"#;
    println!("{}", help_text);
}
