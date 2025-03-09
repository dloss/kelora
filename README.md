# Kelora - A High-Performance Log Viewer

## Overview

Kelora is a command-line log viewing and processing tool designed to help developers and system administrators efficiently parse, filter, and analyze log data. This MVP implementation focuses on the most essential functionality:

- Reading JSON Lines (JSONL) format log data from stdin
- Filtering log entries based on specified keys
- Outputting logs in logfmt format to stdout

## Installation

### Prerequisites

Make sure you have Rust and Cargo installed on your system. If not, you can install them from [rustup.rs](https://rustup.rs/).

### Building from Source

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/kelora.git
   cd kelora
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. The executable will be available at `target/release/kelora`

## Usage

```bash
kelora [OPTIONS]
```

### Options

- `-k, --keys <KEYS>`: Comma-separated list of keys to display
- `-i, --include-only`: When specified, only include the keys listed with `-k` in the output
- `-h, --help`: Print help information
- `-V, --version`: Print version information

### Examples

#### Process JSONL and output all fields in logfmt format

```bash
cat logs.jsonl | kelora
```

#### Display only specific fields

```bash
cat logs.jsonl | kelora -k timestamp,level,message
```

#### Only include specific fields in the output (exclude all others)

```bash
cat logs.jsonl | kelora -k timestamp,level,message -i
```

## Input Format

Kelora currently supports JSON Lines (JSONL) as input format. Each line should be a valid JSON object:

```json
{"timestamp": "2023-07-18T15:04:23.456Z", "level": "ERROR", "message": "Failed to connect to database", "host": "db.example.com"}
{"timestamp": "2023-07-18T15:04:25.789Z", "level": "WARN", "message": "Using fallback connection settings", "fallback": "localhost:5433"}
```

## Output Format

The output is formatted as logfmt, which is a key-value format commonly used for logging:

```
timestamp="2023-07-18T15:04:23.456Z" level="ERROR" message="Failed to connect to database" host="db.example.com"
timestamp="2023-07-18T15:04:25.789Z" level="WARN" message="Using fallback connection settings" fallback="localhost:5433"
```


# Kelora Example Usage

Let's walk through some practical examples of using the Kelora log viewer.

## Sample Log Data

First, let's create a sample JSONL file to work with:

```bash
cat > sample_logs.jsonl << 'EOF'
{"timestamp": "2023-07-18T15:04:23.456Z", "level": "ERROR", "component": "app.server", "message": "Failed to connect to database", "host": "db.example.com", "port": 5432, "retry": 3, "error": "connection refused"}
{"timestamp": "2023-07-18T15:04:25.789Z", "level": "WARN", "component": "app.server", "message": "Using fallback connection settings", "fallback": "localhost:5433"}
{"timestamp": "2023-07-18T15:06:41.210Z", "level": "INFO", "component": "app.server", "message": "Database connection established", "host": "localhost", "port": 5433, "latency": 45}
{"timestamp": "2023-07-18T15:07:12.345Z", "level": "DEBUG", "component": "app.auth", "message": "User authentication attempt", "user_id": "user123", "method": "password", "success": true}
{"timestamp": "2023-07-18T15:08:30.678Z", "level": "INFO", "component": "app.api", "message": "API request completed", "endpoint": "/users", "method": "GET", "status": 200, "duration_ms": 127}
EOF
```

## Basic Usage

### View All Log Fields

Process the sample log file and output all fields:

```bash
cat sample_logs.jsonl | kelora
```

Output:
```
component="app.server" error="connection refused" host="db.example.com" level="ERROR" message="Failed to connect to database" port=5432 retry=3 timestamp="2023-07-18T15:04:23.456Z"
component="app.server" fallback="localhost:5433" level="WARN" message="Using fallback connection settings" timestamp="2023-07-18T15:04:25.789Z"
component="app.server" host="localhost" latency=45 level="INFO" message="Database connection established" port=5433 timestamp="2023-07-18T15:06:41.210Z"
component="app.auth" level="DEBUG" message="User authentication attempt" method="password" success=true timestamp="2023-07-18T15:07:12.345Z" user_id="user123"
component="app.api" duration_ms=127 endpoint="/users" level="INFO" message="API request completed" method="GET" status=200 timestamp="2023-07-18T15:08:30.678Z"
```

### Focus on Key Information

Show only timestamp, level, and message fields:

```bash
cat sample_logs.jsonl | kelora -k timestamp,level,message
```

Output:
```
component="app.server" error="connection refused" host="db.example.com" level="ERROR" message="Failed to connect to database" port=5432 retry=3 timestamp="2023-07-18T15:04:23.456Z"
component="app.server" fallback="localhost:5433" level="WARN" message="Using fallback connection settings" timestamp="2023-07-18T15:04:25.789Z"
component="app.server" host="localhost" latency=45 level="INFO" message="Database connection established" port=5433 timestamp="2023-07-18T15:06:41.210Z"
component="app.auth" level="DEBUG" message="User authentication attempt" method="password" success=true timestamp="2023-07-18T15:07:12.345Z" user_id="user123"
component="app.api" duration_ms=127 endpoint="/users" level="INFO" message="API request completed" method="GET" status=200 timestamp="2023-07-18T15:08:30.678Z"
```

### Include Only Specific Fields

Show only timestamp, level, and message fields, excluding all others:

```bash
cat sample_logs.jsonl | kelora -k timestamp,level,message -i
```

Output:
```
level="ERROR" message="Failed to connect to database" timestamp="2023-07-18T15:04:23.456Z"
level="WARN" message="Using fallback connection settings" timestamp="2023-07-18T15:04:25.789Z"
level="INFO" message="Database connection established" timestamp="2023-07-18T15:06:41.210Z"
level="DEBUG" message="User authentication attempt" timestamp="2023-07-18T15:07:12.345Z"
level="INFO" message="API request completed" timestamp="2023-07-18T15:08:30.678Z"
```

### Filtering and Piping

You can combine Kelora with other command-line tools for more advanced filtering:

```bash
# Show only ERROR and WARN logs with timestamps and messages
cat sample_logs.jsonl | kelora -k timestamp,level,message -i | grep -E 'level="ERROR"|level="WARN"'
```

Output:
```
level="ERROR" message="Failed to connect to database" timestamp="2023-07-18T15:04:23.456Z"
level="WARN" message="Using fallback connection settings" timestamp="2023-07-18T15:04:25.789Z"
```

### Processing Real-time Logs

You can also use Kelora with live logs by piping from a command that generates log output:

```bash
# Example with an application that outputs JSONL logs
your_application | kelora -k timestamp,level,message -i
```

This is particularly useful for monitoring applications in real-time while focusing only on the information you need.

## Future Development

This is an MVP (Minimum Viable Product) implementation. Future versions will include:

- Support for additional input formats (logfmt, CSV, syslog)
- More advanced filtering options (by patterns, time ranges, log levels)
- Time-based analysis
- Syntax highlighting
- Pattern recognition and deduplication
- Statistical views and visualizations