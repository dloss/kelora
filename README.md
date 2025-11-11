# Kelora

[![CI](https://github.com/dloss/kelora/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/dloss/kelora/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/kelora.svg)](https://crates.io/crates/kelora) [![Documentation](https://img.shields.io/badge/docs-kelora.dev-blue)](https://kelora.dev)

Scriptable log processor for the command line. Treats logs as structured events and lets you filter, transform, and analyze them using embedded [Rhai](https://rhai.rs) scripts with 150+ built-in functions.

Kelora parses log streams into structured events and runs them through a programmable pipeline powered by Rhai scripting.

It turns noisy lines into structured events (`e.field`/`e["field-name"]`), ships with 150+ Rhai helpers, and works with JSON, logfmt, syslog, CSV/TSV, column specs, and gzip. Stream tailing and archive crunching are supported through sequential or `--parallel` execution modes with built-in metrics for live observability.

## Installation

Download from **[GitHub Releases](https://github.com/dloss/kelora/releases)** (macOS, Linux, Windows) or:

```bash
cargo install kelora
```

## Documentation

Read the full documentation at **[kelora.dev](https://kelora.dev)**:

- [Quickstart](https://kelora.dev/latest/quickstart/)
- [How-To Guides](https://kelora.dev/latest/how-to/)
- [Concepts](https://kelora.dev/latest/concepts/)
- [Reference](https://kelora.dev/latest/reference/)

## Examples

Browse runnable samples in [`examples/README.md`](examples/README.md#file-categories).

## Development Approach

Kelora is an experiment in agentic AI development. Claude Code (Sonnet 4.5) and Codex CLI (gpt-5-codex) generate all implementation and tests; I guide requirements but do not write or review code. Validation relies on the automated test suite plus `cargo audit` and `cargo deny`, so please inspect the code yourself before relying on it in production and expect the API to evolve until 1.0. This is a spare-time solo project, so responses and updates happen on a best-effort basis.

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).
