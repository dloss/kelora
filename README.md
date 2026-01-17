# ![Kelora logo](docs/kelora-logo.svg)

# Kelora

[![CI](https://github.com/dloss/kelora/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/dloss/kelora/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/kelora.svg)](https://crates.io/crates/kelora) [![Documentation](https://img.shields.io/badge/docs-kelora.dev-blue)](https://kelora.dev)

**Turn messy logs into structured data.**

Kelora is a scriptable log processor for the command line. Parse structured or semi-structured logs (one format per file/stream), filter with complex logic, and analyze streams using embedded [Rhai](https://rhai.rs) scripting with 150+ built-in functions. Handles JSON, logfmt, syslog, CSV/TSV, gzip, with sequential or `--parallel` execution and built-in metrics.

## Quick Example

```bash
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error -e 'e.absorb_kv("msg")' --normalize-ts -J
```

**Input (unstructured logs with embedded key=value pairs):**
```
Jan 15 10:00:15 ERROR Payment timeout order=1234 gateway=stripe duration=5s
Jan 15 10:00:22 ERROR Gateway unreachable host=stripe.com
Jan 15 10:00:28 ERROR Authentication failed user=admin ip=192.168.1.50 reason=invalid_token
```

**Output (structured JSON with extracted fields):**
```json
{"ts":"2025-01-15T10:00:15+00:00","level":"ERROR","msg":"Payment timeout","order":"1234","gateway":"stripe","duration":"5s"}
{"ts":"2025-01-15T10:00:22+00:00","level":"ERROR","msg":"Gateway unreachable","host":"stripe.com"}
{"ts":"2025-01-15T10:00:28+00:00","level":"ERROR","msg":"Authentication failed","user":"admin","ip":"192.168.1.50","reason":"invalid_token"}
```

Kelora also handles live streams: `tail -f app.log | kelora -j -l error,warn`.

**Interactive Mode:** Run `kelora` without arguments to enter an interactive REPL with readline support, automatic glob expansion, and command history—especially helpful on Windows where shell quoting is difficult.

## When to Use Kelora

Kelora trades speed for programmability—slower than grep/awk/jq, but adds stateful scripting for complex transformations. Use it when your logs are **messy** (stick to one format per file/stream, but pull out embedded JSON/logfmt fields), need **stateful logic** (counters, windowed metrics, lookup tables), or are **chaining multiple tools**. For simple text search use `grep`, for JSON queries use `jq`.

See [Power-User Techniques](https://kelora.dev/latest/how-to/power-user-techniques/) for JWT parsing, cryptographic pseudonymization, pattern normalization, and deterministic sampling.

## Installation

**Download pre-built binaries:**

| Platform | Download |
|----------|----------|
| **Windows** (x64) | [kelora-x86_64-pc-windows-msvc.zip](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-pc-windows-msvc.zip) |
| **macOS** (Apple Silicon) | [kelora-aarch64-apple-darwin.tar.gz](https://github.com/dloss/kelora/releases/latest/download/kelora-aarch64-apple-darwin.tar.gz) |
| **macOS** (Intel) | [kelora-x86_64-apple-darwin.tar.gz](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-apple-darwin.tar.gz) |
| **Linux** (x64) | [kelora-x86_64-unknown-linux-musl.tar.gz](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-unknown-linux-musl.tar.gz) |
| **Linux** (ARM64) | [kelora-aarch64-unknown-linux-musl.tar.gz](https://github.com/dloss/kelora/releases/latest/download/kelora-aarch64-unknown-linux-musl.tar.gz) |
| **Other platforms** | [All releases](https://github.com/dloss/kelora/releases) (ARMv7, FreeBSD, OpenBSD) |

Extract the archive and add the `kelora` binary to your PATH.

**Or install via Cargo:**

```bash
cargo install kelora
```

## Stability Promise (1.0+)

Starting with v1.0, Kelora follows semantic versioning:
- **CLI flags, Rhai functions, exit codes**: Stable. Breaking changes only in major versions (2.0, 3.0, etc.)
- **Deprecations**: Marked for one minor version before removal

## Documentation

> 📚 **[Read the full documentation at kelora.dev](https://kelora.dev)**

- [Quickstart](https://kelora.dev/latest/quickstart/)
- [Tutorials](https://kelora.dev/latest/tutorials/)
- [How-To Guides](https://kelora.dev/latest/how-to/)
- [Concepts](https://kelora.dev/latest/concepts/)
- [Reference](https://kelora.dev/latest/reference/)

## Examples

The [`examples/`](https://github.com/dloss/kelora/tree/main/examples) directory contains 60+ sample log files covering JSON, logfmt, syslog, CSV, and more. Use them to test filters, transformations, and edge cases.

For common patterns and usage recipes, run:
```bash
kelora --help-examples
```

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).

## Development Approach

Kelora is an experiment in agentic AI development. Claude Code (Sonnet 4.5) and Codex CLI (gpt-5-codex) generate all implementation and tests; I steer requirements but do not write or review code. Validation relies on the automated test suite plus `cargo audit` and `cargo deny`, so please inspect the code yourself before relying on it in production. 

This is a spare-time, single-developer project, so support and updates are best-effort.
