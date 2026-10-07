# Big Files, Many Files

Kelora streams: it reads a line, processes it, writes the result, and moves
on, so memory use doesn't grow with file size (exceptions: [Memory](../how-it-works.md#memory)). This page covers
reading many files at once, merging them, and making big jobs faster.

## Many files

Pass several files or a glob. Kelora reads them one after another as a single
stream; `.gz` and `.zst` files are decompressed on the fly.

```bash exec="on" source="above" result="ansi"
kelora examples/merge_*.jsonl -e 'e.file = meta.filename' -k file,ts,msg -n 3
```

| Need | Option |
|---|---|
| know which file an event came from | `meta.filename` in a script |
| read files in name or modification-time order | `--file-order name`, `--file-order mtime` (default: the order given) |
| files in different formats | `-f auto-per-file` detects each file separately |
| read stdin as one of the inputs | `-` as a file name |
| more files than the shell allows | `find … -exec zcat -f {} +`, piped into a single `kelora -f FORMAT` so summaries cover everything |

Multiline events never span two files.

## Merge files by time

When each file is in time order on its own — one per host, pod, or service —
`--merge-sorted` interleaves them into one timeline:

```bash exec="on" source="above" result="ansi"
kelora --merge-sorted examples/merge_api.jsonl examples/merge_worker.jsonl -k ts,service,level,msg
```

Kelora holds one event per file and always emits the earliest, so memory
stays small and output starts immediately. The price is strictness: if a file
is out of order, or an event has no timestamp, the merge stops with an error
rather than produce a wrong timeline. Set `--ts-field` when the timestamp
field has an unusual name. `--merge-sorted` doesn't combine with `--parallel`
or `-f auto-per-file`.

## Make it faster

In rough order of effect:

1. **Drop lines before parsing.** `--keep-lines`/`--ignore-lines`, or a fast
   search tool in front (`rg -z 'checkout' *.gz | kelora …`), skip the parsing
   cost for everything you don't need.
2. **Prefer options over scripts.** `-l error` is faster than
   `--filter 'e.level == "ERROR"'`, and `--freq status` than a `track_freq`
   script. Filters built from comparisons, `&&`/`||`/`!`, and
   `contains`/`starts_with`/`ends_with` take a fast path; other function calls
   go through the script interpreter.
3. **Filter before you transform.** Put `--filter` stages before expensive
   `--exec` stages, so the expensive work runs on fewer events.
4. **Use `--parallel` (`-P`) for batch jobs.** It splits the input across CPU
   cores. Output order is preserved; `--unordered` drops that guarantee for a
   little more speed. Tune with `--threads` and `--batch-size`.
5. **Turn off bookkeeping you don't need.** `--silent` or `--no-diagnostics`
   skips per-event statistics.
6. **Try on a sample first.** `--head 10000` reads only the first 10 000 lines.

`--stats` reports the throughput of each run, so you can compare variants.

### What `--parallel` can't do {#what-parallel-cant-do}

Features that depend on event order need sequential processing. With spans,
context lines (`-A/-B/-C`), or `--window`, Kelora ignores `--parallel` with a
warning; with `--discover`, `--drain`, `--merge-sorted`, and the map formats
it refuses the combination. `state` raises a script error on every event — the run still exits 0, so check for the error summary.
`track_*()` metrics work and are merged correctly across workers.

## How fast is it?

Kelora favors flexibility over speed, and tools built for one job are faster.
Built-in options like `-l`, `--keep-lines`, and `--freq` are several times
faster than scripts that run on every event; `--parallel` gave about 3× on six
cores. Plain-text search (`rg`),
JSON reshaping (`jq`), and CSV analytics (`qsv`, `mlr`) are faster in their
niche — combine them with Kelora rather than choosing. [Benchmarks](../reference/benchmarks.md)
has the measurements.
