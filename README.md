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


# Testing Kelora

This document describes how to test kelora to ensure it works correctly.

## Quick Start

The easiest way to run all tests is:

```bash
# Run all tests (recommended)
make test-full

# Or just the automated tests
make test
```

## Test Types

### 1. Unit Tests
These test individual components in isolation:

```bash
# Run unit tests
cargo test --lib
# or
make test-unit
```

Unit tests cover:
- Event parsing and field extraction
- Formatter output correctness
- Core field identification
- Error handling in parsers

### 2. Integration Tests
These test the complete application end-to-end:

```bash
# Run integration tests
cargo test --test integration_tests
# or 
make test-integration
```

Integration tests cover:
- Complete parsing pipelines
- CLI argument handling
- Input/output processing
- Error scenarios
- Performance characteristics

### 3. Manual Tests
Comprehensive tests including real file processing:

```bash
# Run the full test suite
./test_kelora.sh
# or
make test-full
```

## Setting Up Test Environment

1. **Install dependencies:**
   ```bash
   cargo fetch
   # or
   make install
   ```

2. **Build the project:**
   ```bash
   cargo build --release
   # or
   make build
   ```

3. **Make test script executable:**
   ```bash
   chmod +x test_kelora.sh
   ```

## Test Data

Create test data files in a `test_data/` directory:

```bash
mkdir -p test_data
```

**test_data/sample.jsonl:**
```json
{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Connection failed","host":"db.example.com"}
{"timestamp":"2023-07-18T15:04:25.789Z","level":"INFO","message":"Connection established","host":"localhost"}
```

**test_data/sample.logfmt:**
```
timestamp="2023-07-18T15:04:23.456Z" level=ERROR message="Connection failed" host="db.example.com"
timestamp="2023-07-18T15:04:25.789Z" level=INFO message="Connection established" host="localhost"
```

## Manual Testing Examples

### Basic Functionality
```bash
# Test JSONL parsing
cat test_data/sample.jsonl | ./target/release/kelora -f jsonl

# Test key filtering
./target/release/kelora -f jsonl -k timestamp,level,message test_data/sample.jsonl

# Test level filtering
./target/release/kelora -f jsonl -l ERROR,WARN test_data/sample.jsonl

# Test statistics
./target/release/kelora -f jsonl -S test_data/sample.jsonl
```

### Format Conversion
```bash
# Convert logfmt to JSONL
./target/release/kelora -f logfmt -F jsonl test_data/sample.logfmt

# Show only core fields
./target/release/kelora -f jsonl -c test_data/sample.jsonl
```

### Error Handling
```bash
# Test with malformed data
echo '{"valid":"json"}
{invalid json}
{"another":"valid"}' | ./target/release/kelora -f jsonl

# Debug mode
echo '{invalid}' | ./target/release/kelora -f jsonl --debug
```

## Performance Testing

Test with larger datasets:

```bash
# Generate 10,000 log entries
for i in $(seq 1 10000); do
  echo "{\"timestamp\":\"2023-07-18T15:04:23.456Z\",\"level\":\"INFO\",\"message\":\"Message $i\",\"id\":$i}"
done > large_test.jsonl

# Test performance
time ./target/release/kelora -f jsonl -S large_test.jsonl
```

## Expected Test Results

### Unit Tests
All unit tests should pass:
```
running 8 tests
test event::tests::test_filter_keys ... ok
test formatters::tests::test_default_formatter_empty_event ... ok
test formatters::tests::test_escape_quotes ... ok
test parsers::tests::test_logfmt_parser_basic ... ok
...
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Integration Tests
All integration tests should pass:
```
running 18 tests
test test_basic_jsonl_parsing ... ok
test test_basic_logfmt_parsing ... ok
test test_key_filtering ... ok
...
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Manual Tests
The test script should show:
```
🧪 Running kelora test suite...
================================
📦 Building kelora...
✅ Build successful
🔧 Running unit tests...
✅ Unit tests passed
🔄 Running integration tests...
✅ Integration tests passed
📝 Running manual tests with sample data...
✅ JSONL parsing works
✅ Key filtering works
✅ Level filtering works
✅ Statistics mode works
✅ All tests completed successfully!
```

## Troubleshooting

### Build Issues
```bash
# Clean and rebuild
make clean
make build

# Check Rust version
rustc --version  # Should be 1.70+
```

### Test Failures
```bash
# Run tests with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_basic_jsonl_parsing

# Debug integration test
cargo test --test integration_tests test_basic_jsonl_parsing -- --nocapture
```

### Permission Issues
```bash
# Make test script executable
chmod +x test_kelora.sh

# Check if binary was built
ls -la target/release/kelora
```

## Continuous Integration

For CI/CD pipelines, use:

```bash
# Install dependencies
cargo fetch

# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Run all tests
cargo test

# Build release
cargo build --release
```

## Adding New Tests

### Unit Tests
Add to the appropriate module's `#[cfg(test)]` section:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_new_functionality() {
        // Your test here
    }
}
```

### Integration Tests
Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_new_feature() {
    let input = r#"test input"#;
    let (stdout, stderr, exit_code) = run_kelora_with_input(&["-f", "jsonl"], input);
    assert_eq!(exit_code, 0);
    // Your assertions here
}
```

### Manual Tests
Add to `test_kelora.sh`:

```bash
# Test N: Description
print_status "Test N: Description" $YELLOW
./target/release/kelora [args] > "$TEMP_DIR/outputN.txt"
if [[ condition ]]; then
    print_status "✅ Test N passed" $GREEN
else
    print_status "❌ Test N failed" $RED
fi
```

Happy testing! 🧪

## Future Development

This is an MVP (Minimum Viable Product) implementation. Future versions will include:

- Support for additional input formats (logfmt, CSV, syslog)
- More advanced filtering options (by patterns, time ranges, log levels)
- Time-based analysis
- Syntax highlighting
- Pattern recognition and deduplication
- Statistical views and visualizations