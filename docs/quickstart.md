# Quickstart

Get Kelora running in 5 minutes with three commands.

## Installation

[**Download**](https://github.com/dloss/kelora/releases) the latest release from [GitHub](https://github.com/dloss/kelora), extract it and put it on your PATH. Or install via Cargo:

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

Filter to show only ERROR level events and display statistics. The stats show processing metrics: 11 lines parsed, 4 errors output (7 filtered out), time span covered, and which levels were present in the log.

### 3. Extract hidden data

```bash exec="on" source="above" result="ansi"
kelora examples/quickstart.log -f 'cols:ts(3) level *msg' -l error \
  -e 'e.absorb_kv("msg")' --normalize-ts -J
```

Extract key-value pairs from error messages into structured JSON fields. Notice how `order=1234`, `gateway=stripe`, `user=admin`, and other embedded data are now proper JSON fields. The `--normalize-ts` flag also converts the syslog timestamp (`Jan 15 10:00:00`) into full ISO 8601 format, ready for analysis or ingestion into other tools.

## Get Help

```bash
kelora --help              # Complete CLI reference
kelora --help-examples     # More usage patterns
kelora --help-rhai         # Rhai scripting guide
kelora --help-functions    # All built-in Rhai functions
kelora --help-time         # Timestamp format reference
```

## Next Steps

You've seen Kelora in action. Now **learn how it actually works**:

### Recommended Learning Path

Follow this sequence to build your Kelora skills systematically:

1. **[Tutorial: Basics](tutorials/basics.md)** (30 min) - Master input formats (`-f`, `-j`), display options (`-k`, `-b`, `-c`), level filtering (`-l`, `-L`), and output formats (`-F`, `-J`). Learn what events are and how to work with them.

2. **[Tutorial: Introduction to Rhai](tutorials/intro-to-rhai.md)** (20 min) - Learn to write filter expressions and simple transforms. Understand how to access event fields, use conditionals, and convert types safely.

3. **[Tutorial: Working with Time](tutorials/working-with-time.md)** (15 min) - Handle timestamps, filter by time ranges, and work with timezones.

4. **[Tutorial: Advanced Scripting](tutorials/advanced-scripting.md)** (30 min) - Master complex transformations, window operations, and advanced patterns.

### Jump to Solutions

Already know what you need? Check the **[How-To Guides](how-to/find-errors-in-logs.md)** for specific solutions like triaging errors, parsing custom formats, and tracking metrics.

### Reference

- **[Glossary](glossary.md)** - Definitions of terms like "event", "field", "span", and "stage"
- **[Functions Reference](reference/functions.md)** - Complete catalog of 40+ built-in functions
- **[CLI Reference](reference/cli-reference.md)** - All command-line flags and options
