# Integrate Kelora with External Tools

Kelora thrives in Unix pipelines—combine it with specialized tools to filter, transform, store, visualize, and explore log data.

## When This Guide Helps

- You want to pre-filter logs before Kelora parses them.
- Kelora's output needs further analysis (SQL, advanced JSON queries, statistical aggregation).
- You need interactive exploration, visualization, or long-term storage.
- Your workflow requires format conversion or integration with existing tooling.

## Before You Start

- Kelora installed (see [Installation](../quickstart.md#installation)).
- Basic familiarity with Kelora's input formats (`-j`, `-f logfmt`, `-f csv`, etc.) and output formats (`-F json`, `-F csv`, `-F tsv`).
- Access to the external tools you want to use (installation instructions vary by tool).

## Quick Reference

### Upstream Tools (Input Side)

Tools that feed data **into** Kelora:

| Tool | Category | Use For |
|------|----------|---------|
| **grep/ripgrep** | Pre-filter | Fast text filtering before parsing |
| **tail/journalctl** | Streaming | Live log monitoring |
| **kubectl** | Container logs | Stream Kubernetes pod logs |
| **docker/docker compose** | Container logs | Stream Docker container logs |
| **find + xargs** | Discovery | Locate log archives for batch processing |
| **jc** | Converter | Convert command output to JSON for Kelora |

### Downstream Tools (Output Side)

Tools that process data **from** Kelora:

| Tool | Category | Use For | Kelora Output |
|------|----------|---------|---------------|
| **jq** | JSON query | Advanced JSON transformations | `-J` |
| **SQLite/DuckDB** | Database | SQL queries, aggregations, storage | `-F csv`, `-J` |
| **pirkle** | PRQL query | SQL-style queries with pipeline syntax | `-F csv` |
| **sort** | Sorting | Order by timestamp, level, or other fields | `-F tsv`, `-F csv` |
| **column** | Formatter | Pretty-print TSV as aligned tables | `-F tsv` |
| **jtbl** | Formatter | Pretty-print JSON as aligned tables | `-J` |
| **miller** | Data processing | Multi-format analytics and reshaping | `-F csv`, `-J` |
| **qsv** | CSV analytics | Statistical analysis, joins, validation | `-F csv` |
| **lnav** | Interactive viewer | Explore and search logs interactively | `-J` |
| **visidata** | Spreadsheet | Terminal-based data analysis | `-F csv`, `-F tsv` |
| **ov** | Pager | Feature-rich viewing and search | (any) |
| **csvlens** | CSV viewer | Interactive CSV exploration | `-F csv` |
| **rare** | Visualization | Real-time histograms and charts | `-J`, `-F tsv` |
| **angle-grinder** | Alternative processor | Different query syntax | `-J` |

### Shippers (Alongside Kelora)

Tools that handle **collection/transport**; Kelora runs before or after them.

| Tool | Role | Use For |
|------|------|---------|
| **Fluentd / Fluent Bit / Vector** | Collection & delivery | Reliable shipping with buffering/backpressure; feed them normalized JSON/logfmt from Kelora, or replay their output into Kelora for ad-hoc analysis (one format per stream) |

---

## Log Shippers (Fluentd, Fluent Bit, Vector)

Kelora sits next to shippers—it transforms, the shipper handles collection, buffering, retries, and delivery.

### Pre-process before shipping

Clean up messy logs, extract embedded JSON/logfmt, and emit newline-delimited JSON or logfmt for fast ingestion.

```bash
tail -F /var/log/app.log | \
  kelora -f logfmt --exec 'e.absorb_kv("msg")' -J \
  > /tmp/app.normalized.jsonl
# Point Fluent Bit tail (or Fluentd tail/exec) at /tmp/app.normalized.jsonl
```

### Post-process after shipping

Replay files or stdout streams produced by the shipper into Kelora for ad-hoc filtering, aggregation, or enrichment without touching the forwarding pipeline.

**Tips:** Ensure each input stream uses a single format (mixed formats need explicit branching in `--exec`), prefer `-J` or `-F logfmt --no-emoji` for shipper-friendly output, and wrap Kelora with a supervisor if you need it to stay running.

---

## Upstream Tools (Pre-Processing)

### grep / ripgrep — Fast Pre-Filtering

Search for text patterns before parsing to reduce Kelora's workload. Grep can be faster than parsing every line when you only need a small subset. Use when scanning huge files for specific keywords.

```bash
# Pre-filter for "ERROR" lines before parsing
grep -i "ERROR" /var/log/app.log | kelora -f logfmt

# Faster with ripgrep, case-insensitive, show context
rg -i "timeout" /var/log/app.log -A 2 | kelora -j

# Alternative: Use Kelora's built-in line filtering (faster parsing)
kelora -f logfmt --keep-lines 'ERROR|WARN' /var/log/app.log

# Combine with time-based filtering
grep "ERROR" /var/log/app.log | kelora -j --since '1 hour ago'

# Extract multi-line stack traces after grep
grep -A 20 "Exception" /var/log/app.log | kelora -f line -M 'regex:match=^[A-Z]'
```

**When to use grep vs Kelora:**

- Use `grep` for raw text scanning across massive unstructured files
- Use `--keep-lines`/`--ignore-lines` when you want Kelora to track parse errors
- Use `--filter` for structured field checks after parsing (e.g., `e.status >= 500`)
- Use `--since`/`--until` for time-based filtering (no grep needed)

---

### find + xargs — Archive Discovery

Locate log files across directories and process them in parallel. Use when you have scattered archives that need systematic batch processing. The `find` command provides powerful file selection (by date, size, name), and `-print0 | xargs -0` handles paths with spaces safely.

```bash
# Find all compressed JSON logs from April 2024 - maximum throughput
find /archives -name "app-2024-04-*.jsonl.gz" -print0 | \
  xargs -0 kelora -j --parallel --unordered -l error --stats

# Recursive search for all .log files modified in last 7 days
find /var/log -name "*.log" -mtime -7 -print0 | \
  xargs -0 kelora -f auto --filter 'e.level == "ERROR"' -J

# Alternative: Use --since instead of find -mtime for time filtering
find /var/log -name "*.log" -print0 | \
  xargs -0 kelora -f auto --since '7 days ago' -l error -J

# Parallel file processing with xargs AND parallel Kelora processing
find /archives -name "*.jsonl.gz" -print0 | \
  xargs -0 -P 4 -n 1 kelora -j --parallel --unordered \
    -e 'track_count(e.service)' -m
```

See [Process Archives at Scale](batch-process-archives.md) for parallel processing strategies.

---

### tail / journalctl — Live Streaming

Stream live log data into Kelora for real-time monitoring and alerting during deployments or incidents. Use `tail -F` (capital F) to survive log rotation. Combine with Kelora's `-q` to suppress event output while keeping diagnostics and metrics visible.

```bash
# Monitor live logs for critical errors
tail -F /var/log/app.log | kelora -j -l critical -q

# Stream systemd journal logs for a specific service
journalctl -u myapp.service -f --output=json | \
  kelora -j --filter 'e.priority <= 3' -F json
```

See [Design Streaming Alerts](build-streaming-alerts.md) for complete alert workflows.

---

### kubectl — Kubernetes Pod Logs

Stream logs from Kubernetes pods into Kelora for real-time monitoring and analysis. Essential for debugging containerized applications and tracking issues across pod restarts. Use `-f` for live streaming, `--tail` to limit history, and `--all-containers` for multi-container pods.

```bash
# Stream logs from a specific pod with context around errors
kubectl logs -f pod-name | kelora -j -l error,warn -C 2

# Follow logs from last hour only
kubectl logs -f pod-name --since=1h | kelora -j --filter 'e.level == "ERROR"' -J

# Stream logs from previous container with spike detection
kubectl logs -f pod-name --previous | \
  kelora -j --window 10 \
    -e 'e.recent_errors = window.pluck("level").filter(|x| x == "ERROR").len()' \
    --filter 'e.recent_errors > 5'  # Alert on error spikes

# Multi-pod log aggregation with pod name extraction
kubectl logs -f -l app=myapp --prefix=true | \
  kelora -f auto --extract-prefix pod --prefix-sep ' ' -J

# Separate logs by container using section selection
kubectl logs -f pod-name --all-containers=true | \
  kelora --section-from '^\[container-name\]' --section-before '^\[' -j
```

See [Design Streaming Alerts](build-streaming-alerts.md) for alerting patterns that work well with kubectl streaming.

---

### docker / docker compose — Container Logs

Stream Docker container logs into Kelora for local development and single-host deployments. Use `docker logs -f` or `docker compose logs -f` for live streaming. Combine `--tail` to limit history and `--timestamps` for temporal analysis.

```bash
# Stream logs from a single container with error context
docker logs -f container-name | kelora -j -l error,critical -A 3

# Extract specific service logs from compose output
docker compose logs -f | \
  kelora --section-from '^web_1' --section-before '^(db_1|api_1)' -f auto

# Stream with timestamps and time-based filtering
docker logs -f --timestamps myapp --since 10m | \
  kelora -f auto --since '5 minutes ago' -e 'track_count(e.level)' -m

# Multiple containers with slow request detection
docker compose logs -f api worker | \
  kelora -j --filter 'e.response_time.to_float() > 1.0' \
    -k timestamp,service,response_time,path

# Aggregate metrics per 1-minute windows
docker logs -f --timestamps myapp | \
  kelora -j --span 1m --span-close 'track_count("span_events")' \
    -e 'track_count(e.level)' -m
```

---

### jc — Command Output Converter

Converts output from 100+ CLI tools (ls, ps, netstat, etc.) into JSON for Kelora to parse. Turns unstructured command output into queryable structured data. Pairs well with Kelora's parsing functions for deeper analysis.

```bash
# Parse directory listings as structured data
jc ls -la | kelora -j --filter 'e.size > 1000000' -k filename,size

# Analyze running processes with aggregation
jc ps aux | kelora -j --filter 'e.pcpu.to_float() > 50.0' \
  -e 'track_sum(e.user, e.pcpu.to_float())' -m

# Extract and parse URLs from command output
jc curl -I https://example.com | kelora -j \
  -e 'e.url_parts = e.location.parse_url()' \
  -e 'e.domain = e.location.extract_domain()' -J

# Parse user-agent strings from web server logs
kelora -f combined access.log \
  -e 'e.ua = e.user_agent.parse_user_agent()' \
  -e 'track_count("browser|" + e.ua.browser)' -m
```

**Kelora parsing functions that work well with jc output:**

- `parse_url()` - Extract URL components
- `parse_user_agent()` - Parse browser/OS from user-agent strings
- `parse_email()` - Extract email parts
- `parse_path()` - Parse filesystem paths
- `extract_domain()` - Extract domains from URLs
- `extract_email()` / `extract_emails()` - Extract email addresses from text
- `extract_ip()` / `extract_ips()` - Extract IP addresses
- `extract_url()` - Extract URLs from text

[jc documentation](https://kellyjonbrazil.github.io/jc/)

---

## Downstream Tools (Post-Processing)

### jq — Advanced JSON Manipulation

Powerful JSON querying, reshaping, and computation beyond Kelora's built-in capabilities. Use for complex JSON restructuring, recursive descent, or computations that are awkward in Rhai. Kelora focuses on log-specific operations; jq excels at generic JSON transformation.

**When to use Kelora vs jq:**

- Use Kelora for filtering, aggregation, and log-specific operations
- Use jq for complex JSON reshaping, recursive queries, and output formatting
- Many common jq tasks can be done natively in Kelora (see examples below)

```bash
# This jq pattern...
kelora -j app.jsonl -l error -J | \
  jq 'group_by(.service) | map({service: .[0].service, count: length})'

# ...can often be done with Kelora's tracking functions:
kelora -j app.jsonl -l error -e 'track_count(e.service)' -m

# When jq is better: Complex nested field extraction
kelora -j logs/app.jsonl --filter 'e.status >= 500' -J | \
  jq -r '.request.headers | to_entries[] | "\(.key): \(.value)"'

# Kelora alternative: Use get_path() and flatten()
kelora -j logs/app.jsonl --filter 'e.status >= 500' \
  -e 'e.headers_flat = e.get_path("request.headers", #{}).flatten(".", "dot")' -J

# Best of both: Kelora parses/filters/extracts, jq reshapes complex output
kelora -j logs/app.jsonl \
  -e 'e.url_parts = e.request_url.parse_url()' \
  --filter 'e.url_parts.path.starts_with("/api")' -J | \
  jq 'group_by(.url_parts.host) | map({host: .[0].url_parts.host, requests: length})'
```

[jq manual](https://jqlang.github.io/jq/)

---

### qsv — CSV Analytics Powerhouse

High-performance CSV toolkit with statistics, joins, validation, SQL queries, and more. Adds SQL-like operations, statistical analysis, and data validation to CSV workflows. Use when Kelora's CSV output needs further analysis, validation, or joining with other datasets.

```bash
# Kelora pre-processes, qsv generates detailed statistics
kelora -j logs/app.jsonl -k timestamp,response_time,status -F csv | \
  qsv stats --select response_time

# Frequency analysis - compare with Kelora's track_count()
kelora -j logs/app.jsonl -l error -k error_code,service -F csv | \
  qsv frequency --select error_code | \
  qsv sort --select count --reverse

# Alternative: Use Kelora's tracking for simple counts
kelora -j logs/app.jsonl -l error -e 'track_count(e.error_code)' -m

# Join Kelora output with reference data (qsv excels here)
kelora -j logs/orders.jsonl -k order_id,user_id,amount -F csv > orders.csv
qsv join user_id orders.csv user_id users.csv | qsv select 'user_id,name,order_id,amount'

# Kelora can pre-aggregate before export for faster qsv processing
kelora -j logs/app.jsonl --span 5m --span-close \
  'print(span.id + "," + span.size.to_string())' -q > summary.csv
qsv stats summary.csv
```

[qsv documentation](https://github.com/jqnatividad/qsv)

---

### miller — Multi-Format Data Processing

Like awk/sed/cut for name-indexed data (CSV, TSV, JSON). Supports aggregations, joins, and format conversion. Bridges formats and adds SQL-style aggregations with simple syntax.

```bash
# Kelora to JSON, miller calculates aggregates
kelora -j logs/app.jsonl -k service,response_time -J | \
  mlr --ijson --opprint stats1 -a mean,p50,p99 -f response_time -g service

# Convert Kelora CSV to pretty-printed table with computed columns
kelora -j logs/app.jsonl -k timestamp,bytes_sent,bytes_received -F csv | \
  mlr --csv --opprint put '$total_bytes = $bytes_sent + $bytes_received'

# JSON to TSV conversion with field selection
kelora -j logs/app.jsonl -J | \
  mlr --ijson --otsv cut -f timestamp,level,message
```

[miller documentation](https://miller.readthedocs.io/)

---

### sort — Column-Based Sorting

Sorts Kelora's TSV/CSV output by columns. Use `sort` when output order matters for chronological analysis, top-N queries, and ordered exports. Use `-t$'\t'` for tab delimiter. Column numbers are 1-indexed.

**Kelora alternatives to consider first:**

- Use `--since`/`--until` for time-based filtering (often eliminates need for sorting)
- Use `sorted()` or `sorted_by()` Rhai functions for array sorting within events
- Use `--take N` to limit output (though not sorted)

```bash
# Sort errors by timestamp
kelora -j logs/app.jsonl -l error -k timestamp,service,message -F tsv | \
  sort -t$'\t' -k1,1

# Sort by response time (numeric, descending) - top 20 slowest
kelora -j logs/app.jsonl -k timestamp,response_time,path -F tsv | \
  sort -t$'\t' -k2,2nr | head -20

# Alternative: Track top values with Kelora
kelora -j logs/app.jsonl -e 'track_max("slowest|" + e.path, e.response_time)' -m

# Multi-column sort: service (ascending), then timestamp (descending)
kelora -j logs/app.jsonl -F tsv | \
  sort -t$'\t' -k2,2 -k1,1r

# Sort array values within Rhai
kelora -j logs/app.jsonl \
  -e 'e.sorted_tags = e.tags.sorted()' \
  -e 'e.users_by_age = e.users.sorted_by("age")' -J
```

---

### SQLite / DuckDB — SQL Analytics

Load Kelora output into a SQL database for complex queries, aggregations, and long-term storage. Use when you need JOINs, window functions, complex GROUP BY, or persistent storage. SQL provides rich aggregation, time-series analysis, and joins that complement Kelora's streaming model.

**SQLite example:**

```bash
# Export to CSV, import to SQLite
kelora -j logs/app.jsonl -k timestamp,level,service,message -F csv > logs.csv

sqlite3 logs.db <<EOF
.mode csv
.import logs.csv logs
CREATE INDEX idx_level ON logs(level);
SELECT service, COUNT(*) as error_count
FROM logs
WHERE level = 'ERROR'
GROUP BY service
ORDER BY error_count DESC;
EOF
```

**DuckDB example (faster, better CSV/JSON support):**

```bash
# Query Kelora JSON output directly with DuckDB
kelora -j logs/app.jsonl -J > logs.json

duckdb -c "
  SELECT service,
         COUNT(*) as total,
         AVG(response_time) as avg_response_time
  FROM read_json_auto('logs.json')
  WHERE status >= 500
  GROUP BY service
  ORDER BY total DESC;
"

# Or import CSV for repeated queries
kelora -j logs/app.jsonl -F csv | \
  duckdb -c "CREATE TABLE logs AS SELECT * FROM read_csv_auto('/dev/stdin');
             SELECT DATE(timestamp) as day, COUNT(*) FROM logs GROUP BY day;"
```

[SQLite](https://sqlite.org/), [DuckDB](https://duckdb.org/)

---

### pirkle — PRQL Query Tool

Query CSV files and SQLite databases using PRQL (Pipelined Relational Query Language). Rust-based with excellent stdin support for Unix pipelines. Outputs JSON Lines, logfmt, CSV, or tables for bidirectional integration with Kelora.

```bash
# Kelora preprocesses, pirkle performs SQL analytics
kelora -j logs/app.jsonl -k timestamp,level,service,message -F csv | \
  pirkle stdin --format table --query '
    from stdin
    | group service (aggregate {error_count = count this})
    | sort {-error_count}'

# Join Kelora output with reference data
kelora -j logs/orders.jsonl -k order_id,user_id,amount -F csv | \
  pirkle stdin users.csv --format table --query '
    from stdin
    | join users (==user_id)
    | select {stdin.order_id, users.name, stdin.amount, users.region}'
```

**When to use pirkle:** SQL-style JOINs, complex GROUP BY, window functions, or when PRQL syntax is clearer than Rhai.

[pirkle documentation](https://github.com/dloss/pirkle)

---

### angle-grinder — Alternative Query Syntax

Log aggregation and filtering with a query language different from Kelora's Rhai scripting. Offers an alternative when Rhai scripting isn't a good fit, or when you want a more declarative syntax. angle-grinder and Kelora overlap significantly—choose based on syntax preference and feature fit.

```bash
# This angle-grinder pattern...
kelora -j logs/app.jsonl -J | \
  agrind '* | json | count by service'

# ...can be done natively with Kelora:
kelora -j logs/app.jsonl -e 'track_count(e.service)' -m

# angle-grinder's parse pattern
kelora -f logfmt logs/mixed.log -J | \
  agrind '* | json | parse "user_id=*" as user | count by user'

# Kelora equivalent using extract_re() or absorb_kv()
kelora -f logfmt logs/mixed.log \
  -e 'e.user = e.message.extract_regex(r"user_id=(\S+)", 1)' \
  -e 'track_count(e.user)' -m

# For mixed prose + key=value tails, absorb them in-place
kelora -f line logs/mixed.log \
  -e 'let res = e.absorb_kv("message");
      e.user = res.data["user_id"] ?? "";
      track_count(e.user)' -m
```

**When to use angle-grinder:** You prefer its query syntax, or need features Kelora doesn't have.
**When to use Kelora:** You want Rhai's full programming power, need advanced parsing functions, or want better integration with other tools.

[angle-grinder](https://github.com/rcoh/angle-grinder)

---

## Viewers & Exploration

### lnav — Interactive Log Navigator

Advanced TUI for browsing, searching, and analyzing logs with syntax highlighting and SQL queries. Provides interactive search, filtering, and timeline views that complement Kelora's batch processing.

```bash
# Pre-filter with Kelora, explore in lnav
kelora -j logs/app.jsonl -l error,warn -J | lnav

# Export Kelora results and open in lnav
kelora -j logs/app.jsonl -J -o filtered.json
lnav filtered.json
```

[lnav documentation](https://lnav.org/)

---

### csvlens — Terminal CSV Viewer

Interactive CSV viewer with search, filtering, and column inspection. Easier than scrolling through raw CSV in the terminal.

```bash
# Export to CSV and view interactively
kelora -j logs/app.jsonl -k timestamp,service,level,message -F csv | csvlens
```

[csvlens](https://github.com/YS-L/csvlens)

---

### visidata — Terminal Spreadsheet

Powerful TUI for exploring, transforming, and analyzing tabular data (CSV, TSV, JSON, SQLite, and more). Combines spreadsheet-like exploration with vim keybindings and powerful data manipulation.

```bash
# Open Kelora CSV output in visidata
kelora -j logs/app.jsonl -F csv | vd

# Open JSON output (visidata can parse nested structures)
kelora -j logs/app.jsonl -J | vd -f jsonl
```

[visidata documentation](https://www.visidata.org/)

---

### ov — Feature-Rich Terminal Pager

Advanced pager with search, filtering, and better navigation than `less`. Better UX for large outputs.

```bash
# Page through Kelora output with enhanced search
kelora -j logs/app.jsonl | ov

# Pre-filter and page
kelora -j logs/app.jsonl -l error,warn | ov
```

[ov](https://github.com/noborus/ov)

---

## Visualization

### rare — Real-Time Histograms and Charts

Creates console histograms, bar graphs, tables, and heatmaps from streaming data. Quick visual insights without leaving the terminal.

**Note:** Kelora's `track_bucket()` function can create histograms natively with `-m`. Use rare for visual charts.

```bash
# Bar chart of log levels
kelora -j logs/app.jsonl -k level -F tsv | rare histo --field level

# Alternative: Kelora's native histogram tracking
kelora -j logs/app.jsonl -e 'track_bucket("level", e.level)' -m

# Time-series histogram of events per minute
kelora -j logs/app.jsonl -k timestamp -F tsv | \
  rare histo --field timestamp --time

# Response time histogram with bucketing in Kelora
kelora -j logs/app.jsonl \
  -e 'let bucket = floor(e.response_time / 100) * 100; track_bucket("latency_ms", bucket)' \
  -m

# Table summary of services and error counts - rare for visual output
kelora -j logs/app.jsonl -l error -k service -F tsv | rare table

# Or use Kelora tracking for text output
kelora -j logs/app.jsonl -l error -e 'track_count(e.service)' -m
```

[rare](https://github.com/zix99/rare)

---

## Format Conversion

### column — Pretty-Print TSV Tables

Formats tab-separated data into aligned columns for readability. Makes TSV output readable in the terminal. GNU `column` (from util-linux) has more features than BSD `column`.

```bash
# Pretty-print TSV output
kelora -j logs/app.jsonl -k timestamp,level,service,message -F tsv | \
  column -ts $'\t'

# With custom separators and JSON output
kelora -j logs/app.jsonl -F tsv | \
  column -ts $'\t' -N timestamp,level,service,message -J > output.json
```

---

### jtbl — JSON to Table Converter

Converts JSON/JSON Lines to formatted tables for terminal display. Simpler than spreadsheet tools for quick viewing, automatically detects columns and formats data. Created by Kelly Brazil (author of `jc`).

```bash
# Pretty-print JSON output as a table
kelora -j logs/app.jsonl -k timestamp,level,service,message -J | jtbl

# Works with full JSON output too
kelora -j logs/app.jsonl --filter 'e.status >= 500' -J | jtbl

# Combine with jq for column selection
kelora -j logs/app.jsonl -J | \
  jq '{timestamp, level, service, message}' | jtbl

# Compare with column (TSV-based approach)
kelora -j logs/app.jsonl -k timestamp,level,service,message -F tsv | \
  column -ts $'\t'
```

**When to use jtbl vs column:**

- Use `jtbl` when working with JSON output (no need to convert to TSV first)
- Use `column` for TSV output or when you need more control over formatting
- `jtbl` auto-detects columns; `column` requires explicit configuration

[jtbl documentation](https://github.com/kellyjonbrazil/jtbl)

---

## Destinations (Storage & Aggregation)

For production log aggregation and long-term storage, Kelora's JSON output (`-J`) integrates well with centralized logging systems:

- **VictoriaLogs** — Fast, resource-efficient log database with LogsQL querying and Unix tool integration
- **Grafana Loki** — Horizontally-scalable, multi-tenant log aggregation
- **Graylog** — Free and open log management platform
- **OpenObserve** — Cloud-native observability for logs, metrics, and traces at petabyte scale
- **qryn** — Polyglot observability platform

**Typical integration pattern:**

```bash
# Stream Kelora output to aggregation system via HTTP
kelora -j logs/app.jsonl -J | \
  while IFS= read -r line; do
    curl -X POST http://loki:3100/loki/api/v1/push \
      -H "Content-Type: application/json" \
      -d "$line"
  done
```

**Or batch upload with span aggregation:**

```bash
# Process logs and upload to object storage for ingestion
kelora -j logs/*.jsonl.gz --parallel --unordered -J -o processed.json
rclone copy processed.json remote:logs/$(date +%Y-%m-%d)/

# Pre-aggregate with spans before uploading
kelora -j logs/*.jsonl.gz --span 5m \
  --span-close 'print(span.metrics.to_json())' \
  -e 'track_count("events"); track_sum("bytes", e.size)' -q > metrics.jsonl
curl -X POST http://victorialogs:9428/insert/jsonl -d @metrics.jsonl
```

---

## Common Patterns

### Parallel Discovery and Processing

Combine `find`, `xargs`, and Kelora's `--parallel --unordered` for maximum throughput:

```bash
# Maximum performance: parallel find + parallel kelora + unordered output
find /archives -name "*.jsonl.gz" -print0 | \
  xargs -0 -P 4 kelora -j --parallel --unordered \
    -l error \
    -e 'track_count(e.service)' \
    -m

# With time-based filtering instead of file modification time
find /archives -name "*.jsonl.gz" -print0 | \
  xargs -0 kelora -j --parallel --since '24 hours ago' \
    --filter 'e.status >= 500' \
    -e 'track_count(e.path); track_max("slowest", e.response_time)' -m
```

---

### Multi-Stage Pipelines

Chain tools for complex transformations. Consider whether Kelora can do it natively before adding external tools.

```bash
# Multi-stage: grep pre-filter → Kelora parse/extract → qsv stats → rare visualize
grep -i "checkout" /var/log/app.log | \
  kelora -f logfmt -k timestamp,duration,user_id -F csv | \
  qsv stats --select duration | \
  rare table

# Often better: Do it all in Kelora
kelora -f logfmt /var/log/app.log \
  --keep-lines checkout \
  -e 'track_bucket("duration_ms", floor(e.duration / 100) * 100)' \
  -e 'track_unique("users", e.user_id)' -m

# Complex extraction pipeline with parsing functions
kelora -f line /var/log/app.log \
  -e 'e.url_parts = e.message.extract_url().parse_url()' \
  -e 'e.params = e.url_parts.query.parse_query_params()' \
  -e 'e.checkout_id = e.params["id"]' \
  --filter 'e.has("checkout_id")' \
  -e 'track_count(e.url_parts.path)' -m
```

---

### Live Monitoring with Alerts

Stream logs, filter with Kelora, visualize with rare, alert on anomalies. Use span aggregation for rate-based alerting.

```bash
# Real-time visualization with rare
tail -F /var/log/app.log | \
  kelora -j -l error,critical -J | \
  rare histo --field service &

# Simple alert on specific conditions
tail -F /var/log/app.log | \
  kelora -j -l error -q \
    --filter 'e.service == "payments"' \
    -e 'eprint("PAYMENT ERROR: " + e.message)'

# Rate-based alerting with span aggregation
tail -F /var/log/app.log | \
  kelora -j --span 1m \
    -e 'if e.level == "ERROR" { track_count("errors") }' \
    --span-close 'if span.metrics["errors"].or_empty() > 10 { eprint("⚠️  High error rate: " + span.metrics["errors"].to_string() + " errors/min") }' \
    -q

# Spike detection with window functions
tail -F /var/log/app.log | \
  kelora -j --window 20 \
    -e 'e.recent_500s = window.pluck("status").filter(|x| x >= 500).len()' \
    --filter 'e.recent_500s > 5' \
    -e 'eprint("🚨 Error spike detected: " + e.recent_500s.to_string() + " 5xx in last 20 requests")' \
    -q
```

---

## When to Use Kelora vs External Tools

Before reaching for external tools, check if Kelora can handle it natively:

**Kelora excels at:**

- Time-based filtering (`--since`, `--until`) - no grep needed
- Structured field filtering (`--filter 'e.status >= 500'`)
- Pattern extraction (`extract_re()`, `extract_re_maps()`)
- Parsing structured formats (`parse_url()`, `parse_kv()`, `parse_json()`, etc.)
- Aggregation and counting (`track_count()`, `track_sum()`, `track_bucket()`)
- Array operations (`sorted()`, `filter()`, `map()`, `percentile()`)
- Window-based analysis (`--window` with `window.pluck()`)
- Time-windowed aggregation (`--span` with `--span-close`)
- Multi-line event detection (`-M timestamp`, `-M regex:...`)
- Section extraction (`--section-from`, `--section-before`)

**Use external tools when:**

- jq: Complex JSON reshaping, recursive descent
- qsv: Statistical analysis, CSV joins, data validation
- sort: Output ordering (Kelora doesn't sort output)
- grep: Raw text scanning across massive unstructured files
- SQL databases: Long-term storage, complex JOINs, persistent queries
- Visualization tools: Interactive exploration (lnav, visidata), charts (rare)

---

## Performance Tips

**When to pre-filter with grep:**

- Scanning for specific keywords in huge unstructured files
- Text search is simpler than structured field checks
- Benchmarking shows grep is faster for your use case

**When to use Kelora's --filter instead:**

- Checking structured fields (e.g., `e.status >= 500`)
- Complex boolean logic or numerical comparisons
- You need type-aware filtering (strings, numbers, timestamps)

**When to post-process with jq/qsv/miller:**

- Kelora extracted and normalized the data
- You need reshaping, aggregation, or format conversion
- SQL-style operations (GROUP BY, JOINs) are needed

**Benchmark your pipeline:**

```bash
# Measure each stage
time grep "ERROR" logs/huge.log | wc -l
time kelora -j logs/huge.log --filter 'e.level == "ERROR"' -q | wc -l

# Compare integrated vs multi-tool approaches
hyperfine \
  'kelora -j logs/app.jsonl -l error --stats' \
  'grep -i error logs/app.log | kelora -f logfmt --stats'
```

See [Concept: Performance Comparisons](../concepts/performance-comparisons.md) for detailed benchmarks.

---

## Common Pitfalls

**Shell quoting:** Remember to quote arguments with special characters:

```bash
# Wrong: shell may interpret special characters
kelora -f 'cols:ts *message' -e 'e.new_field = $other'

# Right: quote properly and use correct Rhai syntax
kelora -f 'cols:ts *message' -e 'e.new_field = e.other'
```

**Pipe buffering:** Some tools buffer output. Use `stdbuf` or tool-specific flags for live streaming:

```bash
# Force line buffering for live output
tail -F /var/log/app.log | stdbuf -oL kelora -j | stdbuf -oL grep "ERROR"
```

**Column indexing:** `sort` uses 1-indexed columns, not 0-indexed:

```bash
# Wrong: -k0 doesn't exist
kelora -F tsv | sort -t$'\t' -k0,0

# Right: first column is -k1
kelora -F tsv | sort -t$'\t' -k1,1
```

**Resource limits:** Parallel processing with multiple tools can exhaust file descriptors or memory:

```bash
# Increase limits if needed
ulimit -n 4096
kelora --parallel --threads 8 ... | qsv ... | visidata
```

---

## See Also

- [Process Archives at Scale](batch-process-archives.md) — Parallel processing strategies
- [Design Streaming Alerts](build-streaming-alerts.md) — Real-time monitoring workflows
- [Triage Production Errors](find-errors-in-logs.md) — Error investigation patterns
- [Concept: Performance Comparisons](../concepts/performance-comparisons.md) — Benchmark data for tool selection
- [CLI Reference](../reference/cli-reference.md) — Complete Kelora command-line reference
