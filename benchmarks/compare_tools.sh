#!/usr/bin/env bash

# Kelora External Tool Comparison Benchmarks
# Compares Kelora against grep, awk, jq, miller, etc. for honest performance documentation

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Configuration
BENCHMARK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="./target/release/kelora"
RESULTS_DIR="${BENCHMARK_DIR}/comparison_results"
RUNS=3  # Number of runs per test

# Ensure results directory exists
mkdir -p "$RESULTS_DIR"

# Get system info
get_system_info() {
    local os=$(uname -s)
    local cpu=""
    local cores=""

    if [[ "$os" == "Darwin" ]]; then
        cpu=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "Unknown")
        cores=$(sysctl -n hw.ncpu 2>/dev/null || echo "Unknown")
    elif [[ "$os" == "Linux" ]]; then
        cpu=$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2 | xargs)
        cores=$(nproc)
    fi

    echo "- **OS:** $os"
    echo "- **CPU:** $cpu"
    echo "- **Cores:** $cores"
    echo "- **Date:** $(date '+%Y-%m-%d')"
}

# Check which tools are available
check_available_tools() {
    local tools=("$@")
    local available=()

    for tool in "${tools[@]}"; do
        if command -v "$tool" >/dev/null 2>&1; then
            available+=("$tool")
            echo -e "${GREEN}✓${NC} $tool $(command -v $tool)" >&2
        else
            echo -e "${YELLOW}✗${NC} $tool (not installed)" >&2
        fi
    done

    echo "${available[@]}"
}

# Ensure Kelora binary exists
check_kelora() {
    if [ ! -f "$BINARY" ]; then
        echo -e "${RED}Error: Kelora binary not found at $BINARY${NC}"
        echo "Please run: cargo build --release"
        exit 1
    fi
    echo -e "${GREEN}✓${NC} kelora $BINARY" >&2
}

# Generate benchmark datasets
ensure_datasets() {
    # JSON datasets (already exist from internal benchmarks)
    for size in 100k 500k; do
        local file="benchmarks/bench_${size}.jsonl"
        if [ ! -f "$file" ]; then
            local lines=${size//k/000}
            echo -e "${YELLOW}Generating $file...${NC}" >&2
            python3 benchmarks/generate_test_data.py "$lines" > "$file"
        fi
    done

    # CSV dataset for miller/qsv comparison
    if [ ! -f "benchmarks/bench_100k.csv" ]; then
        echo -e "${YELLOW}Generating CSV dataset...${NC}" >&2
        # Convert first 100k JSON to CSV
        head -100000 benchmarks/bench_100k.jsonl | \
            jq -r '[.timestamp, .level, .component, .message, .request_id, .host] | @csv' > benchmarks/bench_100k_temp.csv
        echo "timestamp,level,component,message,request_id,host" | cat - benchmarks/bench_100k_temp.csv > benchmarks/bench_100k.csv
        rm benchmarks/bench_100k_temp.csv
    fi

    # Syslog-style dataset for grep/awk comparison
    if [ ! -f "benchmarks/bench_100k.log" ]; then
        echo -e "${YELLOW}Generating syslog-style dataset...${NC}" >&2
        head -100000 benchmarks/bench_100k.jsonl | \
            jq -r '"\(.timestamp) \(.host) \(.component): \(.level) \(.message)"' > benchmarks/bench_100k.log
    fi

    echo -e "${GREEN}Datasets ready${NC}" >&2
}

# Time a command with multiple runs
time_command() {
    local description="$1"
    shift
    local cmd="$@"
    local times=()
    local total=0

    for i in $(seq 1 $RUNS); do
        if command -v gtime >/dev/null 2>&1; then
            # GNU time for better precision
            local result=$(gtime -f "%e" sh -c "$cmd" 2>&1 >/dev/null | tail -1)
        else
            # Fallback to bash time
            local start=$(date +%s.%N)
            eval "$cmd" >/dev/null 2>&1
            local end=$(date +%s.%N)
            local result=$(echo "$end - $start" | bc -l)
        fi
        times+=("$result")
        total=$(echo "$total + $result" | bc -l)
    done

    local avg=$(echo "scale=3; $total / $RUNS" | bc -l)
    local min=$(printf '%s\n' "${times[@]}" | sort -n | head -1)

    printf "%.3f" "$avg"
}

# Format results as markdown table row
format_result() {
    local tool="$1"
    local time="$2"
    local notes="$3"

    # If time is empty or error, mark as N/A
    if [ -z "$time" ] || [ "$time" = "0.000" ]; then
        echo "| $tool | N/A | $notes |"
    else
        echo "| $tool | ${time}s | $notes |"
    fi
}

# Benchmark 1: Simple grep-style filtering
benchmark_simple_filter() {
    echo -e "\n${BLUE}=== Benchmark 1: Simple Text Filtering ===${NC}"
    echo "Task: Find all ERROR lines in 100k line log file"
    echo ""

    local file="benchmarks/bench_100k.log"
    local results_file="$RESULTS_DIR/01_simple_filter.md"

    {
        echo "## Simple Text Filtering"
        echo ""
        echo "**Task:** Find all ERROR lines in 100k line log file"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # grep
    if command -v grep >/dev/null 2>&1; then
        echo -n "  Testing grep... " >&2
        local time=$(time_command "grep" "grep 'ERROR' $file")
        echo "${time}s" >&2
        format_result "grep" "$time" "Baseline - fastest text search" >> "$results_file"
    fi

    # ripgrep
    if command -v rg >/dev/null 2>&1; then
        echo -n "  Testing ripgrep... " >&2
        local time=$(time_command "rg" "rg 'ERROR' $file")
        echo "${time}s" >&2
        format_result "ripgrep (rg)" "$time" "Modern grep alternative" >> "$results_file"
    fi

    # kelora - line format (using grep-like pattern matching)
    echo -n "  Testing kelora (line)... " >&2
    local time=$(time_command "kelora-line" "$BINARY -f line $file --keep-lines 'ERROR' -q")
    echo "${time}s" >&2
    format_result "kelora (line)" "$time" "Full log parsing + structured output" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | parse "* * *[*]: *" as timestamp, host, component, level, message | where level == "ERROR"'
        local time=$(time_command "agrind-simple" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        format_result "angle-grinder" "$time" "parse + where level == ERROR" >> "$results_file"
    fi

    # klp
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp... " >&2
        local time=$(time_command "klp-simple" "klp --input-format line -l error $file")
        echo "${time}s" >&2
        format_result "klp" "$time" "line input + -l error" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 2: Field extraction
benchmark_field_extraction() {
    echo -e "\n${BLUE}=== Benchmark 2: Field Extraction ===${NC}"
    echo "Task: Extract timestamp, level, component from logs"
    echo ""

    local file="benchmarks/bench_100k.log"
    local results_file="$RESULTS_DIR/02_field_extraction.md"

    {
        echo "## Field Extraction"
        echo ""
        echo "**Task:** Extract timestamp, level, component from 100k log lines"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # awk
    if command -v awk >/dev/null 2>&1; then
        echo -n "  Testing awk... " >&2
        local time=$(time_command "awk" "awk '{print \$1, \$3, \$4}' $file")
        echo "${time}s" >&2
        format_result "awk" "$time" "Field splitting by whitespace" >> "$results_file"
    fi

    # kelora - cols format
    echo -n "  Testing kelora (cols)... " >&2
    local time=$(time_command "kelora-cols" "$BINARY -f 'cols:timestamp host component level *message' $file -k timestamp,level,component")
    echo "${time}s" >&2
    format_result "kelora (cols)" "$time" "Structured parsing + type awareness" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | parse "* * *[*]: *" as timestamp, host, component, level, message | fields + timestamp, level, component'
        local time=$(time_command "agrind-fields" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        format_result "angle-grinder" "$time" "parse + fields timestamp/level/component" >> "$results_file"
    fi

    # klp
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp... " >&2
        local klp_field_exec="m = re.match(r'(?P<timestamp>\\S+) (?P<host>\\S+) (?P<component>\\w+)\\[(?P<level>\\w+)\\]: (?P<message>.*)', line); _klp_event_add = m.groupdict() if m else {}"
        local time=$(time_command "klp-fields" "klp --input-format line --input-exec \"$klp_field_exec\" --keys timestamp,level,component $file")
        echo "${time}s" >&2
        format_result "klp" "$time" "line input + regex parse via --input-exec" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 3: JSON filtering
benchmark_json_filter() {
    echo -e "\n${BLUE}=== Benchmark 3: JSON Filtering ===${NC}"
    echo "Task: Filter JSON logs where level == ERROR"
    echo ""

    local file="benchmarks/bench_100k.jsonl"
    local results_file="$RESULTS_DIR/03_json_filter.md"

    {
        echo "## JSON Filtering"
        echo ""
        echo "**Task:** Filter 100k JSON logs where level == 'ERROR'"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # jq
    if command -v jq >/dev/null 2>&1; then
        echo -n "  Testing jq... " >&2
        local time=$(time_command "jq" "jq -c 'select(.level == \"ERROR\")' $file")
        echo "${time}s" >&2
        format_result "jq" "$time" "Standard JSON processor" >> "$results_file"
    fi

    # kelora
    echo -n "  Testing kelora... " >&2
    local time=$(time_command "kelora-json" "$BINARY -j $file -l error -F json -q")
    echo "${time}s" >&2
    format_result "kelora" "$time" "JSON parsing with level filter" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | json | where level == "ERROR"'
        local time=$(time_command "agrind-json" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        format_result "angle-grinder" "$time" "json + where level == ERROR" >> "$results_file"
    fi

    # klp
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp... " >&2
        local time=$(time_command "klp-json" "klp --input-format jsonl -l error $file")
        echo "${time}s" >&2
        format_result "klp" "$time" "jsonl input + -l error" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 4: JSON transformation
benchmark_json_transform() {
    echo -e "\n${BLUE}=== Benchmark 4: JSON Transformation ===${NC}"
    echo "Task: Extract nested fields + add computed field"
    echo ""

    local file="benchmarks/bench_100k.jsonl"
    local results_file="$RESULTS_DIR/04_json_transform.md"

    {
        echo "## JSON Transformation"
        echo ""
        echo "**Task:** Filter API logs, extract status code, add is_error field"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # jq
    if command -v jq >/dev/null 2>&1; then
        echo -n "  Testing jq... " >&2
        local time=$(time_command "jq-transform" "jq -c 'select(.component == \"api\") | {timestamp, method, status, is_error: (.status >= 400)}' $file")
        echo "${time}s" >&2
        format_result "jq" "$time" "Complex jq query" >> "$results_file"
    fi

    # kelora
    echo -n "  Testing kelora... " >&2
    local time=$(time_command "kelora-transform" "$BINARY -j $file --filter 'e.component == \"api\"' --exec 'e.is_error = (e.status.to_int() >= 400)' -k timestamp,method,status,is_error -q")
    echo "${time}s" >&2
    format_result "kelora" "$time" "Multi-stage pipeline" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | json | where component == "api" | (num(status) >= 400) as is_error | fields + timestamp, method, status, is_error'
        local time=$(time_command "agrind-transform" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        format_result "angle-grinder" "$time" "json + calc is_error + fields" >> "$results_file"
    fi

    # klp
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp... " >&2
        local klp_transform_exec="status_num = int(status or 0); _klp_event_add = {'is_error': status_num >= 400}"
        local time=$(time_command "klp-transform" "klp --input-format jsonl -l error --where 'component == \"api\"' --input-exec \"$klp_transform_exec\" --keys timestamp,method,status,is_error $file")
        echo "${time}s" >&2
        format_result "klp" "$time" "jsonl + -l error + computed key" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 5: Complex pipeline
benchmark_complex_pipeline() {
    echo -e "\n${BLUE}=== Benchmark 5: Complex Multi-Stage Pipeline ===${NC}"
    echo "Task: Parse → filter → aggregate → count by component"
    echo ""

    local file="benchmarks/bench_100k.jsonl"
    local results_file="$RESULTS_DIR/05_complex_pipeline.md"

    {
        echo "## Complex Pipeline"
        echo ""
        echo "**Task:** Filter errors, count by component, sort by frequency"
        echo ""
        echo "| Tool | Time | Command Complexity |"
        echo "|------|------|-------------------|"
    } > "$results_file"

    # bash pipeline
    if command -v jq >/dev/null 2>&1 && command -v sort >/dev/null 2>&1; then
        echo -n "  Testing bash pipeline... " >&2
        local time=$(time_command "bash-pipeline" "jq -r 'select(.level == \"ERROR\") | .component' $file | sort | uniq -c | sort -rn")
        echo "${time}s" >&2
        echo "| bash + jq + sort + uniq | ${time}s | \`jq ... \\| sort \\| uniq -c \\| sort -rn\` |" >> "$results_file"
    fi

    # kelora
    echo -n "  Testing kelora... " >&2
    local time=$(time_command "kelora-pipeline" "$BINARY -j $file -l error --exec 'track_freq("component", e.component)' --metrics")
    echo "${time}s" >&2
    echo "| kelora | ${time}s | \`kelora -l error --exec 'track_freq(...)' --metrics\` |" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | json | where level == "ERROR" | count by component | sort by _count desc'
        local time=$(time_command "agrind-pipeline" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        echo "| angle-grinder | ${time}s | \`agrind '* | json | where level == \"ERROR\" | count by component'\` |" >> "$results_file"
    fi

    # klp (piped aggregation)
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp pipeline... " >&2
        local time=$(time_command "klp-pipeline" "klp --input-format jsonl -l error --output-template '{component}' --plain --no-color $file | sort | uniq -c | sort -rn")
        echo "${time}s" >&2
        echo "| klp + sort + uniq | ${time}s | \`klp -l error --output-template '{component}' \\| sort \\| uniq -c \\| sort -rn\` |" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 6: Parallel processing
benchmark_parallel() {
    echo -e "\n${BLUE}=== Benchmark 6: Parallel Processing ===${NC}"
    echo "Task: Process 500k logs with aggregation"
    echo ""

    local file="benchmarks/bench_500k.jsonl"
    local results_file="$RESULTS_DIR/06_parallel.md"

    {
        echo "## Parallel Processing"
        echo ""
        echo "**Task:** Process 500k JSON logs, filter + count by component"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # jq (sequential)
    if command -v jq >/dev/null 2>&1; then
        echo -n "  Testing jq (sequential)... " >&2
        local time=$(time_command "jq-seq" "jq -c 'select(.component == \"api\")' $file | wc -l")
        echo "${time}s" >&2
        format_result "jq (sequential)" "$time" "Single-threaded processing" >> "$results_file"
    fi

    # kelora sequential
    echo -n "  Testing kelora (sequential)... " >&2
    local time=$(time_command "kelora-seq" "$BINARY -j $file --filter 'e.component == \"api\"' -F json -q")
    echo "${time}s" >&2
    format_result "kelora (sequential)" "$time" "Single-threaded baseline" >> "$results_file"

    # kelora parallel
    echo -n "  Testing kelora (parallel)... " >&2
    local time=$(time_command "kelora-par" "$BINARY -j $file --filter 'e.component == \"api\"' --parallel -F json -q")
    echo "${time}s" >&2
    format_result "kelora (--parallel)" "$time" "Multi-core processing" >> "$results_file"

    # angle-grinder
    if command -v agrind >/dev/null 2>&1; then
        echo -n "  Testing angle-grinder... " >&2
        local agrind_query='* | json | where component == "api"'
        local time=$(time_command "agrind-par" "agrind '$agrind_query' -f $file")
        echo "${time}s" >&2
        format_result "angle-grinder" "$time" "json + where component == api" >> "$results_file"
    fi

    # klp sequential
    if command -v klp >/dev/null 2>&1; then
        echo -n "  Testing klp (sequential)... " >&2
        local time=$(time_command "klp-seq" "klp --input-format jsonl --where 'component == \"api\"' $file")
        echo "${time}s" >&2
        format_result "klp (sequential)" "$time" "jsonl input + --where" >> "$results_file"

        echo -n "  Testing klp (parallel)... " >&2
        local time=$(time_command "klp-par" "klp --input-format jsonl --where 'component == \"api\"' --parallel 0 $file")
        echo "${time}s" >&2
        format_result "klp (--parallel 0)" "$time" "Multiprocess (--parallel 0)" >> "$results_file"
    fi

    echo "" >> "$results_file"
    cat "$results_file"
}

# Benchmark 7: CSV processing
benchmark_csv() {
    echo -e "\n${BLUE}=== Benchmark 7: CSV Processing ===${NC}"
    echo "Task: Filter and aggregate CSV data"
    echo ""

    local file="benchmarks/bench_100k.csv"
    local results_file="$RESULTS_DIR/07_csv.md"

    {
        echo "## CSV Processing"
        echo ""
        echo "**Task:** Filter CSV by level, select columns"
        echo ""
        echo "| Tool | Time | Notes |"
        echo "|------|------|-------|"
    } > "$results_file"

    # miller
    if command -v mlr >/dev/null 2>&1; then
        echo -n "  Testing miller... " >&2
        local time=$(time_command "miller" "mlr --csv filter '\$level == \"ERROR\"' then cut -f timestamp,level,component $file")
        echo "${time}s" >&2
        format_result "miller" "$time" "CSV swiss army knife" >> "$results_file"
    fi

    # qsv
    if command -v qsv >/dev/null 2>&1; then
        echo -n "  Testing qsv... " >&2
        local time=$(time_command "qsv" "qsv search -s level 'ERROR' $file | qsv select timestamp,level,component")
        echo "${time}s" >&2
        format_result "qsv" "$time" "High-performance CSV tool" >> "$results_file"
    fi

    # kelora
    echo -n "  Testing kelora... " >&2
    local time=$(time_command "kelora-csv" "$BINARY -f csv $file -l error -k timestamp,level,component -q")
    echo "${time}s" >&2
    format_result "kelora" "$time" "CSV + level filtering" >> "$results_file"

    echo "" >> "$results_file"
    cat "$results_file"
}

# Main benchmark runner
main() {
    echo -e "${GREEN}=== Kelora External Tool Comparison Benchmarks ===${NC}"
    echo ""

    echo "System Information:"
    get_system_info
    echo ""

    echo "Checking available tools:"
check_kelora
local available=$(check_available_tools grep rg awk sed jq mlr qsv agrind klp)
    echo ""

    echo "Preparing datasets..."
    ensure_datasets
    echo ""

    # Run benchmarks
    benchmark_simple_filter
    benchmark_field_extraction
    benchmark_json_filter
    benchmark_json_transform
    benchmark_complex_pipeline
    benchmark_parallel

    # Only run CSV benchmarks if tools are available
    if [ -f "benchmarks/bench_100k.csv" ]; then
        benchmark_csv
    fi

    echo -e "\n${GREEN}=== Benchmark Complete ===${NC}"
    echo "Results saved to: $RESULTS_DIR/"
    echo ""
    echo "To add your results to the documentation:"
    echo "  1. Copy the system info and benchmark tables from $RESULTS_DIR/"
    echo "  2. Add a new section to docs/concepts/benchmark-results.md"
    echo "  3. Include your CPU, OS, and date"
    echo ""
    echo "Quick preview of all results:"
    echo "  cat $RESULTS_DIR/*.md"
}

main "$@"
