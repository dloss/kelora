# AGENTS.md

Kelora is a Rust-based command-line log analysis tool using the Rhai scripting engine. This guide provides essentials for AI agents working on the codebase.

## Documentation - Don't Duplicate, Reference!

**First, check these sources instead of guessing:**
- **README.md** - User overview, quick start, CLI feature tour
- **examples/README.md** - 60+ example files with usage patterns
- **Built-in help** - Run `./target/release/kelora --help-*` for detailed references:
  - `-h` - Quick reference (one-screen cheat sheet)
  - `--help` - Full CLI reference (all 100+ options)
  - `--help-rhai` - Rhai scripting guide
  - `--help-functions` - All 150+ built-in functions
  - `--help-examples` - Common patterns
  - `--help-time` - Timestamp formats
  - `--help-multiline` - Multiline strategies
  - `--help-regex` - Regex parsing guide

## Essential Commands (Using Just)

```bash
# Quality checks before commit (REQUIRED)
just fmt                # Format code
just lint               # Run clippy
just test               # All tests

# Additional checks
just check              # fmt + lint + audit + deny + test
just audit              # Security audit
just deny               # License/dependency policy

# Benchmarking (for performance changes)
just bench-quick        # Quick benchmarks
just bench              # Full suite
just bench-update       # Update baseline

# Documentation
just docs-check         # Fast strict build (debug binary): broken links or failing examples fail
just docs-serve         # Serve with auto-reload
just docs-build         # Strict build with the release binary
```

## Code Quality Rules (REQUIRED Before Commit)

1. **Always run `just fmt`** (or `cargo fmt --all`)
2. **Always run `just lint`** (or `cargo clippy --all-targets --all-features -- -D warnings`)
3. **Run tests** with `just test`
4. **For performance changes**: Run `just bench` to check for regressions

## Key Development Conventions

**Builds:** Use dev builds (`cargo build`, `./target/debug/kelora`) during normal development. Release builds are heavily optimized and slow to compile — use `--release` / `./target/release/kelora` only for performance work (benchmarking, profiling).

**Architecture:** Streaming pipeline: Input → Parsing → Processing (Rhai) → Output

**Fuzzing (manual only):**
- Harnesses live in `fuzz/`. Install [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) and run `just fuzz-json` (wraps `cargo +nightly fuzz run json_parser`) to stress the JSON pipeline.
- Seeds in `fuzz/corpus/json_parser/` keep coverage grounded; extend them when triaging crashes.
- Leave fuzzing out of CI—run it ad-hoc during development.

**Adding Rhai Functions:**
- Implement in `src/rhai_functions/`
- **ALWAYS update `src/rhai_functions/docs.rs`** for `--help-functions`
- **AND add a `####` entry to `docs/reference/functions.md`** (`tests/docs_sync_tests.rs` fails otherwise)
- Remember: Rhai allows method-style calls on first argument

**Emoji Output:**
- 🔹 (blue diamond) for general output
- ⚠️ (warning) for errors
- Support `--no-emoji` flag

**Exit Codes:**
- 0: Success
- 1: Parse/runtime errors
- 2: Invalid CLI usage
- 134: SIGABRT — internal thread panic (a bug). The release profile uses `panic = "abort"`, so an unexpected panic in a reader/worker/sink thread aborts the process (134) rather than unwinding to exit 1. These paths already terminated the run before; only the code changed.

**Stability:** Prefer avoiding breaking changes; document and justify any that are necessary.

## Writing the User Docs (`docs/`)

The site (kelora.dev) is built with MkDocs from `docs/`. Its structure is settled — **extend it, don't reorganize it**:

| Where | What belongs there |
|---|---|
| `guide/*.md` | one page per job (explore, parse, filter, scripting, summarize, time, spans, state, output, files, config); ordered loosely along the pipeline: read → select lines → group → parse → time range → stages → summarize → present |
| `cookbook/*.md` | short task recipes, one page per topic (the overview lists them automatically) ("Which services fail most?"), each a runnable command plus one link into the guide |
| `how-it-works.md` | the pipeline model, processing order, error model, what Kelora prints |
| Help (nav) | `reference/troubleshooting.md` (by symptom) and `faq.md` |
| `reference/` | lookup material; `cli-reference.md` is **generated** from `kelora --help` (edit `src/cli.rs`, not the page) |

Rules:

- **One home per topic.** Explain a feature once, on its page; elsewhere link to it.
- **No template sections**: no "What You'll Learn", "Prerequisites", "Overview", "Summary", "Next Steps", "See Also", "Best Practices", time estimates. Start with a one- or two-sentence lede, then the content.
- **Show, then explain.** Lead with a command and its output; tables over prose for options and choices.
- **Every example runs.** Use ` ```bash exec="on" source="above" result="ansi" ` with fixtures from `examples/` (add a small fixture if none fits; list it in `examples/README.md`). One command per block (a before/after pair is fine). Expected non-zero exits need `returncode="N"`. Unexecuted blocks only for live streams, external tools, or placeholders — keep those few and test them by hand. The output must actually demonstrate the point.
- Blocks run in a pseudo-terminal (`dev/docs_tty_hook.py`), so output looks like an interactive terminal (tables, wrapping at 80 columns, legends).
- **Current idioms only**: `meta.parsed_ts` (not `to_datetime(e.timestamp)`), `--freq`/`--describe`, `--span-summary`, `strip()`/`replace_regex()` (built-in `trim()`/`replace()` return `()`). History and rationale go to `CHANGELOG.md`, not the docs.
- **Plain, dense prose.** No marketing words ("powerful", "seamless", "comprehensive"), no filler. Admins skim: put the answer first.
- Run `just docs-check` before committing doc changes.

**Front pages — settled; don't redesign them without a reason from real readers:**

- `docs/index.md` *explains* (for someone who already came to kelora.dev):
  lede, the `shop.log` investigation, "In short", topic table. No showpiece,
  "Advanced features", or highlight sections — tried twice (Nov 2025, Jun 2026)
  and removed after first-impression tests with fresh readers. A change to its
  structure needs a new first-impression test.
- `README.md` *sells* (for someone browsing GitHub or crates.io): same lede;
  the `shop.log` story continued by at most two showcase outputs (levelmap,
  `--drain-diff`); a short one-line list of further options; install; doc
  links; agent skill; how it's built; license. Absolute URLs only (crates.io).
  Sample output is checked by `dev/readme_check.py` (part of `just docs-check`
  and CI); after an output change, run `just readme-update` and review the diff.

## Project Structure

```
src/
├── main.rs              # CLI entry point
├── config/              # Configuration system
├── formats/             # Format parsers
├── processing/          # Pipeline stages
├── rhai_functions/      # Rhai functions (update docs.rs!)
└── output/              # Output formatters
tests/
examples/                # Usage examples
benchmarks/              # Performance tests
Justfile                 # Build automation
```

## Common Tasks

**Add Format Parser:** Create in `src/formats/`, add to `mod.rs`, update auto-detection, write tests

**Add Rhai Function:** Implement in `src/rhai_functions/`, register in `mod.rs`, **update `docs.rs` and `docs/reference/functions.md`**, write tests

**Performance Work:** Run `just bench` before/after, compare, use `just bench-update` if improved

## Quick Reference

**Test quickly:** `time ./target/release/kelora -f json logfile.json --filter "e.level == 'ERROR'" > /dev/null`

**Output model — rule of silence:** a successful run prints only its data (events on stdout); everything else is stderr. kelora speaks only to (a) report an **error** (⚠️, shown unless `--silent`), (b) flag a non-fatal **warning** (🔸, `--warnings/--no-warnings`), or (c) drop a **hint** at a likely mistake (💡, `--hints/--no-hints`). `--diagnostics/--no-diagnostics` is a shortcut for both advisory tiers. A fourth channel, **status** (🔹 "what kelora did" — detected format, loaded config, applied defaults/aliases), is silent on success and shown only under `-v`/`--verbose` — no exception for an explicit `--config-file`. Warnings/hints are anomaly-triggered and reach redirected stderr (CI); the generic "0 of N matched" nag was dropped — zero-match hints fire only on a concrete footgun. Canonical doc: `docs/how-it-works.md` ("What Kelora prints"). Env vars `KELORA_NO_WARNINGS` / `KELORA_NO_HINTS` mirror the negative flags (precedence: explicit flag > env > config default). `--silent` (suppress terminal output except fatal line; metrics files still write), `--script-output/--no-script-output` (control Rhai print/eprint; suppression implied by --silent and the data-only modes, but an explicit `--script-output` wins), `-m`, `-s`. Data-only modes (`-s`/`-m`/`--freq`/`--describe`/`--card`/`--drain`/`--discover`) hush hints and script output but still surface warnings to stderr — except the "No input format detected" fallback hint, which survives the `--discover` hush (it's the actionable half of that mode's `format: line (auto-detected)` footer) and is emitted at most once per run. Positive flags override env/config defaults.

**Config precedence:** CLI args > `.kelora.ini` (project) > `~/.config/kelora/kelora.ini` (user) > defaults
