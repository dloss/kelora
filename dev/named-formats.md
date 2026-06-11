# Named Log Formats — State & Roadmap

Built-in named log formats live in `src/parsers/lnav_formats.rs`, adapted from
lnav's catalogue (BSD-3-Clause; see `THIRD_PARTY_LICENSES.md`). This note
records the current design and the deferred ideas so we don't lose them.

## Current state

- A `LnavFormat` = `name` + `patterns` (one or more anchored regexes, first
  match wins) + optional `ts_format` + `samples`.
- First-class: selectable via `-f <name>`, usable in cascade lists
  (`-f log4j,line`), shown by name in the auto-detect notice and `--stats`,
  listed in `--help-formats`.
- Auto-detection tries them *last*, just before the `line` fallback (first-match
  over an ordered list — no scoring), so they never reclassify a format kelora
  already detects.
- Timestamp field is captured as `ts` (house convention). `ts_format` is applied
  only when the user hasn't passed `--ts-format`, and only for shapes the
  adaptive parser can't resolve (glog, redis, apache-error). Year-less stamps
  reuse the existing current-year assumption (as syslog does).
- Shipped: `glog`, `nginx-error`, `apache-error`, `log4j`, `python-logging`,
  `redis`, `s3`, `haproxy`, `iso8601-level`.
- Naming convention + a guard test are documented on `LNAV_FORMATS`.
- Self-validation tests: every sample must parse, detect to its own format, and
  (for multi-pattern formats) collectively exercise every pattern.

## Known limitations / forcing functions

- **First-match auto-detect doesn't scale.** Fine at ~10 formats, delicate
  beyond. lnav uses a weighted scoring engine; we deliberately don't.
- **Syslog-transported formats are shadowed.** `haproxy` (and a future `sudo`)
  arrive wrapped in syslog, which the syslog detector claims first — so under
  `-f auto` they detect as `syslog`; only `-f haproxy` extracts the rich fields.
- **One `ts_format` per format.** Multi-pattern formats whose variants use
  different timestamp shapes can't be fully supported (this is why `redis`
  ships modern-3.x-only instead of also matching the year-less 2.x layout).
- **No level-value normalization.** Level matching is case-insensitive, but
  there's no synonym/abbrev map: glog `I/W/E/F`, redis `.-*#`, and numeric
  syslog severities don't satisfy `--levels INFO` etc. — only their literal token.
- **Multi-pattern is required, not optional, in some cases.** Rust's regex
  rejects duplicate capture-group names across alternation branches, so layouts
  that share field names (S3 `std`/`std-v2`, HAProxy http/tcp) *must* be separate
  patterns.

## Roadmap / ideas (roughly in priority order)

1. **`auto_detect: bool` per format.** Separate "selectable via `-f`" from
   "participates in auto-detection". Lets the catalogue grow as `-f`-only
   formats without enlarging/destabilising the auto-detect gauntlet, and is the
   natural home for syslog-shadowed formats (haproxy, sudo). This is the key
   scaling lever before adding many more formats.

2. **Scored / weighted auto-detection.** The bigger version of (1): rank
   candidates instead of first-match, so a specific format (haproxy) can win over
   a generic one (syslog) on confident matches. Cross-ref the "scored
   auto-format detection" candidate in `dev/v2-behavior-notes.md`.

3. **User-loadable format files.** The real endgame for "a lot more formats":
   let users drop in lnav-style format definitions (regex + ts + samples + level
   map) without a kelora release, including private/vendor formats. Decide on
   this *before* hand-porting a large tail of formats into Rust literals.

4. **Per-pattern `ts_format`.** Lift the one-format-one-timestamp restriction so
   multi-layout sources with differing stamps (e.g. redis 2.x vs 3.x) are fully
   covered.

5. **Optional per-format level map.** Normalise abbreviated/numeric levels
   (`I→INFO`, redis glyphs, syslog severities) so `--levels` and level-based
   output work. lnav formats already carry this metadata.

6. **More formats.** Medium tier: `uwsgi`, `postgres`, `zookeeper`, `dpkg`,
   `papertrail`; plus AWS `elb`/`alb` (niche but distinct) and `sudo`
   (security-relevant, but syslog-shadowed → wants (1)). Skip the vendor long
   tail (VMware ESX, Candlepin, Katello, OpenAM, vdsm, sssd, …) — better served
   by (3).

7. **Broaden `log4j`.** Add a second pattern for the common logback default that
   omits `[thread]`. Works today as single-pattern; this is coverage only.

## Adjacent footgun (not specific to named formats)

The adaptive timestamp list (`src/timestamp.rs`) already bakes in locale-
opinionated orderings: `%m/%d/%Y` reads `03/04` as March (US), while `%d.%m.%Y`
reads `03.04` as April (EU). They don't *collide* (separator-gated), but the
silent US/EU assumption is a latent footgun worth a doc note or config knob
someday. This is also why glog's MMDD stays scoped to the glog parser rather than
going into the global adaptive list.
