# Quickstart

Get Kelora running in 5 minutes with three commands.

## Installation

**macOS (Homebrew):**
```bash
brew tap dloss/kelora
brew install kelora
```

**Other platforms:** see [Installation](installation.md) for pre-built binaries (Linux, Windows, ARM, …), or install via Cargo:

```bash
cargo install kelora
```


## Three Essential Commands

Here's a typical log file with unstructured text and key-value pairs buried in the messages:

```bash exec="on" result="ansi"
cat examples/quickstart.log
```

### 1. Parse with Kelora

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg'
```

Kelora parses the custom format into structured fields. The format spec `cols:ts(3) level *msg` tells Kelora that each line has a 3-token timestamp, followed by a level field, and then the rest is the message. Notice how timestamps are formatted, levels are color-coded, and messages are cleanly separated.

### 2. Filter and analyze

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error --stats
```

Filter to ERROR level events and display statistics instead of the events (`--stats` shows only the stats block). The stats show processing metrics: 11 lines parsed, 4 errors output (7 filtered out), time span covered, and which levels and keys were present in both the input and output (when they differ due to filtering or transformations).

### 3. Extract hidden data

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error \
  -e 'e.absorb_kv("msg")' --normalize-ts -J
```

Extract key-value pairs from error messages into structured JSON fields. Notice how `order=1234`, `gateway=stripe`, `user=admin`, and other embedded data are now proper JSON fields. The `--normalize-ts` flag also converts the syslog timestamp (`Jan 15 10:00:00`) into full ISO 8601 format, ready for analysis or ingestion into other tools.

## Interactive Mode

Run `kelora` without any arguments to enter **interactive mode**. This is useful when shell quoting is annoying, especially on Windows. It supports history, glob expansion, and built-in `:help`.

```bash
kelora
```

Try one of these:

- `-j examples/audit.jsonl --stats`
- `examples/quickstart.log -f 'cols:ts(3) level *msg' --take 5`

## Why Did I Get No Output?

Most first-run problems come from one of these cases:

- **Wrong parser**: Try `-j`, `-f logfmt`, `-f combined`, or `-f 'cols:...'` explicitly instead of relying on auto-detection.
- **Wrong field name**: Run `--discover` first to inspect parsed fields and sample values. If you're debugging a filtered or transformed pipeline, use `--discover-final` instead.
- **Filter too strict**: Remove filters and re-add them one at a time.
- **Mixed-format file**: If your file mixes JSON lines and plain text, use cascade mode, for example `-f json,line`.
- **Nothing matches your level/timestamp assumptions**: Prefer explicit filters like `e.level == "ERROR"` and confirm timestamp field names with `--stats`.

When in doubt, start here:

```bash
kelora your.log --discover
kelora your.log --discover-final --filter 'e.level == "ERROR"'
kelora your.log --stats
kelora your.log --take 5
```

## Get Help

```bash
kelora --help              # Complete CLI reference
kelora --help-examples     # More usage patterns
kelora --help-rhai         # Rhai scripting guide
kelora --help-functions    # All built-in Rhai functions (add a KEYWORD to search, e.g. --help-functions ip)
kelora --help-time         # Timestamp format reference
```

**Having trouble?** See **[Debug Issues Systematically](how-to/debug-issues.md)** or the **[Common Errors Reference](reference/common-errors.md)**.

## Next Steps

You've seen Kelora in action. Next, learn how it works:

### Recommended Learning Path

Follow this sequence to build your Kelora skills systematically:

1. **[Tutorial: Basics](tutorials/basics.md)** (30 min) - Master input formats (`-f`, `-j`), field discovery (`--discover`), display options (`-k`, `-b`, `-c`), level filtering (`-l`, `-L`), and output formats (`-F`, `-J`). Learn what events are and how to work with them.

2. **[Tutorial: Introduction to Rhai](tutorials/intro-to-rhai.md)** (20 min) - Learn to write filter expressions and simple transforms. Understand how to access event fields, use conditionals, and convert types safely.

3. **[Tutorial: Working with Time](tutorials/working-with-time.md)** (15 min) - Handle timestamps, filter by time ranges, and work with timezones.

4. **[Tutorial: Advanced Scripting](tutorials/advanced-scripting.md)** (30 min) - Master complex transformations, window operations, and advanced patterns.

### Jump to Solutions

Already know what you need? Check the **[How-To Guides](how-to/index.md)** for specific solutions like triaging errors, parsing custom formats, and tracking metrics.

### Reference

- **[Glossary](glossary.md)** - Definitions of terms like "event", "field", "span", and "stage"
- **[Functions Reference](reference/functions.md)** - Complete catalog of 150+ built-in functions
- **[CLI Reference](reference/cli-reference.md)** - All command-line flags and options
