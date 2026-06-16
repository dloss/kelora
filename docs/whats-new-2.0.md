# What's New in Kelora 2.0

Kelora 2.0 is a feature release with **breaking changes**. The headline additions
are a redesigned tracking-function family, a curated set of named
application-log formats, composable parser cascades, and a much more capable
`--discover` mode.

This page is the migration front door: it leads with what's new, then walks
through every breaking change with **old → new** examples and an upgrade
checklist. For the exhaustive, change-by-change record (including every bug fix
and minor flag), see the [full changelog](https://github.com/dloss/kelora/blob/main/CHANGELOG.md).

!!! tip "In a hurry?"
    Jump to the [Upgrade checklist](#upgrade-checklist). If your scripts use
    `track_count`, `track_bucket`, `track_top`, or `track_bottom`, start with
    [the tracking redesign](#breaking-the-tracking-functions-were-redesigned) —
    that's the change most likely to affect you.

## Highlights

### Named application-log formats

A curated set of common application-log layouts now parse into structured
fields out of the box: `glog` (Go/klog), `nginx-error`, `apache-error`,
`log4j`/Java, `python-logging`, `redis`, `s3` (AWS S3 access log), `haproxy`
(http/tcp), and `iso8601-level`. Select them with `-f <name>`:

```bash
kelora -f log4j app.log -k ts,level,msg
```

They're first-class: usable inside cascades, shown by name in the auto-detect
notice and `--stats`, and documented in `--help-formats`. During
auto-detection they're tried only as the last step before the `line` fallback,
so nothing Kelora already detected changes. (The definitions are adapted from
[lnav](https://lnav.org), BSD-3-Clause — see `THIRD_PARTY_LICENSES.md`.)

A Kelora-original `cri` format covers Kubernetes container logs — the
CRI/containerd on-disk layout `<RFC3339Nano> <stream> <tag> <message>` that
`/var/log/pods/*/*.log`, `kubectl logs --timestamps`, and log shippers emit. It
parses `ts`, `stream` (stdout/stderr), `tag` (`F` full / `P` partial), and
`msg`. Because a CRI message is often itself JSON or logfmt, `cri` is the one
named format detected *early* (before the logfmt/CSV steps) so auto-detection
works regardless of the payload; fan a JSON message back into fields with a
second-stage `--exec 'e.absorb_json("msg")'`:

```bash
kelora pod.log --filter 'e.stream == "stderr"' -k ts,msg
```

### Composable parser cascades with repeatable `-f`

`-f` is now repeatable, building a cascade from each spec in order. This is the
only way to put spec-based parsers (`cols:`, `regex:`) into a cascade, since a
regex pattern may itself contain commas:

```bash
kelora -f json -f 'cols:ts(2) level *msg' app.log
```

This closes the common "JSON lines mixed with custom `timestamp LEVEL message`
plain text in one file" case. Catch-alls (`line`, `raw`, `cols:`) must come
last; a selective `regex:` may sit earlier and fall through to a later
catch-all. A single `-f` (including a comma list) behaves exactly as before.

### `-d` shortcut and a richer `--discover`

Field discovery — the recommended starting point for an unknown file — gets a
short flag and an expanded footer that now reports the primary timestamp field,
the input parser/format, and scanned counts:

```bash
kelora app.log -d           # human-readable profile
kelora app.log -d=json      # machine-readable
```

`--discover-depth=0` now fully flattens deeply nested JSON (the old 3-level cap
is gone).

### Data-driven legends for map outputs

`levelmap` and `keymap` now append a one-line legend decoding their glyphs,
built from the data actually seen (e.g. `E = ERROR | I = INFO | W = WARN`). New
`--legend` / `--no-legend` flags control all three map formats; by default the
legend shows only on an interactive terminal, so piped output stays clean.

### No-script aggregation shortcuts: `--freq`, `--describe`

The two most common aggregations are now plain flags, so you don't have to drop
into Rhai. `--freq FIELD` is a frequency table (`track_freq`) and
`--describe FIELD` is a numeric summary (`track_stats` — count/min/max/avg/
p50/p95/p99). Both run after all filters/transforms and imply `-m`:

```bash
kelora app.log --freq level
kelora app.log --describe duration_ms
```

There's deliberately no `--top`/`--bottom` flag. `--freq` already sorts by
count descending, and — like the new pipe-aware wrapping — metrics output
auto-selects its format: the human table on a terminal, a tab-separated record
stream when piped or redirected. So ranking is left to the shell, which
composes far more flexibly than baked-in selectors:

```bash
kelora app.log --freq url | head     # top-N
kelora app.log --freq url | tail     # bottom-N
kelora app.log --freq url | awk -F'\t' '$3 >= 100'
```

`--metrics=full` forces the table through a pipe; `--metrics=tsv` forces the
stream even to a terminal; `--metrics=json` is unchanged. (Note: `kelora -m … >
file` now writes the `tsv` records rather than the table — add `--metrics=full`
for the old rendering.)

### Smaller niceties

- **`e.get()` map accessor** — `e.get("key")` and `e.get("key", default)`,
  mirroring `get_path` for top-level keys.
- **Keyword search in `--help-functions`** — `kelora --help-functions ip`
  filters the 150+ function catalogue instead of forcing a scroll.
- **Intent-based hints for unknown flags** — habit flags from other tools point
  at the Kelora idiom (`--where`/`--grep` → `--filter`; `--sort`/`--rank` →
  `track_top_by`; `--count`/`--group-by`/`--uniq` → `--freq` / `track_freq`).
  These stay unknown (exit 2), so no namespace is reserved.
- **`-P` short flag for `--parallel`**, following the `xargs`/GNU `parallel`
  convention.
- **`-l/--levels` vocabulary-mismatch warning** — when `-l` drops every event
  because the stream uses a different level dialect than your filter (glog logs
  `I/W/E/F`, syslog uses `CRIT` not `CRITICAL`), Kelora now lists the levels
  actually present (`-l ERROR matched none of the levels present: E,I`) instead
  of returning a silent empty result. Single-letter glog/klog levels are now
  colored in the default and `levelmap` output too.
- **"No input" hint** — a bare `kelora` reading from an empty non-TTY source
  now prints a one-line stderr hint instead of exiting silently.

## Breaking changes & migration

### Breaking: the tracking functions were redesigned

The tracking family is consolidated around one convention:
`track_fn(name, args...)`. This is the change most likely to require edits.

**Frequency tables** — `track_freq(name, value)` counts occurrences of each
distinct value, replacing both the old one-argument `track_count(value)` and
`track_bucket(key, bucket)` (which were the same operation under two names).
Counts now land in separate per-name sub-maps, so different metrics can no
longer collide. Values are stringified automatically.

```bash
# Old (1.x)
kelora app.log --exec 'track_count(e.level)'
kelora app.log --exec 'track_bucket("status", e.status)'

# New (2.0)
kelora app.log --exec 'track_freq("level", e.level)'
kelora app.log --exec 'track_freq("status", e.status)'   # no to_string() needed
```

The name is "freq" rather than "count" because *count* was ambiguous — it read
equally as a per-value frequency table and as a single scalar counter. For a
plain counter, use the dedicated `track_inc("errors")` (or `track_sum("errors", 1)`).

**Score-based ranking** — the 4-argument `track_top(key, item, n, value)` moves
to `track_top_by(name, item, score [, n])` (and likewise
`track_bottom_by`). `n` now defaults to 10 in all four ranking functions.

```bash
# Old (1.x)
kelora app.log --exec 'track_top("slow", e.url, 5, e.ms)'

# New (2.0)
kelora app.log --exec 'track_top_by("slow", e.url, e.ms, 5)'
```

The old forms error with a migration hint, so you won't silently get wrong
results. Other notes:

- **Missing fields are skipped, not errored.** All `track_*` functions now skip
  Unit `()` values instead of failing the event. Skips are counted per metric
  and reported via `--diagnostics`, so typos stay detectable.
- **Name reuse across functions is a call-time error.** Mixing
  `track_sum("x", …)` and `track_min("x", …)` used to silently blend into
  garbage under parallel merging; it now errors.
- **Float value labels are preserved** (`200.0` → `"200"`), so JSON
  consumers keyed on the old `track_bucket` names keep working.
- **Ranking is now exact.** `track_top_by`/`track_bottom_by` (and the legacy
  `track_top`/`track_bottom`) retain every distinct item and rank only when
  metrics are emitted, so a frequent item first seen after the top-N slots
  filled is no longer evicted at count 1 — the 1.x behavior could silently
  return the first N distinct items rather than the most frequent.

| Old (1.x) | New (2.0) |
| --- | --- |
| `track_count(value)` | `track_freq("name", value)` |
| `track_bucket(key, bucket)` | `track_freq(key, bucket)` |
| `track_top(key, item, n, value)` | `track_top_by(key, item, value, n)` |
| `track_bottom(key, item, n, value)` | `track_bottom_by(key, item, value, n)` |
| plain counter via `track_count` | `track_inc("name")` (or `track_sum("name", 1)`) |

### Breaking: a simpler, record-aware exit-code model

The exit code now follows one rule:

> **Kelora exits non-zero when it couldn't do the job you asked — not because the data was messy.**

The model turns on **gates vs. transforms**:

- **Gates — parse and each `--filter` stage — must work.** If a gate never once
  succeeds (no line parses, or a filter errors on *every* event it sees and so
  selects nothing), the output is empty or meaningless, so the run exits `1`.
  Each filter is gated individually, so a working first filter cannot mask a
  completely broken second one.
- **Transforms — exec — are best-effort.** A failing `--exec` rolls back to the
  original event and emits it, so exec errors are reported but never fail the run
  on their own. Use `--strict`/`--assert` to enforce.

Structural failures (a named input that can't be opened) and `--assert`
violations still fail in any mode; `--strict` still escalates any single
parse/filter/exec error.

Two behaviors change from 1.x:

- **A `--filter` that errors on *every* event it sees now exits `1`** (it was `0`). A
  totally broken filter — e.g. the `status >= 500` typo for `e.status >= 500` —
  used to return success with empty output, which silently passed monitoring
  checks ([#241](https://github.com/dloss/kelora/issues/241)). It's now treated
  as the operator error it is. A filter erroring on only *some* events, and any
  `--exec` error (best-effort), are still recovered (exit `0`).
- **A *partial* parse failure now exits `0`** (it was `1`). A few unparseable
  lines among good ones are data noise for a log tool, so the run succeeds with a
  diagnostic. Only an input where **no** line parses (wrong format) still exits
  `1`. Add `--strict` to fail on the first bad line as before.

The signal is computed independently of output collection, so the exit code is
now consistent across `--metrics`, `--drain`, `-q`, and `--no-diagnostics`.

```bash
kelora app.log --strict --exec '…'   # fail on the first runtime/parse error
kelora app.log --assert '…'          # fail on explicit data-quality rules
```

**Action:** if a script relied on a nonzero exit for a broken `--exec`, add
`--strict` (exec is now best-effort). If a pipeline relied on exit `1` for *any*
parse error, add `--strict`. The full model — with a scenario table — is in
[Error Handling](concepts/error-handling.md#exit-codes-the-model).

### Breaking: config files are validated strictly

`.kelora.ini` (and `--config-file`) now reject unknown root keys, unknown
sections, and malformed lines, naming the file and line. Previously a typo such
as `defualts =` or `[alias]` was silently ignored. Only `defaults` (root) and
the `[aliases]` section are recognized. **Action:** check that your config keys
are exactly `defaults` and `[aliases]`.

### Breaking: invalid `--input-tz` is rejected

An unrecognized `--input-tz` (e.g. `Europe/Berln`) now fails fast with exit code
2 instead of silently falling back to local time — which could shift every
timestamp. Use `local`, `UTC`, or a valid IANA timezone name.

### Breaking: failed type annotations yield `()` instead of a string

For `:int`/`:float`/`:bool` annotations in csv/tsv/cols/regex, a value that
can't satisfy the declared type now becomes `()` (explicitly absent) in
resilient mode, instead of silently keeping the original string. `--strict`
still aborts. For tolerant coercion with a chosen fallback, drop the annotation
and coerce in a script stage:

```bash
kelora app.log -f 'cols:status' --exec 'e.status = to_int_or(e.status, 0)'
```

### Breaking: ragged CSV/TSV rows are kept, and `--strict` rejects them

Rows with more columns than the header used to lose the extra fields silently.
Overflow columns are now kept under positional names (`c5`, `c6`, …), short rows
keep trailing fields absent, and both cases are counted in `--stats`. `--strict`
now treats a ragged row as a parse error. **Action:** if you were relying on
silent truncation, expect new `c<N>` fields; add `--strict` to reject ragged
rows instead.

### Breaking: logfmt/CEF stop mangling zero-padded and signed values

The type-inferring parsers (`logfmt`, `cef`) used to coerce *any* token that
Rust's number parser accepted, which silently rewrote data: leading zeros were
dropped (`zip=02134` → `2134`, `id=007` → `7`, `ver=01` → `1`), a leading `+`
was stripped (`phone=+15551234` → `15551234`), and the Rust-only float spellings
`inf`/`nan`/`Infinity` became floats (then `null` on JSON output). Worse, csv/tsv
kept these as strings, so the *same* token got a different type depending on the
format — a real hazard in mixed-format cascades.

A value is now coerced only when it is a valid JSON number (no leading zeros, no
leading `+`, no `inf`/`nan`); everything else stays a string.

```bash
# Old (1.x): leading zero silently lost
echo 'zip=02134' | kelora -f logfmt -F logfmt
# zip=2134

# New (2.0): preserved as a string
echo 'zip=02134' | kelora -f logfmt -F logfmt
# zip=02134
```

Genuine numbers still infer exactly as before (`status=500`, `dur=1.5`, `n=-5`,
`big=123456789012345678`, `sci=1e3`), so the numeric filters and stats these
formats are built around keep working. The win is that the same token now
resolves to the same type whether it arrives via JSON (where leading-zero numbers
are illegal anyway) or a logfmt/CEF field, logfmt round-trips IDs faithfully, and
`--discover` no longer shows already-corrupted sample values. csv/tsv/cols/regex
are unchanged — they remain string-by-default with opt-in `:int`/`:float`
annotations.

**Action:** if a script compared a now-string field numerically (e.g. `code=007`
matched with `== 7`), either compare as a string (`== "007"`) or coerce in a
script stage:

```bash
kelora app.log -f logfmt --exec 'e.code = to_int_or(e.code, 0)'
```

### Breaking: default-format word-wrapping is now TTY-aware

The default output format no longer wraps wide events onto continuation lines
when piped or redirected — wrapping is now **auto** (on for a terminal, off for
a pipe), matching color and emoji. This fixes over-counting by `wc -l`,
`head -n`, and other line-oriented consumers. To keep the old behavior when
paging to `less`:

```bash
kelora app.log --wrap          # force wrapping through a pipe
# or in .kelora.ini:
defaults = --wrap
```

`--no-wrap` disables it everywhere.

## Upgrade checklist

1. **Migrate tracking scripts.** Replace `track_count(value)` and
   `track_bucket(key, bucket)` → `track_freq("name", value)`, plain counters →
   `track_inc("name")` (or `track_sum("name", 1)`), and
   `track_top`/`track_bottom` → `track_top_by`/`track_bottom_by` (score before
   `n`). The old forms error with a hint, so a dry run surfaces every site.
2. **Re-check exit-code expectations.** The exit code now tracks "did the job
   get done", not "were there any errors". Gates must work: a `--filter` that
   errors on *every* event now exits `1` (was `0`), and a *partial* parse failure
   now exits `0` (was `1`; only all-lines-fail still exits `1`). Transforms are
   best-effort: any `--exec` error — even on every event — is recovered (exit
   `0`). Add `--strict` to fail on the first parse/filter/exec error, or
   `--assert` for explicit data-quality gates.
3. **Validate your config.** Run any command with your `.kelora.ini` present;
   a typo'd key or section now errors instead of being ignored.
4. **Verify `--input-tz` values** are `local`, `UTC`, or valid IANA names.
5. **Review typed parsers.** Expect `()` (not the raw string) on failed
   `:int`/`:float`/`:bool` conversions; switch to `to_int_or`-style coercion
   where you want a fallback.
6. **Check CSV/TSV consumers** for new `c<N>` overflow fields, or add `--strict`
   to reject ragged rows.
7. **Re-check logfmt/CEF numeric fields.** Zero-padded IDs, `+`-prefixed values,
   and `inf`/`nan` now stay strings instead of being coerced. If you compared
   such a field numerically, compare as a string or coerce with
   `to_int_or(...)`.
8. **Check line-oriented pipelines.** If you piped default-format output into
   `wc -l`/`head`/`sed`, wrapping is now off by pipe default — add `--wrap` only
   if you actually want continuation lines.

## See also

- [Full changelog](https://github.com/dloss/kelora/blob/main/CHANGELOG.md) — the complete, change-by-change record.
- [Metrics and Tracking tutorial](tutorials/metrics-and-tracking.md) — the redesigned tracking functions in depth.
- [Format Reference](reference/formats.md) — the named application-log formats and cascades.
- [Error Handling](concepts/error-handling.md) — resilient vs. `--strict` vs. `--assert`.
