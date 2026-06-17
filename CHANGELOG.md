# Changelog

All notable changes to Kelora will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

This is the **2.0** line. The headline changes are a redesigned tracking-function family, a set of named application-log formats, composable parser cascades, and a much more capable `--discover` mode. Breaking changes are flagged below — most affect tracking scripts and a few error/validation behaviors. See each entry for migration notes.

### Added

- **Three-tier diagnostic model with granular `--no-warnings` / `--no-hints`** - Kelora's advisory output is now organized into three named tiers, each with one suppression rule, so a user who finds one kind of message noisy can silence just that kind. **Errors (⚠️)** are correctness signals and show unless `--silent`. **Warnings (🔸)** flag a real problem the run recovered from — a recovered `--exec` error, a `--span`/`--parallel` conflict that forced sequential mode, lossy UTF-8 recovery, a cross-stage metric-name collision, an over-large span — and are hidden by `--no-warnings`, the `KELORA_NO_WARNINGS` env var, or `--silent`. **Hints (💡)** are advisory teaching suggestions — zero-result/typo/format tips, "writing to a file named `json`; did you mean `-F`?", the naive-timestamp UTC note — and are hidden by `--no-hints`, `KELORA_NO_HINTS`, or `--silent`. Each tier has a positive counterpart (`--warnings`/`--hints`) so an env var or config default can be overridden for a single run, and `--diagnostics`/`--no-diagnostics` remain as a shortcut that flips both advisory tiers at once. Precedence is **explicit flag > env var > config default**. The gate is driven by the same classification that picks the message's emoji, so the tier a message shows as is exactly the tier its suppression flag controls — no message can drift between how it looks and how it's silenced. The two env vars are first-class for "hush this for the shell session / CI job / container" without touching the command line or `.kelora.ini`. Settable as `defaults = --no-hints` in a `.kelora.ini` like any other flag.

- **`postgres` and `alb` named formats** - Two more common layouts join the named application-log catalogue. `postgres` parses PostgreSQL server logs written with the default `log_line_prefix = '%m [%p] '` — `2024-01-02 15:04:05.123 UTC [1234] LOG:  database system is ready` — into `ts`, `log_tz`, `pid`, `level`, and `msg`; a customized prefix (user@db, application name, …) won't auto-detect, so reach those with `-f regex:`. Its `ts` is naive and resolved through `--input-tz` (default UTC) like the other timestamp-only formats — the logged zone abbreviation is kept in `log_tz` (named so it reads as "what the server logged", not "the zone applied") for inspection but not applied, since abbreviations are ambiguous and can't encode an offset/DST; UTC-logged servers (the common config) are correct by default, and a non-UTC server is handled with `--input-tz <IANA>`. `alb` parses AWS Application Load Balancer access logs into a curated field set (`type`, `ts`, `elb`, `client`/`client_port`, `target`, the three processing times, `elb_status_code`/`target_status_code`, `received_bytes`/`sent_bytes`, `request`, `user_agent`); auto-detection is anchored on the distinctive head (the closed-enum connection `type` http/https/h2/grpcs/ws/wss, an ISO-8601-Z timestamp, and the `app/<name>/<id>` resource id), so it almost never claims a non-ALB line. Like the other access-log formats (`s3`, `haproxy`), `alb` keeps only the useful columns and drops the long, version-dependent tail AWS keeps appending (target group ARN, trace id, action/redirect/classification fields, …) — nothing is lost, since the full raw line stays available to a script as `line` / `meta.line` for a second-stage re-parse, e.g. `--exec 'e.trace_id = meta.line.extract_regex("Root=([0-9a-f-]+)", 1)'`. Both are selectable with `-f <name>`, usable in cascades, and shown in `--help-formats`. (`postgres` is adapted from [lnav](https://lnav.org), BSD-3-Clause; `alb` is Kelora-original.)
- **`--help` now documents the exit-code model** - A new "Exit Codes" section at the end of the full `--help` reference spells out the resilient-by-default contract that the recent error-model rework established: `0` is success even when some lines failed to parse or an `--exec` transform errored on some events (recovered, reported on stderr, counted — but not fatal); `1` covers the genuinely-failed cases (a named input file that couldn't be opened, a failed `--assert`, a *gate* stage — parsing or a `--filter` — that saw input but never once succeeded, a forbidden operation like mutating `conf` outside `--begin`, or under `--strict` any single parse/filter/exec error); `2` is invalid CLI usage; and the signal/panic codes (`130`/`141`/`143`/`134`). The one-screen `kelora -h` is unchanged. Closes the gap where scripts using `kelora … && deploy` could treat a partially-corrupt log as success without the behavior being written down anywhere user-facing.
- **`cri` format for Kubernetes container logs** - The CRI/containerd on-disk container-log layout — `<RFC3339Nano> <stream> <tag> <message>`, e.g. `2024-07-17T12:12:05.123456789Z stdout F {"level":"info"}` — is now a first-class named format, parsed into `ts`, `stream` (stdout/stderr), `tag` (`F` full line / `P` partial), and `msg`. This is the raw shape of `/var/log/pods/*/*.log`, of `kubectl logs --timestamps`, and of what log shippers (Fluent Bit, Vector, promtail) read off a node. Selectable with `-f cri` and usable in cascades (`-f cri,line`). Unlike the other named formats — which are tried only as the last step before the `line` fallback — `cri` gets a dedicated detector that runs *before* the logfmt and CSV steps, because a CRI message is frequently itself JSON or logfmt: a JSON payload's commas would otherwise be misdetected as CSV, and key=value messages as logfmt. As a result auto-detection recognises CRI logs regardless of the message payload (the early detector and `-f cri` share one regex, so they cannot drift). The message is kept verbatim in `msg` for an optional second-stage parse — e.g. `kelora pod.log --exec 'e.absorb_json("msg")'` to fan a structured JSON payload back into top-level fields. Unlike the other named formats, this layout is Kelora-original, not adapted from lnav.
- **Named application-log formats** - A curated set of common application-log layouts — `glog` (Go/klog), `nginx-error`, `apache-error`, `log4j`/Java, `python-logging`, `redis`, `s3` (AWS S3 access log), `haproxy` (http/tcp), and `iso8601-level` (generic ISO-8601 timestamp + level) — are now recognised and parsed into structured fields (`ts`, `level`, `msg`, plus format-specific extras) via the regex engine. They are first-class: selectable with `-f <name>` (e.g. `-f log4j`), usable inside cascade lists (`-f log4j,line`), shown by name in the auto-detect notice and `--stats` ("Detected format: log4j"), and documented in `--help-formats`. During auto-detection they are tried only as the last step before the `line` fallback, so no format Kelora already detected changes. Formats whose source emits more than one line layout (e.g. `s3`, `haproxy`) carry multiple patterns tried in order. Year-less timestamps (`glog`, `redis`) resolve assuming the current year, as syslog already does; `haproxy` lines are syslog-wrapped, so under `-f auto` they detect as `syslog` — pass `-f haproxy` for the structured fields. The definitions are adapted from [lnav](https://lnav.org) (BSD-3-Clause; see `THIRD_PARTY_LICENSES.md`).
- **Composable `cols:`/`regex:` cascades via repeated `-f`** - `-f` is now repeatable, building a cascade from each spec in order: `kelora -f json -f 'cols:ts(2) level *msg' app.log`. This is the only way to put spec-based parsers (`cols:`, `regex:`) into a cascade — a comma list still can't, because a regex pattern may itself contain commas. It closes the common "JSON lines mixed with custom `timestamp LEVEL message` plain text in one file" case (for the standard layouts, prefer the named formats above): previously the text lines fell through to raw `line` and their level was never parsed, so `-l error` silently dropped them. Catch-alls (`line`, `raw`, and `cols:`, which match every line) must come last; `regex:` is selective and may sit earlier, falling through to a later catch-all. A single `-f` (including a comma list) is unchanged.
- **Data-driven legends for map outputs** - The `levelmap` and `keymap` formats now append a one-line legend that decodes their glyphs, matching what `tailmap` already did. Legends are built from the data actually seen, not a fixed table: `levelmap` lists the level strings behind each first-letter glyph (`E = ERROR | I = INFO | W = WARN`, colored to match the map), and `keymap` groups the full field values that collapse to the same first character (`2 = 200,204 | 4 = 404 | 5 = 500,503`). Absent values are labeled (`? = (none)` for `levelmap`, `. = (missing)` for `keymap`). New `--legend` / `--no-legend` flags control all three map formats: by default the legend shows only on an interactive terminal, so piped or redirected output stays clean; `--legend` forces it through a pipe and `--no-legend` suppresses it (including `tailmap`'s, which was previously always on).
- **`-d` shortcut and richer output for `--discover`** - Field discovery, now the documented starting point for unknown files, gets a short flag: `kelora app.log -d` (use `-d=json` for the machine-readable form), featured near the top of `kelora -h`. `--discover-depth=0` fully flattens deeply nested JSON, removing the previous hard-coded 3-level cap. See the Changed section for the expanded discover footer (timestamp field, input parser, counts).
- **`-D` shortcut for `--discover-final`, plus a discoverability hint** - The post-pipeline counterpart to `-d` now has its own short flag (`kelora app.log -D`, `-D=json`), mirroring the lower/upper-case pairs used elsewhere (`-e`/`-E`, `-l`/`-L`). To close the common "I added `--exec` but my computed field isn't in `--discover`" confusion — `--discover` profiles *parsed input*, before scripts run — the plain `--discover` table footer now prints a one-line tip pointing at `--discover-final` whenever the pipeline filters or transforms events (`--exec`/`--filter`/`--levels`/`--span`/`--since`/`--until`/`--take`). A bare probe with no filters or transforms stays uncluttered, the hint never appears under `--discover-final` itself, and JSON output stays machine-clean.
- **`e.get()` map accessor** - `e.get("key")` and `e.get("key", default)` are now registered for events, mirroring `get_path` for top-level keys and matching the hint already shown in missing-field errors. Part of a clearer missing-field mental model (see Changed).
- **Keyword search in `--help-functions`** - `--help-functions [KEYWORD]` now filters the 150+ function catalogue by a case-insensitive substring match against each function's name, description, continuation lines, or section header, instead of forcing a scroll through the whole list: `kelora --help-functions ip` lists the IP-related helpers, `--help-functions string` lists the whole STRING section. Matching entries print under their preserved section headers; `--help-functions=KEYWORD` works too, and a no-match prints a hint to run the bare flag for the full catalogue. Bare `--help-functions` is unchanged.
- **Keyword search in `--help`** - The full CLI reference is now searchable too, the same way the function catalogue is: `kelora --help KEYWORD` prints only the options whose entry matches, each under its section heading (`Filtering Options:`, `Output Options:`, …), instead of paging through 100+ flags. The match adapts to what you type. A keyword that begins with a dash is a **flag query**: a case-sensitive, whole-token match against the option's declaration line, so `--help -j` finds only `-j` — never the `-j` buried inside `--multiline-join` and never the distinct `-J` — and `--help --since` finds just that flag. A bare word (`--help time`, `--help parallel`) is a case-insensitive substring search across both flag names and descriptions. The `--help=KEYWORD` form works as well. Bare `--help` is unchanged — it still hands off to the full renderer — and a no-match prints a hint pointing back to it.
- **No-script shortcuts for the most common aggregations: `--freq`, `--describe`, `--card`** - The highest-frequency tracking operations are now plain CLI flags, so you don't have to drop into Rhai for them. `--freq FIELD` builds a frequency table (shorthand for `track_freq("FIELD", e.FIELD)`, the "count by" operation), `--describe FIELD` prints a numeric summary (`track_stats` — count/min/max/avg/p50/p95/p99), and `--card FIELD` estimates the number of distinct values (`track_cardinality` — HyperLogLog, ~1% error in constant memory, so it scales to high-cardinality fields where `track_freq`/`track_unique` would blow up). All are repeatable, accept dotted paths for nested fields (`--freq user.id`), and run as the *last* per-event stage — so they see the post-pipeline result (fields created/renamed by `--exec`, only events that survived `--filter`), the same vantage as `--discover-final`. They imply `-m`, with output controlled by the usual `--metrics=short|full|tsv|json` / `--metrics-file` (one table even when several flags are combined); an explicit `--no-metrics` still wins. They are pure front-end sugar over the `track_*` functions — no change to the Rhai API or metrics internals — and inherit parallel-merge and span behavior for free. `--freq` is named to match `track_freq` (and to avoid the total-vs-tally ambiguity that "count" carries); `--count` is not a flag but prints a hint pointing here. There is deliberately **no** `--top`/`--bottom` flag: `--freq` already sorts by count descending, so ranking is left to the shell (see the `tsv` metrics output below — `--freq url | head` is top-N, `| tail` is bottom-N), which composes far more flexibly than two baked-in selectors. For anything beyond a single field — grouping, conditionals, custom names, score-based ranking (`track_top_by`) — write the `track_*` call in an `--exec` stage.
- **Pipe-friendly `tsv` metrics output, auto-selected when not on a terminal** - Metrics output (`-m`, `--freq`, `--describe`, and any hand-written `track_*`) now has a tab-separated record form alongside the human table and JSON. Each metric emits one or more `metric<TAB>key<TAB>value` rows — frequency tables and `track_top`/`track_bottom` sorted by count/score **descending**, scalars (sums, percentiles, `--describe`'s `name_p95`, …) as a single row with an empty key column. The three-column shape is fixed regardless of how many metrics a run produces, floats keep full precision (the table rounds for readability; `tsv`/JSON do not), and tabs/newlines inside values are flattened to spaces so every record stays one line. This makes the output a first-class citizen of Unix pipelines: `--freq url | head` (top-N), `| tail` (bottom-N), `| sort -t$'\t' -k3 -rn`, `| awk -F'\t' '$3>=100'`, `| wc -l`. Following the `ls` convention, the format **auto-selects**: bare `-m`/`--freq`/`--describe` render the human table on a terminal but the `tsv` stream when stdout is piped or redirected. The escape hatches are explicit: `--metrics=full` forces the table through a pipe, `--metrics=tsv` forces the stream even to a terminal, and `--metrics=json`/`short` are unchanged. (Note the behavior change: `kelora -m … > file` or `… | less` now yields `tsv`/records rather than the table; add `--metrics=full` for the old rendering.)
- **Intent-based hints for unknown flags borrowed from other tools** - When an unknown flag is one of a small curated set of names users reach for out of habit, kelora now prints guidance toward the kelora idiom instead of clap's edit-distance guess (which matched on string distance, not intent — `--sort` → `--assert`, `--where` → `--help-regex`). `--where`/`--grep`/`--match` point to `--filter`; `--sort`/`--rank`/`--top-n` point to `track_top_by` in an `--exec` stage; `--count`/`--group-by`/`--uniq` point to the `--freq` flag / `track_freq`. These are **not** aliases: the flags stay unknown and still exit 2, so no namespace is reserved and a real `--sort` flag could be added later without a breaking collision. Genuine near-miss typos (e.g. `--filer`) still get clap's suggestion unchanged.
- **`-P` short flag for `--parallel`** - The performance toggle now has a short form, following the `xargs`/GNU `parallel` convention.
- **"No input" hint instead of silent exit** - Running bare `kelora` with stdin redirected from an empty source (`kelora < /dev/null`, an empty pipe, a non-interactive shell) previously exited 0 in silence, even though the help promised interactive mode "with no arguments" — interactive mode only triggers on a real TTY. Kelora now prints a one-line hint to stderr ("No input: stdin is empty and no files were given…") whenever no files are given, stdin is not a terminal, and nothing was read, so an empty run no longer looks like a crash. The hint is advisory: it goes to stderr only (never polluting a downstream pipe), and is suppressed by `--no-input`, `--no-diagnostics`, and `--silent`. The quick (`-h`) and full (`--help`) text now state that interactive mode requires a terminal.
- **`-l/--levels` warns on a vocabulary mismatch instead of returning a silent empty result** - When `-l` drops every event and a level field *was* present but none of the requested levels appear among the values actually seen, kelora now lists the levels present (`-l ERROR matched none of the levels present: E,I`). This catches the dangerous case where a stream uses a different level dialect than the filter — glog logs `I/W/E/F`, syslog uses `CRIT` not `CRITICAL` — so "show me the errors" returning nothing can no longer be misread as "no errors". The filter stays purely lexical (no level normalization or aliasing); the hint just surfaces the actual vocabulary and points at a `--filter 'e.level == "E"'` workaround. Like the existing zero-result hints it goes to stderr, keeps exit code `0` (an empty result is legitimate), and is suppressed by `--no-diagnostics`/`-q`/`--silent`. When a requested level *is* present, the empty result is a genuine "none this time" and stays quiet.
- **Zero-result hint for the number-vs-quoted-string filter mistake** - On typed input (JSON/CSV with `:int` annotations) the classic beginner trap is `--filter 'e.status == "404"'`: in Rhai a number never equals a string, so the test is always false and the result is silently empty. When a `--filter` compares a *seen* field for equality against a quoted, numeric-looking literal and nothing matched, kelora now suggests dropping the quotes (`drop them: e.status == 404`) and points at `-s` to confirm the field's type. It only fires in the existing all-events-dropped path and on a field that was actually present (an unseen field still gets the more precise typo hint first); the advice is phrased conditionally because a genuine string field compared to a numeric-looking value is a legitimate empty result too. Goes to stderr, keeps exit code `0`, suppressed by `--no-diagnostics`/`-q`/`--silent`.
- **Naive-timestamp UTC-assumption hint instead of a silent shift** - Timestamps without a zone offset (syslog, log4j, python-logging, glog, apache-error, postgres, …, and any line carrying only an ambiguous zone *abbreviation* like `CEST`/`PST`, which kelora cannot reliably resolve and so treats as naive) are resolved with `--input-tz`, which defaults to UTC. For a source that logs local time this shifts every timestamp with no signal — quietly moving `--since`/`--until` and `--span` boundaries, throwing off cross-source ordering, and, under `--normalize-ts`, baking the wrong offset into the output itself. Kelora now prints a one-time stderr hint when timestamps are naive *and* no zone was chosen (no `--input-tz`, no `TZ`) *and* the run actually depends on or materializes the assumption — a time filter, `--span`, or `--normalize-ts` is active — pointing at `--input-tz <zone>`; the `--normalize-ts` wording additionally notes the offset is written into output. To avoid crying wolf on the common UTC cloud-log case it stays silent for plain passive output, when a zone was chosen explicitly (`--input-tz` or a non-empty `TZ`), and for timestamps that already carry a numeric offset. Like the other zero-result/typo hints it goes to stderr only, keeps the exit code unchanged, and is suppressed by `--no-diagnostics`/`--silent`. This **surfaces** the existing, intentional timestamp policy (offsets honored / naive defers to `--input-tz` / abbreviations not parsed) rather than changing how timestamps resolve; that policy is now consolidated in `--help-time` and the working-with-time tutorial.
- **`absorb_jwt()` - flatten JWT claims onto the event** - A new whole-event mutator joins the absorb family (`absorb_kv`/`absorb_logfmt`/`absorb_json`/`absorb_regex`): it parses a JWT from a string field and merges its **claims** (the decoded payload) into the event, returning the same status map (`status`/`data`/`written`/`remainder`/`removed_source`/`error`). The header and signature are ignored — only the claims are flattened, exactly as `absorb_json()` flattens a JSON object — so `e.absorb_jwt("token")` turns a token field into top-level `sub`/`role`/`exp`/… fields and drops the source. It is all-or-nothing like `absorb_json()`: a malformed token sets `status = "parse_error"` and leaves the event untouched (no partial extraction), so `remainder` is always `()`. Supports `keep_source`/`overwrite`; `sep`/`kv_sep` are accepted but ignored since a JWT's structure is fixed. Signatures are **not** verified (debugging / trusted tokens only). Time claims merge as their raw integers; for datetime-typed `exp`/`iat`/`nbf` use `parse_jwt()` (below).
- **`parse_jwt()` decodes the standard time claims into datetimes** - The JWT parser now exposes the registered NumericDate claims `exp`, `iat`, and `nbf` as datetime values under `expires_at`, `issued_at`, and `not_before` (in addition to leaving the raw integers in `claims`). Because they are real datetimes, they compose with the rest of kelora's time machinery without hand-converting Unix seconds: flag expired tokens with `--filter 'e.token.parse_jwt().expires_at < now()'`, compute a token's lifetime with `(jwt.expires_at - jwt.issued_at)`, or format the expiry with `jwt.expires_at.to_iso()`. NumericDate claims are read as whole seconds (the universal real-world form); a missing, non-numeric, or out-of-range claim simply omits its field rather than erroring, so existing scripts that read `claims.exp` are unaffected. Still does not verify signatures — debugging / trusted tokens only.
- **`absorb_logfmt()` - quote-aware sibling of `absorb_kv()`** - A new whole-event mutator that parses a logfmt string field, merges its keys into the event, and returns the same status map as the rest of the absorb family (`status`/`data`/`written`/`remainder`/`removed_source`/`error`). Unlike `absorb_kv()`, which is a plain splitter — it keeps surrounding quotes on values and splits on separators that appear *inside* a quoted value, silently mangling logfmt-style input such as `err="connection refused"` — `absorb_logfmt()` is quote-aware (quotes stripped, quoted values may contain spaces) and infers numeric/boolean types. It is all-or-nothing like `absorb_json()`: a bare, unpaired token makes the whole field a `parse_error` (no partial extraction), so `remainder` is always `()`. Supports `keep_source`/`overwrite`; `sep`/`kv_sep` are accepted but ignored since logfmt's syntax is fixed. The docs for `parse_kv()`/`absorb_kv()` now flag their lack of quote-awareness and point at the logfmt variants. (Relatedly, strict-mode option errors from `absorb_json`/`absorb_regex` now name the actual function instead of always reporting `absorb_kv:`.)

### Changed

- **Diagnostic re-tiering and data-only-mode warning behavior (part of the three-tier model above)** - A few advisory messages were reclassified so that how a message *looks* matches the flag that *silences* it. The `--span`/`--parallel` and `--window`/`-B`/`-C` conflict notices, the over-large-span notice, and the "parsing mostly failed" notice now print as warnings (🔸) rather than errors (⚠️) — they were never fatal, and they are now controllable with `--no-warnings`. The "writing to a file named `json`; did you mean `-F`?" notice moved the other way, from a warning to a hint (💡), since it is pure did-you-mean guidance. Separately, **data-only modes (`-m`/`--drain`/`--discover`) now hush only hints, not warnings**: warnings go to stderr (never the stdout data channel) and may flag a real problem — e.g. recovered `--exec` errors — so they keep surfacing under `-m`, while the chattier hints stay suppressed to keep machine output focused. `--no-warnings` or `--silent` still hide them explicitly. As before, `--silent` is the only switch that hides error summaries entirely. Relatedly, the detection notices shed an inconsistent terminal-only gate: the "parsing mostly failed" **warning** and the "no input format detected" **hint** now follow only their tier flags, so they reach a stuck user even when stderr is redirected to a file or captured by CI (previously they were silently dropped off a TTY). The neutral **info** notices — the 🔹 "Auto-detected format: …" line and the config-expansion output — are reclassified as *status*, not diagnostics: they stay terminal-only and are governed by visibility (`-q`/`--silent`), independent of `--no-warnings`/`--no-hints`/`--no-diagnostics`. The one behavior change there: `--no-diagnostics` no longer hides the auto-detect status line — use `-q` or `--silent`.

- **Removed: undocumented `KELORA_NO_TIPS` env var** - The single-purpose, never-documented `KELORA_NO_TIPS` (it only ever gated the format-detection tip, not the other hints) is gone. Use `KELORA_NO_HINTS` to silence all hints, or `KELORA_NO_WARNINGS` for warnings — the env vars now mirror the flags exactly. No deprecation alias is kept: the variable had no documented contract and a strictly narrower effect than its successor.

- **Human metrics table gets labeled, aligned columns** - In the `--metrics=full`/human table, list-valued metrics (frequency tables and `track_top`/`track_bottom[_by]` rankings) now print a two-column table with a header row naming each column and the numbers right-aligned underneath, so a row like `200   3` reads unambiguously as a `value` of `200` with a `count` of `3`, rather than two adjacent numbers. The columns are named for context: a `track_freq` table is `value`/`count`, a `track_top`/`track_bottom` ranking is `item`/`count`, and a score-based `track_top_by`/`track_bottom_by` ranking is `item`/`score`. The block header's count noun agrees with the left column — `(3 values):` over a `value` column, `(3 items):` over an `item` column — and still reports the full total when the list is truncated. The redundant `#1`/`#2` rank prefix is dropped: rows are already emitted in rank order, so position conveys rank. Only the human table's layout changed; the stored numbers, the `tsv` stream, and the JSON output are untouched, as are scalar metrics (`= …` for sums/averages, `≈ …` for cardinality).
- **Breaking: tracking-function redesign (`track_freq`, `track_inc`, `track_top`/`track_bottom`)** - The tracking family is consolidated around one convention: `track_fn(name, categorical-or-numeric args...)`.
    - `track_freq(name, value)` is the frequency table — it counts occurrences of each distinct value, replacing both the old one-argument `track_count(value)` and `track_bucket(key, bucket)` (which were the same operation under two names). Values may be strings, numbers, or bools — they are stringified into the map key, so `track_freq("status", e.status)` works without `to_string()`. Counts land in separate per-name sub-maps, so `track_freq("level", e.level)` and `track_freq("method", e.method)` can no longer collide. `track_count` and `track_bucket` both error with a migration hint. (The function was briefly named `track_count` during 2.0 development; it was renamed to `track_freq` before release because "count" was ambiguous between a per-value frequency table and a plain scalar counter.)
    - `track_inc(name)` increments a running counter by 1 — readable sugar for `track_sum(name, 1)`, which it shares an operation with (so the two are interchangeable and merge identically). For weighted accumulation keep using `track_sum(name, value)`.
    - Score-based ranking moved from the 4-argument `track_top(key, item, n, value)` to `track_top_by(name, item, score [, n])` / `track_bottom_by(name, item, score [, n])`; the old 4-argument form errors with a migration hint. `n` now defaults to 10 in all four ranking functions, and items accept numbers and bools.
    - All `track_*` functions now skip Unit `()` values (missing fields) instead of erroring — previously the categorical counter failed on every event when the field was absent. Skips are counted per metric, and a `--diagnostics` hint surfaces a likely field-name typo. The hint fires only when a metric recorded a value on *no* event (the field was missing from every event), so a field that is simply present in some events and absent in others (normal for varying-shape logs) no longer triggers it.
    - Reusing one metric name across different track functions (e.g. `track_sum("x", ...)` then `track_min("x", ...)`) is now a call-time error. Previously the conflicting values were silently blended into garbage during parallel merging. (In `--parallel`, a name reused across `--begin` vs the event stages is caught only at merge time, as a warning rather than a per-call error.)
    - Float values keep their 1.x `track_bucket` labels (`200.0` → `"200"`), so migrated scripts and JSON consumers keyed on the old bucket names keep working.
    - `track_unique` warns once past 100,000 stored values (it keeps every distinct value in memory by design); the warning points to `track_cardinality()` for unique counts and honors `--silent`/`--no-diagnostics`.
- **Breaking: gate-vs-transform exit-code model** - The exit code now follows one rule: Kelora exits non-zero when it couldn't do the job, not because the data was messy. It turns on *gates vs. transforms*. **Gates** — parse and each individual `--filter` stage — must work: if a gate never once succeeds (no line parses, or a filter errors on *every* event it sees and so selects nothing), the output is empty or meaningless and the run exits `1`; a gate erroring on only *some* records is recovered (exit `0`). Filters are gated per stage, so a working first filter cannot mask a later filter that is completely broken. **Transforms** — exec — are best-effort: a failing `--exec` rolls back to the original event and emits it, so exec errors are reported but never fail the run on their own, even when they hit every event. Structural failures (a named input that can't be opened) and `--assert` violations still fail in any mode, and `--strict` escalates any single parse/filter/exec error. Two behaviors change from 1.x: (1) a `--filter` that errors on **every** event now exits `1` instead of `0` — a totally broken filter (e.g. the `status >= 500` typo for `e.status >= 500`) used to return success with empty output and silently pass monitoring checks ([#241](https://github.com/dloss/kelora/issues/241)); (2) a *partial* parse failure now exits `0` instead of `1` — a few unparseable lines among good ones are recovered with a diagnostic, and only an all-lines-fail (wrong format) still exits `1`. The signal is tracked in the always-on tracker (parse and filter record successes alongside errors), so it is independent of `--stats` collection and consistent across `--metrics`, `--drain`, `-q`, and `--no-diagnostics`. The full model with a scenario table lives in the Error Handling and Exit Codes docs.
- **Breaking: config files are validated strictly** - `.kelora.ini` (and `--config-file`) now reject unknown root keys, unknown sections, and lines that are not a comment, `[section]` header, or `key = value` pair, naming the file and line (with a "did you mean" hint for case mismatches). Previously a typo such as `defualts =` or `[alias]` was silently ignored, leaving defaults/aliases quietly unapplied. Only `defaults` (root) and the `[aliases]` section are recognized.
- **Breaking: invalid `--input-tz` is rejected** - An unrecognized `--input-tz` value (e.g. a typo like `Europe/Berln`) now fails fast during configuration validation with exit code 2, instead of silently falling back to the machine's local time. Silent fallback could shift every timestamp — and thus time filters and span boundaries — without any visible error. Use `local`, `UTC`, or a valid IANA timezone name.
- **Breaking: failed type annotations yield `()` instead of a string** - For `:int`/`:float`/`:bool` annotations in csv/tsv/cols/regex, a value that can't satisfy the declared type now becomes `()` (explicitly absent, e.g. JSON `null`) in resilient mode instead of silently keeping the original string; the rest of the row is preserved. `--strict` still aborts on the failure. This makes all four typed parsers behave identically and fixes a bug where `cols` ignored `--strict` for conversions. For tolerant coercion with a chosen fallback, drop the annotation and use `to_int`/`to_int_or` in a script stage (e.g. `--exec 'e.status = to_int_or(e.status, 0)'`).
- **Breaking: ragged CSV/TSV rows are kept, and `--strict` rejects them** - Rows with more columns than the header previously lost the extra fields silently. Overflow columns are now kept under positional names (`c5`, `c6`, …, the same convention as headerless `csvnh`/`tsvnh`), short rows keep their trailing fields absent (preserving `field in e` semantics), and both cases are counted in `--stats` ("Ragged rows: …") with a stderr hint pointing at an inspection filter. `--strict` now treats a ragged row as a parse error (previously `--strict` only governed type conversion), and strict shape errors name where the expected width came from (`expected 4 (from header)` / `(from first line)`).
- **Breaking: logfmt/CEF numeric inference no longer mangles zero-padded or signed values** - The type-inferring parsers (`logfmt`, `cef`) used to coerce any token that Rust's `i64`/`f64` parser would accept, which silently rewrote data: leading zeros were dropped (`zip=02134` → `2134`, `id=007` → `7`, `ver=01` → `1`), a leading `+` was stripped (`phone=+15551234` → `15551234`), and the Rust-only float spellings `inf`/`nan`/`Infinity` became floats (then `null` on JSON output). A value is now coerced only when it is a syntactically valid JSON number (RFC 8259: no leading zeros, no leading `+`, no `inf`/`nan`); everything else stays a string. Genuine numbers are unaffected (`status=500`, `dur=1.5`, `n=-5`, `big=123456789012345678`, `sci=1e3` still parse as before), so the numeric filters and stats those formats are built around keep working. This makes the same token resolve to the same type whether it arrives via JSON (where leading-zero numbers are illegal anyway) or a logfmt/CEF field — which matters for mixed-format cascades — restores logfmt round-trip fidelity for IDs, and stops `--discover` from displaying already-corrupted sample values. csv/tsv/cols/regex are unchanged: they stay string-by-default with opt-in `:int`/`:float` annotations. Migration: a field that relied on the old coercion (e.g. `code=007` compared with `== 7`) now compares as a string (`== "007"`); add a script stage like `--exec 'e.code = to_int_or(e.code, 0)'` to restore a numeric value.
- **Breaking: default-format word-wrapping is now TTY-aware** - The default output format used to wrap wide events onto indented continuation lines unconditionally, including when piped or redirected (falling back to a 100-column width with no terminal). That silently turned one event into several lines, so `wc -l`, `head -n`, `sed -n`, and other line-oriented consumers over-counted. Wrapping now follows the same "human vs. machine" rule as color, emoji, and map legends: it is **auto** by default — on when stdout is a terminal, off when piped or redirected, so each event stays on one line downstream. `--wrap` forces wrapping through a pipe and `--no-wrap` disables it everywhere; both can be set via the `defaults` line in `.kelora.ini` (e.g. `defaults = --wrap` to keep the old behavior when paging to `less`). Interactive terminal output is unchanged.
- **Parallel mode now delivers worker tracking metadata reliably** - Worker threads previously shipped their internal tracking state (operation metadata, error counters) to the global tracker only when the final flush happened to carry output, and mid-run flushes could deliver cumulative counters twice. Internal state is now delivered exactly once per worker, per-batch user deltas are cleared after each send, and the multiline (event-batch) path attaches operation metadata like the line path. This fixes several `--parallel` inconsistencies: `track_avg`/`track_stats` metrics are properly finalized inside `--end` (previously a raw `{sum, count}` map), the skipped-missing-value diagnostic and exec-error summaries now appear as in sequential mode, and multiline runs merge averages correctly instead of keeping the last batch.
- **Faster JSON parsing** - The JSON line parser now deserializes straight into the event's field map via a custom serde visitor, skipping the intermediate `serde_json::Value` tree that dominated worker-thread CPU. Output is byte-identical (numbers, escapes, nesting, error messages); measured ~17–31% faster wall-clock on JSON inputs, with the largest gains on wide objects.
- **`--metrics` renders `track_freq` maps as an aligned, sorted list** - In the human-readable `--metrics` text view, a `track_freq` (frequency-table) metric previously dumped raw Rhai map syntax (`status = #{"500": 67, "404": 12}`). It now prints as an aligned list sorted by count descending, matching the existing `track_top`/`track_bottom` style, and truncates to 5 entries (with a "+N more" hint) unless `--metrics=full` is used or the map has 10 or fewer categories. Display-only: the `--metrics-file` and JSON outputs keep the full structured map.
- **Consistent count label across grouped metric outputs** - The human-readable `--metrics` view labeled a `track_freq`/`--freq` frequency table with `(N categories):` while the sibling ranked (`track_top`/`track_bottom`) and `--drain` outputs used `(N items):`. The frequency table now also uses `(N items):`, so the three grouped summaries read the same; `track_unique`'s distinct `(N unique):` label (a different aggregation) is unchanged. Display-only: JSON / `--metrics-file` output is unaffected.
- **`--stats`/`--metrics` format flags are more discoverable** - Three small new-user papercuts around the `=`-only format flags: `--help` for `--stats` and `--metrics` now notes that the format attaches with `=` (so `-s json` reads `json` as a *filename*, not a format); a failed open of a "file" named exactly `json`/`table`/`short`/`full` now appends a hint pointing at the `--stats=json` form; and the otherwise-hidden config-override negations (`--no-strict`, `--no-parallel`, `--no-stats`, `--no-metrics`, `--no-silent`) are now mentioned in their positive flag's help text.
- **`--metrics` text view rounds floats consistently** - Float metrics in the human-readable view are now rounded to 6 significant figures, so a `track_stats` block no longer mixes clean percentiles (`880.16`) with noisy raw floats (`146.6142714694471`). Display-only: stored values and the JSON / `--metrics-file` output keep full precision.
- **`--discover` footer expanded** - The discover table now reports more about what it saw. It identifies the primary timestamp field — the one kelora would use for `--since`/`--until` — and shows it in the footer (`timestamp: ts`, `timestamp: ts (60% parsed)` when some values don't parse, `timestamp: when (--ts-field)` for an override); it names the input parser (`format: cef (auto-detected)`, or `formats: cef 12, json 3 (events)` for cascades/per-file modes) so a mis-detected format is visible; and it moves the scanned-event count to a quiet footer line with a trailing ellipsis on Examples lists that don't cover every distinct value. JSON output (`-d=json`) gains matching `timestamp` and `format`/`format_counts` objects. String examples are now quoted and escaped (`"hello"`, `""`, `\n`) to match `-F inspect`, the examples column grows to the full terminal width (the old 60-char cap is gone), and the Field column no longer pads to a 12-char floor.
- **Clearer missing-field model and hints in Rhai scripts** - A missing field is `()` and access never throws by itself; this single mental model is now documented in `--help-rhai` with the two safe idioms (`e.has`/`e.get`/`??`). Referencing a field without the `e.` prefix (e.g. `status` instead of `e.status`) — the most common newcomer mistake — now suggests `e.<field>` directly when the bare name matches (or closely resembles) a real field, instead of offering string-similar but wrong scope variables.
- **`line`-fallback and mixed-format hints point at field extraction** - When auto-detection keeps whole lines as `line`, the hint now suggests extracting fields from `timestamp LEVEL message` app logs with `-f 'cols:ts(2) level *msg'` (or a `regex:`) and cascading a mixed file with repeated `-f`. When `-f auto` locks onto one format but the input is actually mixed, the parse-failure warning now re-detects the failing line and prints a concrete, copy-pasteable cascade (e.g. `Detected mixed formats (json + line). Try: -f json,line`).
- **`--include` no longer double-executes before the first stage** - An `--include` placed before the first `--filter`/`--exec` stage was loaded into both that stage and a synthesized begin stage, so any top-level statements in the include ran twice (once at startup, then per event). Includes now only form a begin/end script when an explicit `--begin`/`--end` is present; otherwise the include is loaded solely into the adjacent stage. Helper-function-only includes (the documented use) are unaffected; `--begin`/`--end` with includes still work.
- **Parse error summaries include filenames** - Parse error messages now show which file the error came from, making multi-file runs easier to debug.
- **Non-UTF-8 input is decoded losslessly instead of aborting the stream** - A single invalid UTF-8 byte previously tore down the whole run: lines before it were emitted, everything after was silently dropped, and the process exited `1` with `stream did not contain valid UTF-8` — with no way to opt out. This sat at the byte→`String` boundary above every parser, so it hit all formats and both stdin and file input. Kelora now tolerates non-UTF-8 input the way `grep`/`ripgrep` do: invalid byte sequences are replaced with `U+FFFD` (�) and the valid ASCII/UTF-8 structure parsers key on is preserved, so a stray Latin-1/Windows-1252 byte or embedded binary no longer truncates the rest of a multi-GB file. Recovery is visible, not silent — a diagnostic reports how many lines were affected ("N lines contained invalid UTF-8, decoded with U+FFFD substitution", also shown in `--stats`) — and because this is a recovery rather than a failure, the exit code stays `0`. Clean (already-valid) logs are byte-for-byte unchanged with no measurable throughput cost. Pass `--strict-utf8` to restore the old hard-failure behavior (abort + exit `1`).

### Fixed

- **`emit_each()` in `--begin`/`--end` is now a clear error instead of silently misbehaving** - `emit_each()` defers its events to a thread-local buffer that is only materialized by the per-event loop (`ExecStage`). The `--begin`/`--end` stages run *outside* that loop, so events emitted there were never drained on their own: under `--no-input` (or any run where no event reached an `--exec` stage — e.g. `--filter false`) they were silently dropped while `emit_each()` still returned a truthful non-zero count, and with normal input a `--begin` emission was picked up only as an accidental side effect of the *first* event's processing, appearing interleaved after the first output line and inheriting its `line_num`. `--end` emissions were always lost. Because that "working" case was undefined and order-dependent (and absent entirely in `--parallel`, where begin runs on a different thread than the workers), there was no coherent behavior to preserve. `emit_each()` now raises a runtime error when called from `--begin`/`--end` — `emit_each() is not available in the --begin stage; it only works in a per-event stage (-e/--exec or --filter)` — exiting `1`, mirroring how `state`/`drain_template` reject `--parallel`. The guard resets before the per-event loop, so `emit_each()` in `--exec`/`--filter` is unchanged even when a `--begin` stage is present. **Breaking:** a `--begin 'emit_each(…)'` that happened to produce interleaved output now errors; move the call to a per-event `--exec` stage. To generate events without an input file, pipe a counter (`seq 1 N | kelora -f line --exec 'emit_each(…)'`).
- **`-o <format>` (meant for `-F <format>`) now warns instead of silently writing a mystery file** - `-o`/`--output-file` takes a file path, but the near-universal `-o`-as-output-*format* convention (and kelora's own `-f` input-format / `-F` output-format pairing) makes `kelora … -o json` an easy slip. It silently wrote a file literally named `json` in the cwd, printed nothing to stdout, and exited `0` — a request for JSON conversion producing only a confusing artifact and a success code. (Two such files, `json` and `logfmt`, had themselves been committed to the repo root this way and are now removed.) When `--output-file`'s value is a bare name (no path separator, no extension) that exactly matches a known format keyword, kelora now prints a one-line stderr warning — `did you mean -F json (--output-format)?` — while still writing the file as asked, so existing scripts are unaffected. The warning honors `--silent`; real filenames like `out.json` or paths like `/tmp/json` never trigger it.
- **Blank CSV header columns get positional `cN` names instead of an empty key (which silently dropped data)** - A blank header cell (`a,,c`, or a trailing comma) was used verbatim as the field name, producing an empty-string key. Because events are keyed maps, *multiple* blank columns collided into one `""` field and silently dropped data: `a,,,c` over `1,2,3,4` parsed to `{"a":"1","":"3","c":"4"}` — the second column's `2` was gone. Empty keys are also unrepresentable in `logfmt` (the parser rejects `=value` as `Empty key found`), so `kelora -f csv -F logfmt` emitted output it couldn't itself read back. Blank header cells now take the same positional `cN` name already used for headerless input and ragged-overflow columns (`{"a":"1","c2":"2","c3":"3","c":"4"}`), so every column stays addressable, no data is lost, and the output round-trips. As defense-in-depth for empty keys reaching the formatter from other sources (e.g. a JSON `{"":...}` field or a script), the `logfmt`/`to_logfmt()` key sanitizer now also maps an empty key to the `_` placeholder rather than emitting an unparseable `=value`.
- **A single-key nested object no longer drops its key when flattened to `logfmt`/`csv`/`to_logfmt()`** - The compact flattening used by the `logfmt` and `csv` output formatters and by the `to_logfmt()` script function rendered a nested map/array as `"key=val,key=val"` (`logfmt`/`to_logfmt`) or `"key:val,key:val"` (`csv`) — *except* when the structure flattened to exactly one entry, where a special case emitted only the value and discarded the key. So `{"a":{"b":1}}` formatted as `a=1` (losing `b`), `{"a":{"b":{"c":1}}}` as `a=1` (losing the whole `b_c` path), and a CSV `obj` cell as `1` instead of `b:1`; `to_logfmt(#{outer:#{b:1}})` yielded `outer=1`. This silently dropped field names and broke the round-tripping the multi-key path was explicitly designed to preserve. The single-entry branch now keeps its `key=value`/`key:value` shape like the multi-entry case — `a="b=1"`, `a="b_c=1"`, cell `b:1`, `outer="b=1"` — and single-element arrays keep their index (`a="0=42"`, matching the existing multi-element `"0=blue,1=green"`). Empty maps/arrays still render as an empty value (they flatten to a single UNIT placeholder, which is preserved), so no empty/`null` behavior changed.
- **`array.percentile()` accepts an integer percentile, matching its own docs** - The array percentile helper was registered only with an `f64` percentile argument, but Rhai does not auto-coerce an integer literal to `f64`, so the idiomatic `arr.percentile(95)` failed with `Function not found: percentile (array, i64)` — only `arr.percentile(95.0)` worked. Every documented example used the integer form (`docs/reference/functions.md`'s `e.latencies.percentile(95)` / `e.values.percentile(50)`, the in-source docstring's `.percentile(95)` / `[1,2,3,4,5].percentile(50)`), and it is the natural way to compute per-window percentiles from `span.events` (the recommended workaround for non-additive metrics). An integer overload is now registered alongside the float one, so both `.percentile(95)` and `.percentile(95.0)` work; the float behavior is unchanged.
- **Rhai docs no longer show the unsupported `for (key, value) in map` form** - The `--help-rhai` guide, the Rhai cheatsheet, and the span-aggregation cookbook all illustrated map iteration with `for (key, value) in map` / `for (key, val) in e`, which Rhai does not support: object maps are not directly iterable (the two-variable `for (item, counter)` form is element-plus-numeric-index over *arrays/ranges* only). A map literal failed to compile (`Expecting an iterable value, not an object map`) and a map-valued variable such as the event `e` failed at runtime (`For loop expects iterable type`), so a new user copying the cheatsheet hit an immediate error. All four spots now use the working idiom — iterate `map.keys()` (or `.values()`) and index back in (`for key in e.keys() { … e[key] … }`). The cookbook's per-span CSV example is also corrected: it iterated `span.metrics` directly (same bug) and, because `span.metrics` is keyed by metric *name* with a `{value: count}` map underneath, it now reads `span.metrics["service"]` and iterates its keys; the redundant manual `\n` is dropped (`append_file()` already adds one per call).
- **Datetime values render as RFC3339 in scripts instead of an internal type name** - Interpolating a `DateTimeWrapper` in a Rhai string template (`` `${span.start}` ``), printing one with `print(dt)`, or letting one fall through `to_string()`/`to_debug()` previously emitted the fully-qualified Rust type path (`kelora::rhai_functions::datetime::DateTimeWrapper`) rather than the timestamp. The type already had a `Display` impl, but Rhai's interpolation dispatches to a *registered* `to_string`/`to_debug` function, and only `DurationWrapper` had registered them. Both are now registered for `DateTimeWrapper` (mirroring the duration registration), so `` `${span.start}` `` yields `2024-07-01T09:00:00+00:00`. Most visible in `--span-close` hooks reporting `span.start`/`span.end`; the explicit `.to_iso()` / `.format()` methods are unchanged.
- **No spurious "rerun with -m" metrics hint when `--end` consumes the metrics** - The advisory nudge printed after a run that records `track_*` metrics without a display option (`Metrics recorded; rerun with -m …`) also fired when an `--end` stage was present, even though `--end` sees the `metrics` global and is the idiomatic way to consume metrics into a custom report. The hint now treats an `--end` stage (alongside `-m`/`--metrics-file`) as the metrics already being handled, so report-style runs no longer get the redundant line. The bare track-and-forgot-`-m` case still gets the nudge.
- **`clamp()` with an inverted range reports an error instead of aborting the process** - `clamp(value, min, max)` (both the integer and float overloads) forwarded straight to `std`'s `Ord::clamp`/`f64::clamp`, which **panic** when `min > max`. Under the release `panic = "abort"` profile a simple argument mix-up such as `clamp(50, 100, 10)` (or a `NaN` bound on the float path) aborted the whole run with exit `134`. Both overloads now return a runtime error naming the offending bounds, so the misuse is reported per-event and the run continues. In-range clamping is unchanged.
- **Dividing a `duration` by zero reports an error instead of aborting the process** - The Rhai `duration / n` operator forwarded to chrono's `Duration / i32`, which **panics** on a zero divisor. `duration_from_seconds(60) / 0` therefore aborted the whole run with exit `134` under the release `panic = "abort"` profile; because the `i64` argument is narrowed to `i32`, a value that truncates to zero (e.g. `4294967296`) hit the same panic despite being non-zero. The operator now rejects a zero (narrowed) divisor with a runtime error. Valid divisors are unchanged.
- **A large `--span` time duration no longer aborts the process** - A time-window span whose duration fit in `i64` milliseconds (the only bound the parser checked) but exceeded chrono's representable datetime range — e.g. `--span 1000000000d` — pushed the window-end timestamp past the range where `timestamp_millis_opt` returns a value, and the unchecked `.unwrap()` panicked (`No such local time`). Under the release `panic = "abort"` profile that aborted the whole run with exit `134` on otherwise-valid input. The window-boundary conversion now clamps to chrono's representable min/max instead of unwrapping, so an oversized span degrades to a single all-encompassing window and the run completes with exit `0`.
- **`ts_nanos()` reports an error out of range instead of silently returning `0`** - `to_datetime(...).ts_nanos()` is backed by chrono's `timestamp_nanos_opt()`, which is `None` for any datetime outside roughly `1677-09-21..2262-04-11` (the `i64`-nanosecond range). The previous `.unwrap_or(0)` mapped every such timestamp — including ordinary parseable values such as `9999-12-31` — to the Unix epoch, silently corrupting downstream arithmetic and comparisons. It now returns a runtime error naming the supported range, matching how the sibling `round_to`/`ceil_to` already reject out-of-range datetimes. In-range conversions are unchanged.
- **`--stats=json` emits JSON instead of silently falling back to the table** - The `--stats` flag documented (and `--help` advertised) a `json` format alongside `table`, matching its siblings `--discover=json` and `--metrics=json`, but the `StatsFormat::Json` value was never wired to a renderer: `--stats=json` printed the human-readable table, so a script reaching for machine-readable run statistics got table text with no error. It now produces a JSON object whose groups mirror the table view (`format`, `lines`, `events`, `throughput`, `timestamp`, `time_span`, `levels`, `keys`, plus `ragged_rows`/`decode_warnings`/`assertion_failures`/`files` when relevant). Bare `-s` and `--stats=table` are unchanged.
- **Minute-precision timestamps (`2024-01-15 12:00`) now parse** - `--help` for `--since`/`--until` advertised journalctl-style `'2024-01-15 12:00'` (date plus `HH:MM`, no seconds), but the timestamp parser had no matching format: only the seconds-bearing form (`2024-01-15 12:00:00`), date-only, and time-only parsed. The documented value failed with `Could not parse timestamp` and exit 2, and the same gap silently dropped minute-precision *event* timestamps from filtering/sorting. The parser now accepts minute precision in both separators and with an optional zone marker — `2024-01-15 12:00`, `2024-01-15T12:00`, `…T12:00Z`, `…T12:00+00:00` — for both `--since`/`--until` bounds and event-field parsing. Seconds-bearing forms are unaffected (`parse_from_str` is full-match, so the new entries cannot shadow them).
- **Missing-field typo hint now survives the implied `-m` of `--freq`/`--describe`** - The `track_*` functions skip events whose field is `()` (missing) and, on a high skip count, print a "Tracking skipped events with missing values: NAME (N). A high count can indicate a field-name typo." hint — the signal that catches a typo'd field name. But the `--freq`/`--describe` sugar (and `--metrics`/`--drain`) imply diagnostics suppression to keep machine-readable stdout clean, and that suppression also hid this hint. So `kelora app.log --freq stauts` (a typo) reported a bare `No metrics tracked` with no clue why — exactly the stuck-user case the hint exists for. The hint is now treated like the script-error summary: it survives a data-only mode's *implicit* suppression (printed on stderr, never polluting stdout, exit code unchanged at `0`), while an *explicit* `--no-diagnostics` and `--silent` still suppress it. A genuinely-tracked field stays quiet.
- **`--merge-sorted` on stdin no longer silently skips its validation** - `--merge-sorted` opens named files, merges them by timestamp, and aborts on disorder or missing timestamps. Fed from stdin (no file arguments), it instead passed the stream straight through — disordered events came out in their original order with exit `0`, and missing-timestamp/parse-failure checks were dropped entirely, so the flag's whole contract was silently void on the one input you can't pre-sort. Stdin is now rejected with a clear error pointing at the files-take-arguments form (`kelora --merge-sorted app-*.log`); file inputs (single or multiple) are unchanged.
- **`-k/--keys` and `-K/--exclude-keys` now trim whitespace around comma-separated entries** - Writing the list with spaces after commas — `-k 'ts, level, msg'`, a natural habit and how many shells leave it after editing — silently selected only `ts` and dropped ` level`/` msg`, because clap's `value_delimiter` split on commas without trimming, leaving leading spaces on every entry but the first. (`--exclude-keys ' secret '` likewise failed to drop the field, leaving data meant to be scrubbed in the output.) `--levels` and field specs already trimmed; keys now match, and empty entries (`-k 'a,,b'`) are dropped. Genuine typos among the names are still flagged by the existing unseen-key hint.
- **Self-relative `now+`/`now-` time bounds now work, and space-separated negative durations parse** - Two documented `--since`/`--until` forms were broken. (1) `kelora --since now-15m` ("the last 15 minutes", straight from `--help-time`) failed with `Could not parse timestamp: now-15m`: `resolve_time_range` only routed a bound through the anchored-timestamp parser when the *other* bound referenced it (`since+`/`since-`/`until+`/`until-`), so a self-relative `now+`/`now-` expression standing alone never reached the `now`-anchor branch and fell through to the plain timestamp parser. All independent bounds now parse through `parse_anchored_timestamp`, which understands `now±` and falls back to normal parsing for plain timestamps and bare durations. (2) The space-separated negative form `--since -30m` (advertised in both the `--since` help text and `--help-time`, e.g. `--since -2h --until since+1h`) was rejected by the argument parser as `unexpected argument '-3' found`, forcing the `--since=-30m` equals form; `--since`/`--until` now accept leading-hyphen values. Garbage like `--since now-bogus` still exits `2`.
- **`--discover` Seen/Miss are now per-event for array and nested fields** - A flattened array-element row (`tags[]`) counted one "observation" per element, but the Miss% divided by the *event* count — so an array present in every event read as mostly-missing (e.g. a 3-element array in 1 of 20 events showed "Seen 3, Miss 85%" instead of "Seen 1, Miss 95%"), and the row disagreed with its own container row. The Seen column and Miss% now count the number of events that contained the path (matching scalar fields and the container row); element-level type breakdown, cardinality (Uniq), and samples stay element-scoped as before. The machine-readable `--discover=json` form follows suit: `seen`/`missing` are per-event, and a new `observations` field carries the raw per-element count.
- **`-k/--keys` hint guides nested paths to `get_path` instead of guessing the parent** - Pasting a nested name from `--discover` (`-k api.queries`, `-k tags[]`) failed silently-ish: `-k` selects whole top-level fields and can't address nested values, and the "never present" hint's nearest-match heuristic just suggested the bare parent (`Did you mean 'api'?`), discarding the nesting the user asked for. When an unseen key looks like a flattened path whose leading segment is a real field, the hint now explains that `-k`/`--exclude-keys` act on top-level fields and points at the fix: `--exec 'e.val = e.get_path("api.queries")'` for a value nested in a map, or `-k tags` to keep the whole array for the `tags[]` element notation. A top-level field whose literal name contains a dot is unaffected — it's present, so the hint never fires for it. `--help` for `-k`/`-K` documents the top-level-only behavior.
- **CSV/TSV quoted fields with embedded newlines are reassembled instead of corrupting rows** - A valid RFC 4180 record whose quoted value contained a newline (`"a","line1<newline>line2"`) was split on the physical newline before parsing, so one logical row became two — the first truncated and the second misaligned into the wrong columns — reported only as a soft "ragged rows" hint with exit `0`. Worst of all, Kelora's *own* CSV output round-tripped into corruption (`kelora -F csv | kelora -f csv`). The line-oriented reader now uses a quote-aware chunker for the CSV/TSV family that tracks double-quote parity across physical lines and hands the parser one complete record, so embedded-newline values survive intact and `--strict` no longer raises a misleading column-count error on them. This reassembly is sequential-only; under `-P`/`--parallel` (which reads line-by-line) a split record is now reported as a clear `Unterminated quoted field` parse error — pointing at sequential mode — rather than silently corrupted. A new parser guard rejects any record that ends inside an open quoted field, so corruption can no longer masquerade as success in either mode.
- **`-k/--keys` no longer falsely warns about fields created in `--exec`** - The "names field '…', which was never present in the input" typo hint compared `-k`/`--exclude-keys` names against the fields discovered in the *input*, captured before script stages run. A field produced by a transform and then selected — `--exec 'e.total = …' -k total`, a core workflow — was flagged as missing on stderr even while its value printed on every line, both misleading users and training them to ignore the genuinely useful hint. The check now also counts fields produced by scripts (the union of input and output keys), so created-then-selected fields are quiet while real typos are still flagged.
- **Level hints and `--stats` no longer advertise non-level values as levels** - The level filter (`-l/--levels`) reads the first present field from the level-name list (`level`, `lvl`, `severity`, …) and ignores the rest, but the stats collector that feeds the "levels present" hint and the `--stats` "Levels seen" line recorded values from *every* such field. An event with `level:"WARN"` and `severity:"high"` made `high` show up as a level, so `-l`'s mismatch hint and `--stats` advertised a value the filter could never match (the collector's `break` was gated on the dedup insert, so a repeated primary level let a lower-priority field through). Both collectors now stop at the first present level field, exactly as the filter does.
- **Duplicate error when no input file could be opened** - When auto-detection couldn't open any input file (e.g. `kelora missing.log`, or `kelora -P missing.log`), Kelora printed the specific per-file reason (`Failed to open file 'missing.log': No such file…`) *and* a redundant generic line (`Pipeline error: Failed to open any input files for detection`). The per-file reason already names every file that failed and why, so the generic line is now suppressed — both in sequential and parallel auto-detection, for single and multiple missing files, and for directory-only inputs. The run still exits `1`, and the generic line was only ever redundant: explicit-format runs (`-f json missing.log`) already reported once. Genuine pipeline errors (a failed `--exec` under `--strict`, a `--merge-sorted` abort, …) still print normally; suppression is scoped to the already-reported "all inputs failed to open" case via a typed marker error, not a string or counter match.
- **Misleading "stdin is empty" hint on unparseable input** - Piping content that produced zero events (e.g. plain text fed with `-j`) printed a "Parse errors" report *and* a contradictory "No input: stdin is empty…" nudge. The hint's guard checked `lines_read`, which is only incremented under `-s/--stats`, so on the normal diagnostics path it stayed `0` even when lines were read. The guard now also checks `lines_errors` — lines that were read but failed to parse — so input that arrived is no longer reported as empty. Genuinely empty input (`kelora < /dev/null`, an empty upstream pipe) still gets the nudge.
- **`track_top`/`track_bottom`/`track_top_by`/`track_bottom_by` no longer drop heavy hitters** - All four ranking functions truncated their tracked list to N after *every* event, so a frequent item first seen once the N slots were already filled re-entered at count 1 and was evicted before it could accumulate — silently returning the first N distinct items rather than the most frequent. (On a stream of `aaa`,`bbb`,`ccc` followed by `zzz` 100 times interleaved with fresh singletons, `track_top("m", line, 3)` reported `aaa`/`bbb`/`ccc` at count 1 and never showed `zzz`.) Each function now retains every distinct item (like `track_freq`) and ranks/truncates to N only when metrics are emitted — text, JSON, `--metrics-file`, and the `--end`/`--span-close` `metrics` global — and the parallel merge keeps all items instead of trimming per worker. Results are now exact. Memory for these metrics is now proportional to the number of distinct items, as it already is for `track_freq`.
- **In-place `absorb_*`/`merge` mutations no longer dropped from events** - Whole-event mutating calls — `absorb_kv`, `absorb_logfmt`, `absorb_json`, `absorb_regex`, `merge`, `enrich`, `rename_field` — and Rhai's in-place collection mutators on a nested field (`e.tags.push(x)`, `e.meta.set(k, v)`, …) were visible within the same script but silently discarded from the emitted event and from later `--exec` stages, unless the script also contained an explicit `e.field = …` assignment. This broke the documented `e.absorb_kv("msg")` workflow (including the quickstart) outright. Kelora now detects these mutators rooted at `e` and runs the write-back; read-only methods (`has`, `get_path`, …) stay unflagged so read-only execs keep their fast path.
- **`span.metrics` no longer silently drops non-additive aggregators** - Inside `--span-close`, `span.metrics` was computed by diffing the global tracker against the span's opening baseline, which only works for additive aggregators. `track_avg`/`track_percentiles` never appeared, and `track_max`/`track_min` surfaced only when a window happened to move the global extreme (and then reported the *global* extreme, not the window's) — yielding silently wrong-or-missing per-window stats for core queries like "max latency per 5-min window". Now: `track_avg` reports the true per-window average (computed as `Δsum / Δcount` from its cumulative `{sum, count}`), joining `track_freq`/`track_sum`/`track_unique` as correct per-window values. Genuinely non-additive aggregators (`track_min`, `track_max`, `track_percentiles`, `track_cardinality`, `track_top`/`track_bottom`, `track_top_by`/`track_bottom_by`) cannot be reduced to a single window, so they are omitted and Kelora prints a one-time warning per metric key pointing to the `span.events` workaround (suppressed by `--no-diagnostics`/`--silent`).
- **Filter and exec error counts no longer undercounted** - A `--filter` that errored on every line reported "Filter errors: 1 total" instead of the true count, because the filter error paths skipped the thread-local→context sync that the success path performs (the next event then clobbered the increment). Both filter error branches now persist their counts, matching the exec path. (Exec error counts, separately, were being discarded by the stage's atomic-rollback path and now survive rollback.)
- **Spurious `conf` read-only error fixed** - Reading `conf` in an `--exec` stage and then filtering on the derived field in a separate `--filter` that doesn't name `conf` raised a false "conf map is read-only outside --begin" error, breaking the documented read-in-exec / filter-later pattern. The immutability check is now gated on whether the stage actually references `conf`; genuine `conf` mutations are still rejected.
- **Script-error scope restored in `--metrics`/`--drain`** - The "affecting every event" total-failure indicator in the script-error summary is derived from event counts, but data-only modes disabled stats collection to keep the hot path lean — silently dropping the most useful part of the summary exactly where a stuck user lands (e.g. a one-argument `track_freq(e.missing_field)` — which errors with a usage hint on every event — reported a bare error count with no scope). The scope now surfaces in these modes; the advisory follow-up ("…Use `--strict`…") honors `--no-diagnostics` and the suppression implied by data-only modes, and is re-enabled with `--diagnostics`.
- **Parse errors no longer swallowed in `--metrics`/`--drain`** - These modes disabled stats collection, so parse failures produced no summary and exited `0` — contradicting the documented contract that parse errors exit `1` and that exit codes are preserved across quiet/data-only modes. Parse errors are now reported on stderr and exit `1`, matching normal mode. (Plain `--no-diagnostics` on event output keeps its existing fast path.)
- **Zero-result hint for level/time filters on the wrong input** - Running `-l/--levels` against unstructured input that has no level field (e.g. plain `line` logs), or `--since/--until` against input with no parseable timestamp, silently dropped every event with no explanation. Kelora now prints a hint naming the structural cause and a workaround (parse levels with `-f cols/regex` or match text with `--filter`; set `--ts-field`/`--ts-format`), matching the existing "0 events matched" hint for unseen `--filter` fields. A genuine value mismatch (level present but unmatched, timestamps present but out of range) is still treated as a legitimate empty result.
- **Typo hint for `-k/--keys` and `--exclude-keys` on names never present** - Naming a field that never appears anywhere in the stream was silent: `-k timestamp` against `ts`-keyed logs emptied every event and produced empty output with exit `0`, and `--exclude-keys passwrd` (a typo for `passwd`) quietly failed to drop the field, leaving data meant to be scrubbed in the output. Kelora now prints a hint naming the unseen key, with a "Did you mean '<field>'?" suggestion for a near match or an inline list of the fields actually present otherwise (falling back to `--discover` when that list is long). The exclude variant also states that nothing was removed. The check fires per key — so a typo among otherwise-valid keys (`-k ts,levle`) is still flagged even when output is non-empty — but only on names absent from the *entire* stream, so heterogeneous logs whose fields are present in only some rows are never flagged. It remains a hint (exit `0`), like the other zero-result hints, and honors `--silent`/`--no-diagnostics`.
- **`track_stats` metrics now usable in `--end` and `span_close`** - Percentile, average, and cardinality sketches were exposed as raw blobs, making `metrics["foo_p95"]` unusable. They are now properly finalized to scalar values.
- **`track_freq` average false positive** - A `track_freq` frequency map whose values are literally named `sum` and `count` no longer renders as a bogus average; output finalization keys off the recorded operation instead of sniffing the value's shape.
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
