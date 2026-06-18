---
name: log-analysis
description: Analyze, filter, transform, and convert log files using Kelora. Use for parsing logs, extracting patterns, investigating incidents, calculating metrics, or converting formats.
metadata:
  version: "2.0"
---

# Log Analysis with Kelora

Kelora is a streaming log processor with embedded Rhai scripting. It auto-detects formats (JSON, logfmt, syslog, Apache/combined, CSV, and built-in application-log formats), transparently decompresses `.gz`/`.zst` input, and ships 150+ built-in functions.

## Getting Help

Kelora ships its own reference on these dedicated screens — consult them before asking the user:
- `kelora -h` - One-screen quick reference (cheat sheet)
- `kelora --help [KEYWORD]` - Full CLI reference (searchable, e.g. `--help since`)
- `kelora --help-examples` - Practical end-to-end patterns
- `kelora --help-functions [KEYWORD]` - All 150+ functions (KEYWORD filters, e.g. `--help-functions ip`)
- `kelora --help-rhai` - Rhai scripting guide and stage semantics
- `kelora --help-formats` - Input/output format reference
- `kelora --help-time` - Timestamp formats (`--since`/`--until`, parsing)
- `kelora --help-multiline` - Multiline event strategies (`-M`)
- `kelora --help-regex` - Regex parsing guide (`-f regex:...`)

## Start Here: Explore Unknown Logs

When you don't know what's in a file, profile it first — no flags or regex needed:

```bash
kelora --discover app.log          # Field names, types, cardinality, samples (-d)
kelora --drain -k msg app.log      # Cluster near-duplicate lines into templates
```

`--discover` reports the detected format and how every field maps. `--drain` collapses noisy, near-identical messages into a handful of templates so you see what's actually happening.

## Core Patterns

**Filter:**
```bash
kelora -l ERROR,WARN app.log                          # By level
kelora --filter 'e.status >= 500' api.log             # By expression
kelora --since "1 hour ago" --until now app.log       # By time
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

**Mixed formats in one file (cascade):** try each parser per line, first success wins; the winner is tagged in `_format`.
```bash
kelora -f json,line mixed.log --filter 'e._format == "json"'
```

**Metrics (one-flag aggregations, all imply -q):**
```bash
kelora --freq level app.log                           # Count per distinct value
kelora --describe duration_ms app.log                 # count/min/max/avg/p50/p95/p99
kelora --card user.id app.log                         # Approx distinct count (HyperLogLog)
kelora -s app.log                                     # Summary stats
kelora -m -e 'track_freq("by_level", e.level)' app.log  # Custom metrics via Rhai
```
`--freq`/`--describe`/`--card` are repeatable and accept dotted paths (e.g. `user.id`). Output as `--metrics=short|full|tsv|json`; tsv is auto-selected when piped, so `--freq url | head` gives the top-N.

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
| `-f <fmt>` | Input format (auto/json/logfmt/syslog/combined/csv/cols:.../regex:...); comma-list or repeated `-f` builds a cascade |
| `-F <fmt>` | Output format (default/json/logfmt/csv/tsv/inspect/levelmap/keymap/tailmap) |
| `-j` / `-J` | Shorthand for `-f json` / `-F json` |
| `--filter` | Boolean expression filter |
| `-e` / `-E` | Rhai script / script file per event |
| `--begin` / `--end` | Run once before / after processing |
| `-l` / `-L` | Include / exclude log levels |
| `-k` / `-K` | Include / exclude top-level fields |
| `-n` / `--head` | Limit output events / input lines (faster) |
| `--since` / `--until` | Time-window filter (journalctl-style) |
| `-d` / `-D` | Profile fields (input / final) |
| `--drain` | Cluster lines into message templates |
| `--freq` / `--describe` / `--card` | Frequency / numeric stats / distinct-count over a field |
| `-s` / `-m` | Show stats / metrics (both imply `-q`) |
| `--span` | Aggregate events into consecutive spans |
| `-C` / `-B` / `-A` | Context lines around matches |
| `-P` | Parallel processing (default is sequential) |

## Tips

1. Use `-f auto` (default) — Kelora detects JSON, logfmt, syslog, combined, CSV, and named app-log formats. Use a cascade (`-f json,line`) for files that mix formats.
2. Profile first with `--discover` / `--drain`; preview with `-n 10` or `--head 100` before processing large files.
3. Reach for `--freq` / `--describe` / `--card` before hand-writing `track_*()` calls — they're the common aggregations in one flag.
4. Run `kelora --help-functions KEYWORD` to find a function (e.g. `--help-functions ip`); `--help KEYWORD` searches the CLI reference.
