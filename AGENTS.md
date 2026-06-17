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
just docs-serve         # Serve with auto-reload
just docs-build         # Build locally
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

**Add Rhai Function:** Implement in `src/rhai_functions/`, register in `mod.rs`, **update `docs.rs`**, write tests

**Performance Work:** Run `just bench` before/after, compare, use `just bench-update` if improved

## Quick Reference

**Test quickly:** `time ./target/release/kelora -f json logfile.json --filter "e.level == 'ERROR'" > /dev/null`

**Output model — rule of silence:** a successful run prints only its data (events on stdout); everything else is stderr. kelora speaks only to (a) report an **error** (⚠️, shown unless `--silent`), (b) flag a non-fatal **warning** (🔸, `--warnings/--no-warnings`), or (c) drop a **hint** at a likely mistake (💡, `--hints/--no-hints`). `--diagnostics/--no-diagnostics` is a shortcut for both advisory tiers. A fourth channel, **status** (🔹 "what kelora did" — detected format, loaded config, applied defaults/aliases), is silent on success and shown only under `-v`/`--verbose` — no exception for an explicit `--config-file`. Warnings/hints are anomaly-triggered and reach redirected stderr (CI); the generic "0 of N matched" nag was dropped — zero-match hints fire only on a concrete footgun. Canonical doc: `docs/concepts/error-handling.md` ("What kelora prints"). Env vars `KELORA_NO_WARNINGS` / `KELORA_NO_HINTS` mirror the negative flags (precedence: explicit flag > env > config default). `--silent` (suppress terminal output except fatal line; metrics files still write), `--script-output/--no-script-output` (control Rhai print/eprint; suppression implied by --silent, -m, -s), `-m`, `-s`. Data-only modes (`-m`/`--drain`/`--discover`) hush hints but still surface warnings to stderr. Positive flags override env/config defaults.

**Config precedence:** CLI args > `.kelora.ini` (project) > `~/.config/kelora/kelora.ini` (user) > defaults
