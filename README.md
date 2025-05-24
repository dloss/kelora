# Kelora - A Fast, Extensible Log Parser

Kelora is a command-line log parsing and analysis tool built in Rust, designed to help developers and system administrators efficiently process, filter, and analyze structured log data. It focuses on performance, simplicity, and extensibility.

## Features

- **Multiple Input Formats**: Support for logfmt, JSON Lines (JSONL), and syslog formats
- **Flexible Output**: Choose between logfmt (default) and JSONL output formats
- **Smart Filtering**: Filter by log levels and specific fields
- **Statistics**: Get comprehensive statistics about your log data
- **Core Field Detection**: Automatically detects timestamps, log levels, and messages
- **Performance**: Built in Rust for speed and memory efficiency
- **Error Handling**: Graceful handling of malformed log entries

## Installation

### Prerequisites

- Rust 1.70+ and Cargo (install from [rustup.rs](https://rustup.rs/))

### Building from Source

```bash
git clone https://github.com/dloss/kelora.git
cd kelora
cargo build --release
```

The executable will be available at `target/release/kelora`.

### Installing with Cargo

```bash
cargo install --path .
```

## Quick Start

```bash
# Parse logfmt logs from a file
kelora access.logfmt

# Parse JSON Lines from stdin
cat app.jsonl | kelora -f jsonl

# Show only error and warning logs
kelora -l error,warn app.logfmt

# Get statistics about your logs
kelora -S server.log

# Show only core fields (timestamp, level, message)
kelora -c app.jsonl
```

## Usage

```
kelora [OPTIONS] [FILES...]
```

### Options

#### Input Control
- `-f, --format <FORMAT>`: Input format [default: logfmt] [possible values: logfmt, jsonl, syslog]
- `<FILES>`: Input files (reads from stdin if not specified)

#### Output Control
- `-F, --output-format <FORMAT>`: Output format [default: default] [possible values: default, jsonl]
- `-k, --keys <KEYS>`: Only show specific keys (comma-separated)
- `-c, --common`: Show only core fields (timestamp, level, message)

#### Filtering
- `-l, --level <LEVELS>`: Filter by log levels (comma-separated)

#### Information
- `-S, --stats-only`: Show statistics only (no log output)
- `-s, --stats`: Show statistics alongside log output
- `--debug`: Enable debug output for troubleshooting

#### Help
- `-h, --help`: Print help information
- `-V, --version`: Print version information

## Input Formats

### Logfmt (Default)
Key-value pairs with optional quoted values:
```
timestamp="2024-01-15T10:30:00Z" level=info message="Server started" port=8080
timestamp="2024-01-15T10:30:05Z" level=error message="Database connection failed" error="timeout"
```

### JSON Lines (JSONL)
One JSON object per line:
```json
{"timestamp": "2024-01-15T10:30:00Z", "level": "info", "message": "Server started", "port": 8080}
{"timestamp": "2024-01-15T10:30:05Z", "level": "error", "message": "Database connection failed", "error": "timeout"}
```

### Syslog
Standard syslog format with priority, timestamp, hostname, and process:
```
<13>Jan 15 10:30:00 server01 myapp[1234]: Connection established
<11>Jan 15 10:30:05 server01 myapp[1234]: Database error occurred
```

## Output Formats

### Default (Logfmt)
Clean, readable key-value format:
```
timestamp="2024-01-15T10:30:00.000Z" level="info" message="Server started" port=8080
```

### JSONL
One JSON object per line for easy programmatic processing:
```json
{"timestamp":"2024-01-15T10:30:00.000Z","level":"info","message":"Server started","port":8080}
```

## Examples

### Basic Usage

```bash
# View all fields from a logfmt file
kelora app.logfmt

# Parse JSONL input and show in logfmt format
cat logs.jsonl | kelora -f jsonl

# Convert logfmt to JSONL
kelora -f logfmt -F jsonl app.logfmt > app.jsonl
```

### Filtering

```bash
# Show only error and warning logs
kelora -l error,warn app.logfmt

# Show only specific fields
kelora -k timestamp,level,message,error app.logfmt

# Show only core fields (timestamp, level, message)
kelora -c app.logfmt

# Combine filtering options
kelora -l error -k timestamp,message,error app.logfmt
```

### Statistics and Analysis

```bash
# Get comprehensive statistics
kelora -S app.logfmt

# Show logs with statistics
kelora -s app.logfmt

# Statistics output includes:
# - Number of events processed and shown
# - Parse errors and filtered events
# - Time span of logs
# - Log levels distribution
```

Example statistics output:
```
Events shown: 1542 (parse errors: 3, lines seen: 1545, filtered: 0)
Time span: 2024-01-15T10:00:00.000Z to 2024-01-15T11:30:45.123Z (duration: 1h30m45s)
Log levels: DEBUG(234), ERROR(45), INFO(1205), WARN(58)
```

### Working with Multiple Files

```bash
# Process multiple log files
kelora app1.logfmt app2.logfmt error.log

# Combine with shell globbing
kelora logs/*.logfmt

# Process different formats
kelora -f jsonl app.jsonl
kelora -f syslog system.log
```

### Pipeline Integration

```bash
# Use with other command-line tools
kelora app.logfmt | grep "database"
kelora -l error app.logfmt | wc -l

# Real-time log monitoring
tail -f app.log | kelora -f jsonl -l error,warn

# Convert and process
kelora -F jsonl app.logfmt | jq '.message'
```

### Error Handling

```bash
# Debug parsing issues
kelora --debug malformed.log

# Continue processing despite errors (default behavior)
kelora mixed-quality.log

# View statistics to see parse error counts
kelora -S problematic.log
```

## Core Field Detection

Kelora automatically recognizes common field names for core log components:

- **Timestamp**: `timestamp`, `ts`, `time`, `at`, `_t`, `@t`, `t`
- **Log Level**: `level`, `log_level`, `loglevel`, `lvl`, `severity`, `@l`
- **Message**: `message`, `msg`, `@m`

These fields receive special treatment in filtering, statistics, and output formatting.

## Log Level Filtering

Kelora recognizes standard log levels and handles case-insensitive matching:

- `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`
- `FATAL`, `CRITICAL`, `NOTICE` (syslog levels)
- Custom levels are also supported

Examples:
```bash
# Case-insensitive level filtering
kelora -l error,WARN,Info app.logfmt

# Multiple level specification
kelora -l error -l warn app.logfmt
```

## Performance Tips

- **Streaming**: Kelora processes logs in a streaming fashion, handling large files efficiently
- **Memory Usage**: Low memory footprint even with large log files
- **Error Recovery**: Continues processing even when individual log entries are malformed
- **Broken Pipe Handling**: Gracefully handles interruption when piping to tools like `head`

## Integration Examples

### With Standard Unix Tools

```bash
# Count error logs
kelora -l error app.logfmt | wc -l

# Find specific patterns
kelora -k message app.logfmt | grep -i "database"

# Get unique error messages
kelora -l error -k message app.logfmt | sort | uniq -c

# Time-based analysis with awk
kelora -k timestamp,level app.logfmt | awk -F'"' '{print $2, $4}' | sort
```

### With JSON Tools

```bash
# Use with jq for complex JSON processing
kelora -F jsonl app.logfmt | jq 'select(.level == "error") | .message'

# Extract specific fields
kelora -F jsonl app.logfmt | jq -r '.timestamp + " " + .message'

# Group by field values
kelora -F jsonl app.logfmt | jq -r '.level' | sort | uniq -c
```

### Log Monitoring Workflows

```bash
# Monitor application errors in real-time
tail -f app.log | kelora -f jsonl -l error,warn -k timestamp,level,message

# Process logs and save filtered results
kelora -l error app.logfmt > error_logs.txt

# Create summary reports
kelora -S *.logfmt > daily_summary.txt
```

## Error Handling and Debugging

Kelora is designed to be robust when dealing with real-world log data:

- **Parse Errors**: Malformed entries are skipped with optional debug output
- **Missing Fields**: Gracefully handles logs with inconsistent field sets
- **Format Detection**: Automatically works with variations in timestamp and level formats
- **Empty Lines**: Skips empty lines without errors

Enable debug mode to see detailed error information:
```bash
kelora --debug problematic.log
```

## Supported Timestamp Formats

Kelora automatically recognizes various timestamp formats:

- ISO 8601: `2024-01-15T10:30:00.123Z`
- ISO 8601 with timezone: `2024-01-15T10:30:00.123+01:00`
- Common log format: `2024-01-15 10:30:00.123`
- Syslog format: `Jan 15 10:30:00`
- RFC 3339: `2024-01-15T10:30:00Z`

## Development and Contributing

### Building and Testing

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run with optimizations
cargo build --release

# Check code formatting
cargo fmt --check

# Run linter
cargo clippy
```

### Project Structure

```
src/
├── main.rs          # CLI interface and main application logic
├── event.rs         # Event data structure and core field extraction
├── parsers.rs       # Input format parsers (logfmt, JSONL, syslog)
├── formatters.rs    # Output formatters (logfmt, JSONL)
└── lib.rs          # Library interface
```

### Architecture

Kelora follows a pipeline architecture:

```
Input → Parser → Event → Filter → Formatter → Output
```

Each component is designed to be:
- **Composable**: Easy to add new parsers and formatters
- **Testable**: Individual components can be tested in isolation
- **Extensible**: New features can be added without major refactoring

## Future Roadmap

Planned enhancements include:

- **Advanced Filtering**: Time range filtering, regex patterns, field conditions
- **Compression Support**: Gzip, zip, and other compressed log formats
- **Configuration Files**: Persistent settings and custom field mappings
- **Enhanced Statistics**: Histograms, pattern detection, anomaly detection
- **Performance Optimizations**: Parallel processing, zero-copy parsing
- **Additional Formats**: CSV, TSV, custom format definitions

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Kelora draws inspiration from tools like:
- [klp](https://github.com/dloss/klp) - Kool Log Parser (Python)
- [jq](https://jqlang.github.io/jq/) - Command-line JSON processor
- [angle-grinder](https://github.com/rcoh/angle-grinder) - Slice and dice logs on the command line

## Support

- Create an issue on GitHub for bug reports or feature requests
- Check existing issues for known problems and solutions
- Contribute code improvements via pull requests

---

**Happy log parsing!** 🪵✨