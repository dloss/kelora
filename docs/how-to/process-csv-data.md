# Prepare CSV Exports for Analytics

Clean up CSV or TSV logs, enforce types, and produce analysis-ready datasets for spreadsheets, SQL engines, or BI tools.

## Use This Guide When
- Your application or appliance emits CSV/TSV logs that need quick filtering or enrichment.
- You need to normalise raw exports before loading them into DuckDB, Pandas, or spreadsheets.
- You want to ship lightweight metrics without building a full ETL job.

## Before You Start
- Examples use `examples/simple_csv.csv`. Replace paths with your real data.
- Confirm whether the file has a header row. Use `-f csvnh`/`tsvnh` for header-less data.
- Decide which columns require numeric or boolean types; type annotations enable filters and arithmetic.

## Step 1: Inspect the Source File
Glance at a handful of rows to understand column names and potential anomalies.

```bash
kelora -f csv examples/simple_csv.csv -n 5
```

- Look for blank fields, mixed types, or embedded quotes.
- Use `--stats` during early runs to catch parsing errors.

## Step 2: Enforce Column Types
Typed columns allow numerical comparisons and aggregations.

```bash
kelora -f 'csv status:int bytes:int duration_ms:int' examples/simple_csv.csv \
  --take 3
```

Alternatives:

- Embed annotations in the header row (e.g., `status:int,duration_ms:int`).
- For TSV, replace `csv` with `tsv` in the format string.

## Step 3: Clean and Transform Rows
Normalise values, derive new columns, or drop unnecessary data.

```bash
kelora -f 'csv status:int bytes:int duration_ms:int' examples/simple_csv.csv \
  -e 'e.endpoint = e.path.split("/")[1]' \
  -e 'e.duration_s = e.duration_ms / 1000.0' \
  -e 'e.success = e.status < 400' \
  -k timestamp,method,endpoint,status,duration_s,success
```

Tips:

- Use `to_int_or()` / `to_float_or()` for defensive conversions.
- Remove sensitive data with assignments like `e.user_email = ()` before export.
- Apply `strip()` when whitespace is inconsistent.

## Step 4: Summarise for Sanity Checks
Validate assumptions and produce quick metrics prior to export.

```bash
kelora -f 'csv status:int duration_ms:int' examples/simple_csv.csv \
  -e 'track_count("total")' \
  -e 'if e.status >= 500 { track_count("errors") }' \
  -e 'track_sum("latency_total_ms", e.duration_ms)' \
  -e 'track_count("latency_samples")' \
  -m \
  --end '
    if metrics.contains("latency_total_ms") && metrics["latency_samples"] != 0 {
      let avg = metrics["latency_total_ms"] / metrics["latency_samples"];
      print("avg_latency_ms=" + avg.to_string());
    }
  ' \
  --metrics
```

- Compare counts with what you expect from upstream systems.
- Divide `latency_total_ms` by `latency_samples` for averages, or compute per-service stats with additional prefixes.
- Use `track_bucket()` to build histograms or `track_unique()` to measure cardinality.

## Step 5: Export the Prepared Dataset
Write the cleaned result to CSV, JSON, or logfmt depending on downstream needs.

```bash
kelora -f 'csv status:int duration_ms:int bytes:int' examples/simple_csv.csv \
  --filter 'e.status < 500' \
  -k timestamp,method,path,status,duration_ms,bytes \
  -F csv > cleaned.csv
```

Other formats:

- `-J` or `-F json` for ingestion into document stores.
- `-F logfmt` when shipping data to systems that expect key=value lines.

## Variations
- **Files without headers**  
  ```bash
  kelora -f csvnh data.csv \
    -e 'e.timestamp = e._1; e.status = e._2.to_int(); e.bytes = e._3.to_int()' \
    -k timestamp,status,bytes
  ```

- **Large archives**  
  ```bash
  kelora -f 'csv status:int bytes:int' logs/2024-*.csv.gz \
    --parallel \
    -e 'track_sum("bytes_total", e.bytes)' \
    --metrics
  ```

- **Ragged data validation**  
  ```bash
  kelora -f csv raw.csv \
    --stats \
    --verbose \
    --filter 'meta.parse_errors > 0'
  ```
  Switch to `--strict` to abort on the first malformed row once cleanup is complete.

## Quality Checklist
- Compare row counts between raw and cleaned files; mismatches often indicate parse errors or filters that were too aggressive.
- If you drop columns, annotate the export with a README describing what was removed.
- Capture `--stats` output in automation logs so data consumers see parsing success rates.

## See Also
- [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md) for masking sensitive fields prior to export.
- [Flatten Nested JSON for Analysis](fan-out-nested-structures.md) when CSV columns contain arrays or embedded JSON.
- [Process Archives at Scale](batch-process-archives.md) for high-volume CSV crunching.
