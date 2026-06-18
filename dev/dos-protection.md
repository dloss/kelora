# Resource Limits & Capability Gating for kelora

Status: design / evaluation. Defaults unchanged until implemented.

## Framing: where the real risk is

kelora is a local log-analysis CLI that aims for parity with `jq`/`awk`/`python`.
For the dominant use case — you running your own scripts over your own logs —
"DoS protection" is protection against your own typos, which `Ctrl-C` already
handles (the engine cooperatively checks `SHOULD_TERMINATE` in `on_progress`).

Resource limits only earn their keep in two situations:

1. **Untrusted input** — kelora ingests arbitrary/attacker-controlled log files.
2. **Unattended execution** — CI/cron, or a service that accepts user-supplied
   Rhai, where a runaway job must fail instead of hang or OOM the host.

The realistic DoS surface for a log tool is **(1), the input pipeline** — not the
Rhai script. An earlier draft of this spec only constrained the Rhai *script*
(`max_string`/`max_array`/`max_map`). Those limits do nothing for the input
pipeline: an oversized line is allocated in the reader **before any Rhai code
runs**, so by the time a filter sees `e.message` the giant `String` is already
resident. This spec leads with the input side.

## Threat inventory (current behavior)

| Vector | Current state | Verdict |
|---|---|---|
| **42.zip** (recursive ZIP bomb) | ZIP rejected outright (`decompression.rs`) | Non-issue |
| **gzip bomb** | Streamed; DEFLATE capped at 1032:1 per member | Low risk (see below) |
| **zstd bomb** | Streamed, but zstd ratios are effectively unbounded | Bounded by streaming + line cap |
| **Huge line** (no `\n`) | `read_until(b'\n', …)` in `read_line_lossy`, **no cap** | **Real OOM — unmitigated** |
| **Multiline accumulation** | Logical event accumulates physical lines, no ceiling | Same class as huge line |
| **Rhai FS writes** | Gated behind `--allow-fs-writes` (default off) | OK |
| **Rhai FS reads** (`read_file`, `read_lines`) | **Ungated** | Capability gap |
| **Rhai env** (`get_env`) | **Ungated** | Capability gap |
| **Runaway Rhai** (CPU/depth) | Only `Ctrl-C` | Opt-in limits, low priority |

### Why streaming defuses most "bomb" framing

Decompression (`decompression.rs`) and line reading (`readers.rs`) are both
**streaming**: a `.gz`/`.zst` that expands to many GB across many lines is
processed line-by-line in roughly constant memory — that is just "a big log,"
and capping total size would break parity with `zcat | awk`. So:

- **Total decompressed size** → no cap needed; streaming handles it.
- **Compression ratio** → not a *memory* issue; at most CPU/work amplification.
- **Per-line bytes** → the one buffer that grows without bound (no `\n`), and the
  only number that actually pins RAM.

The dangerous combination is **bomb + no newline**: a few-KB file that
decompresses to one enormous newline-free "line." Streaming does not help,
because `read_until` grows a single buffer until `\n` (never) or OOM. The
per-line cap closes this case for every format at once.

## Two tiers (do not conflate)

|  | **Circuit breaker** | **Policy limit** |
|---|---|---|
| Purpose | turn OOM/crash into a clean error | restrict untrusted input/scripts |
| Default | **on** | **off** (opt-in) |
| Value | very high — ~zero false positives | low — tuned to context |
| Example | `--max-line-bytes 64MiB` | `--max-line-bytes 1MiB`, timeout, max-ops |

Only the per-line circuit breaker has a defensible "on by default" value.
Everything else defaults **off**: a legitimate batch job over a huge file *does*
run for minutes and *does* execute billions of ops, so any default timeout or
op-budget is a parity-breaking footgun. Those knobs ship as documented
*recommendations* for untrusted contexts, not as defaults.

## Defaults, derived (not arbitrary)

### `--max-line-bytes` (circuit breaker, on by default: 64 MiB)

Anchor to real data. Largest *legitimate* single lines observed in practice:

| Source | Typical line |
|---|---|
| syslog / app logs | 100 B – 2 KB (RFC 5424 historically 480–2048 B) |
| Docker json-file / K8s CRI | 16 KB (they *split* longer lines) |
| JSON logs w/ embedded stack trace or base64 blob | 10 KB – low single-digit MB |

Derivation:

> default = (largest plausible legit line ≈ a few MB) × generous headroom,
> bounded so `N_workers × cap` stays ≈ 1 GiB.

→ **64 MiB**: ~20–1000× above any real line (≈ zero false positives), and even at
16 parallel workers the worst case is ~1 GiB transient — and only the *offending*
stream approaches the cap; normal lines stay tiny.

- Untrusted-input policy value: **1 MiB**.
- Optional RAM-relative form: `max(64 MiB, RAM/64)` for tiny-RAM hosts. Prefer a
  fixed, memorable value for reproducibility (tests/CI).
- On overflow: error in `--strict`; otherwise truncate the line and warn (🔸).
- Apply the same ceiling to multiline accumulation (max bytes per logical event).

### Compression ratio guard (off; likely skip)

Mostly unnecessary given streaming + the line cap:

- **gzip/DEFLATE** is mathematically capped at **1032:1** per member — a 1 KB `.gz`
  expands to ≤ ~1 MB, so a *tiny* gzip bomb is not possible.
- **zstd** is the only real outlier (RLE + large windows → very high ratios), so a
  guard, if added, should be **zstd-scoped**.

If implemented anyway: trip only after a meaningful absolute output (e.g. >100 MB)
**and** ratio > 1000:1 — this clears even highly repetitive real logs while
catching bombs. Recommend deferring until there is a concrete need.

### Rhai script limits (off; opt-in, low priority)

These protect the *script* surface (untrusted scripts / unattended runs), not the
input pipeline. Default off to preserve batch-job parity. Recommended values for
untrusted-script contexts only:

| Knob | Recommended (untrusted) | Rhai API |
|---|---|---|
| Wall-clock timeout | 30–60 s (anchor: "interactive query") | `on_progress` callback |
| Max operations | 10⁸–10⁹ | `set_max_operations` |
| Max call depth | 128 | `set_max_call_levels` |
| Max string / array / map | as needed | `set_max_string_size` / `set_max_array_size` / `set_max_map_size` |

Note: Rhai has no single total-memory cap; strict total caps need OS controls
(`ulimit`, cgroups).

### Capability gating (close the read/env gap)

`read_file`, `read_lines`, and `get_env` are currently ungated while
`--allow-fs-writes` already gates writes. Rather than introduce a second
capability axis (`--sandbox` + `--allow-rhai-io`) and force users to reason about
three overlapping flags, **generalize the existing gate**: extend
`--allow-fs-writes` to also cover Rhai reads/env (or rename to a single
`--allow-rhai-io` covering read+write+env). Default remains: writes blocked,
reads/env blocked — opt in explicitly. OS permissions still apply on top.

## CLI surface

```
--max-line-bytes <size>     Per-line buffer cap (default 64MiB; circuit breaker)
--allow-rhai-io             Allow Rhai filesystem read/write + env (default off)

# Opt-in, untrusted-context knobs (default off):
--script-timeout <dur>      Wall-clock per Rhai engine
--max-ops <n>               Rhai operation budget
```

Config (`.kelora.ini`) mirrors these under `[script]` / `[input]`; precedence
follows the project default: CLI > project config > user config > defaults.

## Behavior on limit hit

- Fail fast with a guard-specific diagnostic naming the tripped limit
  (e.g. "Line exceeds --max-line-bytes (64 MiB)"); honor `--no-emoji`.
- Exit code 1 for hard errors; truncate-and-warn path stays exit 0.
- Avoid flushing partial output beyond what already streamed.

## Implementation notes

- **Per-line cap**: bound `read_until` in `read_line_lossy` (`readers.rs`) via a
  `Read::take`-style limit or a manual capped loop. Single chokepoint → covers
  plain, gzip, and zstd inputs uniformly, before any value reaches Rhai.
- **Script limits**: applied when constructing each `RhaiEngine` and cloned into
  worker engines (`engine/mod.rs`); reuse the existing `on_progress` hook for the
  timeout.
- **Capability gate**: thread the existing `RuntimeConfig`/`allow_fs_writes`
  plumbing (`rhai_functions/file_ops.rs`, `pipeline/`) to also cover `conf.rs`
  reads and `environment.rs`.

## Testing

- Newline-free input (plain and as a gzip/zstd payload) hits `--max-line-bytes`
  with a clean error / truncation, not OOM.
- Large *multi-line* `.gz`/`.zst` streams in bounded memory (no false trip).
- Multiline accumulation respects the per-event ceiling.
- Normal examples pass unchanged with defaults (only the 64 MiB breaker active).
- Untrusted-context: `--script-timeout` / `--max-ops` trip on infinite loops;
  off by default lets long batch jobs run.
- `--allow-rhai-io` toggles read/write/env access; default denies.

## Rollout

- Defaults unchanged except the 64 MiB per-line circuit breaker (designed for
  ~zero false positives).
- Document in CHANGELOG and `--help-rhai`; clarify the two tiers (always-on
  breaker vs opt-in untrusted-context limits) and that 42.zip/ZIP is already
  rejected.
