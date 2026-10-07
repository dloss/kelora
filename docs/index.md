# Kelora

**One command for messy logs.** Parse, filter, transform, and summarize logs in
JSON, logfmt, syslog, CSV, Apache/Nginx, Kubernetes, plain text, and your own
formats — with an embedded scripting language for everything the options don't
cover.

[Install Kelora](installation.md){ .md-button } [Explore a log file](guide/explore.md){ .md-button }

## A quick tour

**Find out what's in a file.** Kelora decompresses it, recognizes the format,
and profiles every field:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --discover
```

**Handle mixed formats.** A cascade tries parsers in order on each line, so
JSON and plain text can share a file. Keep the JSON, drop the noise, write CSV:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f json,line examples/mixed_format.log \
      --filter 'e._format == "json"' -k timestamp,level,msg -F csv
    ```

=== "Input"

    ```bash exec="on" result="ansi"
    cat examples/mixed_format.log
    ```

**See what's actually breaking.** `--drain` groups messages that differ only
in hostnames, IDs, or durations — here, 742 lines become four patterns:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/syslog_errors.log --drain -k msg
    ```

=== "Input (first 6 of 742 lines)"

    ```bash exec="on" result="ansi"
    head -6 examples/syslog_errors.log
    ```

Hack the Clown's [5-minute video](https://www.youtube.com/watch?v=IwkicmS3RYo)
shows more.

## When to reach for it

Kelora is the middle ground between "grep is enough" and "I need a log
platform" — the tool for the throwaway Python script you'd otherwise write.

- **One command instead of a pipe chain.** `grep | awk | jq | script.py`
  becomes one pass, with state kept across events.
- **Messy input is normal.** Mixed formats, `key=value` pairs inside messages,
  JSON inside text, stack traces across lines.
- **No script for simple jobs.** `-l error`, `--since 1h`, `--freq status`.
  For stateful logic — sessions, request/response pairs, error rates per
  window — there's a scripting language.
- **Composes.** `rg` in front; `jq`, DuckDB, or a spreadsheet behind.

## What it does

| Topic | Covers |
|---|---|
| [Get logs into shape](guide/parse.md) | 20+ formats, auto-detection, cascades, columns, regex, multiline events |
| [Filter](guide/filter.md) | raw lines, file sections, time ranges, levels, expressions, context lines |
| [Transform](guide/scripting.md) | computed fields, extraction from text, fan-out of arrays, masking and pseudonyms |
| [Summarize](guide/summarize.md) | counts, percentiles, top-N, distinct values, message templates, before/after diffs |
| [Group into spans](guide/spans.md) | per-minute rollups, batches, sessions |
| [Cross-event logic](guide/state.md) | deduplication, pairing requests with responses, gap detection |
| [Big files](guide/files.md) | gzip/zstd, many files, merging by time, parallel processing |

The [Cookbook](cookbook.md) has ready-made commands for common questions.

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
