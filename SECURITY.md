# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Kelora, please report it privately:

- **Email:** security@dirk-loss.de
- **Subject line:** "Kelora Security: [brief description]"
- **Include:** Version number, description of the issue, and steps to reproduce

**Please do not open public GitHub issues for security vulnerabilities.**

### What to Expect

Kelora is maintained by a single developer on a best-effort basis. Security reports are taken seriously, but response times reflect this reality:

- **Acknowledgment:** Within 1 week
- **Assessment:** Within 2-4 weeks depending on complexity
- **Resolution:** Timelines depend on severity and complexity; critical issues will be prioritized
- **Disclosure:** After a fix is released, the vulnerability will be documented in the changelog

For critical vulnerabilities requiring immediate action, please clearly mark them as such in your report.

## Supported Versions

Kelora is currently in active development. Security updates and bug fixes are applied to the latest release only. Kelora does not provide backports to older versions.

## Security Measures

Kelora implements multiple layers of security controls to ensure safe operation:

### Automated Testing

- **Hundreds of test functions** covering core functionality, parsers, and edge cases
- **Integration tests** validating end-to-end behavior
- **CI enforcement** on every commit (formatting, linting, tests)

### Dependency Security

- **cargo-audit** runs via `just audit` to detect known vulnerabilities in dependencies
- **cargo-deny** enforces dependency policies via `just deny`:
  - License compliance (MIT, Apache-2.0, BSD, etc.)
  - Advisory checking against RustSec database
  - Duplicate dependency detection
  - Source verification (crates.io only)
- **No-networking policy check** runs via `just check-no-networking`:
  - Rejects common HTTP, websocket, TLS, and telemetry crates in the shipped dependency graph
  - Rejects obvious socket/client API usage in `src/`
  - Runs in CI and release workflows so built-in network support cannot be added silently
- **Subprocess usage check** runs via `just check-subprocess-usage`:
  - Fails if new `Command::new(...)` call sites appear in `src/` without being explicitly reviewed and allowlisted

### Code Safety

- **Clippy enforcement** with warnings-as-errors (`-D warnings`) blocks any lints from merging
- **Memory safety:** Rust's ownership system prevents buffer overflows, use-after-free, and data races

### Development Process

Kelora is developed using AI-generated code (Claude, GPT-5). Validated through automated testing, not manual review. Quality assurance includes:
- Comprehensive automated tests for all features
- Functional validation through testing and usage
- Continuous integration checks on every change
- Security tooling (audit, deny) integrated into development workflow
- Resource limits for Rhai execution (DoS protection presets) are not implemented yet; proposals live in `dev/dos-protection.md`

### CI/CD Pipeline

Every commit and pull request must pass:
- `cargo fmt --all --check` (code formatting)
- `cargo clippy --all-targets --all-features -- -D warnings` (static analysis)
- `bash dev/check-no-networking.sh` (enforce the no-networking policy)
- `bash dev/check-subprocess-usage.sh` (flag new subprocess execution in `src/`)
- `cargo test --all-features` (all tests must pass)

## No-Networking Policy

Kelora is designed to process data locally. The repo includes a small policy check:

```bash
just check-no-networking
```

It checks for common networking and telemetry crates in the production dependency graph, obvious socket/client APIs in `src/`, and obvious spawned network tools such as `curl` or `wget`. CI and release automation run the same script. This is a narrow policy guardrail, not a security audit or proof against malicious behavior.

## Verifying Release Binaries

Release binaries include build provenance attestations (SLSA Level 2). These attestations cryptographically prove that binaries were built by GitHub Actions from the exact source code, not on a compromised machine.

### How to Verify

Requires [GitHub CLI](https://cli.github.com/):

```bash
# Download and extract
wget https://github.com/dloss/kelora/releases/download/v0.8.0/kelora-x86_64-unknown-linux-musl.tar.gz
tar -xzf kelora-x86_64-unknown-linux-musl.tar.gz

# Verify
gh attestation verify kelora --owner dloss
```

Verification checks:
- Binary hash matches build output
- Attestation signed by GitHub (via Sigstore)
- Built on GitHub Actions (not local machine)
- Exact commit and workflow used

### Notes

- Verification requires `gh` CLI and network access
- Only applies to GitHub Releases (not `cargo install` builds)
- See https://slsa.dev for more on supply chain security

## Known Security Advisories

Kelora currently ignores one advisory in `deny.toml`:

- **RUSTSEC-2024-0384** - `instant` crate is unmaintained
  - **Reason:** Transitive dependency via `chrono`, acceptable risk for now
  - **Mitigation:** Monitoring for upstream fixes or alternatives
  - **Impact:** No known exploits affecting Kelora's use case

All other advisories result in build failures.

## Threat Model

### What Kelora Does

- Processes log files locally on your machine
- Executes user-provided Rhai scripts against log data
- Reads from files, stdin, and gzip/zstd compressed streams
- Writes to stdout or files

### What Kelora Does NOT Do

- No network access (no outbound connections)
- No privilege escalation
- No persistent daemons or background processes
- No telemetry or data collection
- No modification of input files unless you explicitly enable filesystem write functions with `--allow-fs-writes`

### Security Boundaries

**Trusted inputs:**
- Rhai scripts provided by the user (via `--exec`, `--filter`, `-E`)
- Configuration files (`.kelora.ini`, aliases)

**Untrusted inputs:**
- Log file contents (may contain attacker-controlled data)
- Standard input streams

**Protections:**
- Log data is parsed and processed but cannot execute code
- Rhai is a capability-based sandbox: a script can only call functions the host registers, and the language itself has no built-in filesystem, network, or system access. Kelora registers a deliberately limited set of capabilities:
  - No network egress: no networking functions are registered, and the no-networking policy check (`just check-no-networking`) is enforced in CI
  - No subprocess execution: scripts cannot shell out, and new `Command::new(...)` call sites in `src/` are flagged by `just check-subprocess-usage`
  - File writes are denied by default and require the explicit `--allow-fs-writes` flag (enables functions such as `append_file()`, `truncate_file()`, and `mkdir()`)
  - Standard output redirection (`>`, `>>`) is controlled by the shell, not Kelora
  - Capabilities Kelora *does* grant: scripts can read environment variables (`get_env`) and, during `--begin`, read files (`read_file`/`read_lines`). Because there is no network or subprocess capability, a script cannot exfiltrate this data itself, but treat scripts from untrusted sources accordingly (see Known Limitations)
- Rhai's default safety limits are active (e.g. call-stack-depth limits guard against stack overflow). However, Rhai's *optional* limits against runaway operations and over-sized data (`max_operations`, `max_string_size`, etc.) default to unlimited and are **not** enabled by Kelora — see Known Limitation 2 on resource exhaustion
- Malformed log entries are skipped with diagnostics (default resilient mode)

### Known Limitations

1. **Rhai script safety:** User-provided scripts execute with the same privileges as the Kelora process. Users should review scripts from untrusted sources.

2. **Resource exhaustion:** There are currently no built-in CPU/time/memory guardrails for Rhai execution. Processing very large files or complex scripts can consume significant resources. Use `--parallel` for large archives and monitor resource usage; apply OS-level limits (`ulimit`, cgroups) when handling untrusted inputs.

3. **Regex complexity:** User-provided regex patterns in scripts could be computationally expensive on crafted input. The regex engine (Rust `regex` crate) has DoS protections, but extremely complex patterns may still be slow.

## Security Best Practices for Users

### Running Kelora Safely

1. **Review scripts before execution:** If using scripts from external sources, review them first
2. **Use `--strict` mode cautiously:** Strict mode fails on parse errors; default resilient mode is safer for production
3. **Limit resource usage:** Use `ulimit` or containerization when processing untrusted files
4. **Keep Kelora updated:** Security fixes are only applied to the latest version
5. **Validate file sources:** Only process log files from trusted sources when handling sensitive data

### Example: Safe Processing of Untrusted Logs

```bash
# Process with resource limits (Linux/macOS)
ulimit -v 2000000  # 2GB virtual memory limit

# With timeout (Linux with GNU coreutils)
timeout 60s kelora -j untrusted.jsonl --filter 'e.level == "ERROR"'

# Or use shell job control (cross-platform)
kelora -j untrusted.jsonl --filter 'e.level == "ERROR"' &
PID=$!
sleep 60 && kill $PID 2>/dev/null

# Use strict mode to fail fast on malformed input
kelora -j logs.jsonl --strict --filter 'e.valid_field'
```

## Dependency Policy

Kelora only uses dependencies from crates.io with the following criteria:

- **License:** Must be compatible with MIT and not impose additional restrictions (no copyleft licenses like GPL)
  - Allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, Zlib, Unlicense, CC0-1.0, MPL-2.0, Unicode-3.0, BSL-1.0
- **Source:** Must come from crates.io (no git dependencies)
- **Maintenance:** Prefer actively maintained crates
- **Security:** No known high-severity vulnerabilities (enforced by cargo-audit)
- **Scope:** Only well-established crates for critical functions (parsing, crypto, compression)

See `deny.toml` for the complete dependency policy configuration and enforcement rules.

## Security Audit History

No formal third-party security audits have been conducted. The project relies on:
- Automated tooling (cargo-audit, cargo-deny, clippy)
- Rust's memory safety guarantees
- Comprehensive test coverage
- Community review (open source)

## Contact

For security concerns, contact: security@dirk-loss.de

For general issues: https://github.com/dloss/kelora/issues
