# Triage Production Errors

Quickly focus on error-level events, capture the surrounding context, and hand off a concise summary for incident response.

## Who This Is For
- On-call engineers triaging incidents across multiple services.
- SREs verifying that a fix reduced error volume.
- Developers preparing error samples for debugging.

## Before You Start
- Kelora installed (see [Installation](../quickstart.md#installation) for download options).
- Access to the relevant log files or streams. Examples below use `examples/simple_json.jsonl`; swap in your own paths.
- Familiarity with the log format you are reading (`-j` for JSON, `-f logfmt` for logfmt, `-f combined` for web access logs, etc.).

## Step 1: Scope the Log Set
Decide which files and time ranges matter for the investigation. Use level filtering for the fastest coarse grained scan.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  --since "2024-01-15T10:00:00Z" \
  --until 30m
```

- `-l` (or `--levels`) executes as soon as it appears, so place it before heavier stages to prune work early.
- You can mix absolute ISO timestamps with relative offsets to anchor investigations (as shown above).
- Prefer explicit formats (`-j`, `-f logfmt`, `-f combined`) over `-f auto` detection to avoid surprises.
- For a directory of files, pass a glob (`logs/app/*.jsonl`) or feed a file list via `find … -print0 | xargs -0 kelora …`.

## Step 2: Narrow to Relevant Signals
Add scripted filters to isolate services, customers, or error types. Combine filter expressions with level-based filters for precision.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  --filter 'e.service == "orders" || e.message.contains("timeout")' \
  -k timestamp,service,message
```

Guidance:

- Prefer `--filter` (Rhai expression) for field checks, pattern scans, or numerical comparisons.
- Chain multiple `--filter` flags if it reads better than a long expression.
- Use safe accessors (`e.get_path("error.code", "unknown")`) when fields may be missing.

## Step 3: Pull Contextual Events
Show surrounding events to understand what happened before and after each error. Combine context flags with multiline handling when stack traces are present.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  -B 2 \
  -A 1
```

- `-B`/`-A` (short for `--before-context`/`--after-context`) mimic `grep`’s context flags. Use them sparingly to avoid flooding output.
- If the log contains multi-line traces, run the relevant strategy from [Choose a Multiline Strategy](handle-multiline-stacktraces.md) first, then apply level/context filtering.

## Step 4: Summarise Severity and Ownership
Track counts while you inspect events so you can state who is affected and how often it happens.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  -e 'track_count(e.service)' \
  -e 'track_count(e.get_path("error.code", "unknown"))' \
  --metrics \
  --stats
```

- `track_count()` tallies per-key counts; combine with `--metrics` to view the table at the end.
- `--stats` reports records processed, parse failures, and throughput so you can mention data quality in the incident summary.

## Step 5: Export Evidence
Produce a shareable artifact once you know which events matter.

```bash
kelora -j examples/simple_json.jsonl \
  -l error,critical \
  -e 'e.error_code = e.get_path("error.code", "unknown")' \
  -k timestamp,service,error_code,message \
  -F csv \
  -o errors.csv
```

Alternatives:

- `-J` (or `-F json`) for structured archives.
- `-q` or `--silent` when piping into shell scripts that only care about exit codes.

## Variations
- **Web server failures**  
  ```bash
  kelora -f combined examples/simple_combined.log \
    --filter 'e.status >= 500' \
    -k timestamp,status,request
  ```

- **Search compressed archives in parallel**  
  ```bash
  kelora -j logs/2024-04-*.jsonl.gz \
    --parallel \
    -l error,critical \
    --filter 'e.message.contains("timeout")'
  ```

- **Pivot by customer or shard**  
  ```bash
  kelora -j examples/simple_json.jsonl \
    -l error,critical \
    -e 'track_count(e.account_id)' \
    --metrics
  ```

- **Fail fast in CI**  
  ```bash
  kelora -q -j build/kelora.log -l error \
    && echo "no errors" \
    || (echo "errors detected" >&2; exit 1)
  ```

## Validate and Hand Off
- Inspect `--stats` output to ensure parse error counts are zero (exit code 1 indicates parse/runtime failures).
- Sample a few events with `--take 20` before exporting to confirm filters captured the right incidents.
- Note any masking or redaction you applied so downstream teams know whether to consult raw logs.

## See Also
- [Concept: Error Handling](../concepts/error-handling.md) for pipeline failure semantics.
- [Build a Service Health Snapshot](monitor-application-health.md) for ongoing health metrics.
- [Design Streaming Alerts](build-streaming-alerts.md) to turn this triage flow into a live alert.
- [CLI Reference: Filtering](../reference/cli-reference.md#filtering-options) for complete flag and expression details.
