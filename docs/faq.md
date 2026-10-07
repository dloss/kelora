# FAQ

## When should I use Kelora instead of grep, jq, or awk?

When the job involves structure: parsing a format, filtering on fields,
counting, comparing events, or several of those in one pass. For finding a
string, `grep`/`rg` are simpler and faster; for reshaping JSON documents, `jq`
is more expressive. Kelora is the middle ground between "grep is enough" and
"I need a log platform", and it composes with both — see
[Output and Integration](guide/output.md#kelora-in-a-pipeline).

## Does Kelora store, index, or follow logs?

No. It reads input once, streams it, and exits. To follow a growing file, pipe
it in: `tail -F app.log | kelora -j -l error`. For an interactive, indexed log
viewer, use a tool like lnav alongside Kelora.

## How do I filter "WARN and above"?

`-l` takes a set of level names, not a threshold. List the levels you want
(`-l warn,error,critical,fatal`) or exclude the ones you don't
(`-L trace,debug,info`). For a numeric cutoff across many level names, map
levels to numbers:

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl --freq level --filter '(#{"trace": 0, "debug": 1, "info": 2, "warn": 3, "error": 4, "critical": 5, "fatal": 5}.get(e.level.to_lower()) ?? -1) >= 3'
```

## I fed Kelora JSON. Why isn't the output JSON?

The default output is a readable `key='value'` view for every input format.
Add `-J` for JSON Lines, or `-F csv`, `-F logfmt`, and so on.

## Which compressed files can it read?

gzip (`.gz`) and zstd (`.zst`), detected by content and decompressed on the
fly. For `.zip` or `.tar`, extract first or pipe: `unzip -p logs.zip | kelora …`.

## Does it work on Windows?

Yes. Shell quoting is the hard part: run `kelora` with no arguments to get an
interactive prompt with history and normal quoting.

## Why Rhai?

Rhai is a small scripting language embedded in Rust: safe (scripts can't touch
files or the network unless you allow it), fast to start, and familiar to
anyone who has written JavaScript or Rust. Kelora's built-in functions cover
most log work, so scripts stay short. See
[Transform with Scripts](guide/scripting.md).

## Does Kelora phone home?

No. Kelora has no networking code and sends no telemetry; a CI check
(`just check-no-networking`) keeps it that way.

## Was Kelora built with AI?

Yes. Kelora is an experiment in agentic development: AI agents write the
implementation and the tests, while the maintainer sets requirements and
validates behavior. An extensive test suite, `cargo audit`, and `cargo deny`
run on every change. Read the
[security policy](https://github.com/dloss/kelora/blob/main/SECURITY.md)
before using Kelora on sensitive data.

## Why is there so much code for a CLI tool?

Kelora bundles many parsers, multiline handling, time parsing, an embedded
scripting runtime with 150+ functions, streaming aggregation, parallel
processing, and several output formats — plus tests for a long tail of
real-world log quirks. The size follows the feature set.

## Where do I report bugs or ask questions?

[GitHub issues](https://github.com/dloss/kelora/issues). Include
`kelora --version`, the command, and a few sample lines. Kelora is a
spare-time project; support is best-effort.
