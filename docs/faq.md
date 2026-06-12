# Frequently Asked Questions

Quick answers to common questions, with pointers to deeper docs.

## When should I use Kelora instead of grep, awk, or jq?

Use Kelora when logs are messy, you need stateful transforms, or you want to combine parsing, filtering, and analysis in one streaming pipeline. For simple text search use `grep`, and for pure JSON querying use `jq`. The [Quickstart](quickstart.md) shows where Kelora shines, and [When to Use Kelora vs External Tools](how-to/integrate-external-tools.md#when-to-use-kelora-vs-external-tools) goes deeper.

## How do I choose the right input format?

Start with auto-detection (`-f auto`, the default) when the whole stream uses one format. If you are processing many files and each file is internally consistent but formats differ between files, use `-f auto-per-file`. If detection disagrees, force a format like `-j` for JSON, `-f logfmt`, or `-f line`. For streams that genuinely mix multiple formats line by line (e.g. JSON logs with plain-text panics/stack traces), use **cascade mode**: `-f json,line` tries each parser in order and tags events with `_format`. See [Cascade Mode](reference/formats.md#cascade-mode) for details.

## How do I parse a custom format?

Use `-f 'cols:...'` for fixed columns or `-f 'regex:...'` for pattern-based parsing, then iterate with `-F inspect`. The [Parsing Custom Formats](tutorials/parsing-custom-formats.md) tutorial walks through both approaches.

## Why am I getting no output?

Typical causes are filtering everything out, quiet/silent output flags, or time filters that exclude your data. The [Common Errors Reference](reference/common-errors.md) has a quick checklist.

## Why does my filter not see fields I just created?

Stages run in the **exact CLI order** you specify. Create fields before filtering on them, and place `--levels` where you want level filtering to occur. See [Scripting Stages](concepts/scripting-stages.md) for examples.

## How do I filter by severity, like "WARN and above"?

`-l/--levels` matches an explicit **set** of level names — it has no built-in notion of severity ranking, so it cannot express an ordering like "WARN or worse." There are three ways to get the effect you want:

**List the levels you want to keep** with `-l/--levels`:

```bash
kelora -j --levels warn,error,critical,fatal app.log
```

**Exclude the levels below your cutoff** with `-L/--exclude-levels` — often simpler, since the lower levels are usually fewer:

```bash
kelora -j --exclude-levels trace,debug,info app.log
```

**Use a real threshold** by mapping levels to numbers in a Rhai filter, when you want one cutoff to cover many level names:

```bash
kelora -j --filter '#{"trace":0, "debug":1, "info":2, "warn":3, "error":4, "fatal":5}.get(e.level.to_lower(), -1) >= 3' app.log
```

The `-l`/`-L` flags are the faster choice; reach for the Rhai filter only when you need a numeric threshold. See the [CLI Reference](reference/cli-reference.md) for level-filtering details.

## How do I debug filters or Rhai scripts?

Use `-F inspect` to see fields and types, `--verbose` to surface errors, and `--strict` to fail fast. The [Common Errors Reference](reference/common-errors.md) and [Functions Reference](reference/functions.md) cover patterns for safe access and type conversion.

## How do I filter by time or control timezones?

Use `--since/--until` for time ranges, `--ts-format` when timestamps are custom, and `--input-tz` if logs lack timezone info. See the [Time Reference](reference/time-reference.md) for full syntax.

## How do I handle multiline logs or stack traces?

Enable multiline with `-M` and pick a strategy. See [Multiline Strategies](concepts/multiline-strategies.md) and run `kelora --help-multiline` for detailed options.

## Can Kelora read compressed files or archives?

Gzip files (`.gz`) are handled automatically. For archives and batch processing patterns, see [Process Archives at Scale](how-to/batch-process-archives.md).

## I fed Kelora JSON — why isn't the output JSON?

By default Kelora reformats every event into a readable, colored `key=value` view, regardless of the input format. To keep JSON, use `-J` (or `-F json`); for other formats use `-F logfmt`, `-F csv`, `-F tsv`, etc. See [Output Formats](reference/formats.md) and the [CLI Reference](reference/cli-reference.md).

In a terminal, wide events wrap onto indented continuation lines. When the output is piped or redirected, wrapping is off by default, so each event stays on one line and `wc -l` counts events correctly. Use `--wrap` to force wrapping through a pipe, or `--no-wrap` to disable it everywhere.

## How do I control output, stats, and diagnostics?

Use `-F` to pick an output format, `-q/--quiet` to suppress events, and `-s/--stats` or `-m/--metrics` for summaries. For zero terminal output with metrics files still written, use `--silent`. See the [CLI Reference](reference/cli-reference.md).

## How do I make Kelora faster on large files?

Prefer native flags like `--levels` over Rhai filters, prune with `--keep-lines` early, and use `--parallel` for big files. Disable diagnostics with `--silent` or `--no-diagnostics` when you only need output files. See the [Performance Model](concepts/performance-model.md).

## Can I disable emoji output or colors?

Yes. Use `--no-emoji` to switch to plain text output and `--no-color` (or the `NO_COLOR` environment variable) to disable colors. See the [CLI Reference](reference/cli-reference.md).

## Was Kelora built with AI?

Yes. Kelora is an experiment in agentic AI development: AI agents generate the implementation and tests, and human oversight focuses on requirements and validation. See the [Development Approach](index.md#development-approach) and the [Security Policy](https://github.com/dloss/kelora/blob/main/SECURITY.md) before production use.

## Does Kelora phone home or send telemetry?

No. Kelora is a local-only tool and does not include any built-in networking or telemetry features. The repository also includes `just check-no-networking`, a small CI-enforced check that Kelora stays free of common networking and telemetry dependencies.

## Why does Kelora have so much code for a CLI tool?

Kelora is not a tiny CLI. It combines multiple parsers, multiline handling, time parsing, streaming state, parallel execution, output formatting, and an embedded Rhai runtime with a large built-in function set. A substantial part of the repository is also tests, examples, and documentation for real-world log edge cases. The codebase is large because the feature surface is large, not because it is padded with filler.

## How does configuration precedence work?

CLI flags override `.kelora.ini`, which overrides `~/.config/kelora/kelora.ini`, which overrides defaults. The [Configuration System](concepts/configuration-system.md) explains precedence and aliases.

## Is there an interactive mode for tricky shell quoting?

Yes. Run `kelora` with no arguments to enter the REPL. It supports history, glob expansion, and built-in `:help`. See [Quickstart](quickstart.md).

## Where is the full CLI and function reference?

Docs live in the [CLI Reference](reference/cli-reference.md) and [Functions Reference](reference/functions.md). On the command line, use `kelora --help` and `kelora --help-functions` for the same information.

## What exit codes does Kelora use?

See the [Exit Codes Reference](reference/exit-codes.md) for the full list and automation tips.
