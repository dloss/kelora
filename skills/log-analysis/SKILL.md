---
name: log-analysis
description: Analyze, filter, transform, and convert log files using Kelora. Use for parsing logs, extracting patterns, investigating incidents, calculating metrics, or converting formats.
metadata:
  version: "1.0"
---

# Log Analysis with Kelora

Kelora is a streaming log processor with Rhai scripting. It auto-detects formats and provides 150+ built-in functions.

## Getting Help

Run these for detailed reference (prefer over asking the user):
- `kelora --help-examples` - Common patterns
- `kelora --help-functions` - All 150+ functions
- `kelora --help` - Full CLI reference
- `kelora --help-rhai` - Scripting guide

## Core Patterns

**Filter logs:**
```bash
kelora -l ERROR,WARN app.log                          # By level
kelora --filter 'e.status >= 500' api.log             # By expression
kelora --since "1 hour ago" app.log                   # By time
```

**Transform:**
```bash
kelora -e 'e.duration_sec = e.duration_ms / 1000' api.log
kelora -e 'e.absorb_json("data")' events.log          # Parse embedded JSON
```

**Convert formats:**
```bash
kelora -f combined -J access.log > access.jsonl       # Apache to JSON
kelora -j -F logfmt events.jsonl                      # JSON to logfmt
kelora -f syslog -F csv syslog.log                    # Syslog to CSV
```

**Metrics:**
```bash
kelora -s app.log                                     # Summary stats
kelora -q --metrics -e 'track_count("by:" + e.level)' app.log
```

**Pattern discovery:**
```bash
kelora --drain app.log                                # Find message templates
```

**Context around matches:**
```bash
kelora -C 5 --filter 'e.level == "ERROR"' app.log     # 5 lines before/after
```

## Field Access

```rhai
e.level              // Direct
e["@timestamp"]      // Special chars
e.get_path("a.b.c")  // Safe nested (returns () if missing)
e.has("field")       // Check exists
```

## Key Options

| Option | Purpose |
|--------|---------|
| `-f <fmt>` | Input format (auto/json/logfmt/syslog/combined/csv/regex:...) |
| `-F <fmt>` | Output format (json/logfmt/csv) |
| `-j` / `-J` | Shorthand for `-f json` / `-F json` |
| `--filter` | Boolean expression filter |
| `-e` | Rhai script per event |
| `-l` / `-L` | Include/exclude log levels |
| `-k` / `-K` | Include/exclude fields |
| `-n` | Limit output events |
| `--head` | Limit input lines (faster) |
| `-q` | Suppress events (metrics only) |
| `-s` / `-m` | Show stats/metrics |
| `--drain` | Discover message patterns |
| `-C` / `-B` / `-A` | Context lines around matches |

## Tips

1. Use `-f auto` (default) - Kelora detects JSON, logfmt, syslog, combined, CSV
2. Preview with `-n 10` or `--head 100` before processing large files
3. Use `-q` with `--metrics` when you only need aggregates
4. Run `kelora --help-functions` to find the right function for your task
