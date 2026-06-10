# Kelora

**One command for messy logs.** Parse, filter, transform, and summarize logs across JSON, logfmt, syslog, CSV, plain text, and your own custom formats — with embedded [Rhai](https://rhai.rs) scripting when simple filters aren't enough.

Watch Hack the Clown's [**5-minute introduction video**](https://www.youtube.com/watch?v=IwkicmS3RYo) to see Kelora in action.

[Install Kelora](#installation){ .md-button }

## A quick tour

**You don't even know what's in the file yet. Start there** — no flags, no regex. Kelora decompresses the gzip, recognizes the Apache combined format, and profiles every field with real sample values:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/web_access_large.log.gz --discover
    ```

**Mixed formats in one file are the normal case, not the exception.** Give Kelora a cascade of parsers (`-f json,line`) and it tries each one per line, tagging every event with the winner in `_format` — so you keep the structured lines, drop the noise, and emit clean CSV in a single pass:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f json,line examples/mixed_format.log \
      --filter 'e._format == "json"' -k timestamp,level,msg -F csv
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/mixed_format.log
    ```

**And when those logs are a wall of near-duplicate errors that differ only by hostname, UUID, or timestamp, cut straight to what's actually breaking.** Point `--drain` at a field (`-k msg`, the syslog message here) and it groups near-identical lines by inferring where the values varied — `<fqdn>`, `<uuid>`, `<path>`, `<duration>` — so 742 noisy lines collapse into the handful of patterns causing the noise:

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/syslog_errors.log --drain -k msg
    ```

=== "Input (8 of 742 lines)"

    ```bash exec="on" result="ansi"
    head -8 examples/syslog_errors.log
    ```

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

## More examples

### Filter & Convert (The Basics)
*Scenario: Filter a Logfmt file for slow requests and output clean JSON.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/traffic_logfmt.log \
      --filter 'e.status >= 500 || e.latency_ms > 1000' \
      -F json
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/traffic_logfmt.log
    ```

### Modify & Anonymize (Scripting)
*Scenario: Mask user emails for privacy and convert milliseconds to seconds before printing.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/audit.jsonl \
      --exec 'e.email = "***"; e.duration_sec = e.ms / 1000.0;' \
      --keys timestamp,user_id,email,duration_sec
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    cat examples/audit.jsonl
    ```

### Stateful Analysis (Streaming Stats)
*Scenario: 800 API calls across three endpoints. The average latency looks fine — but the tail might not be. Compute a full distribution summary (avg, min/max, p50/p95/p99) per endpoint in one pass, no external aggregator.*

=== "Command/Output"

    ```bash exec="on" source="above" result="ansi"
    kelora examples/api_latency_incident.jsonl --metrics \
      --exec 'track_stats("latency_" + e.endpoint.after("/", -1), e.response_time_ms)'
    ```

=== "Input Data"

    ```bash exec="on" result="ansi"
    head -3 examples/api_latency_incident.jsonl
    ```

Look at `latency_posts`: the average (~147ms) looks healthy, but p99 is ~880ms — a 6× tail the average hides entirely. `track_stats` maintains streaming state across events (averages and counts directly, percentiles via t-digest), so this scales to files of any size without holding everything in memory. `--exec` runs per event; `--metrics` prints just the tracked metrics at the end (it implies `--quiet`, so individual events are suppressed).

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
