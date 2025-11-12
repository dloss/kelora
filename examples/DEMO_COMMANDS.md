# Demo Commands Cheat Sheet

Quick reference for video walkthroughs and live demos.

## Files Used

- `demo_api_errors_large.jsonl` - 130 events showing error spike pattern (created for demos)
- `api_errors.jsonl` - Small sample (6 events) for quick tests
- `payments_latency.jsonl` - Payment processing with latency and regions
- `web_access.log` - Apache/Nginx combined format logs
- `simple_json.jsonl` - Basic JSON for format detection demos

## Video 1: 90-Second Hero Demo

```bash
# Show the problem - grep (ugly)
grep ERROR examples/demo_api_errors_large.jsonl

# The solution - Kelora
kelora -j examples/demo_api_errors_large.jsonl \
  --levels error \
  --exec 'e.msg = e.endpoint + " → " + e.error'

# Quick visualization
kelora -j examples/demo_api_errors_large.jsonl -F levelmap

# With metrics
kelora -j examples/demo_api_errors_large.jsonl \
  --levels error \
  --metrics \
  --exec 'track_count(e.endpoint)'
```

## Video 2: Realistic Debugging Journey

```bash
# Act 1: Visualize the timeline
kelora -j examples/demo_api_errors_large.jsonl -F levelmap

# Act 2: Find top error endpoints
kelora -j examples/demo_api_errors_large.jsonl \
  --levels error \
  --metrics \
  --exec 'track_count(e.endpoint)'

# Drill into specific endpoint
kelora -j examples/demo_api_errors_large.jsonl \
  --filter 'e.endpoint == "/api/data"' \
  --metrics \
  --exec 'track_count(e.error ?? "unknown")'

# Act 3: Timeline with gap detection
kelora -j examples/demo_api_errors_large.jsonl \
  --filter 'e.status >= 500' \
  --mark-gaps 5m
```

## Video 3: Power User Tour

```bash
# Part 1: Basics
kelora examples/simple_json.jsonl
kelora -f combined examples/web_access.log
kelora -j examples/api_errors.jsonl --levels error,critical

# Part 2: Scripting
kelora -j examples/payments_latency.jsonl \
  --exec 'e.slow = e.duration_ms > 1000' \
  --filter 'e.slow'

kelora -j examples/payments_latency.jsonl \
  --metrics \
  --exec 'track_unique("regions", e.region)' \
  --exec 'track_sum("total_ms", e.duration_ms)'

kelora -j examples/payments_latency.jsonl \
  --window 100 \
  --end 'let times = window.pluck_as_nums("duration_ms");
         print("p95: " + times.percentile(95))'

# Part 3: Advanced
kelora -j examples/demo_api_errors_large.jsonl \
  --span 100 \
  --span-close 'let errors = span.events.filter(|e| e.level == "ERROR");
                if errors.len() > 10 {
                  print("Spike: " + errors.len() + " errors in span " + span.id)
                }'

kelora -j examples/api_errors.jsonl \
  --exec 'let metadata = read_file("examples/service_metadata.json").parse_json();
          let service = e.endpoint.extract_re(r"/api/(\w+)", 1);
          e.owner = metadata[service].owner ?? "unknown"'

kelora -j examples/api_errors.jsonl --filter 'e.level == "ERROR"' -F csv

# Part 4: Real-world integration
kelora --save-alias errors \
  --levels error \
  --metrics \
  --exec 'track_count(e.endpoint)'

kelora -j examples/api_errors.jsonl -a errors

# Exit codes for CI
kelora -j examples/api_errors.jsonl --levels error -qq
echo $?
```

## Video 4: Comparisons

```bash
# Task 1: Find errors with high latency
# Traditional
grep ERROR app.log | jq 'select(.response_ms > 1000)'

# Kelora
kelora -j examples/demo_api_errors_large.jsonl \
  --levels error \
  --filter 'e.response_ms > 1000'

# Task 2: Calculate percentiles
# jq (loads entire file)
jq -s '[.[].response_ms] | sort | .[length * 0.95 | floor]' examples/payments_latency.jsonl

# Kelora (streaming)
kelora -j examples/payments_latency.jsonl \
  --window 1000 \
  --end 'print(window.pluck_as_nums("duration_ms").percentile(95))'

# Task 3: Group and count
# Traditional
grep ERROR examples/demo_api_errors_large.jsonl | \
  jq -r '.endpoint' | sort | uniq -c

# Kelora
kelora -j examples/demo_api_errors_large.jsonl \
  --levels error \
  --metrics \
  --exec 'track_count(e.endpoint)'
```

## Tips for Live Demos

### Terminal Setup
```bash
# Use a large, readable font
# Minimal prompt
export PS1='$ '

# Clear screen before each demo
clear

# Show what you're typing (optional)
set -x  # Enable command echoing
set +x  # Disable command echoing
```

### Pacing
- Type commands slowly or use pre-prepared scripts
- Pause 2-3 seconds after output appears
- Use `| head -10` to limit output if needed
- Use `-c` (core fields only) for cleaner output

### Error Recovery
If a command fails:
1. Check the file path is correct
2. Ensure you're in the right directory
3. Have backup commands ready

### Making it Visual
```bash
# Add color highlighting
export KELORA_COLORS=1

# Pipe through less for scrolling
kelora ... | less -R

# Save output for comparison
kelora ... > output.txt

# Time commands to show performance
time kelora ...
```

## One-Liners for Social Media

```bash
# "Find errors in 100k lines - instantly"
kelora -j logs.jsonl --levels error

# "Calculate p95 latency - streaming"
kelora -j logs.jsonl --window 1000 --end 'print(window.pluck_as_nums("ms").percentile(95))'

# "Group errors by endpoint - one command"
kelora -j logs.jsonl --levels error --metrics --exec 'track_count(e.endpoint)'

# "Detect anomaly patterns - visually"
kelora -j logs.jsonl -F levelmap

# "Find time gaps - automatically"
kelora -j logs.jsonl --mark-gaps 5m
```

## Recording Commands

```bash
# Using asciinema (recommended)
asciinema rec demo.cast

# Convert to GIF
agg demo.cast demo.gif

# Convert to MP4 (with ffmpeg)
asciinema upload demo.cast  # Get URL
# Use third-party converter or:
docker run --rm -v $PWD:/data asciinema/asciicast2gif demo.cast demo.gif
```

## Troubleshooting

**Command not found:**
```bash
cargo install kelora
# or
./target/release/kelora
```

**File not found:**
```bash
ls examples/  # List available files
pwd           # Check current directory
```

**Output too verbose:**
```bash
# Add -c for core fields only
kelora -j file.jsonl -c

# Add -q to suppress diagnostics
kelora -j file.jsonl -q

# Add --head to limit output
kelora -j file.jsonl --head 20
```

**No color in output:**
```bash
# Force color
kelora -j file.jsonl --force-color

# Or disable color
kelora -j file.jsonl --no-color
```
