/// Print quick help / cheat sheet
pub fn print_quick_help() {
    let help_text = r#"Kelora - Scriptable log processor for the command line

Usage:
  kelora [OPTIONS] [FILES]...
  kelora [OPTIONS] < input.log
  kelora            # Run without arguments to enter interactive mode
  kelora --help     # Full CLI reference (all options)

Quick Examples:
  kelora web_access_large.log.gz -d           # Start here: profile an unknown file
  kelora -f logfmt -l error simple_logfmt.log
  kelora -f json,line mixed_format.log --filter 'e._format == "json"'
  kelora -f auto-per-file -F json services/*/*.log
  kelora web_access_large.log.gz --stats
  kelora simple_json.jsonl --filter 'e.service == "database"' --exec 'e.duration_s = e.get_path("duration_ms", 0) / 1000' -k timestamp,message,duration_s
  kelora simple_json.jsonl --since 2024-01-15T10:01:00Z --until now -l warn,error --stats
  kelora audit.jsonl --exec 'track_count("action", e.action)' --metrics
  kelora app_monitoring.jsonl --drain -k message
  kelora -f json --merge-sorted app-*.jsonl
  kelora payments_latency.jsonl --parallel --filter 'e.duration_ms > 500' -k order_id,duration_ms,status
  kelora levelmap-demo.jsonl -F levelmap          # one char per event (E/W/I/D…): a visual log timeline
  tail -f app.log | kelora -j -l error,warn

Common Options:
  -f, --input-format <FORMAT>   Choose parser (auto, auto-per-file, json, line, raw, logfmt, syslog, cef, csv, tsv, csvnh, tsvnh, combined, cols:<spec>, regex:<pattern>, or cascade list e.g. json,line)
  -j                            Shortcut for -f json
  -d, --discover                Profile field names, types, and sample values — start here on unknown files (sequential only)
  --filter <EXPR>               Keep events where expression is true (can repeat; run in the order given)
  -l, --levels <LEVELS>         Keep only these log levels (comma-separated)
  --since/--until <TIME>        Keep events within a time range (journalctl-style; see --help-time)
  -e, --exec <EXPR>             Transform events or emit metrics (can repeat; run in the order given)
  -k, --keys <KEYS>             Pick or reorder output fields
  -F, --output-format <FORMAT>  Output format (default/json/logfmt/inspect/levelmap/keymap/tailmap/csv/tsv/csvnh/tsvnh)
  -q, --quiet                   Suppress event output (-s/--stats and -m/--metrics imply this)
  -n, --take <N>                Limit output to first N events
  -s, --stats                   Show only the statistics, with discovered fields
  -m, --metrics                 Show only the tracked metrics
  --drain                       Summarize log templates (requires -k/--keys, sequential only)
  --merge-sorted                Merge already-sorted files by timestamp; aborts on missing timestamps, parse failures, or disorder (sequential only)
  -P, --parallel                Process in parallel for high-throughput batch analysis (sequential by default)

More Help:
  kelora --help              Full CLI reference (all 100+ options grouped by category)
  kelora --help-rhai         Rhai language guide + stage semantics
  kelora --help-functions    Complete built-in function catalogue (150+ functions)
  kelora --help-examples     Common patterns and example walkthroughs
  kelora --help-formats      Format reference with extracted fields
  kelora --help-time         Timestamp format reference
  kelora --help-multiline    Multiline event strategies
  kelora --help-regex        Regex format parsing guide
"#;
    println!("{}", help_text);
}
