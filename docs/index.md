# Kelora

**One command for messy logs.** Parse, filter, transform, and summarize logs across JSON, logfmt, syslog, CSV, and plain text — with embedded [Rhai](https://rhai.rs) scripting when simple filters aren't enough.

Watch Hack the Clown's [**5-minute introduction video**](https://www.youtube.com/watch?v=IwkicmS3RYo) to see Kelora in action.

Kelora is AI-generated; see [Development Approach](#development-approach).

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

For a concrete tour of standout capabilities — pattern mining, embedded JSON extraction, deterministic sampling, pseudonymization, span windows, and more — see **[Core Features](features.md)**.

## When Kelora helps

Reach for Kelora when you'd otherwise be writing a throwaway Python script. It's the middle ground between "grep is enough" and "I need a real observability platform."

- **Chained pipelines collapse into one command.** `grep | awk | jq | script.py` becomes `kelora`, with state preserved across the pipeline instead of lost between pipes.
- **Messy formats parse cleanly.** Mixed JSON and plaintext in the same file, key=value pairs inside message strings, nested JSON fanned out to flat rows — without regex gymnastics.
- **Embedded scripting when you need it.** Simple filters are one-liners. When logic gets stateful — session reconstruction, per-service error rates, request/response correlation — there's a full scripting layer.
- **Plays well with your existing tools.** Pipe `ripgrep` or `jq` upstream to pre-filter; pipe Kelora's JSON or CSV output into whatever comes next.

Kelora trades raw [speed](concepts/performance-comparisons.md) for programmability. Simple filters and format conversions handle multi-GB files comfortably; heavy Rhai scripting tops out in the low hundreds of thousands of lines before you'll want to pre-filter. Kelora [plays well](how-to/integrate-external-tools.md) with `ripgrep`, `jq`, `qsv`, and other Unix tools.

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

Kelora is an experiment in agentic AI development: AI agents generate all implementation and tests, and I steer requirements rather than writing or reviewing code. Validation relies on an extensive automated test suite plus `cargo audit` and `cargo deny`. Kelora is local-only with no networking or telemetry, enforced by a CI check.

This is a single-developer spare-time project, and support is best-effort. Review the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before using it on sensitive data in production.
