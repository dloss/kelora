# Changelog

All notable changes to Kelora will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

This is the **2.0** line. The headline changes are a redesigned tracking-function family, a set of named application-log formats, composable parser cascades, and a much more capable `--discover` mode. Breaking changes are flagged below — most affect tracking scripts and a few error/validation behaviors. See each entry for migration notes.

### Added

- **Named application-log formats** - A curated set of common application-log layouts — `glog` (Go/klog), `nginx-error`, `apache-error`, `log4j`/Java, `python-logging`, `redis`, `s3` (AWS S3 access log), `haproxy` (http/tcp), and `iso8601-level` (generic ISO-8601 timestamp + level) — are now recognised and parsed into structured fields (`ts`, `level`, `msg`, plus format-specific extras) via the regex engine. They are first-class: selectable with `-f <name>` (e.g. `-f log4j`), usable inside cascade lists (`-f log4j,line`), shown by name in the auto-detect notice and `--stats` ("Detected format: log4j"), and documented in `--help-formats`. During auto-detection they are tried only as the last step before the `line` fallback, so no format Kelora already detected changes. Formats whose source emits more than one line layout (e.g. `s3`, `haproxy`) carry multiple patterns tried in order. Year-less timestamps (`glog`, `redis`) resolve assuming the current year, as syslog already does; `haproxy` lines are syslog-wrapped, so under `-f auto` they detect as `syslog` — pass `-f haproxy` for the structured fields. The definitions are adapted from [lnav](https://lnav.org) (BSD-3-Clause; see `THIRD_PARTY_LICENSES.md`).
- **Composable `cols:`/`regex:` cascades via repeated `-f`** - `-f` is now repeatable, building a cascade from each spec in order: `kelora -f json -f 'cols:ts(2) level *msg' app.log`. This is the only way to put spec-based parsers (`cols:`, `regex:`) into a cascade — a comma list still can't, because a regex pattern may itself contain commas. It closes the common "JSON lines mixed with custom `timestamp LEVEL message` plain text in one file" case (for the standard layouts, prefer the named formats above): previously the text lines fell through to raw `line` and their level was never parsed, so `-l error` silently dropped them. Catch-alls (`line`, `raw`, and `cols:`, which match every line) must come last; `regex:` is selective and may sit earlier, falling through to a later catch-all. A single `-f` (including a comma list) is unchanged.
- **Data-driven legends for map outputs** - The `levelmap` and `keymap` formats now append a one-line legend that decodes their glyphs, matching what `tailmap` already did. Legends are built from the data actually seen, not a fixed table: `levelmap` lists the level strings behind each first-letter glyph (`E = ERROR | I = INFO | W = WARN`, colored to match the map), and `keymap` groups the full field values that collapse to the same first character (`2 = 200,204 | 4 = 404 | 5 = 500,503`). Absent values are labeled (`? = (none)` for `levelmap`, `. = (missing)` for `keymap`). New `--legend` / `--no-legend` flags control all three map formats: by default the legend shows only on an interactive terminal, so piped or redirected output stays clean; `--legend` forces it through a pipe and `--no-legend` suppresses it (including `tailmap`'s, which was previously always on).
- **`-d` shortcut and richer output for `--discover`** - Field discovery, now the documented starting point for unknown files, gets a short flag: `kelora app.log -d` (use `-d=json` for the machine-readable form), featured near the top of `kelora -h`. `--discover-depth=0` fully flattens deeply nested JSON, removing the previous hard-coded 3-level cap. See the Changed section for the expanded discover footer (timestamp field, input parser, counts).
- **`e.get()` map accessor** - `e.get("key")` and `e.get("key", default)` are now registered for events, mirroring `get_path` for top-level keys and matching the hint already shown in missing-field errors. Part of a clearer missing-field mental model (see Changed).
- **Keyword search in `--help-functions`** - `--help-functions [KEYWORD]` now filters the 150+ function catalogue by a case-insensitive substring match against each function's name, description, continuation lines, or section header, instead of forcing a scroll through the whole list: `kelora --help-functions ip` lists the IP-related helpers, `--help-functions string` lists the whole STRING section. Matching entries print under their preserved section headers; `--help-functions=KEYWORD` works too, and a no-match prints a hint to run the bare flag for the full catalogue. Bare `--help-functions` is unchanged.
- **Intent-based hints for unknown flags borrowed from other tools** - When an unknown flag is one of a small curated set of names users reach for out of habit, kelora now prints guidance toward the kelora idiom instead of clap's edit-distance guess (which matched on string distance, not intent — `--sort` → `--assert`, `--where` → `--help-regex`). `--where`/`--grep`/`--match` point to `--filter`; `--sort`/`--top`/`--rank` point to `track_top_by` in an `--exec` stage; `--count`/`--group-by`/`--uniq` point to `track_count`. These are **not** aliases: the flags stay unknown and still exit 2, so no namespace is reserved and a real `--sort`/`--top` flag could be added later without a breaking collision. Genuine near-miss typos (e.g. `--filer`) still get clap's suggestion unchanged.
- **`-P` short flag for `--parallel`** - The performance toggle now has a short form, following the `xargs`/GNU `parallel` convention.

### Changed

- **Breaking: tracking-function redesign (`track_count`, `track_bucket`, `track_top`/`track_bottom`)** - The tracking family is consolidated around one convention: `track_fn(name, categorical-or-numeric args...)`.
    - `track_count(name, category)` replaces both the old one-argument `track_count(value)` and `track_bucket(key, bucket)` (which were the same operation under two names). Categories may be strings, numbers, or bools — they are stringified into the category key, so `track_count("status", e.status)` works without `to_string()`. Counts land in separate per-name sub-maps, so `track_count("level", e.level)` and `track_count("method", e.method)` can no longer collide. The old forms error with a migration hint; for a plain counter use `track_sum("errors", 1)`.
    - Score-based ranking moved from the 4-argument `track_top(key, item, n, value)` to `track_top_by(name, item, score [, n])` / `track_bottom_by(name, item, score [, n])`; the old 4-argument form errors with a migration hint. `n` now defaults to 10 in all four ranking functions, and items accept numbers and bools.
    - All `track_*` functions now skip Unit `()` values (missing fields) instead of erroring — previously `track_count` failed on every event when the field was absent. Skips are counted per metric and reported via `--diagnostics`, so field-name typos stay detectable.
    - Reusing one metric name across different track functions (e.g. `track_sum("x", ...)` then `track_min("x", ...)`) is now a call-time error. Previously the conflicting values were silently blended into garbage during parallel merging. (In `--parallel`, a name reused across `--begin` vs the event stages is caught only at merge time, as a warning rather than a per-call error.)
    - Float categories keep their 1.x `track_bucket` labels (`200.0` → `"200"`), so migrated scripts and JSON consumers keyed on the old bucket names keep working.
    - `track_unique` warns once past 100,000 stored values (it keeps every distinct value in memory by design); the warning points to `track_cardinality()` for unique counts and honors `--silent`/`--no-diagnostics`.
- **Breaking: resilient-mode runtime errors no longer fail the process** - In default resilient mode, recovered `--filter` and `--exec` runtime errors are reported as diagnostics but exit `0`. Use `--strict` to fail on runtime errors, or `--assert` to fail on explicit data-quality rules.
- **Breaking: config files are validated strictly** - `.kelora.ini` (and `--config-file`) now reject unknown root keys, unknown sections, and lines that are not a comment, `[section]` header, or `key = value` pair, naming the file and line (with a "did you mean" hint for case mismatches). Previously a typo such as `defualts =` or `[alias]` was silently ignored, leaving defaults/aliases quietly unapplied. Only `defaults` (root) and the `[aliases]` section are recognized.
- **Breaking: invalid `--input-tz` is rejected** - An unrecognized `--input-tz` value (e.g. a typo like `Europe/Berln`) now fails fast during configuration validation with exit code 2, instead of silently falling back to the machine's local time. Silent fallback could shift every timestamp — and thus time filters and span boundaries — without any visible error. Use `local`, `UTC`, or a valid IANA timezone name.
- **Breaking: failed type annotations yield `()` instead of a string** - For `:int`/`:float`/`:bool` annotations in csv/tsv/cols/regex, a value that can't satisfy the declared type now becomes `()` (explicitly absent, e.g. JSON `null`) in resilient mode instead of silently keeping the original string; the rest of the row is preserved. `--strict` still aborts on the failure. This makes all four typed parsers behave identically and fixes a bug where `cols` ignored `--strict` for conversions. For tolerant coercion with a chosen fallback, drop the annotation and use `to_int`/`to_int_or` in a script stage (e.g. `--exec 'e.status = to_int_or(e.status, 0)'`).
- **Breaking: ragged CSV/TSV rows are kept, and `--strict` rejects them** - Rows with more columns than the header previously lost the extra fields silently. Overflow columns are now kept under positional names (`c5`, `c6`, …, the same convention as headerless `csvnh`/`tsvnh`), short rows keep their trailing fields absent (preserving `field in e` semantics), and both cases are counted in `--stats` ("Ragged rows: …") with a stderr hint pointing at an inspection filter. `--strict` now treats a ragged row as a parse error (previously `--strict` only governed type conversion), and strict shape errors name where the expected width came from (`expected 4 (from header)` / `(from first line)`).
- **Parallel mode now delivers worker tracking metadata reliably** - Worker threads previously shipped their internal tracking state (operation metadata, error counters) to the global tracker only when the final flush happened to carry output, and mid-run flushes could deliver cumulative counters twice. Internal state is now delivered exactly once per worker, per-batch user deltas are cleared after each send, and the multiline (event-batch) path attaches operation metadata like the line path. This fixes several `--parallel` inconsistencies: `track_avg`/`track_stats` metrics are properly finalized inside `--end` (previously a raw `{sum, count}` map), the skipped-missing-value diagnostic and exec-error summaries now appear as in sequential mode, and multiline runs merge averages correctly instead of keeping the last batch.
- **Faster JSON parsing** - The JSON line parser now deserializes straight into the event's field map via a custom serde visitor, skipping the intermediate `serde_json::Value` tree that dominated worker-thread CPU. Output is byte-identical (numbers, escapes, nesting, error messages); measured ~17–31% faster wall-clock on JSON inputs, with the largest gains on wide objects.
- **`--metrics` renders `track_count` maps as an aligned, sorted list** - In the human-readable `--metrics` text view, a `track_count` (category-count) metric previously dumped raw Rhai map syntax (`status = #{"500": 67, "404": 12}`). It now prints as an aligned list sorted by count descending, matching the existing `track_top`/`track_bottom` style, and truncates to 5 entries (with a "+N more" hint) unless `--metrics=full` is used or the map has 10 or fewer categories. Display-only: the `--metrics-file` and JSON outputs keep the full structured map.
- **`--metrics` text view rounds floats consistently** - Float metrics in the human-readable view are now rounded to 6 significant figures, so a `track_stats` block no longer mixes clean percentiles (`880.16`) with noisy raw floats (`146.6142714694471`). Display-only: stored values and the JSON / `--metrics-file` output keep full precision.
- **`--discover` footer expanded** - The discover table now reports more about what it saw. It identifies the primary timestamp field — the one kelora would use for `--since`/`--until` — and shows it in the footer (`timestamp: ts`, `timestamp: ts (60% parsed)` when some values don't parse, `timestamp: when (--ts-field)` for an override); it names the input parser (`format: cef (auto-detected)`, or `formats: cef 12, json 3 (events)` for cascades/per-file modes) so a mis-detected format is visible; and it moves the scanned-event count to a quiet footer line with a trailing ellipsis on Examples lists that don't cover every distinct value. JSON output (`-d=json`) gains matching `timestamp` and `format`/`format_counts` objects. String examples are now quoted and escaped (`"hello"`, `""`, `\n`) to match `-F inspect`, the examples column grows to the full terminal width (the old 60-char cap is gone), and the Field column no longer pads to a 12-char floor.
- **Clearer missing-field model and hints in Rhai scripts** - A missing field is `()` and access never throws by itself; this single mental model is now documented in `--help-rhai` with the two safe idioms (`e.has`/`e.get`/`??`). Referencing a field without the `e.` prefix (e.g. `status` instead of `e.status`) — the most common newcomer mistake — now suggests `e.<field>` directly when the bare name matches (or closely resembles) a real field, instead of offering string-similar but wrong scope variables.
- **`line`-fallback and mixed-format hints point at field extraction** - When auto-detection keeps whole lines as `line`, the hint now suggests extracting fields from `timestamp LEVEL message` app logs with `-f 'cols:ts(2) level *msg'` (or a `regex:`) and cascading a mixed file with repeated `-f`. When `-f auto` locks onto one format but the input is actually mixed, the parse-failure warning now re-detects the failing line and prints a concrete, copy-pasteable cascade (e.g. `Detected mixed formats (json + line). Try: -f json,line`).
- **`--include` no longer double-executes before the first stage** - An `--include` placed before the first `--filter`/`--exec` stage was loaded into both that stage and a synthesized begin stage, so any top-level statements in the include ran twice (once at startup, then per event). Includes now only form a begin/end script when an explicit `--begin`/`--end` is present; otherwise the include is loaded solely into the adjacent stage. Helper-function-only includes (the documented use) are unaffected; `--begin`/`--end` with includes still work.
- **Parse error summaries include filenames** - Parse error messages now show which file the error came from, making multi-file runs easier to debug.

### Fixed

- **In-place `absorb_*`/`merge` mutations no longer dropped from events** - Whole-event mutating calls — `absorb_kv`, `absorb_json`, `absorb_regex`, `merge`, `enrich`, `rename_field` — and Rhai's in-place collection mutators on a nested field (`e.tags.push(x)`, `e.meta.set(k, v)`, …) were visible within the same script but silently discarded from the emitted event and from later `--exec` stages, unless the script also contained an explicit `e.field = …` assignment. This broke the documented `e.absorb_kv("msg")` workflow (including the quickstart) outright. Kelora now detects these mutators rooted at `e` and runs the write-back; read-only methods (`has`, `get_path`, …) stay unflagged so read-only execs keep their fast path.
- **`span.metrics` no longer silently drops non-additive aggregators** - Inside `--span-close`, `span.metrics` was computed by diffing the global tracker against the span's opening baseline, which only works for additive aggregators. `track_avg`/`track_percentiles` never appeared, and `track_max`/`track_min` surfaced only when a window happened to move the global extreme (and then reported the *global* extreme, not the window's) — yielding silently wrong-or-missing per-window stats for core queries like "max latency per 5-min window". Now: `track_avg` reports the true per-window average (computed as `Δsum / Δcount` from its cumulative `{sum, count}`), joining `track_count`/`track_sum`/`track_unique` as correct per-window values. Genuinely non-additive aggregators (`track_min`, `track_max`, `track_percentiles`, `track_cardinality`, `track_top`/`track_bottom`, `track_top_by`/`track_bottom_by`) cannot be reduced to a single window, so they are omitted and Kelora prints a one-time warning per metric key pointing to the `span.events` workaround (suppressed by `--no-diagnostics`/`--silent`).
- **Filter and exec error counts no longer undercounted** - A `--filter` that errored on every line reported "Filter errors: 1 total" instead of the true count, because the filter error paths skipped the thread-local→context sync that the success path performs (the next event then clobbered the increment). Both filter error branches now persist their counts, matching the exec path. (Exec error counts, separately, were being discarded by the stage's atomic-rollback path and now survive rollback.)
- **Spurious `conf` read-only error fixed** - Reading `conf` in an `--exec` stage and then filtering on the derived field in a separate `--filter` that doesn't name `conf` raised a false "conf map is read-only outside --begin" error, breaking the documented read-in-exec / filter-later pattern. The immutability check is now gated on whether the stage actually references `conf`; genuine `conf` mutations are still rejected.
- **Script-error scope restored in `--metrics`/`--drain`** - The "affecting every event" total-failure indicator in the script-error summary is derived from event counts, but data-only modes disabled stats collection to keep the hot path lean — silently dropping the most useful part of the summary exactly where a stuck user lands (e.g. `track_count(e.missing_field)` on a missing field reported a bare error count with no scope). The scope now surfaces in these modes; the advisory follow-up ("…Use `--strict`…") honors `--no-diagnostics` and the suppression implied by data-only modes, and is re-enabled with `--diagnostics`.
- **Parse errors no longer swallowed in `--metrics`/`--drain`** - These modes disabled stats collection, so parse failures produced no summary and exited `0` — contradicting the documented contract that parse errors exit `1` and that exit codes are preserved across quiet/data-only modes. Parse errors are now reported on stderr and exit `1`, matching normal mode. (Plain `--no-diagnostics` on event output keeps its existing fast path.)
- **Zero-result hint for level/time filters on the wrong input** - Running `-l/--levels` against unstructured input that has no level field (e.g. plain `line` logs), or `--since/--until` against input with no parseable timestamp, silently dropped every event with no explanation. Kelora now prints a hint naming the structural cause and a workaround (parse levels with `-f cols/regex` or match text with `--filter`; set `--ts-field`/`--ts-format`), matching the existing "0 events matched" hint for unseen `--filter` fields. A genuine value mismatch (level present but unmatched, timestamps present but out of range) is still treated as a legitimate empty result.
- **`track_stats` metrics now usable in `--end` and `span_close`** - Percentile, average, and cardinality sketches were exposed as raw blobs, making `metrics["foo_p95"]` unusable. They are now properly finalized to scalar values.
- **`track_count` average false positive** - A `track_count` category map whose categories are literally named `sum` and `count` no longer renders as a bogus average; output finalization keys off the recorded operation instead of sniffing the value's shape.
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
