# ![Kelora logo](docs/kelora-logo.svg)

# Kelora

[![CI](https://github.com/dloss/kelora/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/dloss/kelora/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/kelora.svg)](https://crates.io/crates/kelora) [![Documentation](https://img.shields.io/badge/docs-kelora.dev-blue)](https://kelora.dev)

**One command for messy logs.** Parse, filter, transform, and summarize logs across JSON, logfmt, syslog, CSV, and plain text — with embedded [Rhai](https://rhai.rs) scripting when simple filters aren't enough.

Watch Hack the Clown's [**5-minute introduction video**](https://www.youtube.com/watch?v=IwkicmS3RYo) to see Kelora in action.

## See it

You have a log file full of errors. You want to know what's actually breaking — not scroll through hundreds of near-duplicates that differ only by hostname, UUID, or timestamp.

```bash
kelora -f syslog examples/syslog_errors.log --drain -k msg
```

```
templates (4 items):
  438: Connection timeout to database host <fqdn> after <duration>
  187: Upstream <fqdn> returned <num> for request <uuid>
   94: Failed to acquire lock on resource <path> after <duration>
   23: Payment gateway <fqdn> rejected transaction <uuid> insufficient_funds
```

One command. No temp files, no intermediate scripts, no manual regex. `--drain` auto-groups similar messages so you see the handful of patterns actually causing the noise.

Kelora also handles live streams: `tail -f app.log | kelora -j -l error,warn`.

Run `kelora` without arguments for an interactive REPL with readline, glob expansion, and history — handy on Windows where shell quoting is awkward.

## When Kelora helps

Reach for Kelora when you'd otherwise be writing a throwaway Python script. It's the middle ground between "grep is enough" and "I need a real observability platform."

- **Chained pipelines collapse into one command.** `grep | awk | jq | script.py` becomes `kelora`, with state preserved across the pipeline instead of lost between pipes.
- **Messy formats parse cleanly.** Mixed JSON and plaintext in the same file, key=value pairs inside message strings, nested JSON fanned out to flat rows — without regex gymnastics.
- **Embedded scripting when you need it.** Simple filters are one-liners. When logic gets stateful — session reconstruction, per-service error rates, request/response correlation — there's a full scripting layer.
- **Plays well with your existing tools.** Pipe `ripgrep` or `jq` upstream to pre-filter; pipe Kelora's JSON or CSV output into whatever comes next.

Kelora trades raw speed for programmability. Simple filters and format conversions handle multi-GB files comfortably; heavy Rhai scripting tops out in the low hundreds of thousands of lines before you'll want to pre-filter. For pure text search use `grep`; for pure JSON queries use `jq`.

See [Power-User Techniques](https://kelora.dev/latest/how-to/power-user-techniques/) for JWT parsing, cryptographic pseudonymization, pattern normalization, and deterministic sampling.

## Installation

**macOS (Homebrew):**

```bash
brew tap dloss/kelora && brew install kelora
```

**Linux (binary):**

```bash
curl -LO https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-unknown-linux-musl.tar.gz
tar xzf kelora-x86_64-unknown-linux-musl.tar.gz
sudo mv kelora /usr/local/bin/
```

**Rust (any platform):**

```bash
cargo install kelora
```

On Windows, download [kelora-x86_64-pc-windows-msvc.zip](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-pc-windows-msvc.zip), extract, and add to PATH.

For Debian/Ubuntu (`.deb`), Fedora/RHEL (`.rpm`), ARM Linux, FreeBSD, OpenBSD, and other platforms: see [all releases](https://github.com/dloss/kelora/releases).

Kelora follows semver starting with v1.0 — CLI flags and Rhai functions are stable.

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

## How Kelora is built

Kelora is an experiment in agentic AI development: AI agents generate all implementation and tests, and I steer requirements rather than writing or reviewing code. Validation relies on an extensive automated test suite plus `cargo audit` and `cargo deny`. Kelora is local-only with no networking or telemetry, enforced by a CI check.

This is a single-developer spare-time project, and support is best-effort. Review the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before using it on sensitive data in production.

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).
