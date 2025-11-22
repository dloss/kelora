# Multiline Strategies

Kelora can treat clusters of lines as a single event so stack traces, YAML
payloads, JSON blobs, and other multi-line records stay intact. This page
explains how multiline detection fits into the pipeline and how to pick the
right strategy for your data set.

## Why Multiline Matters

- Application errors often spill over multiple lines (Java stack traces, Python
  tracebacks, Go panics).

- Structured payloads such as JSON, YAML, or CEF frequently span multiple lines
  when logged with indentation.

- Batch systems may wrap related log entries between explicit boundary markers
  like `BEGIN`/`END`.

Without multiline detection, Kelora parses each physical line as its own event,
making it hard to correlate context.

## How Multiline Processing Works

1. **Pre-parse stage** – Multiline runs before the input parser. The chunker
   groups input lines into blocks according to the configured strategy.

2. **Parsing** – The aggregated block is fed into the selected parser (`-f`).
   Use `-f raw` when you want to keep the block exactly as-is, including
   newlines.

3. **Downstream pipeline** – Filters, exec scripts, and formatters see the
   aggregated event exactly once.

Multiline increases per-event memory usage. When processing large files, keep an
eye on chunk size via `--stats` and consider tuning `--batch-size`/`--batch-timeout`
when using `--parallel`.

## Built-in Strategies

Kelora ships four strategies. Only one can be active at a time.

### 1. Timestamp Headers (`--multiline timestamp`)

Best for logs where each entry begins with a timestamp. Detection uses Kelora's
adaptive timestamp parser; you can hint a specific format with
`timestamp:format=<chrono>`.

=== "Command"

    ```bash
    kelora -f raw examples/multiline_stacktrace.log \
      --multiline timestamp \
      --filter 'e.raw.contains("Traceback")' \
      -F json --take 1
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f raw examples/multiline_stacktrace.log \
      --multiline timestamp \
      --filter 'e.raw.contains("Traceback")' \
      -F json --take 1
    ```

The event now contains the full Python traceback until the next timestamped
header. Pair this strategy with `--ts-format` if you also need chronological
filtering later in the pipeline.

### 2. Indentation Continuations (`--multiline indent`)

Combine lines that start with leading whitespace. This matches Java stack traces
and similar outputs where continuation lines are indented.

=== "Command"

    ```bash
    kelora -f raw examples/multiline_stacktrace.log \
      --multiline indent \
      --filter 'e.raw.contains("SQLException")' \
      -F json --take 1
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f raw examples/multiline_stacktrace.log \
      --multiline indent \
      --filter 'e.raw.contains("SQLException")' \
      -F json --take 1
    ```

In this example the stack trace block remains an atomic event. If the first line
of a block is not indented (for example, `Traceback ...`), combine strategies by
preferring `timestamp` or switching to `regex` (see below) so the header line is
included.

### 3. Regex Boundaries (`--multiline regex:match=...[:end=...]`)

Define explicit start and optional end markers. This is ideal for logs that wrap
records with guard strings such as `BEGIN`/`END` or XML tags.

=== "Command"

    ```bash
    kelora -f raw examples/multiline_boundary.log \
      --multiline 'regex:match=^BEGIN:end=^END' \
      --filter 'e.raw.contains("database_backup")' \
      -F json --take 1
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -f raw examples/multiline_boundary.log \
      --multiline 'regex:match=^BEGIN:end=^END' \
      --filter 'e.raw.contains("database_backup")' \
      -F json --take 1
    ```

If no `end=` is provided, a new `match=` line flushes the previous block. Regex
patterns are Rust regular expressions—the same engine used by `--filter`.

### 4. Treat Everything as One Event (`--multiline all`)

This strategy buffers the entire stream and emits it as a single event. Useful
for one-off conversions (for example piping a whole JSON array into a script).
Use with care: the entire input must fit in memory.

```bash
kelora -f raw big.json --multiline all --exec 'print(e.raw.len())'
```

## Choosing the Right Parser

- **`-f raw`** preserves newlines (`\n`) so you can post-process blocks with
  `split("\n")`, regex extractions, or write them to disk unchanged.

- **Structured parsers** (`-f json`, `-f logfmt`, `-f cols:...`) expect a single
  logical record. Use multiline to restore that logical record before parsing.

- After parsing, you can still keep the original text by copying the raw block
  into another field inside an exec script.

## Observability and Debugging

- Run with `--stats` or `--stats-only` to see how many events were emitted after
  chunking. A sudden drop or spike indicates the strategy might be too broad or
  too narrow.

- Use `--take` while experimenting so you do not print massive aggregates to the
  terminal.

- Inspect the aggregated text with `-f raw -F json` during tuning to confirm the
  block boundaries look correct.

## Advanced Tips

- **Custom timestamp formats**: `--multiline 'timestamp:format=%d/%b/%Y:%H:%M:%S %z'`
  mirrors Apache/Nginx access log headers.

- **Prefix extraction**: When container runtimes prepend metadata, run
  `--extract-prefix` *before* multiline so the separator line is preserved.

- **Parallel mode**: With `--parallel`, tune `--batch-size` and
  `--batch-timeout` if you have extremely large blocks to prevent workers from
  buffering too much at once.

- **Fallback for JSON/YAML**: Complex nested documents may require `regex`
  boundaries or pre-processing (for example, `jq`) because closing braces often
  return to column zero, breaking the `indent` heuristic.

## Troubleshooting

- **Strategy misfires**: If you see every line printed individually, your start
  detector did not trigger. Try `--multiline regex` with an explicit pattern, or
  switch to `timestamp` with a format hint.

- **Truncated blocks**: For JSON or YAML, remember that closing braces/brackets
  often start at column zero. Use regex boundaries that match `^}` or `^\]` to
  keep the termination line.

- **Out-of-memory risk**: `--multiline all` and poorly tuned regex patterns can
  accumulate the entire file. Run on a sample first, or set `--take`/`--stats`
  to monitor chunk counts.

- **Context flags**: `-A/-B/-C` require a sliding window. If you combine context
  with multiline, increase `--window` so the context has enough buffered events.

## Related Reading

- [Pipeline Model](pipeline-model.md) – see where multiline sits relative to
  parsing and transformation.

- [Reference: CLI Options](../reference/cli-reference.md#input-options) – full
  flag syntax for `--multiline`, `--extract-prefix`, and timestamp controls.

- [Tutorial: Parsing Custom Formats](../tutorials/parsing-custom-formats.md) –
  practical recipes that often start with multiline normalization.
