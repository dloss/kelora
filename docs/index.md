# Kelora

**Turn messy logs into structured data.**

Kelora is a scriptable log processor for the command line. Parse structured or semi-structured logs (one format per file/stream), filter with complex logic, and analyze streams using embedded [Rhai](https://rhai.rs) scripting—all in a single binary. It can also extract logfmt/JSON blobs embedded inside a single event.

**Download:** [Windows](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-pc-windows-msvc.zip) | [macOS (Apple Silicon)](https://github.com/dloss/kelora/releases/latest/download/kelora-aarch64-apple-darwin.tar.gz) | [macOS (Intel)](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-apple-darwin.tar.gz) | [Linux (x86_64)](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-unknown-linux-musl.tar.gz) | [Other platforms](https://github.com/dloss/kelora/releases)

**Or install via Cargo:**

```bash
cargo install kelora
```

!!! note "Built with AI"
    Generated entirely by AI agents; validated by a large test suite and Rust security tools; see [Development Approach](#development-approach) and the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before production use.

## When to Use Kelora

Kelora trades speed for programmability. It's slower than grep, awk, and jq, but adds stateful scripting for complex multi-stage transformations.

**Reach for Kelora when:**

- Your logs are **messy** - One format per file/stream, with embedded JSON/logfmt fields you want to pull out
- You're **chaining tools** - Replacing `grep | awk | jq | custom-script.py` with one command
- You need **stateful logic** - Counting errors per service, windowed metrics, lookup tables
- You want **embedded scripting** - Complex transformations without leaving your shell

**Use specialized tools for:**

- **Fast search**: `grep`/`rg` (50-100× faster) - Finding text patterns in logs
- **Simple splitting**: `awk` (faster, ubiquitous) - Field extraction and simple statistics
- **JSON queries**: `jq` (faster, everywhere) - Querying structured JSON documents
- **Interactive exploration**: `lnav` (TUI with SQL) - Browsing logs with a visual interface

Kelora reads from files or stdin and outputs JSON, CSV, or Logfmt. Combine it with [lnav, jq, qsv, and other tools](how-to/integrate-external-tools.md) for visualization, analytics, and storage.

---

## Live Examples

### 1. Filter & Convert (The Basics)
*Scenario: Filter a Logfmt file for slow requests and output clean JSON.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f logfmt examples/traffic_logfmt.log \
      --filter 'e.status.to_int() >= 500 || e.latency_ms.to_int() > 1000' \
      -F json
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/traffic_logfmt.log
    ```

### 2. Modify & Anonymize (Scripting)
*Scenario: Mask user emails for privacy and convert milliseconds to seconds before printing.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/audit.jsonl \
      --exec 'e.email = "***"; e.duration_sec = e.ms / 1000.0;' \
      --keys timestamp,user_id,email,duration_sec
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/audit.jsonl
    ```

### 3. Contextual Analysis (Pipeline Order)
*Scenario: An error occurred. We want to see the error, but enrich it with context from the **previous** log line (even if that line wasn't an error).*

!!! tip "Sequential Pipeline"
    Kelora processes flags in order. We run `--exec` **before** `--filter` so the sliding window can see the normal events before they are discarded.

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/api_errors.jsonl \
      --window 2 \
      --exec 'if e.level == "ERROR" && window.len() > 1 {
          e.prev_ctx = window[1].endpoint;
      }' \
      --filter 'e.level == "ERROR"' \
      -F logfmt
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/api_errors.jsonl
    ```

---

## Advanced Features

Beyond basic filtering and conversion, Kelora includes specialized functions that solve problems you'd otherwise need multiple tools or custom scripts for:

- **[Deep flattening](how-to/power-user-techniques.md#deep-structure-flattening)** - Fan out nested JSON arrays to flat records (`emit_each()`)
- **[Extract JSON from text](how-to/power-user-techniques.md#extract-json-from-unstructured-text)** - Pull structured data from unstructured log lines
- **[Template mining](how-to/power-user-techniques.md#template-mining-with---drain)** - Automatic log pattern discovery and clustering using Drain algorithm
- **[JWT parsing](how-to/power-user-techniques.md#jwt-parsing-without-verification)** - Extract claims without signature verification for debugging
- **[Cryptographic pseudonymization](how-to/power-user-techniques.md#multiple-hash-algorithms)** - Privacy-preserving anonymization with Argon2id + HKDF + HMAC
- **[Pattern normalization](how-to/power-user-techniques.md#pattern-normalization)** - Group error messages by replacing IPs, UUIDs, emails with placeholders
- **[Deterministic sampling](how-to/power-user-techniques.md#deterministic-sampling-with-bucket)** - Hash-based sampling that's consistent across log rotations and distributed systems

See **[Power-User Techniques](how-to/power-user-techniques.md)** for real-world examples. For performance characteristics and when to use specialized tools instead, see [Performance Comparisons](concepts/performance-comparisons.md).

---

## Get Started

**[→ Quickstart (5 minutes)](quickstart.md)** - Install and run your first commands

**[→ Tutorial: Basics (30 minutes)](tutorials/basics.md)** - Learn input formats, filtering, and output

**[→ How-To Guides](how-to/index.md)** - Solve specific problems (including [debugging](how-to/debug-issues.md))

For deeper understanding, see [Concepts](concepts/index.md). For complete reference, see [Glossary](glossary.md), [Functions](reference/functions.md), [Formats](reference/formats.md), and [CLI options](reference/cli-reference.md).

---

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).

## Development Approach

Kelora is an experiment in agentic AI development. Claude Code (Sonnet 4.5) and Codex CLI (gpt-5-codex) generate all implementation and tests; I steer requirements but do not write or review code. Validation relies on the automated test suite plus `cargo audit` and `cargo deny`, so please inspect the code yourself before relying on it in production.

This is a spare-time, single-developer project, so support and updates are best-effort.
