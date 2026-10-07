# Kelora

Kelora is a command-line tool for reading log files. It recognizes common
formats on its own — application logs, syslog, web server logs, JSON, CSV —
splits each line into named fields, and lets you filter, count, and summarize
them with short options. For logic the options can't express, it has a small
scripting language.

[Install Kelora](installation.md){ .md-button } [Explore a log file](guide/explore.md){ .md-button }

## What using it looks like

Checkout is failing. This is the shop's application log, plain text (it's in
the repository's [`examples/`](https://github.com/dloss/kelora/tree/main/examples)
folder, like every file in these docs):

```bash exec="on" result="ansi"
head -3 examples/shop.log
```

**Errors only, with the fields you want.** Kelora recognized the format, so
fields have names:

```bash exec="on" source="above" result="ansi"
kelora examples/shop.log -l error -k ts,logger,msg -n 3
```

**Which component fails?**

```bash exec="on" source="above" result="ansi"
kelora examples/shop.log -l error --freq logger
```

**What do the errors say?** `--drain` groups messages that differ only in IDs
and numbers:

```bash exec="on" source="above" result="ansi"
kelora examples/shop.log -l error --drain -k msg
```

**When did it start?** One letter per event, one row per stretch of time:

```bash exec="on" source="above" result="ansi"
kelora examples/shop.log -F levelmap
```

The payment provider has been timing out since about 14:20. The
[guide](guide/explore.md) starts from here.

## In short

- **Install:** `brew install dloss/kelora/kelora`, or one binary for Linux,
  macOS, and Windows ([all options](installation.md)).
- **Input:** files, `.gz` and `.zst`, globs, or stdin:
  `tail -F app.log | kelora -l error`.
- **Speed:** filtering 100 000 JSON lines by level takes about 0.14 s (jq:
  0.95 s); `--parallel` spreads big batch jobs over all cores
  ([benchmarks](reference/benchmarks.md)).
- **Scripting:** [Rhai](https://rhai.rs), for what the options can't express:
  `--filter 'e.status >= 500 && e.path.starts_with("/api")'`.
- **Video:** a [5-minute introduction](https://www.youtube.com/watch?v=IwkicmS3RYo)
  by the YouTube channel Hack the Clown.
- **Compared with grep, awk, and jq:** grep finds lines and jq queries JSON.
  Kelora knows the log format, so you work with named fields, and counting,
  grouping, and time windows are built in. For plain text search, `rg` is
  faster — pipe it in front.

## What it does

| Topic | Covers |
|---|---|
| [Get logs into shape](guide/parse.md) | [20+ formats](reference/formats.md) detected automatically; your own via columns or a regex; stack traces and other multi-line events |
| [Filter](guide/filter.md) | by text, file section, time range, level, or any field; with surrounding lines like `grep -C` |
| [Transform](guide/scripting.md) | computed fields, values pulled out of messages, one row per array element, masking and pseudonyms |
| [Summarize](guide/summarize.md) | counts, percentiles, top values, distinct values, message patterns, comparing two logs |
| [Group into spans](guide/spans.md) | per-minute rollups, batches, sessions |
| [Cross-event logic](guide/state.md) | deduplication, pairing requests with responses, gap detection |
| [Big files](guide/files.md) | gzip/zstd, many files, merging by time, parallel processing |

The [Cookbook](cookbook/index.md) has ready-made commands for common questions.

## About

Kelora is open source under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE);
a few included third-party files are under Apache-2.0.
It runs locally: no networking, no telemetry, enforced by a CI check.

Kelora is an experiment in agentic AI development: AI agents write all
implementation and tests, and I steer requirements
([more](faq.md#was-kelora-built-with-ai)). It is a single-developer spare-time
project with best-effort support; review the
[security policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before
using it on sensitive data.
