# Choose a Multiline Strategy

Group multi-line events (stack traces, continuation lines, JSON blocks) into single records before parsing or filtering.

## When to Use This
- Application logs include stack traces or exceptions that span multiple lines.
- Text logs hold multi-line JSON payloads or request/response pairs.
- You need grep-like context without losing event boundaries.

## Decision Quick Reference
- **Timestamp at the start of every event?** Use `--multiline timestamp`.
- **Continuation lines start with whitespace?** Use `--multiline indent`.
- **Events start with specific keywords?** Use `--multiline 'regex:match=^PATTERN'`.
- **Events have explicit end markers?** Use `--multiline 'regex:match=^BEGIN:end=^END'`.
- **Single event per file?** Use `--multiline all` (rare; loads full file in memory).

## Step 1: Inspect the Raw Log
Confirm line prefixes and indentation before choosing a strategy.

```bash
head -n 20 examples/multiline_stacktrace.log
```

Look for:

- Consistent timestamps (e.g., `2024-05-03 12:10:45 ERROR …`).
- Indentation on continuation lines.
- Unique keywords or start/end markers.

## Step 2: Timestamp-Based Grouping
Best for logs where each event starts with a timestamp.

```bash
kelora examples/multiline_stacktrace.log \
  --multiline timestamp \
  -n 2
```

Customization:

- Specify a format if auto-detect fails: `--multiline 'timestamp:format=%Y-%m-%d %H:%M:%S'`.
- Combine with parsing flags (e.g., `-f logfmt`) after multiline grouping.

## Step 3: Indentation-Based Grouping
Ideal for Python/Java stack traces and YAML-style continuations.

```bash
kelora examples/multiline_indent.log \
  --multiline indent \
  -n 2
```

- Lines beginning with whitespace attach to the previous event.
- Works even when timestamps are missing.

## Step 4: Regex-Based Boundaries
Use custom patterns when timestamps or indentation are unreliable.

```bash
kelora app.log \
  --multiline 'regex:match=^(ERROR|Exception|Traceback)' \
  -n 2
```

Variants:

- Start and end markers: `--multiline 'regex:match=^BEGIN:end=^END'`.
- Negated starts: treat lines that **do not** match as continuations with `--multiline 'regex:match=^[^\\s]'`.

## Step 5: Buffer Entire Inputs (Last Resort)
`--multiline all` reads the full file into one event. Use sparingly for small JSON documents or configuration files.

```bash
kelora config.json --multiline all -J
```

- Avoid on large files; it consumes memory proportional to file size.
- Combine with `--filter` or `--end` logic to summarise the whole document.

## Validate the Result
- Run `-n 3` or `--take 3` to verify event boundaries before adding filters.
- Use `--stats` to confirm Kelora parsed the expected number of events.
- When debugging, add `--debug-multiline` to see how patterns match (see `kelora --help-multiline`).

## Variations
- **Chain with format parsing**  
  ```bash
  kelora --multiline timestamp \
    -f logfmt app.log \
    -l error
  ```

- **Extract stack trace metadata**
  ```bash
  kelora --multiline timestamp --multiline-join=newline app.log \
    --filter 'e.line.contains("Traceback")' \
    -e 'e.frames = e.line.split("\\n").len()' \
    -k timestamp,frames,line
  ```

  **Note:** Use `--multiline-join=newline` to preserve line breaks in the grouped stack trace. The default `--multiline-join=space` joins lines with spaces, which would prevent counting frames via `split("\\n")`.

- **Combine with context flags**  
  After grouping, use `--before-context` / `--after-context` to include neighbouring events.

## See Also
- [Concept: Multiline Strategies](../concepts/multiline-strategies.md) for deeper explanations and debugging tips.
- [Triage Production Errors](find-errors-in-logs.md) to apply the results during incident response.
- `kelora --help-multiline` for full syntax and additional examples.
