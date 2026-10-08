# Output and Integration

Kelora follows the Unix rule of silence: the result you asked for goes to
stdout, everything else (errors, warnings, hints) goes to stderr. The result
is the events, or the summary when you ask for one with `-s`, `-m`, `--freq`
or `--drain`. A successful run prints nothing but its data, which makes Kelora
safe to pipe into other tools and to use in scripts and CI.

## Output formats

```bash exec="on" source="above" result="ansi"
kelora examples/simple_json.jsonl -l error -k timestamp,service,message -J
```

| Format | Option | Use for |
|---|---|---|
| `key='value'`, colored | (default) | reading in a terminal |
| JSON Lines | `-J` or `-F json` | `jq`, DuckDB, log shippers, re-reading with Kelora |
| logfmt | `-F logfmt` | compact text that tools can still parse |
| CSV / TSV with header | `-F csv`, `-F tsv` | spreadsheets, `qsv`, `mlr`, databases |
| CSV / TSV without header | `-F csvnh`, `-F tsvnh` | appending to existing files, `sort`, `awk` |
| field types | `-F inspect` | debugging scripts |
| timelines | `-F levelmap`, `-F keymap`, `-F tailmap` | spotting bursts ([Summarize](summarize.md#see-it-over-time)) |

For CSV and TSV, pick the columns with `-k`. A nested value (map or array) is
squeezed into a single cell, so pull out what you need first
(`e.user_id = e.get_path("user.id")`). The default format
prints nested values inline; `--expand-nested` spreads them over indented
lines.

`-o FILE` writes events to a file instead of stdout. `--metrics-file FILE`
writes tracked metrics as JSON, and `--stats=json` prints the run statistics
as JSON.

## Kelora in a pipeline

Upstream, anything that writes lines works, including live streams:

```bash
tail -F /var/log/app.log | kelora -j -l error,warn
kubectl logs -f deploy/api | kelora -f json,line -l error -C 2
journalctl -u nginx -o json -f | kelora -j -k __REALTIME_TIMESTAMP,MESSAGE
docker compose logs --no-color | kelora --extract-prefix service -f json,line
```

On stdin, format detection uses the first line only, so pass `-f` for mixed
streams. Multiline events are flushed after 400 ms without input, so the last
stack trace of a quiet stream still appears.

For very large files, a fast line search in front can do most of the
filtering before Kelora parses anything:

```bash
rg -zI 'checkout' huge-*.log.gz | kelora -j --filter 'e.duration_ms > 1000'
```

(`-I` stops `rg` from prefixing each line with its file name, which would break
the JSON.)

Downstream:

| Tool | Hand off with |
|---|---|
| `jq` | `-J` |
| DuckDB, SQLite | `-J` or `-F csv` |
| `qsv`, `mlr`, spreadsheets | `-F csv -k …` |
| `sort`, `awk`, `cut` | `-F tsvnh`, or a summary piped (one TSV row per value) |
| a time-series tool | `--span-summary=tsv` |

```bash
kelora app.jsonl -l error -J | jq -s 'group_by(.service) | map({service: .[0].service, n: length})'
kelora app.jsonl -F csv -k ts,level,msg > logs.csv && duckdb -c "select level, count(*) from 'logs.csv' group by 1"
kelora access.log --freq ip | head
```

Kelora also reads its own JSON output, so you can split work into steps:
`kelora raw.log -f 'cols:…' -J > clean.jsonl`, then explore `clean.jsonl`
without re-parsing.

## Exit codes in scripts and CI

Kelora exits 0 when the run did its job — even if some lines didn't parse —
and 1 when it couldn't ([all codes](../reference/exit-codes.md)). Matching
nothing is not an error, so the exit code doesn't tell you whether events
matched. To make a CI step fail when a log contains errors, use `--assert`:

```bash exec="on" source="above" result="ansi" returncode="1"
kelora examples/ci_pipeline.log -q --assert 'e.level != "ERROR"'
```

`--strict` turns every parse or script error into exit 1, so malformed input
can't pass silently.

## Less noise

| Option | Hides |
|---|---|
| `-q` | the events (keeps stats, metrics, diagnostics) |
| `--no-hints` | suggestions (💡) |
| `--no-warnings` | warnings about problems that didn't stop the run (🔸) |
| `--no-diagnostics` | both of the above |
| `--silent` | all of Kelora's own terminal output except fatal errors |
| `--no-color`, `--no-emoji` | colors, emoji prefixes (also: `NO_COLOR=1`) |

Script output (`print()` in Rhai) is shown by default and hidden in summary
modes like `-m`, `-s`, or `--drain`; `--script-output` forces it on. The
environment variables `KELORA_NO_HINTS` and `KELORA_NO_WARNINGS` set the
defaults, and so can your [configuration file](config.md).
