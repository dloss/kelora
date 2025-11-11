# Basics: Input, Display & Filtering

Master the essential commands for reading logs, controlling display output, and filtering by log level. This tutorial covers the foundation you'll use in every Kelora workflow.

## What You'll Learn

- Specify input formats with `-f` and `-j`
- Control what fields are displayed with `-b`, `-c`, `-k`, and `-K`
- Filter events by log level with `-l` and `-L`
- Export data in different formats with `-F` and `-J`
- Combine options for common workflows

## About This Tutorial

In the [Quickstart](../quickstart.md), you ran three commands to see Kelora in action. Now we'll teach you what each flag means, how they combine, and when to use them. By the end, you'll understand the building blocks for any Kelora workflow.

## Prerequisites

- Kelora installed and in your PATH
- Basic command-line familiarity

## Sample Data

Commands below use `examples/basics.jsonl` — a small JSON-formatted log file with 6 events designed for this tutorial:

```bash exec="on" result="ansi"
cat examples/basics.jsonl
```

If you cloned the project, run commands from the repository root.

---

## Part 1: Input Formats (`-f`, `-j`)

### Explicit Format Selection Required

You've seen `-j` in the [Quickstart](../quickstart.md) to read JSON logs. Let's understand what this flag really means and what other formats Kelora supports.

**Important:** Kelora does **NOT** auto-detect format based on filename. The default is `-f line` (plain text). You must specify the format explicitly.

Let's see what happens without specifying the format:

```bash exec="on" source="above" result="ansi"
kelora examples/basics.jsonl
```

Notice it treats the entire JSON line as plain text (`line='...'`). Now with `-j`:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl
```

**Three ways to read JSON logs:**

```bash
kelora -f json examples/basics.jsonl    # Explicit format
kelora -j examples/basics.jsonl         # -j is shortcut for -f json
kelora -f auto examples/basics.jsonl    # Auto-detect by examining content
```

**Key Points:**

- `-f auto` detects format by **examining the content** (not filename)
- Kelora does **NOT** look at file extensions (`.jsonl`, `.log`, `.csv`)
- Default is always `-f line` unless you specify otherwise
- Best practice: Be explicit with `-j` for JSON

### Common Input Formats

```bash
-f json         # JSON lines (or use -j shortcut)
-f logfmt       # key=value format
-f combined     # Apache/Nginx access logs
-f syslog       # Syslog format (RFC3164/RFC5424)
-f csv          # CSV with header
-f tsv          # Tab-separated values
-f line         # Plain text (default)
-f auto         # Auto-detect by content
```

---

## Part 2: Understanding the Default Display

Let's examine what Kelora shows by default:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl
```

**The default output format shows:**

- **Field names and values** in `key='value'` format
- **Automatic wrapping** - long events wrap with indentation
- **Colors** (when terminal supports it)
- **Smart ordering** - timestamp, level, message first, then others alphabetically

**Key observations:**

1. Strings are quoted (`'Application started'`)
2. Numbers are not quoted (`max_connections=50`)
3. **Intelligent wrapping** - When output is too wide for your terminal, Kelora wraps **between fields** (never in the middle of a field) and indents continuation lines for readability
4. Each event is separated by a blank line
5. Field names are highlighted in color for better readability

---

## Part 2.5: Understanding Events

Before we dive into display options, let's clarify what an **event** is and how you'll work with it in filters and scripts.

### What is an Event?

After Kelora parses a log line, it becomes an **event** — a structured object (like a map or dictionary) containing fields you can access and manipulate.

Looking at the output above, each block like this is one event:

```
timestamp='2024-01-15T14:23:45Z' level='INFO' message='Application started'
    service='api' version='1.2.3'
```

### The Event Object: `e`

In filter expressions and scripts, you access the current event using the variable **`e`**. Each field becomes a property:

```rhai
e.timestamp   // Access the timestamp field
e.level       // Access the level field
e.service     // Access the service field
e.message     // Access the message field
```

**Example:** To filter for ERROR events, you write `--filter 'e.level == "ERROR"'` which means "keep events where the level field equals ERROR."

**Example:** To check if status code is 500 or higher, you write `--filter 'e.status >= 500'` which means "keep events where the status field is 500 or more."

### Why This Matters

Understanding events is crucial because:

- **Filtering** uses event fields: `--filter 'e.service == "database"'`
- **Scripts** read and modify event fields: `--exec 'e.user_type = "admin"'`
- **Display options** control which event fields you see: `--keys timestamp,level,message`

You'll encounter `e` throughout the documentation. Remember: `e` = the current event, and `e.field_name` = accessing a field in that event.

!!! tip "Want to learn more?"
    For complete details on event structure, nested fields, and type handling, see [Events and Fields](../concepts/events-and-fields.md).

---

## Part 3: Display Modifiers (`-b`, `-c`, `-k`, `-K`)

### Brief Mode (`-b`) - Values Only

Omit field names, show only values for compact output:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -b
```

**Use `-b` when:** You want compact, grep-friendly output.

### Core Fields (`-c`) - Essentials Only

Show only timestamp, level, and message:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -c
```

**Use `-c` when:** You want to focus on the essentials, hiding extra metadata.

### Select Fields (`-k`) - Choose What to Show

Choose exactly which fields to show (and in what order):

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -k level,service,message
```

**Pro tip:** Fields appear in the order you specify!

### Exclude Fields (`-K`) - Hide Sensitive Data

Remove specific fields (like passwords, tokens, or verbose metadata):

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -K service,version
```

**Use `-K` when:** Hiding sensitive data (passwords, API keys) or reducing noise.

---

## Part 4: Level Filtering (`-l`, `-L`)

### Include Levels (`-l`) - Show Only Specific Log Levels

Filter to show only errors and warnings:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -l error,warn
```

**Common patterns:**

```bash
kelora -j app.log -l error                    # Errors only
kelora -j app.log -l error,warn,critical      # Problems only (case-insensitive)
kelora -j app.log -l info                     # Application flow (skip debug noise)
```

### Exclude Levels (`-L`) - Hide Debug Noise

Remove verbose log levels:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -L debug,info
```

**Use `-L` when:** You want to exclude chatty debug/trace output.

---

## Part 5: Output Formats (`-F`, `-J`)

The default `key='value'` format is great for reading, but sometimes you need machine-readable output.

### JSON Output (`-F json` or `-J`)

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -J
```

**Use JSON when:** Piping to `jq`, saving to file, or integrating with other tools.

### CSV Output (`-F csv`)

Perfect for spreadsheet export:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -F csv -k timestamp,level,service,message
```

**Use CSV when:** Exporting to Excel, Google Sheets, or data analysis tools.

### Logfmt Output (`-F logfmt`)

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -F logfmt
```

**Use logfmt when:** You want parseable output that's also human-readable.

### Inspect Output (`-F inspect`) - Debug with Types

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -F inspect
```

**Use inspect when:** Debugging type mismatches or understanding field types.

### No Output (`-F none`) - Stats Only

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -F none --stats
```

**Use `-F none --stats` when:** You want to analyze log structure without seeing the events.

---

## Part 6: Practical Combinations

### Exercise 1: Find Errors, Show Essentials

Show only errors with just timestamp, service, and message:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -l error -k timestamp,service,message
```

### Exercise 2: Export Problems to CSV

Export warnings and errors to CSV for Excel analysis:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -l error,warn -k timestamp,level,service,message -F csv
```

### Exercise 3: Compact View Without Debug

Brief output excluding debug noise:

```bash exec="on" source="above" result="ansi"
kelora -j examples/basics.jsonl -L debug -b
```

### Real-World Patterns

Here are some patterns you'll use frequently in practice:

```bash
# Stream processing (tail -f, kubectl logs, etc.)
kubectl logs -f deployment/api | kelora -f json -l error

# Multiple files - track which files have errors
kelora -f json logs/*.log --metrics \
  --exec 'if e.level == "ERROR" { track_count(meta.filename) }'

# Time-based filtering
kelora -f combined access.log --since "1 hour ago" --until "10 minutes ago"

# Extract prefixes (Docker Compose, systemd, etc.)
docker compose logs | kelora --extract-prefix container -f json

# Auto-detect format and output brief values only
kelora -f auto mixed.log -k timestamp,level,message -b

# Custom timestamp formats
kelora -f line app.log --ts-format "%d/%b/%Y:%H:%M:%S" --ts-field timestamp
```

---

## Quick Reference Cheat Sheet

### Input Formats
```bash
-f json         # JSON lines (or use -j shortcut)
-f logfmt       # key=value format
-f combined     # Apache/Nginx access logs
-f syslog       # Syslog format
-f csv          # CSV with header
-f line         # Plain text (default)
-f auto         # Auto-detect by content
```

### Display Modifiers
```bash
-b              # Brief: values only, no field names
-c              # Core: timestamp + level + message only
-k level,msg    # Keys: show only these fields (in this order)
-K password,ip  # Exclude: hide these fields
```

### Level Filtering
```bash
-l error,warn   # Include: show only these levels
-L debug,trace  # Exclude: hide these levels
```

### Output Formats
```bash
-F default      # Pretty key='value' with colors (default)
-F json         # JSON lines (or use -J shortcut)
-F csv          # CSV with header
-F logfmt       # Logfmt key=value
-F inspect      # Debug with types
-F none         # No output (use with --stats)
```

---

## Understanding the Pipeline Order

Kelora processes your options in this order:

```
1. Read file      (-f json, -j)
2. Filter levels  (-l error, -L debug)
3. Select fields  (-k, -K, -c)
4. Format output  (-F csv, -J, -b)
5. Write output   (stdout or -o file)
```

**This means:**

- `-l` filters happen **before** `-k` (you can filter on fields you won't see in output)
- `-b` affects display, not what gets filtered
- `-F none --stats` still processes everything, just doesn't show events

---

## Common Workflows

### Error Analysis Pipeline
```bash
kelora -j app.log -l error -k timestamp,service,message -F csv -o errors.csv
# Filter → Select fields → Export to CSV → Save to file
```

### Quick Scan (Hide Noise)
```bash
kelora -j app.log -L debug,trace -b --take 20
# Exclude verbose levels → Brief output → First 20 events
```

### Investigation Mode (Full Detail)
```bash
kelora -j app.log -l warn,error,critical -K password,token
# Show problems → Hide sensitive data → Keep all other fields
```

### Stats-Only Analysis
```bash
kelora -j app.log -F none --stats
# No event output → Show processing statistics
```

---

## When to Use What

| Goal | Use | Example |
|------|-----|---------|
| **Find errors fast** | `-l error` | `kelora -j app.log -l error -c` |
| **Hide debug spam** | `-L debug,trace` | `kelora -j app.log -L debug` |
| **Export to Excel** | `-F csv` | `kelora -j app.log -F csv -o report.csv` |
| **Pipe to jq** | `-J` | `kelora -j app.log -J \| jq '.level'` |
| **Quick scan** | `-b --take 20` | `kelora -j app.log -b --take 20` |
| **Hide secrets** | `-K password,token` | `kelora -j app.log -K password,apikey` |
| **See types** | `-F inspect` | `kelora -j app.log -F inspect` |

---

## Next Steps

You've mastered the basics of input, display, and filtering. Now **learn to write scripts** for custom logic:

### Recommended Next: Introduction to Rhai

**[→ Introduction to Rhai Scripting](intro-to-rhai.md)** (20 min) - Learn to write filter expressions and transforms. You'll understand how to use the `e` object you just learned about, write conditionals, convert types, and build multi-stage pipelines. This is essential before tackling advanced features.

### After That: Specialized Topics

Pick based on your needs:

- **[Working with Time](working-with-time.md)** (15 min) - Parse timestamps, filter by time ranges, handle timezones
- **[Metrics and Tracking](metrics-and-tracking.md)** (20 min) - Aggregate data with `track_*()` functions
- **[Parsing Custom Formats](parsing-custom-formats.md)** (15 min) - Handle non-standard log formats
- **[Advanced Scripting](advanced-scripting.md)** (30 min) - Complex transformations and window operations

### Or Jump to Solutions

**[How-To Guides](../how-to/index.md)** - Solve specific problems with ready-made solutions
