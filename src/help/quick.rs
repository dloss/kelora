/// Print quick help / cheat sheet
pub fn print_quick_help() {
    let help_text = r#"Kelora - Scriptable log processor for the command line

Usage:
  kelora [OPTIONS] [FILES]...
  kelora [OPTIONS] < input.log
  kelora            # Run without arguments to enter interactive mode
  kelora --help     # Full CLI reference (all options)

When to Use Kelora:
  ✓ Messy logs with embedded key=value or JSON
  ✓ Stateful analysis (counters, percentiles, lookup tables)
  ✓ Incident response (latency spikes, error investigation)
  ✓ Complex transformations beyond grep/jq capabilities
  ✗ Simple text search → use grep instead
  ✗ Pure JSON queries → use jq instead

Quick Examples:
  kelora -f logfmt -l error simple_logfmt.log
  kelora web_access_large.log.gz --stats
  kelora simple_json.jsonl --filter 'e.service == "database"' --exec 'e.duration_s = e.get_path("duration_ms", 0) / 1000' -k timestamp,message,duration_s
  kelora simple_json.jsonl --since 2024-01-15T10:01:00Z --until levels warn,error --stats
  kelora audit.jsonl --exec 'track_count(e.action)' --metrics
  kelora app.jsonl --drain -k message
  kelora payments_latency.jsonl --parallel --filter 'e.duration_ms > 500' -k order_id,duration_ms,status
  tail -f app.log | kelora -j -l error,warn

Common Options:
  -f, --input-format <FORMAT>   Choose parser (auto, json, line, raw, logfmt, syslog, cef, csv, tsv, csvnh, tsvnh, combined, cols:<spec>, regex:<pattern>)
  -j                            Shortcut for -f json
  --filter <expr>               Keep events where expression is true (can repeat; run in the order given)
  -l, --levels <levels>         Keep only these log levels (comma-separated)
  -e, --exec <expr>             Transform events or emit metrics (can repeat; run in the order given)
  -k, --keys <fields>           Pick or reorder output fields
  -F, --output-format <FORMAT>  Output format (default/json/logfmt/inspect/levelmap/keymap/csv/tsv/csvnh/tsvnh)
  -q, --quiet                   Suppress event output (-s/--stats and -m/--metrics imply this)
  -n, --take <N>                Limit output to first N events
  -s, --stats                   Show only the statistics, with discovered fields
  -m, --metrics                 Show only the tracked metrics
  --drain                       Summarize log templates (requires -k/--keys, sequential only)

Interactive Mode:
  kelora                     Run without arguments to enter interactive mode
                             (readline-based REPL with history, glob expansion,
                             and proper quoting - especially helpful on Windows)

More Help:
  kelora --help              Full CLI reference (all 100+ options grouped by category)
  kelora --help-rhai         Rhai language guide + stage semantics
  kelora --help-functions    Complete built-in function catalogue (150+ functions)
  kelora --help-examples     Common patterns and example walkthroughs
  kelora --help-formats      Format reference with extracted fields
  kelora --help-time         Timestamp format reference
  kelora --help-multiline    Multiline event strategies
  kelora --help-regex        Regex format parsing guide

Incident Response:
  See docs/how-to/incident-response-playbooks.md for copy-paste commands:
    • API latency spikes          • Error rate investigation
    • Authentication failures      • Database slow queries
    • Resource exhaustion          • Deployment correlation
    • Rate limit abuse             • Distributed tracing
"#;
    println!("{}", help_text);
}
