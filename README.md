<img src="docs/kelora-logo.svg" alt="Kelora logo" width="200">


# Kelora

[![CI](https://github.com/dloss/kelora/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/dloss/kelora/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/kelora.svg)](https://crates.io/crates/kelora) [![Documentation](https://img.shields.io/badge/docs-kelora.dev-blue)](https://kelora.dev)

Kelora is a command-line tool for reading log files. It recognizes
[20+ formats](https://kelora.dev/latest/reference/formats/) on its own —
application logs, syslog, web server logs, JSON, CSV — and for any other format
you describe the layout on the command line. It splits each line into named
fields and lets you filter, count, and summarize them with short options. For
logic the options can't express, it has a small scripting language
([Rhai](https://rhai.rs)).

A [5-minute introduction video](https://www.youtube.com/watch?v=IwkicmS3RYo)
shows it in action.

## What using it looks like

Checkout is failing. [`examples/shop.log`](https://github.com/dloss/kelora/blob/main/examples/shop.log) is the shop's
plain-text application log. Show only the errors, with the fields you want:

```bash
kelora examples/shop.log -l error -k ts,logger,msg -n 3
```

```
ts='2026-10-06 14:06:28,793' logger='c.e.shop.InventoryClient'
  msg='stock lookup for sku 914 failed: 503 Service Unavailable'
ts='2026-10-06 14:20:14,827' logger='c.e.shop.PaymentClient'
  msg='payment for order 44643 failed: timeout after 5000ms (provider=adyen)'
ts='2026-10-06 14:20:25,134' logger='c.e.shop.PaymentClient'
  msg='payment for order 46371 failed: timeout after 5000ms (provider=adyen)'
```

Group the errors that differ only in IDs and numbers:

```bash
kelora examples/shop.log -l error --drain -k msg
```

```
templates (2 items):
  17: payment for order <num> failed: timeout after <duration> (provider=adyen)
   2: stock lookup for sku <num> failed: <num> Service Unavailable
```

The [documentation](https://kelora.dev) continues this example: which component
fails, when it started, and how to handle formats Kelora doesn't know.

## Installation

**macOS (Homebrew):**

```bash
brew install dloss/kelora/kelora
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

Windows, `.deb`, `.rpm`, ARM, and BSD builds: see
[Installation](https://kelora.dev/latest/installation/) and
[all releases](https://github.com/dloss/kelora/releases).

Kelora follows semver starting with v1.0 — CLI flags and Rhai functions are
stable.

## Documentation

**[kelora.dev](https://kelora.dev)**

- [Explore a log file](https://kelora.dev/latest/guide/explore/) — the first five minutes
- [Get logs into shape](https://kelora.dev/latest/guide/parse/) — parsing any format, from auto-detection to regex
- [Cookbook](https://kelora.dev/latest/cookbook/) — commands for common questions
- [How it works](https://kelora.dev/latest/how-it-works/) — the pipeline and processing order
- [CLI options](https://kelora.dev/latest/reference/cli-reference/) and [functions](https://kelora.dev/latest/reference/functions/)

Offline, `kelora -h` prints a one-screen summary and `kelora --help` the full
reference. The [`examples/`](https://github.com/dloss/kelora/tree/main/examples) directory holds the sample logs used
throughout the docs.

## Use with AI coding agents

Kelora ships an [Agent Skill](https://github.com/dloss/kelora/blob/main/skills/log-analysis/SKILL.md) for Claude Code and
compatible coding agents. Copy the [`skills/log-analysis/`](https://github.com/dloss/kelora/tree/main/skills/log-analysis)
directory into your agent's skills directory to give it a cheat-sheet for
parsing, filtering, and summarizing logs with Kelora.

## How Kelora is built

Kelora is built with agentic AI development: AI agents generate all
implementation and tests, and I steer requirements rather than writing or
reviewing code. Validation relies on an extensive automated test suite plus
`cargo audit` and `cargo deny`. Kelora is local-only with no networking or
telemetry, enforced by a CI check. The [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) lists all
safeguards and where to send security reports.

This is a single-developer spare-time project, and support is best-effort.

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).

The grok pattern engine in `src/drain/grok/` is derived from the
[grok](https://github.com/daschl/grok) crate and its bundled patterns from
[logstash-patterns-core](https://github.com/logstash-plugins/logstash-patterns-core);
it remains under the [Apache License 2.0](https://github.com/dloss/kelora/blob/main/src/drain/grok/LICENSE).
