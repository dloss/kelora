---
name: log-analysis
description: Analyze, filter, transform, and convert log files with Kelora. Use for exploring unknown logs, investigating incidents and stack traces, comparing logs before and after a deploy, counting or timing events (frequencies, percentiles, per-minute histograms), or converting between JSON, logfmt, syslog, Apache/combined, and CSV.
metadata:
  version: "2.1"
---

# Log Analysis with Kelora

Kelora is a streaming log processor with embedded Rhai scripting. It auto-detects formats (JSON, logfmt, syslog, Apache/combined, CSV, and built-in app-log formats such as `cri`, `postgres`, `nginx-error`, `log4j`, `python-logging`), handles files that mix formats, transparently decompresses `.gz`/`.zst`, and ships 150+ built-in functions.

## Getting Help

Kelora ships its own reference — consult it before guessing or asking the user:
- `kelora -h` - One-screen quick reference
- `kelora --help [KEYWORD]` - Full CLI reference (KEYWORD searches it, e.g. `--help since`)
- `kelora --help-functions [KEYWORD]` - All 150+ functions (e.g. `--help-functions ip`)
- `kelora --help-examples` - End-to-end patterns
- `kelora --help-rhai` - Rhai scripting guide and stage semantics
- `kelora --help-formats` - Input/output formats and the fields each one extracts
- `kelora --help-time` - Timestamp formats and `--since`/`--until` syntax
- `kelora --help-multiline` - Multiline strategies (`-M`)
- `kelora --help-regex` - Regex parsing (`-f regex:...`)

## 1. Explore an Unknown Log

Always profile first — field names depend on the format and are the most common source of empty results:

```bash
kelora -d app.log                      # Fields, types, cardinality, samples; footer names the detected format
kelora -d=json app.log                 # Same, machine-readable
kelora --drain -k msg app.log          # Cluster messages into templates (one text field in -k)
kelora -n 5 app.log                    # Peek at the first events
```

On a large file, add `--head 10000` to read only the first N lines. If `-d` reports format `line`, kelora found no structure: extract fields with `-f 'cols:ts(N) level *msg'` (N = whitespace tokens in the timestamp; the hint names it when it can tell) or `-f 'regex:...'` (see `--help-formats`, `--help-regex`).

## 2. Investigate an Incident

**Narrow down:**
```bash
kelora -l error,warn app.log                           # By level (case-insensitive)
kelora --filter 'e.duration_ms > 1000' app.log         # By expression
kelora --since 1h app.log                              # Last hour (also: now-15m, 2024-01-15T10:00:00Z)
kelora --since 10:00 --until since+30m app.log         # Anchored window
kelora -C 2 -l error app.log                           # 2 events of context around each match
```

**Count and time:**
```bash
kelora --freq level app.log                            # Count per distinct value (repeatable)
kelora --describe duration_ms app.log                  # count/min/max/avg/p50/p95/p99
kelora --card user.id app.log                          # Approx distinct count (dotted paths work here)
kelora --span 10m --span-summary app.log               # Events per 10-minute window
kelora --span 10m --span-summary --freq level app.log  # ...broken down by level
kelora -s app.log                                      # Overall stats: format, counts, parse errors, time span
```

These metric flags print only the aggregate (they imply `-q`). Output defaults to an aligned table on a terminal and tsv when piped, so `--freq msg | sort -k3 -nr | head` gives a top-N; pass `--metrics=json` for JSON.

**Stack traces:** group each trace into one event with a preset, then filter or cluster it like any other event:
```bash
kelora -f line -M java trace.log --filter 'e.line.contains("Caused by")'
kelora -f line -M python app_py.log --drain -k line
```
Presets: `java`, `python`, `go`, plus `timestamp`, `indent`, `blank` (paragraphs) and `regex:match=...` for custom boundaries. Kelora hints (💡) at the right preset when it sees traces.

## 3. Find What Changed (Before/After)

`--drain-diff` compares message templates between two logs, ignoring the variable parts (IDs, numbers, durations):

```bash
kelora --drain-diff=table -k msg before.log after.log            # Two files: baseline, target
kelora --drain-diff -k msg --cut-at 2024-01-15T10:00:30Z deploy.log   # One file, split at a time
kelora --drain-diff -k msg --cut-before 'e.msg.contains("deploy")' deploy.log  # ...or at the first match
```

The `table` view reads like a diff: `+` templates only in the target, `-` templates that disappeared, `*` templates whose rate changed materially (e.g. "14x more"), plus a footer on what stayed the same. Without `=table`, piped output is TSV (`new`/`gone`/`freq_changed` rows, no footer); use `=json` to parse it.

## 4. Transform and Convert

```bash
kelora -e 'e.duration_s = e.duration_ms / 1000' -k msg,duration_s app.log
kelora -e 'e.absorb_json("data")' events.log                     # Parse embedded JSON into fields
kelora -f combined -F json access.log > access.jsonl             # Apache/nginx access log to JSON
kelora -j -F logfmt events.jsonl                                 # JSON to logfmt
kelora -f syslog -F csv -k ts,host,level,msg syslog.log          # Syslog to CSV (csv needs -k)
kelora -m -e 'track_freq("per_10m", meta.parsed_ts.round_to("10m"))' app.log   # Custom metric: time histogram
```

Related functions: `absorb_kv`, `absorb_logfmt`, `absorb_regex`, `absorb_jwt`, `get_path`, `track_freq`/`track_inc`/`track_stats`/`track_top` (see `--help-functions track`). `meta.parsed_ts` is the event's parsed timestamp.

## 5. Several Files or Mixed Formats

```bash
kelora -f auto-per-file -F json api.log app.log        # Detect the format per file
kelora --merge-sorted before.log after.log             # Interleave sorted files by timestamp
kelora -f json,line mixed.log --filter 'e._format == "json"'   # Cascade: first parser that succeeds wins
```

Under the default `-f auto`, a file that mixes formats is parsed as a cascade automatically; each event gets `_format` naming the parser that claimed it.

## Field Access (Rhai)

```rhai
e.level                  // Direct; a missing field is ()
e["@timestamp"]          // Names with special characters
e.get_path("a.b.c")      // Safe nested access, () if missing
e.get_path("a.b", 0)     // ...with a default
e.has("field")           // Existence check
meta.parsed_ts           // Parsed event timestamp (datetime)
meta.filename            // Source file
```

## Gotchas

- **Read stderr.** Kelora is silent on success; when a run prints nothing or looks wrong, stderr usually carries a 💡 hint naming the problem (e.g. "Present fields: level, msg, ts").
- **Field names vary by format.** syslog yields `ts`, `host`, `prog`, `level`, `msg` — not `timestamp`/`message`. Check `-d` before writing `-k`, `--freq` or filters.
- **`-k` selects top-level fields only.** For nested values flatten first: `-e 'e.uid = e.get_path("user.id")' -k uid`. (`--freq`/`--describe`/`--card` do accept dotted paths.)
- **Some outputs need `-k`:** `-F csv`/`tsv` (column order), `-F keymap`/`tailmap` (exactly one field), `--drain`/`--drain-diff` (exactly one text field).
- **`--since`/`--until` run before `--filter`/`-e`**, so metrics count only events inside the window. Events without a parseable timestamp are reported, not silently kept.
- **Percentiles are estimates** (t-digest, ~0.1–0.4% error).
- **`--span`, `--drain`, `-d` and `--merge-sorted` are sequential-only;** `-P` (parallel) is for high-throughput batch filtering.
- **Exit codes:** 0 success (malformed lines and best-effort `-e` errors are reported but tolerated), 1 error (unopenable file, failed `--assert`, every line failed to parse, or any error under `--strict`), 2 invalid usage.

## Key Options

| Option | Purpose |
|--------|---------|
| `-f <fmt>` / `-j` | Input format / JSON shorthand; comma-list or repeated `-f` builds a cascade |
| `-F <fmt>` / `-J` | Output format (default/json/logfmt/csv/tsv/inspect/levelmap/keymap/tailmap) / JSON shorthand |
| `-k` / `-K` | Keep / drop top-level fields |
| `-b` | Values only, no keys |
| `--filter` / `-l` / `-L` | Boolean filter / include / exclude levels |
| `-e` / `-E` / `-I` | Rhai per event / script file / include helper functions |
| `--begin` / `--end` | Run once before / after processing |
| `--since` / `--until` | Time window (journalctl-style, see `--help-time`) |
| `-n` (`--take`) / `--head` | Limit output events / input lines |
| `-C` / `-B` / `-A` | Context around matches |
| `-d` / `-D` | Profile input fields / final emitted fields |
| `--drain` / `--drain-diff` | Message templates / template diff between two logs |
| `--freq` / `--describe` / `--card` | One-flag aggregations |
| `-s` / `-m` | Stats / tracked metrics (`=json` for machine-readable) |
| `--span` + `--span-summary` | Per-window rollups (count, duration, or field change) |
| `-M <strategy>` | Multiline events (`java`/`python`/`go`/`timestamp`/`indent`/`blank`/`regex:...`) |
| `--assert` / `--strict` | Fail on a violated condition / on the first error |
| `-P` | Parallel processing (default is sequential) |
