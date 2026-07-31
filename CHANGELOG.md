# Changelog

All notable changes to Kelora will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

Headline changes: stack-trace presets for `--multiline`, `--span-summary` for per-window rollups without a script, `--drain-diff` for comparing two logs at the template level, and a rebuilt `--drain` clustering engine. The rest is fixes, mostly in multiline handling, timestamps, and diagnostics.

Two things to know before upgrading: `--drain` templates and template IDs change for many messages (clustering was substantially wrong before), and `--since`/`--until` now run before your script stages, so they narrow metrics too.

### Added

- **Auto-detection now samples the file head and handles mixed-format files** - `-f auto` (the default) and `-f auto-per-file` used to judge a whole file by its first non-empty line: a stray banner line pinned everything to `line`, and a JSON log with interleaved plain-text lines dropped those lines as parse errors, with a post-run warning telling you to re-run with `-f json,line`. For **file input**, detection now samples up to the first 64 non-empty lines (capped at 256 KiB). A homogeneous sample detects exactly as before; a mixed sample makes kelora do what that warning suggested, automatically — parse with a **capped cascade**: the dominant structured format plus the `line` catch-all (`cascade(json,line)`), adding the usual `_format` field per event. Auto-detection never builds anything wider than two members, deliberately: per-line detection has good recall but imperfect precision, and in a wide auto-built cascade one accidental member changes how every other line in the file is parsed. Three guardrails follow from that. A structured format needs **at least two sampled lines** to anchor a cascade (in samples of four lines or more), so a single `key=value`-shaped line in a CI log can't recruit the logfmt parser for the whole file. Any *further* structured formats in the sample parse as whole `line` events, and a hint (💡) names them along with the explicit cascade that would parse everything (`-f json,syslog,line`) — wider cascades remain available, but only when a human writes them. And the auto-built cascade always ends in `line`, even when no sampled line needed it, so a line past the sample window can never become a parse error on the strength of a guess. Under `-v` the decision is stated as `Auto-detected format: cascade(json,line) (mixed formats in first N lines)`. CSV/TSV stays whole-file: a file starting as csv/tsv never becomes a cascade, and a csv-looking line mid-file counts as `line`. **stdin is unchanged** — a live pipe never waits for a sample, so mixed streams still want an explicit `-f json,line`.

  Detection also puts its sample to a second use: if the sampled lines contain a recognizable stack-trace shape (Java frames / `Caused by:`, Python tracebacks, Go panics and goroutine dumps), kelora drops a once-per-run hint (💡) suggesting the matching `--multiline java|python|go` preset. Just a hint — multiline stays opt-in, since merging lines changes event boundaries (counts, filters, spans) and should never be guessed at. Before auto-cascading, trace lines usually surfaced as parse errors whose warning pointed at `--help-multiline`; now that they parse as `line` events, this hint carries that signal instead. Quiet when `--multiline` is already set, and gated like every other hint (`--no-hints`, `--silent`, data-only modes).

  Plain (uncompressed) files of 32 KiB or more are additionally **probed at a few deeper offsets** (1/4, 1/2, 3/4, and the tail — a bounded window of lines each), because a head sample of any size can't see a format change at line 50,000: concatenated rotations, or a service that switched from plaintext to JSON mid-file. Probe windows skip the partial line they land in and drop any line truncated by the window, so a cut-in-half JSON line can't masquerade as `line` and pollute the cascade. gzip/zstd files sample the head only (byte offsets in a compressed stream are meaningless for the decompressed lines).

- **`--multiline java`, `--multiline python`, `--multiline go` — stack-trace presets** - Say what your logs contain instead of describing boundaries. These cover the case no other strategy could chunk: traces in output *without* reliable timestamps (raw stderr, container stdout, CI logs), where `timestamp` has nothing to lock onto and `indent` splits on the unindented lines every trace pivots on (`Caused by:`, `goroutine N [running]:`, Python's final `ValueError: …`). Each preset knows its language's real trace shapes — Java frames, `Caused by:`/`Suppressed:` chains and `... N more`; Python chained exceptions, exception groups and header-less `SyntaxError` blocks; Go `panic:`/`fatal error:`, signal lines and goroutine dumps — and keeps a trace attached to the line that logged it rather than making it a separate event. Every other line is its own event. Presets join with newlines by default, and a line starting with a timestamp in the file's locked format always begins a new event, so a preset can't bleed a trace across a real boundary; on timestamped input kelora hints once that `--multiline timestamp` is the better tool. Corpus files: `examples/stacktrace_{java,python,go}.log`.

- **`--span-summary[=text|tsv|json]` — one rollup row per closed span, with no script** - "Events per minute" used to mean `-q --span 1m --span-close 'print(...)'`, plus knowing that `-q` is load-bearing, that the label is `span.start` for time spans but `span.id` for count and field spans, and that `span.metrics` omits zero deltas. It is now `--span 1m --span-summary`. Each row carries the span label, the count of events that survived filters, and every per-window metric from an additive `track_*` call — including the ones `--freq`/`--describe`/`--card` synthesize. The same flag covers errors per 5 minutes (`-l error --span 5m`), a per-minute breakdown by field (`--freq level`), session sizes (`--span-idle 5m`), and a time series for DuckDB or gnuplot (`--span-summary=tsv`). A bare `--span-summary` prints `text` on a terminal and `tsv` when piped; `json` gives one object per line with nesting intact.

  Rows are **data on stdout**, so `--no-script-output` and `-m` leave them alone and only `--silent` removes them. A `--span-close` hook keeps working and keeps its output — the hook runs first, then the row. Rows are **sparse**: a window with no events produces no row, so a plotted series needs its gaps filled downstream. Four situations that used to fail quietly now say so: events arriving after their window closed, input with no usable timestamp (which printed nothing at all), a field span whose label repeats (rows per contiguous run, not per distinct value), and non-additive metrics with no per-window value. For hooks that need custom shapes, `span.metric("level.ERROR")` returns a per-window value or `0`, and `span.label` gives the same label the rows use.

- **`--drain-diff` — template-level log comparison** - Answers "what changed?" between a baseline and a target log by diffing *which message templates occur and how often*, not what the messages say. Define the sides three ways: two inputs (`kelora --drain-diff old.log new.log -k msg`, first is baseline), one input split at a timestamp (`--cut-at 2026-07-24T14:00Z`), or one input split at the first match of a predicate (`--cut-before`/`--cut-after`, no timestamps needed). The report borrows unified-diff syntax: `---`/`+++` name the sides with their event counts and spans, then one row per changed template — `+` only in the target, `-` only in the baseline, `*` in both at a materially different rate (`14× more`).

  Comparisons use per-side **shares**, so a 10-minute incident capture diffs fairly against a 24-hour baseline. There are deliberately no threshold flags: a `*` row appears when the change is too big to be chance at those event counts *and* big enough to matter, and everything else is tallied in the footer. Four formats — `table` (terminal default), `tsv` (nine fixed columns, the default when piped; `--drain-diff=table` forces the report through a pipe), `json` (`new`/`gone`/`freq_changed`, with `delta_pp` and a signed `z_score`), and `full` (each row plus template id, counts, shares, and a real sample line). `--filter`/`--exec` run before the comparison, so you can diff only errors or normalize tokens first. Requires `--keys` with exactly one field, like `--drain`; sequential mode only. A `-k` naming a field no event carries fails with exit `1` rather than certifying an unexamined log as unchanged.

- **`--input-year <YYYY|auto>` — supply the year for year-less timestamps** - Syslog, glog and redis carry no year, so kelora guessed one from the wall clock and warned that it had guessed — a warning with nothing to act on. Now `kelora Linux_2k.log --input-year 2005` dates a 2005 archive to 2005, and `--since`/`--until`, `--span` boundaries, `--merge-sorted` ordering and `-z`/`-Z` output follow. An explicit year silences the ±1yr warning. The year is stated once per run, so a log crossing a year boundary still wants `auto` (the default); anything that is neither fails with exit `2` before input is read.

- **`--multiline blank` — paragraph mode** - Blank lines separate events; the separator belongs to no record.

- **`--multiline-max-lines N` — a safety cap on the multiline buffer** - A `regex:match=` that never matched again used to buffer the entire input with no warning. Events now split after N buffered lines (default 10000, `0` = unlimited) with a once-per-run warning (🔸). `--multiline all` is exempt — buffering everything is its purpose.

- **`track_*` functions accept a timestamp as a category** - `track_freq("hour", meta.parsed_ts.round_to("1h"))` used to fail on every event. Timestamps now work across `track_freq`, `track_top`/`track_bottom` (and their `_by` forms), `track_unique` and `track_cardinality`, keyed as ISO 8601 and byte-identical to `.to_iso()`, so mixing both spellings tallies into one bucket. Existing scripts are unaffected.

### Changed

- **Logfmt auto-detection requires two pairs and balanced quotes** - Detection used to be "does the logfmt parser accept the line", and the parser — correctly tolerant under an explicit `-f logfmt` — accepts any line whose every whitespace-separated token contains `=`. That claimed URLs with query strings (`…/search?q=timeout&limit=10`), base64 padding (`…Zw==`), env dumps (`PATH=/usr/local/bin:…`), CLI flags (`--config=/etc/app.yaml`), and — via the unterminated-quote tolerance — any prose behind `msg="…`. With sampled detection each of those could recruit logfmt into a cascade for the whole file. Detection now demands at least two `key=value` pairs and rejects unterminated quotes; **`-f logfmt` itself is unchanged** and stays maximally tolerant, so a single-pair-per-line log (`cache=hit`) still parses — it just wants the explicit flag instead of auto-detection.

- **Syslog (RFC3164) requires a real timestamp** - The pattern accepted any three-letter word as the month and any `\d\d:\d\d:\d\d` as the clock, so `Job 15 12:00:00 worker task: completed` parsed as syslog — inventing `ts='Job 15 12:00:00'`, `host='worker'`, `prog='task'` from a line that was never syslog, and dragging the syslog parser into auto-built cascades. The month must now be a month name (any case), the day 1–31, and the clock a valid 24h time. This tightens explicit `-f syslog` too: a line rejected by the new rule previously "parsed" into fabricated fields whose timestamp failed to parse downstream anyway.

- **A cascade miss names the cascade, not the last member** - A line no cascade member accepted was reported with the last member's own error (`Key cannot contain spaces` from logfmt, say), misattributing the failure to one arbitrary format. The error now reads `No format in cascade(json,logfmt) matched this line; add a catch-all like 'line' as the last format`. Auto-built cascades can no longer hit this at all — they always end in `line` — so it only concerns explicit `-f` lists, where failing loudly is what you asked for.

- **`--multiline timestamp` locks onto the first timestamp format it sees** - It used to accept any prefix the adaptive parser could read as a header, including things that are just prose in a log: `17:03 was the incident window`, `now retrying`, `today …`, `+30m elapsed`, a bare 10-digit number. Header detection now uses a restricted parse and locks onto the first header's format family, so a time-only or syslog-shaped prefix on a continuation line cannot split an ISO-headered event. Files that genuinely mix header formats can opt out with `timestamp:loose`; if the lock latches onto a first line that only *looks* like a header, pin the format with `timestamp:format=…`.

- **`timestamp:format=` is now a contract, not a preference** - The format hint used to fall back to full adaptive detection when it didn't match a line, re-admitting the false-positive headers it was given to rule out. With a format hint, only that format detects headers.

- **The multiline idle flush is off for files, configurable, and no longer silent** - An undocumented 400ms timer flushed a partly-buffered event whenever input went quiet, so a slow writer pausing mid-stack-trace got the trace split — silently, with no flag to prevent it, and only in sequential mode. `--multiline-timeout DURATION` now controls it (`0` = never); the default is **off when every input is a regular file** and 400ms for streams, where it keeps `tail -f | kelora` responsive. Both execution modes behave the same, and a defaulted stream flush hints once (💡) that events may appear split.

- **`--multiline all` honors `--multiline-join`, whose default is now per-strategy** - `all` always joined with newlines and silently ignored an explicit `--multiline-join`. The flag is honored everywhere now; bare `--multiline all` keeps newline joining, everything else keeps space.

- **`--drain` defaults are measured rather than borrowed: `depth` 2, `similarity` 0.8** - `just drain-accuracy` scores all 16 [loghub](https://github.com/logpai/loghub) `_2k` datasets against their ground truth and fails against a committed baseline (corpus fetched on demand; like fuzzing, it stays out of CI). It tracks over- and under-merging separately, since over-merging destroys a distinction the output can't recover. Mean grouping accuracy went from 73.0% to 83.7% over this release, with no dataset worse than it started. Note `depth` now means the number of keyed tokens (see the clustering fix below).

- **`--stats` names the value it announced, and flags an implausible span** - The timestamp line now ends `— access via meta.parsed_ts`, which is the moment you learn a timestamp exists and want to know how to reach it. A span wider than ~10 years now warns (`check for mixed timestamp formats, two-digit years, or clock skew across sources`) instead of being printed as fact beside "100% parsed".

- **An expression typed into `--freq`/`--describe`/`--card` now says so** - These flags take a `FIELD` name, so `--freq 'e.status / 100'` looked for a field with that name and counted nothing, offering only the generic "likely a field-name typo" hint. An argument that recorded no value *and* cannot be a field name (whitespace, parentheses, quotes) now points at the mechanism that does take an expression: `-m --exec 'track_freq("hour", meta.parsed_ts.round_to("1h"))'`. Ordinary typos still get the typo hint, and a field name that genuinely contains a space is never second-guessed.

- **Documentation fixes** - Examples that didn't run as written, and rules that were never stated:
  - Thirty-one regex examples across the function reference, cheatsheet, tutorials and how-tos wrote patterns as `r"\d+"` — Rust and Python syntax, not Rhai, which spells a raw string `#"\d+"#` — so copying one failed to compile before a single event was read. All are fixed, and both forms are now documented in the cheatsheet and `--help-rhai`. Two examples also passed a regex to `replace()` and now call `replace_regex()`.
  - `meta.parsed_ts` appeared nowhere in `--help-functions` or `--help-time`, the two pages you search when hunting for a timestamp. It now leads the datetime block, a new `EVENT METADATA` section lists the whole `meta` map, `--help-time` shows how to read the parsed value in scripts, and searches for `parse_ts`, `strptime` and friends resolve to the entries that answer them.
  - `extract_regex_maps()` was documented as if named groups became fields. Each match becomes a *single-field* map holding capture group 1; the entry now says so and shows the real multi-field fan-out with `extract_regexes()` + `emit_each`, plus `absorb_regex()` for promoting named groups.
  - The `extract_*` and slicing functions (`before`, `after`, `between`, …) return `""` when there is no match, which nothing documented — so `--freq` over a failed extraction silently gets one large empty-labelled bucket at the top of the table. The rule and the fixes (`or_empty()`, or `?? "unknown"`) are now in `--help-functions`, `--help-rhai` and the function reference.
  - [Sanitize Logs Before Sharing](docs/how-to/extract-and-mask-sensitive-data.md) scripted fields its sample data doesn't have, so the redaction step scrubbed nothing, and it gated on `--filter`, which exits `0` whether or not anything matched — so the "potential PII found" branch fired even on clean data. It now uses real fields from `examples/email_logs.log` and gates on `--assert`, whose exit code *is* the verdict.
  - `kelora -h` now lists `--freq`.

### Fixed

- **A `-l`/`--levels` filter can no longer turn a wrong-format run green** - `kelora -f logfmt app.log -l error` returned `0` whether or not the format was right: the raw-line level pre-filter dropped every line before parsing, so the run recorded zero parse successes *and* zero parse errors, and the "parser never once succeeded" condition that produces `exit 1` never tripped. In CI that meant a pipeline pointed at the wrong parser, or at a log whose format changed upstream, reported success while producing no events at all. Whether it happened to work was decided by substring coincidence — `-l json` on unparseable text failed correctly only because the token appeared in it. kelora now parses dropped lines while that signal is still undecided, stopping at the first line that parses, so a run that parses nothing reports exactly what it would with the optimization off (same exit code, same error count) while a normal run pays one extra parse. `--strict` disables the pre-filter outright: a run that asked to abort on the first parse error cannot skip parses, and it too was being silently voided. On runs where the parser *did* succeed, the reported parse-error count still covers only the lines that passed the level filter. (#401)

- **Auto-detection no longer recompiles its regexes for every sampled line** - Head sampling turned a once-per-run cost into a per-line one. Each candidate line ran through detectors that rebuilt their parsers from scratch — the syslog and combined regexes, and, for any line that reached the named-format step, all 13 built-in patterns including the very large S3 and HAProxy ones. At one line that was tolerable; at up to 64 head lines plus mid-file probes it dominated the run. A 12 KB plain-text file went from 23 ms to 790 ms, and a 165 KB one to 1.5 s — small files, spending all their time compiling the same regexes over and over. Detection parsers are now compiled at most once per process (and per named format, so a json or logfmt file never pays for the twelve it doesn't consult). Detected formats, notices, hints and output are unchanged, byte for byte; only the time to reach them is. Files that detect as json were never affected, since JSON is recognised before any regex runs.

- **A `-l`/`--levels` value that matches nothing now explains itself on JSON and logfmt input** - `kelora -j app.jsonl -l error` on a log whose levels are `E`/`I`/`W` printed nothing, hinted nothing, and exited `0` — the "all clear" reading the level-vocabulary hint exists to prevent. The hint was never missing: it fires correctly for glog, csv, `cols:`, `regex:` and syslog, and was being starved on the two formats where a raw-line pre-filter drops lines *before* parsing, leaving `events_created` at zero. Which mistakes got help was decided by substring coincidence — `-l ror` and `-l arn` were diagnosed while `-l eror` and `-l warning` were silent. Two further symptoms went with it: the hint could report `levels present: ERROR` for a log holding `ERROR`, `INFO` *and* `WARN`, because only the lines containing the requested text had been parsed; and a piped run was told `stdin is empty` when it was not, blaming the plumbing for a flag typo. On pre-filtered runs kelora now states only what a substring test proves — that the text appears nowhere in the input — and points at `--freq <level-field>` for the real vocabulary rather than listing a subset as if it were complete. Non-pre-filtered runs keep the fuller `levels present: …` wording, stdout and exit codes are unchanged, and the optimization is intact. (#369)

- **`--keep-lines`/`--ignore-lines`/`--section` dropping every line no longer reports an empty stdin** - `cat app.jsonl | kelora -j --ignore-lines .` answered `No input: stdin is empty and no files were given` — false, and it points at the pipe rather than the pattern. Same cause as the `-l` fix above: these lines are dropped before parsing, and the drop was only counted under `--stats`, so nothing left in the stats could show that input had arrived. Such a run stays otherwise silent, which is correct — you named the pattern, so an empty result is self-evident — and a genuinely empty stdin is still explained. (#369)

- **The `absorb_*` error-handling examples called a function that does not exist** - `docs/reference/functions.md` showed `warn(...)` as the way to surface a `parse_error` from `absorb_logfmt()`, `absorb_json()` and `absorb_jwt()`. There is no `warn()`; pasting the documented pattern failed with `Function not found: warn` (and a suggestion of `bar`). The examples now use `eprint()`.

- **`-A`/`-B`/`-C` context: every event appears once, with the right marker** - A line covered by two matches was printed **twice**, and a match inside another match's before-window once per window it landed in — three consecutive matches with `-C 2` produced 11 lines for 7 events. These were duplicated *events*, not just repeated markers: `-F json` emitted the same object twice, and `-s` reported `12 total, 6 output, 8 filtered` for a 12-event run. Marker semantics are unchanged and now match `grep` exactly.

- **`--levels`/`--exclude-levels` with `-A`/`-B`/`-C` showed no context at all** - `kelora -j -l error -C 1` printed the error events and nothing else — indistinguishable from omitting `-C`, with no warning — while the equivalent `--filter` worked. Both filters now share one implementation and produce identical output. Note that context around an `--exclude-levels` match is frequently *of* an excluded level; that is what the neighbouring lines are.

- **`--filter` with `--levels`, or repeated, gets one context window instead of none or several** - Context was handed to every `--filter` stage (`--filter A --filter B -C 1` emitted the match three times), while the level filter got it only when no `--filter` existed at all (`--filter … -l error -C 1` stripped the context the filter had just selected). Context is now resolved once, against the **combined** verdict of the pipeline's leading run of filters. `--since`/`--until` stay outside that run, so context is drawn from inside the time window; a filter separated from the run by `--exec`/`--assert` also stays outside, and kelora warns (🔸) when one is there, since it can drop matches and leave their context behind.

- **Context markers no longer disappear under `--no-diagnostics`** - The markers are part of the event line on stdout but were gated on the derived quiet level, so flags for quieting *stderr* silently stripped `/`, `\`, `|` and `*` from the data. `-q`/`--silent` are unaffected. The docs also described `|` as marking "separator lines"; it marks a line that is before-context and after-context at once. `-A`/`-B`/`-C` now document the marker set and the sequential-mode requirement in `--help` too.

- **`--drain` groups lines it had been splitting** - The clustering tree keyed on the leading tokens *and* the token at the final level, so every token of a short message was part of the routing key — and a message whose every token is a key can never generalize, which is the algorithm's defining step. `Invalid user <user> from <ip>` mined 102–110 separate templates at every `depth`, with no setting to work around it. The tree is now kelora's own (the vendored `drain-rs` fork is gone), `depth` means the number of keyed tokens, and because kelora masks before clustering the tree *knows* which positions hold values. On the loghub Linux sample `--drain -k msg` goes from 123 templates to 115; OpenSSH goes from 129 to 28, against 27 real message types. **Expect templates and template IDs to change for anything that was fragmented.**

- **`--drain` no longer masks anything with parentheses in it as `<function>`** - The default filter set included a `function` pattern matching `name(args)`, and it replaced *the whole thing*, identifier included — deciding what counted as a function call from the parentheses alone. So `Intel(R) Xeon(TM) CPU 3.60GHz` mined as `<function> <function> CPU <*>`, `Received SNMP packet(s) from #34#` as `Received SNMP <function> from #<num>#`, and `Unknown cmd fd(5) cmd(80081272){00} arg(ffffda44)` as three `<function>`s in a row; one HealthApp template rendered as the bare string `<function>`, covering two unrelated message types and conveying nothing. It also erased distinctions worth keeping, folding `pam_unix(sshd:auth)` and `pam_unix(sshd:session)` into one template. Over the 16 loghub `_2k` datasets the pattern fired 4272 times and 77% of those hits had nothing to generalize — 56% empty argument lists (`jk2_init()`), 5% the English plural idiom (`packet(s)`, `error(s)`, `file(s)`), the rest already-stable text — while whatever *did* vary inside the parentheses was already covered by `num`, `path` and `hexnum`. It is out of the default set: mean grouping accuracy goes from 87.1% to 87.2% with no dataset worse (HealthApp reaches 100%, Mac gains 1.5pp) and 46 fewer events sit in over-merged templates. The pattern is still defined, so `drain_template(msg, #{filters: ["%{KELORA_FUNCTION:function}", ...]})` opts back in, and `normalized()`/`mask()` are untouched — `function` stays in their default set, where masking a call site is the point.

- **`--drain` templates no longer name one value for events that had another** - The step where a position the cluster's members disagree on becomes `<*>` never ran, so a template stayed whatever its first line produced while counting later lines that said something else, and no `--drain` output could contain a `<*>`. On the loghub Linux sample this reported `session closed for user cyrus` over 123 events, of which 43 said `news`, 36 `test` and 1 `root`; it now reads `session closed for user <*>`, with `--drain=full`'s `sample:` still showing a real line.

- **Templates that describe one message are merged once the whole set is visible** - The tree must place each line as it arrives, which fragments predictably: a family of templates identical but for one position. At end of input, `--drain` and `--drain-diff` now merge such a family when the evidence says that position holds a value. A keyword doesn't look like that (`session opened` vs `session closed` is a family of two), and a merge that would leave a template with no literal text of its own is refused, so distinct event names don't collapse into `<*> <num>`. `--drain-diff` gains most: a brute-force burst that reported as **85 separate `+` rows and no rate change** now reports one row, `Invalid user <*> from <ipv4>`, `6.4× more`.

- **`--drain` masking no longer reads a word as a number or deletes the words around one** - A token was replaced *entirely* as soon as a pattern matched anywhere inside it, so a digit inside a word made the whole word a number (`ssh2`, `utf8`, `sha256`, `eth0`, and every syslog daemon name, since `ftpd[19939]:` contains digits), and a partial match discarded the rest of the token (`HTTP/1.1` → `<version>`, `took=1.5s,retries=3` → `took=<version>`). Only the matched span is masked now, and only at word boundaries: `worker-<num>`, `session_<num>`, `uid=<num>`, `GET <path>?id=<num> HTTP/<version>`, while `ssh2` and `sha256` stay words. Where two patterns overlap the longer wins, so `1.5s` reads `<duration>`.

- **`--drain` no longer splits one message across several templates on the date it contains** - ctime/asctime dates (`Mon Jun 13 03:55:15 2005`) span five tokens and masking saw one at a time, so the numbers masked but `Mon` and `Jun` stayed literal: one logical message became a template per weekday and month, each labelled with one weekday while covering lines that said another. The whole date now masks as one `<timestamp>`, with and without the year, as does the bare syslog `%b %e %H:%M:%S` prefix; a date with no time of day is unaffected. An explicit `filters:` list opts out — it still means "mask exactly these".

- **`--drain` refuses a mined field that was never present** - `--drain -k message` on a log whose field is `msg` printed nothing and exited `0`. `--drain-diff` already refused this and named the fields it had seen; the two modes now agree. A field present on only some events stays silent, and empty input still exits `0`.

- **`--drain` no longer grows without bound on an endless stream** - `tail -f … --drain` on a high-cardinality field grew until the process died. It now holds 10,000 templates and evicts the least-recently-matched one beyond that, warning (🔸) that counts are incomplete.

- **`--drain-diff` no longer presents a whole log as changed when the split left one event on a side** - Every template lands on a `+` or `-` row by construction, so a one-event side reads as a dramatic finding when it only means the cut missed: `--cut-before` matching the last event reported every message type as removed. An empty side was already refused (exit `1`); a one-event side used to slip through at exit `0` with nothing said, and now warns (🔸). A homogeneous but well-sampled side stays quiet. `--cut-after`'s help claimed a degenerate split is refused; both predicate flags now state the empty-side refusal and the one-event warning separately.

- **Percentile tracking is linear again: 100k events go from 4m15s to 1.4s** (#377) - `track_percentiles`, `track_stats` and `--describe` were quadratic in event count: the t-digest gained a centroid per value and was never compressed. 20k events took 9.1s, 40k took 38.6s, 100k took **4m15s**, which put 1M events at roughly seven hours. The digest is now compressed as designed — constant per-event work, ~3KB per metric — and the same 100k run takes 1.4s. The same growth in the `--tailmap` formatter is fixed with it.

  **The reported percentiles are now estimates rather than exact values.** Feeding every value into an uncompressed digest happened to make the old numbers exact. Measured error is 0.1–0.4%, inside the ~1% a t-digest advertises and tightest in the p95/p99 tails these functions exist for. One consequence: under `--parallel` the estimate now depends slightly on which values land in which worker, so parallel runs can differ from each other and from a sequential run within that error bar. Sequential runs remain reproducible.

- **`--metrics=tsv/json` no longer carry float artifacts the human views round away** (#372) - Same run, same metric: `full` and `short` printed `lat_p95 = 190.05` while `tsv` and `json` printed `190.04999999999998` — and bare `-m` auto-selects tsv whenever stdout is piped, so the noisy form was the default in any pipeline. Every metrics view now rounds to six significant figures, never coarser than a whole number. In `json` the type is unchanged and an integer-valued percentile reads `71` rather than `71.0`. The `metrics` map handed to `--end` scripts still carries the unrounded value.

- **`--card`/`track_cardinality` no longer report a fractional count, nor more distinct values than there were values** - The raw HyperLogLog estimate reached the user in full float precision: `distinct_a 2.001955671862574` for two events, and `179825.24214183845` distinct trace IDs on a dump where only 176,825 events had one. Counts are now whole numbers, clamped to the number of values fed to that metric, and identical in sequential and `--parallel` runs. `--metrics=full`/`short` still mark the value approximate with `≈`.

- **`--multiline-join=newline` no longer drops every multi-line event** - The join you reach for on a stack trace was the only one of the three that lost data: regex-based line formats end in a trailing message capture and `.` did not match `\n`, so an event with newlines inside no longer matched the format it had just been detected as and was dropped as a parse error — on a Java trace, the `ERROR` event was exactly the one that vanished. `-f syslog` had the same defect. Both now compile with dot-matches-newline, so all three joins produce the same events and only the message text differs. `^`/`$` still match at the ends of the whole event; a pattern that needs the line-bounded `.` can ask with `(?-s:.)`.

- **A multiline event is reported at the line it started on, not the line that flushed it** - An event spanning lines 2–4 had its parse error printed as `trace.log:5`. `meta.line_num` carried the same offset, and under `--parallel` it drifted further with every multi-line event. Both paths now record each event's first physical line, so parse errors, `meta.line_num` and the two execution modes agree.

- **Multiline events no longer span input files, and `meta.filename` points at the event's own file** - With `kelora -M timestamp a.log b.log`, an unterminated event at the end of `a.log` silently swallowed the head of `b.log`, and `meta.filename` was the flush-time filename. The buffer now flushes at a file boundary, and each event carries the filename of its first physical line. Losslessness is pinned by property tests: no strategy, at any cap, may drop, duplicate or reorder a non-blank line.

- **Blank lines no longer corrupt multiline events — differently per format** - The same stack trace with an interior blank produced *different* corruptions depending on `-f`: under `-f line` the blank split the event and left a junk `"\n  at bar"` event, while under `-f raw`/`-f json` it was silently deleted before the chunker saw it. Blank lines now reach the chunker whenever `--multiline` is active and are *continuations* for `timestamp`/`indent` — real Python tracebacks and Java `Caused by` sections contain them. Trailing blanks are trimmed from each event, and `--multiline all` stays byte-faithful.

- **`--parallel` no longer drops blank lines inside multiline events** - An interior blank — between a Go panic header and its goroutine block, or inside a Python chained-exception bridge — survived sequentially and vanished under `--parallel`. The two modes now agree byte-for-byte.

- **Multiline regex patterns may contain colons** - `--multiline 'regex:match=^\d{2}:\d{2}'`, matching a time-of-day header, failed with `Unknown regex option: \d{2}` because the option string was split on every `:`. Only `:match=`/`:end=`/`:format=` separate options now; a segment shaped like an option that isn't a known key still fails loudly, so typos don't silently become part of the pattern.

- **A parse error on a grouped event names `--multiline` instead of blaming your key syntax** (#376) - Under a one-record-per-line grammar (`-f logfmt`, `-f combined`, `-f cols:…`), a `--multiline` strategy that absorbs a free-text continuation makes the assembled event unparseable, so the event is *dropped* — turning multiline on left you with **fewer** events than leaving it off. A Java frame indented under a logfmt `ERROR` line removed that error event and reported `Key cannot contain spaces`, and no join value was a workaround. The error summary now carries a hint (💡) whenever a failing event was assembled from more than one physical line: it names the format, says the event was dropped rather than enriched, and points at the parsers that keep free text (`-f raw`, `-f line`, `-f regex:…`).

- **A hint when line filters meet multiline** - `--keep-lines 'ERROR' -M timestamp` silently amputated stack traces, since line filters run *before* assembly. Pre-assembly filtering stays — it is the only way to drop interleaved noise so assembly can succeed — but kelora now hints once (💡) that assembled events are filtered with `--filter`, and the ordering is documented.

- **`--since`/`--until` now narrow aggregates, not just the printed events** - The time window filtered the event stream correctly but ran *after* the script stages, so every metric accumulated in a stage — the whole `track_*` family, including the `--freq`/`--describe`/`--card` shorthands — counted events the window then discarded, at exit `0` with nothing warned. On a syslog sample, `--since … --until … --freq prog` named `ftpd` (916 events) as the top program for a window whose 90 events were 100% `sshd`. `--freq FIELD` and `--keys FIELD` now always agree, in sequential and `--parallel` mode alike.

  **Behavior change: the time window now runs before every user stage.** That is the only way to fix the counts — once an out-of-window event has passed through an `--exec`, its `track_freq` contribution and side effects have already happened. So an out-of-window event no longer reaches `--filter`, `--exec` or `--assert`, and their side effects (`print`/`eprint`, file writes, `--assert` failures) no longer fire for it, which is what `-l/--levels` has always done. Because fewer events are asserted over, a gate combining `--assert` with `--since`/`--until` can move from exit `1` to exit `0`. Which events are *printed* is unchanged. Script-**created** events are the exception, and there the change is a repair: the window used to destroy every `emit_each` event, including one whose own `ts` sat squarely inside it; those now survive, and the window does not apply to them. To narrow emitted events or metrics by time, place a `--filter` after the relevant stage: `--exec 'track_freq(…)' --filter 'meta.parsed_ts >= to_datetime("…")'`. See [Pipeline Model](docs/concepts/pipeline-model.md#why-the-time-window-runs-first).

- **A timestamp built in `--exec` no longer fails silently against `--since`/`--until`** - The window reads the timestamp the *parser* produced, so `--exec 'e.ts = e.when' --since 2024-05-01` dropped every event and blamed the input ("no timestamps were parsed — set `--ts-field`/`--ts-format`"), advice that only works when the timestamp already sits verbatim in a field. The hint now recognizes a script stage assigning to a timestamp field and names the two routes that work: resolve it at parse time with `--ts-field`/`--ts-format`, or drop `--since` and narrow with a `--filter` placed after the assignment. The reverse case — a script rewriting `e.ts` *out* of the window — now warns (`1 printed event carries a timestamp outside --since/--until: …`), and the same covers `emit_each` events. No behavior or exit code changes. `--help-time` gains a "Which timestamp the window reads" section.

- **Two-digit years no longer parse as the first century** - Auto-detection matched the Spark/log4j layout (`17/06/09 20:10:40`) against a 4-digit-year candidate, because chrono's `%Y` accepts a 2-digit number. The result was year **17 AD** reported as `7997/7997 parsed (100.0%)` with a two-millennium span beside it, silently breaking every `--since`/`--until`, `--merge-sorted` and span calculation. `yy/MM/dd` is now a first-class candidate ahead of the `%Y` variant, and 2-digit years follow the POSIX pivot (`00`–`68` → 2000s, `69`–`99` → 1900s). A parse yielding a year below 1000 is now a non-match, so no format can quietly produce a year-17 timestamp.

- **The year-less timestamp warning no longer reports twice as many timestamps as it saw** - A 1992-line syslog file was reported as "3984 parses", since each timestamp is parsed more than once per run. It is now counted once per event and says `timestamps`; `yearless_inferred` in `--stats=json` reports the same corrected number.

- **Auto-detection no longer turns an app log's first line into a CSV header** - Any application log whose first line contained two commas could be claimed by the `csv` step, which consumed that log line as the header row: every field name became a fragment of a log message, everything downstream was garbage, and nothing said so (no warning, exit `0`). Commas in log messages are routine (`[TERM,HUP,INT]`, comma-separated paths). Auto-detection now picks the with-header `csv`/`tsv` variants only if the first line reads as a header row, and otherwise falls through to the application-log formats and `line`. A field marks the line as data if it holds a run of prose (over 30 characters and six or more words), a full ISO datetime, or — *alongside other words in the same field* — a bare level or a date- or clock-shaped token; that last qualifier keeps all-caps warehouse exports (`TIMESTAMP,ERROR_COUNT`) and wide time-series headers (`region,2024-01-01,…`) detecting as `csv`. Detection still reads only the first line, so `tail -f app.log | kelora` is unaffected; the headerless variants and an explicit `-f csv` are untouched.

- **A malformed `cols:` spec now fails once, before reading input** - `-f cols:ts,level,msg` used to raise the identical runtime error on *every* line (7997 errors on an 8000-line file, exit `1`), burying the one thing you needed to know. Specs are validated when the argument is parsed: one message, exit `2`, nothing read. The comma case names the fix (`cols fields are separated by spaces, not commas — did you mean 'cols:ts level msg'?`), and an unrecognized type annotation lists the valid types instead of reporting `age:banana` as a bad field name.

- **"Function not found" now suggests functions kelora actually has** - The suggester matched against a hand-written list of 74 names while the engine registers 216, so 167 real functions were invisible and a call on one got fuzzy-matched to whatever sat closest in edit distance: `e.ts.format("%H")` answered `Did you mean: sort?`. The list also named seven functions that don't exist, so a failed `e.a.ceil()` was answered with `Did you mean: ceil?`. The catalogue now lists all real names and no phantoms; a datetime-only method called on a string is reported as the type error it is (`format() is a datetime method, but it was called on a string`) with a pointer to `meta.parsed_ts` and `to_datetime()`; and names differing only in word order match by token set, so `regex_extract` finds `extract_regex`. No function names, aliases or signatures changed.

- **A config `defaults` line no longer breaks the short-form CLI alias for the same option** (#370) - With `defaults = -f json` in `.kelora.ini`, `kelora -j` died with `error: the argument '--input-format <FORMAT>' cannot be used with '-j'` and exit `2`, naming a flag that appeared nowhere on the command line; same for `defaults = -F json` with `-J`. Naming any member of an alias family (`-f`/`--input-format`/`-j`, `-F`/`--output-format`/`-J`) on the command line now drops that whole family from `defaults`, matching the documented CLI-wins precedence. This also settles the quieter half: `defaults = -f json` plus a CLI `-f csv` used to *append* into a `json,csv` cascade and fail; the CLI value now replaces the config one. Unrelated flags leave `defaults` in place, and `-v` reports what was dropped.

- **An explicit `--script-output` now wins over the data-only modes** (#379) - `-m`/`--freq`/`--describe`/`--card`/`--drain`/`--discover`/`--stats` imply `--no-script-output`, but the suppression was unconditional: `kelora --freq level --script-output --exec 'print(…)'` accepted the flag, printed nothing and exited `0`. The default stays suppressed and an explicit `--script-output` turns it back on. Relatedly (#384), `--stats` suppressed script output through a separate branch that skipped the hint hush; it is now a data-only mode like the rest, with its report, warnings and exit codes unchanged.

- **`tailmap` and `keymap` now show timestamps instead of `line N`** - These formats prefix each line with the timestamp of the event that opened it, which is the whole basis for reading them as a timeline — but `kelora -F tailmap -k rt api.log` printed `line 1 _________3`. They require `--keys` with exactly one field, and the key filter dropped the timestamp field before the pipeline re-derived it, so the fallback fired every time, with no workaround (`-k ts,rt` is rejected). The key filter now keeps the timestamp candidates (and any `--ts-field`) for the three map formats; `levelmap` degraded the same way when given `--keys` and is fixed too. Input with no detectable timestamp still reports `line N`, and runs without a map format are byte-identical.

- **`--discover` no longer withholds the "No input format detected" hint** - `-h` advertises `-d` as the first command to run on an unknown file, but `-d` was the one command that hid the guidance for unknown files, since data-only modes hush hints. `kelora -d prod.log` on an oslo.log file printed a one-row table and nothing else, while plain `kelora prod.log` named the fix (`-f 'cols:ts(2) level *msg'`, which is not something you guess). The hint now survives the `--discover` hush as the actionable half of that mode's own footer. It stays hushed under `-m` and `--drain`, stays on stderr so `-d=json` still parses, and `--no-hints`/`--silent`/`KELORA_NO_HINTS` still hide it.

- **The "No input format detected" hint is printed once per run, not once per file** - Detection runs per file under `-f auto-per-file`, so a run over a directory of plain-text logs opened with a wall of identical multi-line advice. The hint names no file and its advice doesn't vary, so it is emitted at most once. The `-v` "Auto-detected format: X" status line is unaffected.

## [2.0.1] - 2026-07-19

A maintenance release: one fix for following live streams, plus faster parsing and aggregation across the board. Output is unchanged from 2.0.0 — no new options and nothing to migrate.

### Fixed

- **`--metrics`, `--drain`, and `--discover` now print their results when you stop a live stream with Ctrl-C** - These modes normally show their summary once the input ends. When following an endless stream — for example `tail -f app.log | kelora -m …` — that end only comes when you press Ctrl-C, and until now doing so threw away everything collected so far, printing nothing. Pressing Ctrl-C once now prints the metrics table, drain templates, or discover table you'd built up. (Press Ctrl-C a second time to quit immediately; `--silent` still prints nothing.)

### Performance

Kelora is faster on this release, with the biggest gains on large files. How much depends on what you're doing — filtering by level is the standout, and everyday parsing and field selection are meaningfully quicker too. The improvements are internal only; results are identical to 2.0.0.

- **Filtering by level (`--levels`) is dramatically faster when few lines match** - kelora now does a quick scan of each raw line and skips parsing lines that can't possibly match the levels you asked for. On a file where only ~1% of lines are the level you want, filtering runs **around 8× faster** (e.g. 4.8s → 0.6s over a large log); the win shrinks as more lines match, and files where most lines match are unchanged.
- **Faster field selection with `-k`** - When you keep only a few fields (`-k time,level,msg`) from records with many fields, kelora skips building the fields you didn't ask for — up to **~30% faster** on wide JSON records, ~20% on wide logfmt.
- **Faster logfmt and JSON parsing** - The logfmt parser was rewritten to do far less work per line, and JSON string handling was streamlined — roughly **10–13% faster** on logfmt-heavy workloads.
- **Faster reading from a pipe under `--parallel`** - Lines from standard input are now handed to the worker threads in batches instead of one at a time, so the reader is no longer the bottleneck when a filter is doing most of the discarding.
- **Faster `--stats` and `--drain` grouping** - The grouping behind these aggregations does less work per line, about **7% faster** on large inputs.

## [2.0.0] - 2026-06-18

The **2.0** line. The headline changes are a redesigned tracking-function family, a set of built-in application-log formats, composable parser cascades, and a much more capable `--discover` mode. Breaking changes are flagged below — most affect tracking scripts and a few error/validation behaviors. See [What's New in 2.0](docs/whats-new-2.0.md) for migration guidance with old → new examples.

### Added

- **Three-tier diagnostic model with granular `--no-warnings` / `--no-hints`** - Advisory output is now organized into three tiers, each silenced independently: **errors (⚠️)** show unless `--silent`; **warnings (🔸)** flag a recovered problem and are hidden by `--no-warnings` / `KELORA_NO_WARNINGS` / `--silent`; **hints (💡)** are teaching suggestions, hidden by `--no-hints` / `KELORA_NO_HINTS` / `--silent`. Positive counterparts (`--warnings`/`--hints`) and the combined `--diagnostics`/`--no-diagnostics` shortcut override an env var or config default for a single run; precedence is **explicit flag > env var > config default**. The same classification drives both a message's emoji and the flag that silences it, so the two can't drift apart. See [Error Handling](docs/concepts/error-handling.md).
- **`postgres` built-in format** - Parses PostgreSQL server logs written with the default `log_line_prefix = '%m [%p] '` into `ts`, `log_tz`, `pid`, `level`, and `msg`. The timestamp is naive and resolved through `--input-tz` (default UTC); the logged zone abbreviation is kept in `log_tz` but not applied (abbreviations can't encode an offset/DST), so UTC-logged servers are correct by default and a non-UTC server needs `--input-tz <IANA>`. Pair with `-M indent` to fold multi-line `ERROR:`/`STATEMENT:` records. A customized prefix won't auto-detect — use `-f regex:`. (Adapted from [lnav](https://lnav.org), BSD-3-Clause.)
- **`--help` now documents the exit-code model** - A new "Exit Codes" section in the full `--help` reference spells out the resilient-by-default contract: `0` is success even when some lines/transforms failed (recovered), `1` covers genuine failures (unopenable input, failed `--assert`, a gate that never succeeded, `--strict` errors), `2` is invalid usage, plus the signal/panic codes. The one-screen `kelora -h` is unchanged.
- **`cri` format for Kubernetes container logs** - The CRI/containerd on-disk layout `<RFC3339Nano> <stream> <tag> <message>` (the shape of `/var/log/pods/*/*.log` and `kubectl logs --timestamps`) is now a built-in format, parsed into `ts`, `stream`, `tag`, and `msg`. Selectable with `-f cri` and usable in cascades. Because a CRI message is often itself JSON or logfmt, `cri` has a dedicated detector that runs *before* the logfmt/CSV steps, so auto-detection works regardless of the payload. The message is kept verbatim for an optional second-stage parse (`--exec 'e.absorb_json("msg")'`). Kelora-original, not adapted from lnav.
- **Built-in application-log formats** - A curated set of common layouts — `glog` (Go/klog), `nginx-error`, `apache-error`, `log4j`/Java, `python-logging`, `redis`, `s3` (AWS S3 access log), `haproxy` (http/tcp), and `iso8601-level` — are now parsed into structured fields (`ts`, `level`, `msg`, plus extras) via the regex engine. They are first-class: selectable with `-f <name>`, usable in cascades, shown in the auto-detect notice, `--stats`, and `--help-formats`. They are tried only as the last step before the `line` fallback, so nothing already detected changes. Year-less timestamps (`glog`, `redis`) resolve to the current year; `haproxy` is syslog-wrapped, so use `-f haproxy` for the structured fields. (Adapted from [lnav](https://lnav.org), BSD-3-Clause; see `THIRD_PARTY_LICENSES.md`.)
- **Composable `cols:`/`regex:` cascades via repeated `-f`** - `-f` is now repeatable, building a cascade from each spec in order: `kelora -f json -f 'cols:ts(2) level *msg' app.log`. This is the only way to put spec-based parsers (`cols:`, `regex:`) into a cascade — a comma list can't, since a regex may contain commas — and closes the "JSON lines mixed with custom `timestamp LEVEL message` text in one file" case. Catch-alls (`line`, `raw`, `cols:`) must come last; `regex:` is selective and may sit earlier. A single `-f` (including a comma list) is unchanged.
- **Data-driven legends for map outputs** - `levelmap` and `keymap` now append a one-line legend decoding their glyphs, built from the data seen (`E = ERROR | I = INFO | W = WARN`; `2 = 200,204 | 4 = 404 | 5 = 500,503`), matching `tailmap`. New `--legend`/`--no-legend` flags control all three map formats: by default the legend shows only on an interactive terminal, so piped output stays clean.
- **`-d` shortcut and richer output for `--discover`** - Field discovery, the documented starting point for unknown files, gets a short flag: `kelora app.log -d` (`-d=json` for the machine-readable form). `--discover-depth=0` fully flattens deeply nested JSON, removing the previous 3-level cap. See Changed for the expanded discover footer.
- **`-D` shortcut for `--discover-final`, plus a discoverability hint** - The post-pipeline counterpart to `-d` gets its own short flag (`-D`, `-D=json`). To close the "I added `--exec` but my computed field isn't in `--discover`" confusion (`--discover` profiles *parsed input*, before scripts run), the plain `--discover` footer now prints a tip pointing at `--discover-final` whenever the pipeline filters or transforms events. A bare probe stays uncluttered, and JSON output stays machine-clean.
- **`e.get()` map accessor** - `e.get("key")` and `e.get("key", default)` are now registered for events, mirroring `get_path` for top-level keys. Part of a clearer missing-field model (see Changed).
- **Keyword search in `--help-functions`** - `--help-functions [KEYWORD]` filters the 150+ function catalogue by a case-insensitive substring match (name, description, or section) instead of forcing a scroll: `--help-functions ip` lists the IP helpers. The `=KEYWORD` form works too; bare `--help-functions` is unchanged.
- **Keyword search in `--help`** - The full CLI reference is searchable the same way: `kelora --help KEYWORD` prints only matching options under their section headings. A keyword beginning with a dash is a **flag query** — a case-sensitive, whole-token match, so `--help -j` finds only `-j` (never the `-j` inside `--multiline-join`); a bare word is a case-insensitive substring search across names and descriptions. The `=KEYWORD` form works; bare `--help` is unchanged.
- **No-script shortcuts for the most common aggregations: `--freq`, `--describe`, `--card`** - The most common tracking operations are now plain flags. `--freq FIELD` builds a frequency table (`track_freq`), `--describe FIELD` prints a numeric summary (`track_stats` — count/min/max/avg/p50/p95/p99), and `--card FIELD` estimates distinct values (`track_cardinality` — HyperLogLog, ~1% error in constant memory). All are repeatable, accept dotted paths (`--freq user.id`), and run as the *last* per-event stage (post-filter/transform, like `--discover-final`). They imply `-m`, with output via the usual `--metrics=…`/`--metrics-file`. There is deliberately **no** `--top`/`--bottom` flag — `--freq` already sorts by count descending, so ranking is left to the shell (`--freq url | head`). For grouping, custom names, or score-based ranking, write the `track_*` call in `--exec`.
- **Pipe-friendly `tsv` metrics output, auto-selected when not on a terminal** - Metrics now have a tab-separated form alongside the human table and JSON: each metric emits `metric<TAB>key<TAB>value` rows (lists sorted by count/score **descending**, scalars as a single row with an empty key column), with full float precision and embedded tabs/newlines flattened to spaces. Following the `ls` convention it **auto-selects** — human table on a terminal, `tsv` stream when piped or redirected — so `--freq url | head`, `| sort`, `| awk`, `| wc` compose freely. Escape hatches: `--metrics=full` forces the table through a pipe, `--metrics=tsv` forces the stream to a terminal. (Behavior change: `kelora -m … > file` now yields `tsv` rather than the table; add `--metrics=full` for the old rendering.)
- **Intent-based hints for unknown flags borrowed from other tools** - For a curated set of habit flags, kelora now points to the kelora idiom instead of clap's edit-distance guess: `--where`/`--grep`/`--match` → `--filter`; `--sort`/`--rank`/`--top-n` → `track_top_by`; `--count`/`--group-by`/`--uniq` → `--freq`/`track_freq`. These stay unknown (exit 2), so no namespace is reserved. Genuine near-miss typos (`--filer`) still get clap's suggestion.
- **`-P` short flag for `--parallel`** - The performance toggle now has a short form, following the `xargs`/GNU `parallel` convention.
- **"No input" hint instead of silent exit** - A bare `kelora` reading from an empty non-TTY source (`kelora < /dev/null`, an empty pipe) previously exited 0 in silence. Kelora now prints a one-line stderr hint when no files are given, stdin is not a terminal, and nothing was read, suppressed by `--no-input`/`--no-diagnostics`/`--silent`. The `-h`/`--help` text now states that interactive mode requires a terminal.
- **`-l/--levels` warns on a vocabulary mismatch instead of returning a silent empty result** - When `-l` drops every event because the level field uses a different dialect than the filter (glog logs `I/W/E/F`; syslog uses `CRIT` not `CRITICAL`), kelora now lists the levels present (`-l ERROR matched none of the levels present: E,I`) and points at a `--filter` workaround. The filter stays purely lexical. Goes to stderr, exit `0`, suppressed by `--no-diagnostics`/`-q`/`--silent`. A present-but-unmatched level stays quiet.
- **Zero-result hint for the number-vs-quoted-string filter mistake** - On typed input, `--filter 'e.status == "404"'` is always false (in Rhai a number never equals a string), silently emptying the result. When a `--filter` compares a *seen* field for equality against a quoted numeric-looking literal and nothing matched, kelora now suggests dropping the quotes and points at `-s` to check the type. Goes to stderr, exit `0`, suppressed by `--no-diagnostics`/`-q`/`--silent`.
- **Naive-timestamp UTC-assumption hint instead of a silent shift** - Timestamps without a zone offset (syslog, log4j, postgres, …, and ambiguous abbreviations like `CEST`/`PST`) resolve through `--input-tz`, default UTC; for a source logging local time this silently shifts `--since`/`--until`/`--span` boundaries and, under `--normalize-ts`, bakes the wrong offset into output. Kelora now prints a one-time stderr hint when timestamps are naive, no zone was chosen (no `--input-tz`, no `TZ`), and the run depends on the assumption (a time filter, `--span`, or `--normalize-ts`), pointing at `--input-tz <zone>`. It stays silent for the common UTC case, an explicit zone, and offset-bearing timestamps. This **surfaces** the existing timestamp policy (now consolidated in `--help-time`), not a change to it. Suppressed by `--no-diagnostics`/`--silent`.
- **`absorb_jwt()` - flatten JWT claims onto the event** - A new whole-event mutator joining the absorb family: it parses a JWT from a string field and merges its **claims** into the event (header and signature ignored), returning the usual status map. All-or-nothing like `absorb_json()` — a malformed token sets `status = "parse_error"` and leaves the event untouched. Supports `keep_source`/`overwrite`. Signatures are **not** verified (debugging / trusted tokens only); for datetime-typed `exp`/`iat`/`nbf` use `parse_jwt()`.
- **`parse_jwt()` decodes the standard time claims into datetimes** - The JWT parser now exposes the NumericDate claims `exp`, `iat`, and `nbf` as datetimes under `expires_at`, `issued_at`, and `not_before` (alongside the raw integers in `claims`), so they compose with kelora's time machinery: `--filter 'e.token.parse_jwt().expires_at < now()'`, `(jwt.expires_at - jwt.issued_at)`, `jwt.expires_at.to_iso()`. Claims are read as whole seconds; a missing/invalid claim omits its field rather than erroring. Still does not verify signatures.
- **`absorb_logfmt()` - quote-aware sibling of `absorb_kv()`** - A new whole-event mutator that parses a logfmt string field and merges its keys, returning the usual absorb status map. Unlike the plain-splitter `absorb_kv()` (which keeps quotes and splits inside quoted values, mangling `err="connection refused"`), it is quote-aware and infers numeric/boolean types. All-or-nothing like `absorb_json()`. Supports `keep_source`/`overwrite`. The `parse_kv()`/`absorb_kv()` docs now flag their lack of quote-awareness and point here. (Relatedly, strict-mode option errors from `absorb_json`/`absorb_regex` now name the actual function instead of always reporting `absorb_kv:`.)
- **`--max-line-bytes` per-line memory circuit breaker** - A new safety cap on the bytes a single input line may consume in memory (default **64 MiB**), guarding against runaway RAM from a newline-free stream — e.g. a tiny gzip/zstd payload that decompresses into one enormous line. Reading is streamed, so large *multi-line* files are unaffected; only a single over-long line trips it. An over-limit line is truncated to the cap with a warning (🔸, exit 0); under `--strict` it is a hard error (exit 1). Accepts a byte count or IEC/SI suffix (`--max-line-bytes 1MiB`, `1GiB`, `1048576`); `0`/`off`/`unlimited` disables it. The default is sized for ~zero false positives on real logs (Docker/CRI split lines at 16 KB; fat JSON tops out in low single-digit MB). See [SECURITY.md](SECURITY.md). (Recursive ZIP bombs like `42.zip` were never a risk — ZIP input is rejected; only gzip/zstd are supported.) See Changed for the breaking default.

### Changed

- **Diagnostic re-tiering and data-only-mode warning behavior (part of the three-tier model above)** - Several advisory messages were reclassified so a message's appearance matches the flag that silences it: the `--span`/`--parallel` and `--window`/`-B`/`-C` conflict notices, the over-large-span notice, and the "parsing mostly failed" notice are now warnings (🔸); the "writing to a file named `json`; did you mean `-F`?" notice moved from warning to hint (💡). **Data-only modes (`-m`/`--drain`/`--discover`) now hush only hints, not warnings**, so a recovered `--exec` error still surfaces under `-m`. The detection notices lost an inconsistent terminal-only gate and now follow only their tier flags, reaching a stuck user even through redirected/CI stderr. Going further, **a successful run is now silent** (the Unix "rule of silence"): the neutral 🔹 *status* notices ("Auto-detected format: …", config/defaults/alias expansion) no longer print by default — run `-v`/`--verbose` to see them, with **no exception for an explicit `--config-file`** — while a config that *fails* to load is still a loud error. The **generic zero-match notice** is gone (an empty result after your own filter is self-evident); the *specific* zero-result hints stay, since each names a concrete footgun (`-l/--levels` on unstructured input, no timestamps for `--since/--until`, a typo'd filter field, a quoted numeric comparison).

- **Removed: undocumented `KELORA_NO_TIPS` env var** - The single-purpose, never-documented `KELORA_NO_TIPS` (it only ever gated the format-detection tip) is gone. Use `KELORA_NO_HINTS` for all hints, or `KELORA_NO_WARNINGS` for warnings — the env vars now mirror the flags. No deprecation alias is kept.

- **Human metrics table gets labeled, aligned columns** - In `--metrics=full`, list-valued metrics (frequency tables and `track_top`/`track_bottom[_by]` rankings) now print a two-column table with a header row and right-aligned numbers, so a row like `200   3` reads unambiguously as value `200`, count `3`. Columns are named for context (`value`/`count` for `track_freq`, `item`/`count` or `item`/`score` for rankings), the header's count noun agrees with the left column, and the redundant `#1`/`#2` rank prefix is dropped (rows are already in rank order). Display-only: the stored numbers, `tsv` stream, JSON, and scalar metrics are untouched.
- **Breaking: tracking-function redesign (`track_freq`, `track_inc`, `track_top_by`)** - The tracking family is consolidated around one convention: `track_fn(name, args...)`. See [What's New](docs/whats-new-2.0.md#breaking-the-tracking-functions-were-redesigned) for old → new examples.
    - `track_freq(name, value)` is the frequency table, replacing both the old one-argument `track_count(value)` and `track_bucket(key, bucket)`. Values are stringified automatically (no `to_string()` needed), and counts land in separate per-name sub-maps so different metrics can no longer collide. Both old names error with a migration hint.
    - `track_inc(name)` increments a running counter by 1 — sugar for `track_sum(name, 1)` (the two merge identically). For weighted accumulation use `track_sum(name, value)`.
    - Score-based ranking moved from the 4-argument `track_top(key, item, n, value)` to `track_top_by(name, item, score [, n])` / `track_bottom_by`; the old form errors with a migration hint. `n` now defaults to 10 in all four ranking functions.
    - All `track_*` functions now skip Unit `()` (missing-field) values instead of erroring. Skips are counted per metric, and a `--diagnostics` hint surfaces a likely field-name typo — but only when a metric recorded a value on *no* event, so a field present in some events and absent in others no longer triggers it.
    - Reusing one metric name across different track functions (e.g. `track_sum("x", …)` then `track_min("x", …)`) is now a call-time error (previously silently blended into garbage under parallel merging). In `--parallel`, a name reused across `--begin` vs the event stages is caught at merge time as a warning.
    - Float values keep their 1.x `track_bucket` labels (`200.0` → `"200"`), so JSON consumers keyed on the old names keep working.
    - `track_unique` warns once past 100,000 stored values, pointing at `track_cardinality()` for unique counts; honors `--silent`/`--no-diagnostics`.
- **Breaking: gate-vs-transform exit-code model** - Kelora now exits non-zero only when it couldn't do the job, not because the data was messy. **Gates** — parse and each individual `--filter` stage — must work: if a gate never once succeeds (no line parses, or a filter errors on *every* event it sees) the run exits `1`; a gate erroring on only *some* records is recovered (exit `0`), and filters are gated per stage so a working first filter can't mask a broken later one. **Transforms** — exec — are best-effort: a failing `--exec` rolls back to the original event and emits it, so exec errors never fail the run on their own. Structural failures and `--assert` still fail in any mode; `--strict` escalates any single error. Two changes from 1.x: (1) a `--filter` that errors on **every** event now exits `1` instead of `0` — a totally broken filter used to pass monitoring checks silently ([#241](https://github.com/dloss/kelora/issues/241)); (2) a *partial* parse failure now exits `0` instead of `1` (only all-lines-fail still exits `1`). The full model with a scenario table lives in [Error Handling](docs/concepts/error-handling.md).
- **Breaking: config files are validated strictly** - `.kelora.ini` (and `--config-file`) now reject unknown root keys, unknown sections, and malformed lines, naming the file and line (with a "did you mean" hint for case mismatches). Previously a typo such as `defualts =` or `[alias]` was silently ignored. Only `defaults` (root) and the `[aliases]` section are recognized; a rejected file exits `2` (was `1`).
- **Breaking: invalid `--input-tz` is rejected** - An unrecognized `--input-tz` (e.g. `Europe/Berln`) now fails fast with exit code `2` instead of silently falling back to local time, which could shift every timestamp. Use `local`, `UTC`, or a valid IANA timezone name.
- **Breaking: failed type annotations yield `()` instead of a string** - For `:int`/`:float`/`:bool` annotations in csv/tsv/cols/regex, a value that can't satisfy the declared type now becomes `()` (explicitly absent) in resilient mode instead of keeping the original string; the rest of the row is preserved. `--strict` still aborts. This unifies all four typed parsers (fixing a bug where `cols` ignored `--strict` for conversions). For tolerant coercion with a fallback, drop the annotation and use `to_int_or` in a script stage.
- **Breaking: ragged CSV/TSV rows are kept, and `--strict` rejects them** - Rows with more columns than the header previously lost the extras silently. Overflow columns are now kept under positional names (`c5`, `c6`, …), short rows keep trailing fields absent, and both are counted in `--stats`. `--strict` now treats a ragged row as a parse error (previously it only governed type conversion), naming where the expected width came from (`(from header)` / `(from first line)`).
- **Breaking: logfmt/CEF numeric inference no longer mangles zero-padded or signed values** - The type-inferring parsers (`logfmt`, `cef`) used to coerce any Rust-parseable number, silently rewriting data: leading zeros dropped (`zip=02134` → `2134`), a leading `+` stripped (`phone=+15551234`), and `inf`/`nan`/`Infinity` turned into floats. A value is now coerced only when it is a valid JSON number (no leading zeros, no leading `+`, no `inf`/`nan`); everything else stays a string. Genuine numbers are unaffected (`status=500`, `dur=1.5`, `n=-5`, `sci=1e3`), and the same token now resolves to the same type whether it arrives via JSON or logfmt/CEF — which matters for mixed-format cascades. csv/tsv/cols/regex are unchanged. Migration: a field that relied on the old coercion (`code=007` compared with `== 7`) now compares as a string (`== "007"`); coerce with `--exec 'e.code = to_int_or(e.code, 0)'` if needed.
- **Breaking: default-format word-wrapping is now TTY-aware** - The default output format used to wrap wide events onto continuation lines even when piped or redirected, so `wc -l`, `head`, and other line-oriented consumers over-counted. Wrapping now follows the same human-vs-machine rule as color and emoji: **auto** by default (on for a terminal, off for a pipe), so each event stays on one line downstream. `--wrap` forces wrapping through a pipe, `--no-wrap` disables it everywhere; both settable via `defaults` in `.kelora.ini`. Interactive output is unchanged.
- **Breaking: input lines are capped at 64 MiB by default (`--max-line-bytes`)** - To bound memory against a newline-free stream (see Added), a single input line may now use at most 64 MiB by default; a longer line is truncated to the cap with a warning (exit 0), or errors under `--strict` (exit 1). No real log line approaches this, so normal use is unaffected, but a workflow that genuinely processed >64 MiB single lines must raise or disable the cap: `--max-line-bytes 0` (unlimited) or a higher value like `--max-line-bytes 256MiB`.
- **Parallel mode now delivers worker tracking metadata reliably** - Worker internal tracking state (operation metadata, error counters) is now delivered exactly once per worker, per-batch user deltas are cleared after each send, and the multiline (event-batch) path attaches operation metadata like the line path. This fixes several `--parallel` inconsistencies: `track_avg`/`track_stats` finalize correctly inside `--end` (previously a raw `{sum, count}` map), the skipped-missing-value and exec-error diagnostics now appear as in sequential mode, and multiline runs merge averages correctly.
- **Faster JSON parsing** - The JSON line parser now deserializes straight into the event's field map via a custom serde visitor, skipping the intermediate `serde_json::Value` tree that dominated worker-thread CPU. Output is byte-identical; ~17–31% faster wall-clock on JSON inputs, with the largest gains on wide objects.
- **`--metrics` renders `track_freq` maps as an aligned, sorted list** - The human `--metrics` view previously dumped raw Rhai map syntax (`status = #{"500": 67, "404": 12}`). It now prints an aligned list sorted by count descending, truncated to 5 entries (with a "+N more" hint) unless `--metrics=full` or the map has ≤10 categories. Display-only: `--metrics-file` and JSON keep the full map.
- **Consistent count label across grouped metric outputs** - The human `--metrics` view now labels a `track_freq`/`--freq` table `(N items):` to match the ranked (`track_top`/`track_bottom`) and `--drain` outputs; `track_unique`'s `(N unique):` label is unchanged. Display-only.
- **`--stats`/`--metrics` format flags are more discoverable** - `--help` now notes that the format attaches with `=` (so `-s json` reads `json` as a *filename*); a failed open of a "file" named exactly `json`/`table`/`short`/`full` appends a hint pointing at the `--stats=json` form; and the hidden config-override negations (`--no-strict`, `--no-parallel`, `--no-stats`, `--no-metrics`, `--no-silent`) are now mentioned in their positive flag's help.
- **`--metrics` text view rounds floats consistently** - Float metrics in the human view are now rounded to 6 significant figures, so a `track_stats` block no longer mixes clean percentiles with noisy raw floats. Display-only: stored values and JSON keep full precision.
- **`--discover` footer expanded** - The discover table now identifies the primary timestamp field (`timestamp: ts`, `timestamp: ts (60% parsed)`, or `timestamp: when (--ts-field)` for an override), names the input parser (`format: cef (auto-detected)`, or per-format counts for cascades), and moves the scanned-event count to a footer line. `-d=json` gains matching `timestamp` and `format`/`format_counts` objects. String examples are now quoted and escaped to match `-F inspect`, and the examples column grows to the full terminal width.
- **Clearer missing-field model and hints in Rhai scripts** - A missing field is `()` and access never throws by itself; this is now documented in `--help-rhai` with the safe idioms (`e.has`/`e.get`/`??`). Referencing a field without the `e.` prefix (e.g. `status` instead of `e.status`) now suggests `e.<field>` when the bare name matches a real field, instead of offering wrong-scope variables.
- **`line`-fallback and mixed-format hints point at field extraction** - When auto-detection keeps whole lines as `line`, the hint now suggests `-f 'cols:ts(2) level *msg'` (or a `regex:`) and cascading a mixed file with repeated `-f`. When `-f auto` locks onto one format but the input is mixed, the parse-failure warning re-detects the failing line and prints a copy-pasteable cascade (`Detected mixed formats (json + line). Try: -f json,line`).
- **`--include` no longer double-executes before the first stage** - An `--include` placed before the first `--filter`/`--exec` stage was loaded into both that stage and a synthesized begin stage, running any top-level statements twice. Includes now form a begin/end script only when an explicit `--begin`/`--end` is present. Helper-function-only includes (the documented use) are unaffected.
- **Parse error summaries include filenames** - Parse error messages now show which file the error came from, making multi-file runs easier to debug.
- **Non-UTF-8 input is decoded losslessly instead of aborting the stream** - A single invalid UTF-8 byte previously tore down the whole run (rest of file silently dropped, exit `1`, no opt-out). Kelora now tolerates non-UTF-8 input the way `grep`/`ripgrep` do: invalid byte sequences are replaced with `U+FFFD` (�), so a stray Latin-1/Windows-1252 byte or embedded binary no longer truncates a multi-GB file. Recovery is visible — a diagnostic reports how many lines were affected (also in `--stats`) — and the exit code stays `0`. Clean logs are byte-for-byte unchanged. Pass `--strict-utf8` to restore the old hard-failure behavior.

### Fixed

- **`emit_each()` in `--begin`/`--end` is now a clear error instead of silently misbehaving** - `emit_each()` defers its events to a buffer drained only by the per-event loop, so `--begin`/`--end` emissions were silently dropped (under `--no-input`), interleaved as a side effect of the first event, or lost entirely (`--end`) — and absent under `--parallel`. It now raises a runtime error from `--begin`/`--end` (exit `1`), mirroring how `state`/`drain_template` reject `--parallel`; `emit_each()` in `--exec`/`--filter` is unchanged. **Breaking:** move any `--begin 'emit_each(…)'` to a per-event `--exec` stage, or pipe a counter (`seq 1 N | kelora -f line --exec 'emit_each(…)'`).
- **`-o <format>` (meant for `-F <format>`) now warns instead of silently writing a mystery file** - `kelora … -o json` silently wrote a file named `json`, printed nothing to stdout, and exited `0`. When `--output-file`'s value is a bare name (no separator, no extension) matching a known format keyword, kelora now prints a stderr warning (`did you mean -F json (--output-format)?`) while still writing the file. Honors `--silent`; real filenames like `out.json` or `/tmp/json` never trigger it.
- **Blank CSV header columns get positional `cN` names instead of an empty key (which silently dropped data)** - A blank header cell produced an empty-string key, and multiple blanks collided into one `""` field, silently dropping data (`a,,,c` over `1,2,3,4` lost the `2`). Empty keys are also unrepresentable in `logfmt`. Blank cells now take the positional `cN` name used for headerless and ragged-overflow columns, so every column stays addressable and the output round-trips. The `logfmt`/`to_logfmt()` key sanitizer also maps an empty key to `_` as defense-in-depth.
- **A single-key nested object no longer drops its key when flattened to `logfmt`/`csv`/`to_logfmt()`** - The compact flattening had a special case that, when a nested structure flattened to exactly one entry, emitted only the value and discarded the key — so `{"a":{"b":1}}` became `a=1` (losing `b`). The single-entry branch now keeps its `key=value`/`key:value` shape like the multi-entry case (`a="b=1"`), and single-element arrays keep their index (`a="0=42"`). Empty maps/arrays still render as an empty value, so no empty/`null` behavior changed.
- **`array.percentile()` accepts an integer percentile, matching its own docs** - Rhai doesn't coerce an integer literal to `f64`, so the documented `arr.percentile(95)` failed with `Function not found` — only `.percentile(95.0)` worked. An integer overload is now registered alongside the float one, so both forms work.
- **Rhai docs no longer show the unsupported `for (key, value) in map` form** - The `--help-rhai` guide, cheatsheet, and span-aggregation cookbook illustrated map iteration with `for (key, value) in map`, which Rhai doesn't support (object maps aren't directly iterable), so a new user copying it hit an immediate error. All spots now use the working idiom — `for key in e.keys() { … e[key] … }`. The cookbook's per-span CSV example is corrected to read `span.metrics["service"]` and iterate its keys.
- **Datetime values render as RFC3339 in scripts instead of an internal type name** - Interpolating a datetime in a string template (`` `${span.start}` ``), printing it, or letting it fall through `to_string()`/`to_debug()` emitted the Rust type path rather than the timestamp, because only `DurationWrapper` had registered those functions. Both are now registered for `DateTimeWrapper`, so `` `${span.start}` `` yields `2024-07-01T09:00:00+00:00`. Most visible in `--span-close` hooks; `.to_iso()`/`.format()` are unchanged.
- **Datetime/duration values stored in a field render correctly in output instead of leaking an internal type name** - The companion to the fix above: assigning a datetime/duration to an event field and emitting it (`--exec 'e.t = meta.parsed_ts'`) printed the Rust type path in `default`/`logfmt`/`json`/`csv`/`inspect`. All serialization paths now route these wrappers through their `Display`, so a datetime renders as `2026-01-02T15:04:05+00:00` and a duration as `1m 30s`, and `--discover` reports their type as `datetime`/`duration`.
- **No spurious "rerun with -m" metrics hint when `--end` consumes the metrics** - The nudge printed after a run that records `track_*` metrics without a display option also fired when an `--end` stage was present, even though `--end` is the idiomatic way to consume metrics into a report. The hint now treats an `--end` stage (alongside `-m`/`--metrics-file`) as the metrics already being handled; the bare track-and-forgot-`-m` case still gets the nudge.
- **`clamp()` with an inverted range reports an error instead of aborting the process** - Both `clamp()` overloads forwarded to `std`'s `clamp`, which **panics** when `min > max` — so `clamp(50, 100, 10)` aborted the whole run with exit `134` under `panic = "abort"`. Both now return a runtime error naming the bounds, so the run continues. In-range clamping is unchanged.
- **Dividing a `duration` by zero reports an error instead of aborting the process** - The Rhai `duration / n` operator forwarded to chrono's `Duration / i32`, which **panics** on a zero divisor (including an `i64` value that truncates to zero), aborting the run with exit `134`. The operator now rejects a zero (narrowed) divisor with a runtime error. Valid divisors are unchanged.
- **A large `--span` time duration no longer aborts the process** - A span duration that fit in `i64` milliseconds but exceeded chrono's datetime range (e.g. `--span 1000000000d`) hit an unchecked `.unwrap()` and panicked, aborting with exit `134`. The window-boundary conversion now clamps to chrono's representable min/max, so an oversized span degrades to a single all-encompassing window and exits `0`.
- **`ts_nanos()` reports an error out of range instead of silently returning `0`** - `ts_nanos()` is backed by chrono's `timestamp_nanos_opt()`, `None` outside ~`1677..2262`; the previous `.unwrap_or(0)` mapped every out-of-range timestamp (including `9999-12-31`) to the Unix epoch, silently corrupting downstream arithmetic. It now returns a runtime error naming the supported range, matching `round_to`/`ceil_to`. In-range conversions are unchanged.
- **`--stats=json` emits JSON instead of silently falling back to the table** - `--stats=json` was documented but never wired to a renderer, so it printed the human table. It now produces a JSON object whose groups mirror the table view (`format`, `lines`, `events`, `throughput`, `timestamp`, `time_span`, `levels`, `keys`, plus `ragged_rows`/`decode_warnings`/`assertion_failures`/`files` when relevant). Bare `-s` and `--stats=table` are unchanged.
- **Minute-precision timestamps (`2024-01-15 12:00`) now parse** - `--help` advertised journalctl-style `'2024-01-15 12:00'` (date plus `HH:MM`, no seconds), but the parser had no matching format, so the documented value failed with exit `2` and minute-precision *event* timestamps were silently dropped. The parser now accepts minute precision in both separators and with an optional zone marker (`2024-01-15T12:00Z`, `…+00:00`), for both bounds and event fields. Seconds-bearing forms are unaffected.
- **Missing-field typo hint now survives the implied `-m` of `--freq`/`--describe`** - The `track_*` skipped-missing-value hint — the signal that catches a typo'd field name — was hidden by the diagnostics suppression that `--freq`/`--describe`/`--metrics`/`--drain` imply, so `kelora app.log --freq stauts` reported a bare `No metrics tracked` with no clue. The hint now survives a data-only mode's *implicit* suppression (stderr, exit `0`), while an *explicit* `--no-diagnostics`/`--silent` still suppress it.
- **`--merge-sorted` on stdin no longer silently skips its validation** - Fed from stdin (no file arguments), `--merge-sorted` passed the stream straight through — disordered events came out in original order with exit `0` and the timestamp/disorder checks were dropped, voiding the flag's contract on the one input you can't pre-sort. Stdin is now rejected with an error pointing at the files form (`kelora --merge-sorted app-*.log`); file inputs are unchanged.
- **`-k/--keys` and `-K/--exclude-keys` now trim whitespace around comma-separated entries** - `-k 'ts, level, msg'` silently selected only `ts` and dropped ` level`/` msg`, because clap split on commas without trimming (`--exclude-keys ' secret '` likewise failed to drop the field). Keys now trim like `--levels` and field specs already did, and empty entries (`-k 'a,,b'`) are dropped.
- **Self-relative `now+`/`now-` time bounds now work, and space-separated negative durations parse** - Two documented forms were broken: (1) `--since now-15m` (from `--help-time`) failed because a self-relative `now±` bound standing alone never reached the anchor parser — all independent bounds now parse through `parse_anchored_timestamp`; (2) `--since -30m` was rejected as an unknown argument, forcing the `=` form — `--since`/`--until` now accept leading-hyphen values. Garbage like `--since now-bogus` still exits `2`.
- **`--discover` Seen/Miss are now per-event for array and nested fields** - A flattened array-element row (`tags[]`) counted one observation per element but divided Miss% by the *event* count, so an array present in every event read as mostly-missing and disagreed with its container row. Seen and Miss% now count the number of events that contained the path; type breakdown, Uniq, and samples stay element-scoped. `--discover=json` follows suit: `seen`/`missing` are per-event, with a new `observations` field for the raw per-element count.
- **`-k/--keys` hint guides nested paths to `get_path` instead of guessing the parent** - Pasting a nested name from `--discover` (`-k api.queries`) failed, and the "never present" hint just suggested the bare parent (`Did you mean 'api'?`). When an unseen key looks like a flattened path whose leading segment is a real field, the hint now explains that `-k`/`--exclude-keys` act on top-level fields and points at `--exec 'e.val = e.get_path("api.queries")'` (or `-k tags` for `tags[]`). `--help` documents the top-level-only behavior.
- **CSV/TSV quoted fields with embedded newlines are reassembled instead of corrupting rows** - A valid RFC 4180 record whose quoted value contained a newline was split on the physical newline before parsing, so one logical row became two — reported only as a soft "ragged rows" hint with exit `0`, and Kelora's own CSV output round-tripped into corruption. The reader now uses a quote-aware chunker for the CSV/TSV family that tracks quote parity across physical lines and hands the parser one complete record. Reassembly is sequential-only; under `-P`/`--parallel` a split record is reported as a clear `Unterminated quoted field` error pointing at sequential mode, and a new parser guard rejects any record that ends inside an open quoted field.
- **`-k/--keys` no longer falsely warns about fields created in `--exec`** - The "never present in the input" typo hint compared `-k`/`--exclude-keys` names against fields discovered in the *input*, before script stages ran, so a transform-created-then-selected field (`--exec 'e.total = …' -k total`) was flagged as missing even while printing on every line. The check now counts fields produced by scripts too (the union of input and output keys); real typos are still flagged.
- **Level hints and `--stats` no longer advertise non-level values as levels** - The level filter reads the first present field from the level-name list (`level`, `lvl`, `severity`, …) and ignores the rest, but the stats collector recorded values from *every* such field, so `level:"WARN"` + `severity:"high"` made `high` show up as a level the filter could never match. Both collectors now stop at the first present level field, exactly as the filter does.
- **Duplicate error when no input file could be opened** - When auto-detection couldn't open any input file, Kelora printed the specific per-file reason *and* a redundant generic `Pipeline error: Failed to open any input files for detection`. The generic line is now suppressed (via a typed marker error) in both sequential and parallel detection; the run still exits `1`, and genuine pipeline errors still print normally.
- **Misleading "stdin is empty" hint on unparseable input** - Piping content that produced zero events (e.g. plain text fed with `-j`) printed a "Parse errors" report *and* a contradictory "No input: stdin is empty…" nudge, because the hint's guard checked `lines_read` (only incremented under `-s/--stats`). The guard now also checks `lines_errors`, so input that arrived is no longer reported as empty. Genuinely empty input still gets the nudge.
- **`track_top`/`track_bottom`/`track_top_by`/`track_bottom_by` no longer drop heavy hitters** - All four ranking functions truncated their list to N after *every* event, so a frequent item first seen once the N slots were full re-entered at count 1 and was evicted before it could accumulate — silently returning the first N distinct items rather than the most frequent. Each now retains every distinct item (like `track_freq`) and ranks/truncates only when metrics are emitted, and the parallel merge keeps all items. Results are now exact; memory is proportional to the number of distinct items.
- **In-place `absorb_*`/`merge` mutations no longer dropped from events** - Whole-event mutating calls (`absorb_kv`/`absorb_logfmt`/`absorb_json`/`absorb_regex`/`merge`/`enrich`/`rename_field`) and in-place collection mutators on a nested field (`e.tags.push(x)`, …) were visible within the same script but silently discarded from the emitted event unless the script also had an explicit `e.field = …` assignment — breaking the documented `e.absorb_kv("msg")` workflow. Kelora now detects these mutators rooted at `e` and runs the write-back; read-only methods stay on their fast path.
- **`span.metrics` no longer silently drops non-additive aggregators** - Inside `--span-close`, `span.metrics` was computed by diffing the global tracker against the span baseline, which only works for additive aggregators — so `track_avg`/`track_percentiles` never appeared and `track_max`/`track_min` reported the global extreme, not the window's. `track_avg` now reports the true per-window average (`Δsum / Δcount`), joining `track_freq`/`track_sum`/`track_unique`. Genuinely non-additive aggregators (`track_min`/`track_max`/`track_percentiles`/`track_cardinality`/ranking) are omitted with a one-time warning per metric pointing at the `span.events` workaround (suppressed by `--no-diagnostics`/`--silent`).
- **Filter and exec error counts no longer undercounted** - A `--filter` that errored on every line reported "Filter errors: 1 total" because the error paths skipped the thread-local→context sync the success path performs. Both filter error branches now persist their counts, matching the exec path; exec error counts also now survive the stage's atomic-rollback path.
- **Spurious `conf` read-only error fixed** - Reading `conf` in an `--exec` stage and then filtering on the derived field in a separate `--filter` that doesn't name `conf` raised a false "conf map is read-only outside --begin" error. The immutability check is now gated on whether the stage actually references `conf`; genuine `conf` mutations are still rejected.
- **Script-error scope restored in `--metrics`/`--drain`** - The "affecting every event" total-failure indicator in the script-error summary is derived from event counts, which data-only modes disabled — dropping the most useful part of the summary right where a stuck user lands. The scope now surfaces in these modes; the advisory follow-up honors `--no-diagnostics` and the suppression implied by data-only modes, and is re-enabled with `--diagnostics`.
- **Parse errors no longer swallowed in `--metrics`/`--drain`** - These modes disabled stats collection, so parse failures produced no summary and exited `0`, contradicting the documented contract. Parse errors are now reported on stderr and exit `1`, matching normal mode.
- **Zero-result hint for level/time filters on the wrong input** - Running `-l/--levels` against unstructured input with no level field, or `--since/--until` against input with no parseable timestamp, silently dropped every event. Kelora now prints a hint naming the structural cause and a workaround (parse levels with `-f cols/regex` or match text with `--filter`; set `--ts-field`/`--ts-format`). A genuine value mismatch is still treated as a legitimate empty result.
- **Typo hint for `-k/--keys` and `--exclude-keys` on names never present** - Naming a field that never appears anywhere in the stream was silent: `-k timestamp` against `ts`-keyed logs emptied every event, and `--exclude-keys passwrd` quietly left scrubbed-meant data in the output. Kelora now prints a hint naming the unseen key, with a "Did you mean '<field>'?" suggestion or a list of present fields, and the exclude variant states that nothing was removed. It fires per key but only on names absent from the *entire* stream, so heterogeneous logs are never flagged. Exit `0`, honors `--silent`/`--no-diagnostics`.
- **`track_stats` metrics now usable in `--end` and `span_close`** - Percentile, average, and cardinality sketches were exposed as raw blobs, making `metrics["foo_p95"]` unusable. They are now properly finalized to scalar values.
- **`track_freq` average false positive** - A `track_freq` map whose values are literally named `sum` and `count` no longer renders as a bogus average; finalization keys off the recorded operation instead of sniffing the value's shape.
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

[Unreleased]: https://github.com/dloss/kelora/compare/v2.0.1...HEAD
[2.0.1]: https://github.com/dloss/kelora/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/dloss/kelora/compare/v1.5.0...v2.0.0
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
