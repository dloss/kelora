# Kelora

**Scriptable log processor for the command line.**

Parse messy logs into structured events, then filter, transform, and analyze them with embedded [Rhai](https://rhai.rs) scripting (a Rust-based scripting language with JavaScript-like syntax).

!!! note "Development Status"
    Pre-1.0 tool generated entirely by AI agents. Validated by a large test suite and Rust security tools; see [Development Approach](#development-approach) and the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before relying on it in production. APIs might change without notice before v1.0.

## What It Does

See what you can do in a few commands:

**Detect problems** - Find errors and performance issues by filtering on structured fields. After parsing, each log line becomes an event (a structured object) that you can filter using expressions like `e.status >= 500` (where `e` is the event). This example shows finding server errors and slow requests in web traffic logs:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f logfmt examples/traffic_logfmt.log \
      --filter 'e.status.to_int() >= 500 || e.latency_ms.to_int() > 1200'
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/traffic_logfmt.log
    ```

**Parse custom formats and extract structured data** - Many logs use custom formats with delimited columns or key-value pairs buried in messages. Kelora's `cols` format lets you describe the structure (like `ts level service *message` for "timestamp, level, service, then the rest as message") instead of writing regex patterns. The `absorb_kv()` function then extracts hidden key-value pairs from the message field into proper structured fields:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f 'cols:ts level service request_id *message' examples/release_pipe.log \
      --cols-sep '|' \
      --levels warn,error \
      --exec 'e.absorb_kv("message")' \
      -F json
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/release_pipe.log
    ```

**Enrich events with recent context** - Sometimes you need to understand an event in relation to what happened recently. Kelora's sliding window feature (`--window 5`) keeps the last N events in memory, accessible in scripts via the `window` array. This example adds error context from recent events to each log line, helping you see patterns and correlations:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_errors.jsonl \
      --window 5 \
      --exec 'let err = window.filter(|x| x.level == "ERROR");
              if err.len() > 0 { e.ctx = err[0].error; }' \
      --keys timestamp,endpoint,ctx \
      -F logfmt
    ```

=== "Log Data"

    ```bash exec="on" result="ansi"
    cat examples/api_errors.jsonl
    ```

## Works Well With

Kelora thrives in command-line pipelines. Stream logs from kubectl, tail, or journalctl into Kelora, then pipe output to jq, SQLite, qsv, or visualization tools. See [Integrate Kelora with External Tools](how-to/integrate-external-tools.md) for 18 tools and usage patterns.

## Get Started

**[→ Quickstart (5 minutes)](quickstart.md)** - Install and run your first commands

**[→ Tutorial: Basics (30 minutes)](tutorials/basics.md)** - Learn input formats, filtering, and output

**[→ How-To Guides](how-to/find-errors-in-logs.md)** - Solve specific problems

For deeper understanding, see [Concepts](concepts/pipeline-model.md). For complete reference, see [Glossary](glossary.md), [Functions](reference/functions.md), [Formats](reference/formats.md), and [CLI options](reference/cli-reference.md).

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).

## Development Approach

Kelora is an experiment in agentic AI development. Claude Code (Sonnet 4.5) and Codex CLI (gpt-5-codex) generate all implementation and tests; I steer requirements but do not write or review code. Validation relies on the automated test suite plus `cargo audit` and `cargo deny`, so please inspect the code yourself before relying on it in production and expect the API to evolve until 1.0. This is a spare-time, single-developer project, so support and updates are best-effort.
