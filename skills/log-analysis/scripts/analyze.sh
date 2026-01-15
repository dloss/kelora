#!/usr/bin/env bash
# Quick log analysis script using Kelora
# Usage: analyze.sh <logfile> [options]
#
# Options:
#   --format <fmt>   Input format (auto, json, logfmt, syslog, combined, csv)
#   --errors         Focus on error analysis
#   --latency        Focus on latency analysis
#   --patterns       Discover message patterns (Drain)
#   --full           Run all analyses
#   --json           Output results as JSON

set -euo pipefail

LOGFILE=""
FORMAT="auto"
MODE="summary"
OUTPUT_JSON=false

usage() {
    cat <<EOF
Usage: $(basename "$0") <logfile> [options]

Quick log analysis using Kelora.

Options:
  --format <fmt>   Input format (auto, json, logfmt, syslog, combined, csv)
  --errors         Focus on error analysis
  --latency        Focus on latency analysis
  --patterns       Discover message patterns (Drain algorithm)
  --full           Run all analyses
  --json           Output results as JSON
  -h, --help       Show this help

Examples:
  $(basename "$0") app.log
  $(basename "$0") api.jsonl --format json --latency
  $(basename "$0") access.log --format combined --full
EOF
    exit 0
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            usage
            ;;
        --format)
            FORMAT="$2"
            shift 2
            ;;
        --errors)
            MODE="errors"
            shift
            ;;
        --latency)
            MODE="latency"
            shift
            ;;
        --patterns)
            MODE="patterns"
            shift
            ;;
        --full)
            MODE="full"
            shift
            ;;
        --json)
            OUTPUT_JSON=true
            shift
            ;;
        -*)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
        *)
            LOGFILE="$1"
            shift
            ;;
    esac
done

if [[ -z "$LOGFILE" ]]; then
    echo "Error: No log file specified" >&2
    usage
fi

if [[ ! -f "$LOGFILE" ]]; then
    echo "Error: File not found: $LOGFILE" >&2
    exit 1
fi

# Check if kelora is available
if ! command -v kelora &> /dev/null; then
    echo "Error: kelora command not found" >&2
    exit 1
fi

FORMAT_ARG="-f $FORMAT"

summary_analysis() {
    echo "=== Summary Statistics ==="
    kelora $FORMAT_ARG -s "$LOGFILE"
    echo ""
    echo "=== Detailed Metrics ==="
    kelora $FORMAT_ARG -m "$LOGFILE"
}

error_analysis() {
    echo "=== Error Analysis ==="
    echo ""
    echo "--- Error Count by Level ---"
    kelora $FORMAT_ARG -q --metrics -e 'track_count("level:" + e.level)' "$LOGFILE"
    echo ""
    echo "--- Recent Errors (last 20) ---"
    kelora $FORMAT_ARG -l ERROR,FATAL,CRITICAL -n 20 "$LOGFILE"
    echo ""
    echo "--- Error Patterns ---"
    kelora $FORMAT_ARG -l ERROR,FATAL,CRITICAL --drain "$LOGFILE" 2>/dev/null || echo "(No error patterns found)"
}

latency_analysis() {
    echo "=== Latency Analysis ==="
    echo ""
    echo "--- Duration Metrics ---"
    kelora $FORMAT_ARG -q --metrics -e '
        if e.has("duration") || e.has("duration_ms") || e.has("latency") || e.has("ms") {
            let val = if e.has("duration") { e.duration }
                      else if e.has("duration_ms") { e.duration_ms }
                      else if e.has("latency") { e.latency }
                      else { e.ms };
            if type_of(val) == "i64" || type_of(val) == "f64" {
                track_avg("avg_duration", val);
                track_min("min_duration", val);
                track_max("max_duration", val);
                track_percentiles("duration_pct", val, [50, 90, 95, 99]);
            }
        }
    ' "$LOGFILE"
    echo ""
    echo "--- Slowest Requests (>1s, last 10) ---"
    kelora $FORMAT_ARG --filter 'e.has("duration") && e.duration > 1000 || e.has("duration_ms") && e.duration_ms > 1000 || e.has("latency") && e.latency > 1000' -n 10 "$LOGFILE" 2>/dev/null || echo "(No slow requests found)"
}

pattern_analysis() {
    echo "=== Pattern Analysis (Drain) ==="
    echo ""
    kelora $FORMAT_ARG --drain full "$LOGFILE"
}

full_analysis() {
    summary_analysis
    echo ""
    echo "========================================"
    echo ""
    error_analysis
    echo ""
    echo "========================================"
    echo ""
    latency_analysis
    echo ""
    echo "========================================"
    echo ""
    pattern_analysis
}

json_output() {
    kelora $FORMAT_ARG -m json "$LOGFILE"
}

# Run analysis based on mode
if $OUTPUT_JSON; then
    json_output
else
    case $MODE in
        summary)
            summary_analysis
            ;;
        errors)
            error_analysis
            ;;
        latency)
            latency_analysis
            ;;
        patterns)
            pattern_analysis
            ;;
        full)
            full_analysis
            ;;
    esac
fi
