# Kelora

**Scriptable log analysis for terminals.**

Kelora helps you parse, filter, transform, and summarize logs in one command. It works with JSON, Logfmt, and mixed text logs, and adds embedded [Rhai](https://rhai.rs) scripting when simple filters are not enough.

For a concrete tour of standout capabilities—pattern mining, embedded JSON extraction, deterministic sampling, pseudonymization, span windows, and more—see **[Core Features](features.md)**.

Kelora is best used as a flexible analysis layer in your toolkit: use it on its own, or combine it with `grep`, `jq`, and other Unix tools.

Watch Hack the Clown's [**5-minute introduction video**](https://www.youtube.com/watch?v=IwkicmS3RYo) to see Kelora in action.

Kelora is AI-generated; see [Development Approach](#development-approach).

<a id="installation"></a>
## Installation

=== "macOS"

    ```bash
    brew tap dloss/kelora && brew install kelora
    ```

    Or download a signed binary: [Apple Silicon](https://github.com/dloss/kelora/releases/latest/download/kelora-aarch64-apple-darwin.tar.gz) | [Intel](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-apple-darwin.tar.gz)

=== "Linux"

    **Binary:**
    ```bash
    curl -LO https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-unknown-linux-musl.tar.gz
    tar xzf kelora-x86_64-unknown-linux-musl.tar.gz
    sudo mv kelora /usr/local/bin/
    ```

    **Debian/Ubuntu:** download [.deb](https://github.com/dloss/kelora/releases/latest), then:
    ```bash
    sudo dpkg -i kelora_*_amd64.deb
    ```

    **Fedora/RHEL:** download [.rpm](https://github.com/dloss/kelora/releases/latest), then:
    ```bash
    sudo dnf install kelora-*.x86_64.rpm
    ```

    **ARM:** see [releases](https://github.com/dloss/kelora/releases) for aarch64 binaries.

=== "Windows"

    Download [kelora-x86_64-pc-windows-msvc.zip](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-pc-windows-msvc.zip), extract, and add to PATH.

=== "Cargo"

    ```bash
    cargo install kelora
    ```

=== "Other"

    See [all releases](https://github.com/dloss/kelora/releases) for ARM Linux, FreeBSD, OpenBSD, and more.

## When to Use Kelora

Reach for Kelora when:

- **You're chaining tools** - Replace `grep | awk | jq | custom-script.py` with one command
- **You're parsing custom formats** - Use simple one-liners for non-standard logs (no regex required!) and output clean JSON
- **Logs have embedded structure** - Extract JSON or key-value pairs buried in text lines
- **You need stateful logic** - Count errors per service, tracking sessions, windowed metrics
- **Fields are inconsistent** - Let missing data or errors be handled gracefully, with summary reports at the end

Kelora prioritizes flexibility over [raw speed](concepts/performance-comparisons.md). It shines for exploratory analysis on **small to medium** log files. For larger files, pre-filter with `jq`, `ripgrep`, or `qsv` -- Kelora [plays well](how-to/integrate-external-tools.md) with all of them.

---

## Examples

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

### 3. Pattern Discovery (Template Mining)
*Scenario: You have thousands of error messages that differ only in IPs, emails, and UUIDs. Find the underlying patterns automatically.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/production-errors.jsonl --drain -k message
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/production-errors.jsonl
    ```

The Drain algorithm clusters similar messages and replaces variable parts with placeholders like `<ipv4>`, `<email>`, `<uuid>`. No regex required.

---

## Advanced Features

Beyond basic filtering and conversion, Kelora includes specialized functions that solve problems you'd otherwise need multiple tools or custom scripts for:

- **[Extract JSON from text](how-to/power-user-techniques.md#extract-json-from-unstructured-text)** - Pull structured data from unstructured lines
  `e.data = e.line.extract_json()`

- **[Deep flattening](how-to/power-user-techniques.md#deep-structure-flattening)** - Fan out nested arrays to flat records
  `emit_each(e.get_path("data.orders", []))`

- **[Pattern normalization](how-to/power-user-techniques.md#pattern-normalization)** - Group errors by replacing IPs, UUIDs, emails with placeholders
  `e.error_pattern = e.message.normalized()`

- **[Deterministic sampling](how-to/power-user-techniques.md#deterministic-sampling-with-bucket)** - Consistent sampling across log rotations
  `--filter 'e.request_id.bucket() % 10 == 0'`

- **[JWT parsing](how-to/power-user-techniques.md#jwt-parsing-without-verification)** - Extract claims without verification
  `e.token.parse_jwt().claims.sub`

- **[Cryptographic pseudonymization](how-to/power-user-techniques.md#multiple-hash-algorithms)** - Privacy-preserving anonymization with HMAC
  `e.anon_user = pseudonym(e.email, "users")`

See **[Power-User Techniques](how-to/power-user-techniques.md)** for real-world examples.

---

## Get Started

**[→ Quickstart (5 minutes)](quickstart.md)** - Install and run your first commands

**[→ Tutorial: Basics (30 minutes)](tutorials/basics.md)** - Learn input formats, filtering, and output

**[→ How-To Guides](how-to/index.md)** - Solve specific problems (including [debugging](how-to/debug-issues.md))

Need to reconstruct one timeline from several already-ordered log shards? See
[Merge Sorted Files by Timestamp](how-to/merge-timestamp-sorted-files.md).

For deeper understanding, see [Concepts](concepts/index.md). For complete reference, see [Glossary](glossary.md), [Functions](reference/functions.md), [Formats](reference/formats.md), and [CLI options](reference/cli-reference.md).

!!! tip "On-call?"
    Jump to **[Incident Response Playbooks](how-to/incident-response-playbooks.md)** for copy-paste commands covering latency spikes, error surges, auth failures, and more.

---

## License

Kelora is open source software licensed under the [MIT License](https://github.com/dloss/kelora/blob/main/LICENSE).

## Development Approach

Kelora is an experiment in agentic AI development using [vibe-coding](https://en.wikipedia.org/wiki/Vibe_coding). AI agents generate all implementation and tests; I steer requirements but do not write or review code. Validation relies on a large automated test suite plus Rust security tools (`cargo audit` and `cargo deny`). Review the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before production use.

This is a spare-time, single-developer project, so support and updates are best-effort.
