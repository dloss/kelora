# Changelog

All notable changes to Kelora will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added

- **`-d` shortcut for `--discover`** - Field discovery, now the documented starting point for unknown files, gets a short flag: `kelora app.log -d` (use `-d=json` for the machine-readable form). Featured near the top of `kelora -h`.
- **`--discover-depth=0` for unlimited flattening** - Pass `--discover-depth=0` to fully flatten deeply nested JSON during field discovery, removing the previous hard-coded 3-level cap.

### Changed

- **Breaking: resilient-mode runtime errors no longer fail the process** - In default resilient mode, recovered `--filter` and `--exec` runtime errors are reported as diagnostics but exit `0`. Use `--strict` to fail on runtime errors, or `--assert` to fail on explicit data-quality rules.
- **`--discover` examples in inspect style** - String samples are now quoted and escaped (`"hello"`, `""`, `\n`) matching `-F inspect` rendering, so types are unambiguous at a glance.
- **`--discover` uses full terminal width** - The hard 60-char cap on the examples column is gone; examples grow to fill the available terminal width. Compact layout also spans the full width instead of truncating at ~82 chars.
- **`--discover` tighter field column** - The Field column no longer pads to a 12-char floor, so narrow field names don't waste horizontal space.
- **Parse error summaries include filenames** - Parse error messages now show which file the error came from, making multi-file runs easier to debug.
- **Breaking: invalid `--input-tz` is rejected** - An unrecognized `--input-tz` value (e.g. a typo like `Europe/Berln`) now fails fast during configuration validation with exit code 2, instead of silently falling back to the machine's local time. Silent fallback could shift every timestamp — and thus time filters and span boundaries — without any visible error. Use `local`, `UTC`, or a valid IANA timezone name.
- **`--include` no longer double-executes before the first stage** - An `--include` placed before the first `--filter`/`--exec` stage was loaded into both that stage and a synthesized begin stage, so any top-level statements in the include ran twice (once at startup, then per event). Includes now only form a begin/end script when an explicit `--begin`/`--end` is present; otherwise the include is loaded solely into the adjacent stage. Helper-function-only includes (the documented use) are unaffected; `--begin`/`--end` with includes still work.
- **Breaking: config files are validated strictly** - `.kelora.ini` (and `--config-file`) now reject unknown root keys, unknown sections, and lines that are not a comment, `[section]` header, or `key = value` pair, naming the file and line (with a "did you mean" hint for case mismatches). Previously a typo such as `defualts =` or `[alias]` was silently ignored, leaving defaults/aliases quietly unapplied. Only `defaults` (root) and the `[aliases]` section are recognized.
- **Breaking: failed type annotations yield `()` instead of a string** - For `:int`/`:float`/`:bool` annotations in csv/tsv/cols/regex, a value that can't satisfy the declared type now becomes `()` (explicitly absent, e.g. JSON `null`) in resilient mode instead of silently keeping the original string; the rest of the row is preserved. `--strict` still aborts on the failure. This makes all four typed parsers behave identically and fixes a bug where `cols` ignored `--strict` for conversions. For tolerant coercion with a chosen fallback, drop the annotation and use `to_int`/`to_int_or` in a script stage (e.g. `--exec 'e.status = to_int_or(e.status, 0)'`).

### Fixed

- **Script-error scope restored in `--metrics`/`--drain`** - The "affecting every event" total-failure indicator in the script-error summary is derived from event counts, but data-only modes disabled stats collection to keep the hot path lean — silently dropping the most useful part of the summary exactly where a stuck user lands (e.g. `track_count(e.missing_field)` on a missing field reported a bare error count with no scope). The scope now surfaces in these modes; the advisory follow-up ("…Use `--strict`…") honors `--no-diagnostics` and the suppression implied by data-only modes, and is re-enabled with `--diagnostics`.
- **Parse errors no longer swallowed in `--metrics`/`--drain`** - These modes disabled stats collection, so parse failures produced no summary and exited `0` — contradicting the documented contract that parse errors exit `1` and that exit codes are preserved across quiet/data-only modes. Parse errors are now reported on stderr and exit `1`, matching normal mode. (Plain `--no-diagnostics` on event output keeps its existing fast path.)
- **Zero-result hint for level/time filters on the wrong input** - Running `-l/--levels` against unstructured input that has no level field (e.g. plain `line` logs), or `--since/--until` against input with no parseable timestamp, silently dropped every event with no explanation. Kelora now prints a hint naming the structural cause and a workaround (parse levels with `-f cols/regex` or match text with `--filter`; set `--ts-field`/`--ts-format`), matching the existing "0 events matched" hint for unseen `--filter` fields. A genuine value mismatch (level present but unmatched, timestamps present but out of range) is still treated as a legitimate empty result.
- **`track_stats` metrics now usable in `--end` and `span_close`** - Percentile, average, and cardinality sketches were exposed as raw blobs, making `metrics["foo_p95"]` unusable. They are now properly finalized to scalar values.
- **Exec error counts no longer silently undercounted** - The exec stage's atomic-rollback path was discarding `track_error` writes; error counters now survive rollback.
- **`--discover` HLL cardinality clamped to observation count** - The HyperLogLog estimate could exceed the number of times a field was seen; it is now clamped so "Uniq > Seen" rows no longer appear.
- **`--discover` empty strings render as `""`** - Previously showed a bare gap that looked like a stray separator.
- **Grok alias lookup order** - Fixed unordered alias lookup when building match names.

## [1.5.0] - 2026-04-10

### Added

- **Schema discovery mode** - Added `--discover[=table|json]` for parsed-input schema profiling and `--discover-final[=table|json]` for post-filter/post-transform output schema profiling. Discovery includes nested field flattening, reservoir-sampled examples, and cap warnings for high-cardinality streams.
- **Cascade format mode** - Pass a comma-separated list to `-f` (e.g. `-f json,line`) to try each parser in order per line; the first success wins. Each event is tagged with the winning parser name in `_format`. Supported formats: `json`, `logfmt`, `syslog`, `cef`, `combined`, `line`, `raw`. Put `line` last as a catch-all fallback.
- **Chronological merge for sorted files** - Added `--merge-sorted` to merge multiple already-sorted inputs into one chronological stream with a memory-bounded k-way merge. This is intended for structured batch files with reliable timestamps, works with any input format except `-f auto-per-file`, and aborts on missing timestamps, merge-time parse failures, and per-file disorder.
- **Per-file auto-detection** - Added `-f auto-per-file` for batches where each file is internally consistent but different files use different formats.
- **`--include` now works with `--filter`** - Helper functions defined in an include file (`-I`) can now be called from `--filter` expressions. **Constraint:** the include file must contain only function definitions — top-level statements are rejected with a clear error.
- **New Rhai display helpers** - A cohesive set for scripted terminal summaries:
  - *Numeric:* `human_bytes` / `human_bytes_si`, `format_decimals(v, n)`, `format_percent(ratio, n)`; values just below a unit threshold round up cleanly to the next unit (for example `1.0 GiB` instead of `1024.0 MiB`)
  - *Alignment (Unicode-width-aware):* `ljust(n)`, `rjust(n)`, `center(n)`, `shorten(n)`
  - *ANSI styling:* `red()`, `green()`, `yellow()`, `blue()`, `cyan()`, `magenta()`, `bold()`, `dim()` — chainable; no-op when colour is disabled
  - *Charts:* `bar(value, max, width)` for block bars; `sparkline(array)` for trend lines. For ratios in 0.0–1.0, set `max` to `1.0`.
- **New Rhai projection helpers** - Added `map.keep()` and `map.drop()` for event field projection.
- **New Rhai time/random helpers** - Added `dt.ceil_to()` and `sample_prob()`.
- **New PII validators** - Added `ssn` and `phone` pattern validators to `normalized()`, including SSA-aware SSN checks and NANP-aware validation for US/CA numbers plus permissive support for other international numbers.

### Changed

- **CLI diagnostics** - Improved actionable guidance for wrong mode, wrong format, missing `--keys`, missing input, and common flag conflicts.
- **CLI option validation** - `--discover`/`--drain` conflicts with `--parallel` are now validated at CLI parse time with clearer errors.

### Fixed

- **Parser strict mode behavior** - JSON and CEF parsers now correctly respect the strict parsing flag.
- **Pre-epoch rounding** - Fixed `round_to`/`ceil_to` behavior for timestamps before Unix epoch.
- **Auto-detection behavior** - `-f auto` and `-f auto-per-file` now consistently detect from the first non-empty line, matching the built-in help and format reference. `-f auto-per-file` also now preserves sequential state across files and reports detected formats per file in `--stats`.

## [1.4.10] - 2026-03-10

### Added

- **IPv6 support for network helpers** - Rhai network helper functions now support IPv6 addresses in addition to IPv4
- **Numeric timestamp field support** - `--since`/`--until` timestamp field extraction now works with numeric Rhai values, including JSON logs with integer or floating-point timestamps
- **Unix epoch timestamps with fractional seconds** - Timestamp parsing now accepts float-based Unix epoch values such as `1735566123.456`

### Changed

- **Timestamp parsing performance** - Optimized the timestamp parsing hot path to reduce overhead during log processing
- **Rhai startup performance** - Reduced Rhai scope setup overhead to improve script startup efficiency

### Fixed

- **Quick help output** - Corrected quick help drift so the shortcut help text stays aligned with current behavior

## [1.4.9] - 2026-01-25

### Fixed

- Again fix CI issue with Homebrew token

## [1.4.8] - 2026-01-24

### Fixed

- Fix CI issue with Homebrew token

## [1.4.7] - 2026-01-24

### Fixed

- Fix more CI issues

## [1.4.6] - 2026-01-24

### Fixed

- Fix more CI issues

## [1.4.5] - 2026-01-24

### Fixed

- Fix more Debian and RPM package issues

## [1.4.4] - 2026-01-24

### Fixed

- Debian package build fixed

## [1.4.3] - 2026-01-24

### Added

- **Homebrew tap support** - Install via `brew install dloss/tap/kelora`
- **Debian package builds** - `.deb` packages now included in GitHub releases
- **RPM package builds** - `.rpm` packages now included in GitHub releases

### Fixed

- **Documentation:** Fixed broken installation anchor link

## [1.4.2] - 2026-01-20

### Changed

- **Documentation:** Improved landing page structure, "When to Use Kelora" section clarity, and README formatting

## [1.4.1] - 2026-01-19

### Added

- **Statistical functions for arrays:** New Rhai functions for array aggregation:
  - `sum()` - Calculate sum of numeric array elements
  - `mean()` - Calculate arithmetic mean of numeric array elements
  - `variance()` - Calculate variance of numeric array elements
  - `stddev()` - Calculate standard deviation of numeric array elements
- **`resolve_fields.rhai` example script** - Semantic field resolution for cross-format log analysis. Resolves common log concepts (duration, user, client_ip, error, request_id, status) regardless of field naming convention. Enables queries that work across different log formats without hardcoding field names.
- **Log-analysis skill** - Agent assistance skill for analyzing logs and identifying patterns

### Changed

- **Zero unsafe code:** Removed manual `Send` implementations from `DecompressionReader` and `PeekableLineReader`. The compiler now derives `Send` automatically since all contained types are `Send`. This eliminates the last 2 `unsafe` blocks from the codebase.
- **Documentation:** Added direct download links for common platforms (macOS Apple Silicon/Intel, Linux), highlighted Drain template mining on landing page, added quick-start guidance to high-traffic documentation pages, fixed broken anchor links

### Fixed

- Statistical functions (`sum()`, `mean()`, `variance()`, `stddev()`) now reject mixed types like `min()`/`max()` for consistency

## [1.4.0] - 2026-01-11

### Added
- **Event validation** (`--assert`) - Validate log events against user-defined conditions. Helps ensure data quality and catch unexpected patterns during log analysis. Fails with exit code 1 if any event violates the assertion.

- **`track_cardinality()` function** - Probabilistic cardinality estimation using the HyperLogLog algorithm. Estimates unique counts with ~1% standard error using only ~12KB of memory, regardless of cardinality. Ideal for high-cardinality data (millions of unique IPs, sessions, etc.) where `track_unique()` would consume too much memory.
  - `track_cardinality(key, value)` - Track with default ~1% error rate
  - `track_cardinality(key, value, error_rate)` - Track with custom error rate (0.001-0.26)
  - Output uses `≈` prefix to indicate approximate values
  - Works correctly in parallel mode with proper merge support

### Changed

- Config expansion information now displayed at startup for better visibility into which config files are loaded
- Documentation: Clarified that `--levels` accepts comma-separated values vs multiple flag invocations, with runtime hint for consecutive flags

### Fixed

- Removed truncation of Drain sample output for more complete template samples
- Dependencies: Updated `lru` to 0.16.3


## [1.3.2] - 2026-01-05

### Added

- New drain output modes: `--drain=full` (line ranges, samples, template IDs) and `--drain=json` for automation
- Stable, versioned drain template IDs (`v1:` prefix) with whitespace normalization for long-term comparisons
- Regex-named aliases for Rhai helpers (`extract_regex`, `extract_regexes`, `extract_regex_maps`, `split_regex`, `replace_regex`)
- Linux ARM64/ARM32 release builds added to CI

### Changed

- `--drain` now defaults to a clean count+template table; details are opt-in via `--drain=full`
- Documentation: split `--drain` into a dedicated Template Discovery section and note `normalized()` as a related option
- Filter evaluation now avoids unused `meta/conf/line` scope variables and runs string method predicates natively
- Single-field error messages now hint to use `-s` for available keys

## [1.3.1] - 2026-01-03

### Fixed

- BSD builds no longer require libclang by disabling onig bindgen generation via a vendored grok dependency

## [1.3.0] - 2026-01-02

### Added

- **Drain template mining** (`--drain`) - Automatic log pattern extraction and clustering using the drain-rs algorithm. Identifies recurring log templates by mining message patterns, helpful for discovering log structure in large datasets.
  - `drain_summary()` Rhai function - Returns template mining summary with pattern counts
  - `drain_template_id()` Rhai function - Get the template ID for the current event
  - `drain_template()` Rhai function - Get the template pattern for the current event
  - `drain_filters()` Rhai function - Get drain preprocessing filters (Grok-style patterns for token extraction)
  - `-F drain` output format - Shows template patterns with frequency counts
- **Shell completion generation** (`--completions`) - Generate shell completion scripts for bash, zsh, fish, powershell, and elvish
- **Multiline join option** (`--multiline-join`) - Join multiline log entries into single events using configurable patterns
- **New Rhai functions:**
  - `track_stats()` - Comprehensive statistics tracking combining min, max, avg, count, sum, and percentiles in a single call. Auto-suffixes metrics (e.g., `response_time_min`, `response_time_max`, `response_time_avg`, `response_time_count`, `response_time_sum`, `response_time_p50`, `response_time_p95`, `response_time_p99`). Supports custom percentiles (defaults to [0.50, 0.95, 0.99]).
  - `sample_every(n)` - Counter-based sampling that returns true every Nth call. Fast alternative to `bucket() % n` for approximate sampling in parallel processing. Example: `--filter 'sample_every(100)'` keeps every 100th event.
- **Tailmap output format** (`-F tailmap`) - Visualizes numeric field distributions using SRE-focused tail latency percentiles (p90, p95, p99). Shows `_` (< p90), `1` (p90-p95), `2` (p95-p99), `3` (>= p99), `.` (missing). Uses t-digest for memory-efficient streaming percentile estimation.
- **Keymap output format** (`-F keymap`) - Compact visual format showing the first character of a specified field (requires `-k/--keys` with exactly one field). Shows `.` for empty/missing fields. Similar to levelmap but works with any field.
- **SLSA build provenance attestation** - GitHub releases now include cryptographically signed build provenance for improved supply chain security
- **Incident response playbooks** - Comprehensive how-to guide with 8 ready-to-use playbooks for production incident investigation (API latency spikes, error rate analysis, authentication failures, database performance, resource exhaustion, deployment correlation, rate limits, distributed tracing)
- **Column count flexibility** - `--cols` now accepts count=1 for single-column output

### Changed

- **Interactive mode: CTRL-C to exit** - Double-tap CTRL-C to quickly exit (Node.js REPL style). First press shows hint, second press exits. Counter resets on any input to prevent accidental exits.
- **Interactive mode: `:help` now suggests `-h`** - Points users to quick reference (`-h`) instead of full help (`--help`) for a less overwhelming first experience.
- **Rhai error messages** - Improved raw string syntax error hints
- **Regex functions now warn on invalid patterns** - Functions like `extract_regex()`, `extract_regexes()`, `split_re()`, and `replace_re()` now emit one-time warnings to stderr when receiving invalid regex patterns. Fully backward compatible - existing scripts continue to work identically with fallback values.
- **Internal refactoring** - Improved codebase organization with modular structure for formatters, pipeline processing, parallel execution, format detection, help text, and Rhai functions
- **Documentation improvements** - Added documentation for previously undocumented Rhai functions, cleaned up developer documentation, fixed errors in examples and tutorials (composed pipelines, multiline patterns, incident response playbooks), and refined FAQ guidance with AI development note, external-tools link, and combined emoji/color toggles
- **README** - Updated with docs logo
- **Dependencies** - Updated to latest versions

### Fixed

- **SIGTERM handling for Rhai scripts** - Scripts now respond to SIGTERM signals properly using Rhai's `on_progress` callback. Previously, scripts with infinite loops would hang indefinitely when receiving SIGTERM. The fix adds periodic checks (~every 100-1000 operations) with negligible overhead, resulting in sub-10ms response time to termination signals.
- Stats parse error counts now correctly tracked
- CSV header type hints now properly applied in pipeline
- End-stage window availability documentation corrected
- State unavailability now properly enforced in parallel mode
- `track_*` functions now handle unit types correctly
- Error handling test expectations updated for consistency

## [1.2.1] - 2025-12-25

### Added

- **Interactive mode: Tab completion for files and directories** - Press TAB to complete file and directory names, making it easier to specify log file paths

### Changed

- Improved :help text for interactive mode

### Fixed

- OpenBSD build compatibility by pinning `home` crate to v0.5.9 (transitive dependency via `rustyline`) to support rustc 1.86.0. The newer v0.5.12 requires rustc 1.88.

## [1.2.0] - 2025-12-25

### Added

- **Interactive mode** - Automatically activated when kelora is run without arguments. Provides a readline-based REPL with:
  - Shell-like argument parsing (handles quoting properly)
  - Automatic glob expansion (*.log, test?.json, etc.)
  - Command history saved to `~/.config/kelora/interactive_history.txt`
  - Ctrl-C returns to prompt instead of exiting
  - Ctrl-D or typing `:exit`/`:quit` to exit
  - Built-in `:help` command for quick reference
  - REPL commands prefixed with `:` to avoid conflicts with filenames
  - Especially helpful on Windows where command-line quoting is difficult

### Fixed

- Crate upload size by excluding documentation assets from published package

## [1.1.1] - 2025-12-25

### Added

- **`--stats` output tracking:** Now displays output levels and keys (after filtering/transformations) in addition to input, showing what the pipeline produces. Output lines only appear when they differ from input.

### Fixed

- Unsafe unwrap in timestamp parsing that could cause panics
- Mutex poison handling in parallel processing
- Panic handling in state.rs replaced with proper error handling

### Changed

- **BREAKING: `track_percentiles()` default percentiles changed from [0.90, 0.95, 0.99] to [0.50, 0.95, 0.99]**
  - Fixes design flaw in function added in v1.1.0 (released 2025-12-23)
  - Rationale: P50 (median) provides better insight into typical behavior than P90. The new defaults give a complete picture: median (P50), good performance (P95), and tail latency (P99)
  - **Impact:** Scripts using `track_percentiles()` without explicit percentile array will now create `*_p50` metrics instead of `*_p90`
  - **Mitigation:** Function is only 2 days old; minimal adoption expected
  - **Workaround:** Explicitly specify percentiles to maintain old behavior: `track_percentiles("key", value, [0.90, 0.95, 0.99])`

## [1.1.0] - 2025-12-23

### Added

- **New Rhai functions:**
  - `extract_email()`, `extract_email(nth)`, `extract_emails()` - Extract email addresses from text with nth indexing support (completes the network identifier extraction trio alongside `extract_ip()` and `extract_url()`)
  - `track_percentiles()` - Streaming percentile estimation using t-digest algorithm for memory-efficient, parallel-safe percentile tracking with ~1-2% accuracy (auto-suffixes metric names like "latency_p95", "latency_p99")
  - `to_float(thousands_sep, decimal_sep)`, `to_int(thousands_sep)` - Explicit number format parsing for different locales (US: `"1,234.56".to_float(',', '.')`, EU: `"1.234,56".to_float('.', ',')`, French: `"1 234,56".to_float(' ', ',')`). The `thousands_sep` removes any character in the string for handling messy data, while `decimal_sep` must be single-character or empty
  - `to_float_or(thousands_sep, decimal_sep, default)`, `to_int_or(thousands_sep, default)` - Fallback variants with default values

### Changed

- Input formats now listed in both `-h` (short help) and `--help` output for easier discovery

## [1.0.0] - 2025-12-16

### Added

- 🎉 **Stable release!** CLI flags, Rhai functions, and exit codes now follow semantic versioning. Breaking changes only in major versions.
- **New Rhai functions:**
  - `dt.round_to()` - Round timestamps down to interval boundaries for time bucketing and histograms (supports duration strings like "5m", "1h", "1d")
  - `absorb_regex()` - Extract structured data from text fields using regex named capture groups, completing the absorb family alongside `absorb_kv()` and `absorb_json()` (includes LRU cache for compiled patterns to improve performance)

### Changed

- Documentation: Aligned compatibility stance across developer documentation and added stability guarantees
- Documentation: Added `meta.parsed_ts` examples to time tutorial showing gap detection and hourly bucketing use cases

### Fixed

- Stack traces removed from user-facing errors (missing include files, configuration errors) - now shows clean, actionable error messages instead of internal Rust backtraces

## [0.14.0] - 2025-12-06

### Added

- **New Rhai functions:**
  - `track_avg()` - Streaming average aggregation with map-reduce support for parallel processing
  - `parse_url()` now accepts path-only inputs (e.g., "/api/v1/users") without requiring a full URL
- `to_json()` function now accepts optional indent parameter for pretty-printing JSON output
- `meta.parsed_ts` now exposed in Rhai scripts for accessing the parsed timestamp value
- Log shipper integration guide and quick-reference entry for common pipelines (e.g., Filebeat, Vector, Logstash)
- Positive flag variants for config overrides: `--force-emoji`, `--diagnostics`, and `--script-output` allow overriding config file defaults (e.g., override `defaults = --no-emoji` with `--force-emoji` on the command line)
- "Last one wins" semantics for conflicting flags (`--force-color`/`--no-color`, `--force-emoji`/`--no-emoji`, etc.) enabling flexible config overrides

### Removed

- **Breaking:** `-F none` output format removed; use `-q` or `--quiet` instead to suppress event output
- **Breaking:** Weak/legacy hash algorithms removed from `hash()`; only `sha256` (default) and `xxh3` remain
- **Breaking:** Deprecated `has_matches()` helper removed; use `matches()` for regex checks instead

### Changed

- **Breaking:** `--no-events` renamed to `--quiet` (short flag remains `-q`)
- **Breaking:** Default input format switched to content-based detection (`-f auto`) instead of raw lines
- **Breaking:** `--metrics` now defaults to full output instead of abbreviated. Use `--metrics=short` for the old behavior (first 5 items). The `table` format has been renamed to `short` for clarity.
- **Breaking:** Regex extraction functions renamed for clarity (`extract_pattern` → `extract_regex`, `extract_all_pattern` → `extract_all_regex`)
- **Breaking:** File I/O failures now exit 1 (processing error) and respect `--strict` mode instead of being silently tracked
- **Breaking:** Using both `--include` and `--filter` together now exits 2 (usage error) instead of warning
- Auto-detect now emits detection/fallback notices (TTY-aware) and a parse-failure warning when the auto-chosen format mostly fails
- Auto-detection now uses actual parsers for CEF, Syslog, Combined, and Logfmt formats instead of heuristics, improving accuracy and reducing code by ~260 lines
- Warning messages now use 🔸 (orange diamond) vs ⚠️ (warning sign) for errors
- Metrics and stats headers now only shown when using `--with-metrics` or `--with-stats` flags
- `--save-alias` now resolves referenced aliases when updating an alias in place while preserving composition when saving under a new name
- `--show-config` output now uses `#` as header prefix instead of `Config:`
- `--mark-gaps` output format humanized for better readability
- Help screen organization improved with better categorization of output and config options; emojis removed from help screens (still available in main output unless `--no-emoji` is used)
- Error messages significantly improved: now concise and actionable with better identification of missing fields, error suggestions in exec summaries, and clearer handling of Unit type errors
- Documentation updated to use method syntax consistently, improved function examples for `pluck()`, `track_top()`, and `track_bottom()`, and clarified output suppression across help screens

### Fixed

- Directory inputs are now treated as errors in non-strict mode with clear messaging, while still continuing; strict mode fails immediately
- File failure tracking now works correctly across threads using atomic counters
- Error messages no longer have leading blank lines; emoji display now checks stderr TTY instead of stdout
- Parallel stdin format detection now preserves stats correctly
- Error summary now includes file-open failures (with the offending filename) so missing inputs are visible at the end of a run
- Missing input files now report the offending filename in the pipeline error (covers multi-file invocations)
- Documentation corrections across help screens (flag references, filter examples, time syntax)

## [0.13.1] - 2025-11-30

### Added

- `just coverage` helper wrapping `cargo-llvm-cov` (with LLVM tools checks) that generates an HTML report by default

### Changed

- Filter execution now reuses precompiled ASTs and takes a native fast path for simple comparisons to cut Rhai overhead
- Event ingest stats and JSON parsing optimized (owned JSON→Dynamic conversion, deduped discovered level/key tracking) with stats collection skipped when diagnostics are suppressed
- Documentation refreshed with clearer log-level filter examples, performance guidance, and a more visible docs link in the README

### Fixed

- Coverage/integration test runs now rely on `CARGO_BIN_EXE_kelora` and disable subprocess profraw files; benchmark filters use the event binding to measure correctly

## [0.13.0] - 2025-11-29

### Removed

- **Breaking:** Missing-field warning detection and the `--no-warnings` flag (filter/exec now behave without implicit field warning tracking)

### Added

- FreeBSD release builds in the CI/CD pipeline

### Changed

- Exec examples consolidated across docs to avoid duplication and keep guidance in sync
- CLI help/time tutorial now documents all auto-detected timestamp fields

## [0.12.2] - 2025-11-29

### Fixed

- JWT base64 padding detection now works on OpenBSD/older Rust toolchains (no reliance on `is_multiple_of`)

## [0.12.1] - 2025-11-28

### Added

- **Release improvements:**
  - CHANGELOG.md content now included in GitHub release notes
  - Detailed Added/Changed/Fixed sections appear alongside auto-generated changelog links

### Fixed

- Duplicate "Full Changelog" entries in GitHub releases (was appearing 4-6 times)
- OpenBSD release builds (updated vmactions/openbsd-vm to v1)

## [0.12.0] - 2025-11-27

### Added

- **Span aggregation modes:**
  - `--span-mode field` - Aggregate spans by field values (e.g., group by request ID)
  - `--span-mode idle` - Aggregate spans by idle time gaps
  - Comprehensive span variant tests
- **Build improvements:**
  - OpenBSD release builds in CI/CD pipeline
- **Documentation:**
  - Behaviour-neutral optimization ideas
  - Span aggregation mode documentation
  - Warning behavior note: unguarded `e.field` reads emit warnings; `get_path`/`has`/`??` stay quiet
  - Boolean logic filter examples expanded for complex filters
  - Unit test to pin the compact missing-field warning summary format

### Changed

- **Breaking:** Script output (`print()`/`eprint()`) now allowed in `--silent` mode
- Clarified documentation on single-format streams and embedded extraction
- Missing-field warnings now use a compact single-line summary with an orange diamond marker; other warning prints align to the same icon while errors stay unchanged

## [0.11.2] - 2025-11-26

### Changed

- Improved mixed-format log diagnostics with format hints
- Updated metrics option references to new `--metrics` syntax throughout documentation

### Fixed

- Examples count in documentation

## [0.11.1] - 2025-11-26

### Fixed

- Unhelpful exec error messages now preserve full diagnostic context

## [0.11.0] - 2025-11-26

### Added

- **New Rhai functions:**
  - `track_top()` and `track_bottom()` - Top-N aggregation with count and weighted modes
  - Parallel-safe with bounded memory O(keys × N)
  - Pretty-printed output with rankings and stable sorting
- **Field access warning system:**
  - Detects missing field access with helpful suggestions
  - Levenshtein distance algorithm for field name suggestions
  - Case mismatch detection for field names
  - Context-aware tips based on warning frequency
  - AST-based field access detection for better accuracy
  - `--no-warnings` flag to suppress warnings
  - Warnings automatically suppressed with `--silent` and `--no-diagnostics`
- **Documentation improvements:**
  - Pipeline architecture diagrams
  - Try/catch guidance in error handling docs
  - Working quick help examples
  - Restructured troubleshooting docs aligned with Diataxis
  - Improved README with concrete examples and clearer positioning

### Changed

- **Breaking:** Stats/metrics flags redesigned for stdout-first workflow:
  - `-s` now means stats-only (outputs to stdout, was stats+events)
  - `-m` now means metrics-only (outputs to stdout, was metrics+events)
  - Use `--with-stats` or `--with-metrics` to show data alongside events
  - Removed `-S`, `--stats-only`, `--metrics-only`, `--metrics-json`
  - Added format parameters: `--stats=json`, `--metrics=json`
- **Breaking:** `conf` object now immutable in Rhai scripts
- **Breaking:** Strict `emit()` handling enforced
- **Rhai error diagnostics significantly improved:**
  - Call stack display with expanded hints
  - Shows called argument types in error messages
  - Better suggestions and hints for missing fields
  - Explains unit type in error messages
  - Honors `--no-emoji` flag in diagnostics
  - Avoids double emoji prefixes
  - Hints missing fields when unit arguments appear
- Improved no-input error message to clarify missing filename vs `--no-input` flag
- Updated Rhai to v1.23
- Updated dependencies (Cargo update)
- Renamed windowed stage labels to normal names
- Clarified format hint message for missing `-f` flag

### Fixed

- Old `--stats-only`/`--metrics-only` flag references in documentation
- Step numbering in metrics tutorial
- False positive warnings for field assignments
- Parallel warning reporting in parallel mode
- Alias test terminal noise (now captured)
- Format hint message clarity
- Docs/example mismatch in window example
- Example scripts handling of multiple timing field formats
- `patterns.rhai` syntax and error reporting
- Markdown formatting (empty lines before bullet lists)
- Rust 2024 future incompatibility warning

## [0.10.1] - 2025-11-21

### Added

- **Global state map for Rhai scripts:**
  - `state` global map for persisting data across log events
  - Full Rhai map API support (get(), set(), keys(), values(), etc.)
  - `to_map()` conversion for StateMap compatibility
  - Comprehensive documentation in `--help` and `--help-functions`
- **CLI flags:**
  - `--help-formats` - Format reference documentation
- **Documentation improvements:**
  - Comprehensive power-user techniques guide
  - Runnable example files for power-user techniques
  - Power-User Techniques added to docs navigation
  - Interactive tabbed examples throughout documentation
  - Interactive examples for string extraction and multi-format logs
  - Advanced features section on landing page
  - Missing `audit.jsonl` example file
- **UX improvements:**
  - Hint formatter for tips and guidance
  - Hint for unseen metrics output
  - Auto-detected format now shown as info instead of warning

### Changed

- Polished help page titles to reduce redundancy
- Standardized help page footers to reference `-h` consistently
- Improved landing page structure and messaging
- Simplified integration and ecosystem messaging
- Clarified Kelora input/output capabilities in documentation
- Replaced comparison table with narrative format
- Reordered use cases from concrete to abstract

### Fixed

- Suppressed leading newline in diagnostics when no events are output
- Fixed metrics banner newline handling under quiet mode
- Fixed error suppression guidance in diagnostics summary
- Fixed `emit()` documentation references
- Fixed incorrect `-q` reference in web traffic docs
- Fixed JWT expiration tracking example
- Cleaned up dead code allows and unused items

## [0.10.0] - 2025-11-19

### Added

- **Regex input format parser (#17):**
  - Parse logs using regex patterns with named capture groups
  - Type annotations for automatic field conversion
  - `--help-regex` comprehensive documentation
  - Fuzzing harnesses for regex parser robustness
- **New Rhai function:**
  - `text.extract_json([nth])` - Extract JSON objects from unstructured text (#19)
- **CLI flags:**
  - `--head N` - Limit number of input lines read (complements `--take` for output)
  - `-h` - Quick reference (one-screen cheat sheet)
  - `--help` - Full CLI reference (detailed, auto-generated)
- **UX improvements:**
  - Enhanced field-not-found errors to suggest `--stats` and `-F inspect`
  - Smart fatal error summaries in `--silent` mode with actionable context
  - Format detection hints when `-f` is missing
  - "Discovering Fields" section in `--help-examples`
  - Improved `--help-examples` with filenames and modern syntax
- **Documentation:**
  - `examples/README.md` as discovery guide with file navigation
  - Performance limitations guidance in documentation
  - Cargo.toml dependency documentation with purpose comments
  - Year-less timestamp format warnings

### Changed

- **Breaking:** Replaced `--help-quick` with `-h` following standard CLI conventions (ripgrep, fd, cargo)
- **Breaking:** Revamped quiet/silent controls for clearer semantics and flag interactions
- **Breaking:** Anchored timestamp syntax renamed for clarity:
  - `start+DURATION` → `since+DURATION`
  - `start-DURATION` → `since-DURATION`
  - `end+DURATION` → `until+DURATION`
  - `end-DURATION` → `until-DURATION`
  - Added: `now+DURATION` and `now-DURATION` for current time anchoring
- **Breaking:** Regex parser now returns errors for non-matching lines instead of silently skipping
- Preserve `--no-emoji` flag in saved aliases

### Fixed

- **Critical:** Regex parsing failure on file inputs (worked on stdin, failed on files due to newline handling)
- Syslog year rollover bug for year-less timestamps
- u64 JSON precision loss by adding explicit u64 conversion path
- Signal exit codes with comprehensive integration tests
- Function API inconsistencies and documentation gaps
- Test failures caused by timezone and environment dependencies
- Syntax errors in `examples/README.md` and `patterns.rhai`
- Broken anchor links in documentation and glossary

## [0.9.1] - 2025-11-12

### Added

- **Anchored timestamp syntax for `--since` and `--until`:**
  - `since+DURATION` and `since-DURATION` - anchor to `--since` value
  - `until+DURATION` and `until-DURATION` - anchor to `--until` value
  - `now+DURATION` and `now-DURATION` - anchor to current time
  - Examples: `--since "10:00" --until "since+30m"` (30 minutes starting at 10:00)
  - Examples: `--until "now+5m"` (next 5 minutes)
  - Enables duration-based time windows without manual calculation
- **New Rhai functions:**
  - `absorb_kv()` - Extract key-value pairs from unstructured text with format preservation
  - `absorb_json()` - Parse JSON embedded in text fields
- **Documentation improvements:**
  - Comprehensive glossary and documentation analysis
  - Rewritten quickstart showcasing custom format parsing
  - Improved tutorial structure and examples
  - Time reference documentation
  - Anchored timestamp syntax examples and reference

### Changed

- **Breaking:** `parse_kv()` now skips tokens without separator (more forgiving behavior)
- Improved `--help-functions` terminal output readability
- Enhanced tutorial documentation with better examples and patterns

### Fixed

- Field ordering when only `--exclude-keys` is used
- Function name documentation: `has()` instead of incorrect `has_field()`
- ANSI color rendering in documentation

## [0.9.0] - 2025-11-03

### Added

- **New Rhai functions:**
  - `pluck()` and `pluck_as_nums()` - Extract array elements by index with silent failure handling
  - Micro search helpers for fast substring matching (`find()`, `rfind()`, `contains()`, etc.)
- **CLI flags:**
  - `--no-input` flag to run scripts without reading log input
  - `--normalize-ts` flag to normalize primary timestamps
- **Python-style array slicing** support (e.g., `arr[-1]`, `arr[1:3]`)
- **Raw string literals** support in Rhai scripts for easier regex patterns
- **Fuzzing infrastructure:**
  - JSON parser fuzzing harness with cargo-fuzz
  - Hardened timestamp parsing against edge cases
- **Property-based testing** with proptest for:
  - Format auto-detection
  - Type converters
  - Flattening operations
  - Timestamp handling
- **Documentation improvements:**
  - Raw strings documentation in help text and cheatsheet
  - External tools integration guide (kubectl, docker, pirkle, jtbl)
  - Example data files for `--help-examples`
- **Configuration access:**
  - `conf` object now available in `--end` stage for post-processing

### Changed

- **Breaking:** Renamed `or_unit()` to `or_empty()` for better clarity
- **Breaking:** Renamed `--convert-ts` flag to `--normalize-ts` for consistency
- Simplified help examples to use `??` operator instead of `get_path()`
- Enhanced stats output with better timestamp label alignment
- Condensed README and pointed to full documentation site
- Updated to Rust 2024 edition compatibility

### Fixed

- Timezone inconsistency in result timestamp tracking
- Documentation links and formatting issues
- Chrono duration overflow in relative time calculations
- Timestamp clamping during fuzzing to prevent panics
- Pirkle tool examples and documentation accuracy

## [0.8.1] - 2025-10-28

### Added

- Split integration tests into 18 category-specific modules for better organization
- Enhanced documentation structure to eliminate redundancy

### Changed

- Clarified AI development process across documentation
- Improved multiline documentation with better CLI syntax alignment

## [0.8.0] - 2025-10-25

## [0.7.2] - 2025-10-22

## [0.7.0] - 2025-10-19

## [0.6.3] - 2025-10-12

## [0.6.1] - 2025-10-08

## [0.6.0] - 2025-10-04

## [0.5.0] - 2025-10-01

## [0.4.0] - 2025-09-23

## [0.3.0] - 2025-09-14

## [0.2.3] - 2025-08-28

## [0.2.2] - 2025-07-27

## [0.2.0] - 2025-07-27

## [0.1.1] - 2025-05-24

## [0.1.0] - 2025-05-24

_Initial release (yanked)._

---

[Unreleased]: https://github.com/dloss/kelora/compare/v1.5.0...HEAD
[1.5.0]: https://github.com/dloss/kelora/compare/v1.4.10...v1.5.0
[1.4.10]: https://github.com/dloss/kelora/compare/v1.4.3...v1.4.10
[1.4.3]: https://github.com/dloss/kelora/compare/v1.4.2...v1.4.3
[1.4.2]: https://github.com/dloss/kelora/compare/v1.4.1...v1.4.2
[1.4.1]: https://github.com/dloss/kelora/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/dloss/kelora/compare/v1.3.2...v1.4.0
[1.3.2]: https://github.com/dloss/kelora/compare/v1.3.1...v1.3.2
[1.3.1]: https://github.com/dloss/kelora/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/dloss/kelora/compare/v1.2.1...v1.3.0
[1.2.1]: https://github.com/dloss/kelora/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/dloss/kelora/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/dloss/kelora/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/dloss/kelora/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/dloss/kelora/compare/v0.14.0...v1.0.0
[0.14.0]: https://github.com/dloss/kelora/compare/v0.13.1...v0.14.0
[0.13.1]: https://github.com/dloss/kelora/compare/v0.13.0...v0.13.1
[0.13.0]: https://github.com/dloss/kelora/compare/v0.12.2...v0.13.0
[0.12.2]: https://github.com/dloss/kelora/compare/v0.12.1...v0.12.2
[0.12.1]: https://github.com/dloss/kelora/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/dloss/kelora/compare/v0.11.2...v0.12.0
[0.11.2]: https://github.com/dloss/kelora/compare/v0.11.0...v0.11.2
[0.11.1]: https://github.com/dloss/kelora/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/dloss/kelora/compare/v0.10.1...v0.11.0
[0.10.1]: https://github.com/dloss/kelora/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/dloss/kelora/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/dloss/kelora/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dloss/kelora/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/dloss/kelora/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/dloss/kelora/releases/tag/v0.8.0
[0.7.2]: https://crates.io/crates/kelora/0.7.2
[0.7.0]: https://crates.io/crates/kelora/0.7.0
[0.6.3]: https://crates.io/crates/kelora/0.6.3
[0.6.1]: https://crates.io/crates/kelora/0.6.1
[0.6.0]: https://crates.io/crates/kelora/0.6.0
[0.5.0]: https://crates.io/crates/kelora/0.5.0
[0.4.0]: https://crates.io/crates/kelora/0.4.0
[0.3.0]: https://crates.io/crates/kelora/0.3.0
[0.2.3]: https://crates.io/crates/kelora/0.2.3
[0.2.2]: https://crates.io/crates/kelora/0.2.2
[0.2.0]: https://crates.io/crates/kelora/0.2.0
[0.1.1]: https://crates.io/crates/kelora/0.1.1
[0.1.0]: https://crates.io/crates/kelora/0.1.0
