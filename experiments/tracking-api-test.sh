#!/bin/bash
# Experiment: Test current tracking API behavior

set -e

KELORA="cargo run --release --"

echo "=== Experiment 1: Same key, different operations ==="
echo "What happens when we track_min and track_max with the same key?"
echo ""

cat > /tmp/test.jsonl << 'EOF'
{"value": 10}
{"value": 50}
{"value": 30}
EOF

echo "Test 1a: track_min only"
$KELORA -f json /tmp/test.jsonl \
  --exec 'track_min("latency", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "Test 1b: track_max only"
$KELORA -f json /tmp/test.jsonl \
  --exec 'track_max("latency", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "Test 1c: Both track_min AND track_max with SAME key (should conflict)"
$KELORA -f json /tmp/test.jsonl \
  --exec 'track_min("latency", e.value); track_max("latency", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "Test 1d: Both track_min AND track_max with DIFFERENT keys (should work)"
$KELORA -f json /tmp/test.jsonl \
  --exec 'track_min("latency_min", e.value); track_max("latency_max", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "=== Experiment 2: Parallel mode behavior ==="
echo "Does the same key conflict happen in parallel mode?"
echo ""

echo "Test 2a: Parallel with same key"
$KELORA -f json /tmp/test.jsonl --parallel \
  --exec 'track_min("latency", e.value); track_max("latency", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "Test 2b: Parallel with different keys"
$KELORA -f json /tmp/test.jsonl --parallel \
  --exec 'track_min("latency_min", e.value); track_max("latency_max", e.value)' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "=== Experiment 3: Multiple stats for same concept ==="
echo "Common use case: I want min, max, avg, count of latency"
echo ""

cat > /tmp/test2.jsonl << 'EOF'
{"latency": 100}
{"latency": 200}
{"latency": 150}
{"latency": 300}
{"latency": 50}
EOF

echo "Test 3a: Track all stats with manual suffixes"
$KELORA -f json /tmp/test2.jsonl \
  --exec '
    track_min("latency_min", e.latency);
    track_max("latency_max", e.latency);
    track_avg("latency_avg", e.latency);
    track_count("latency_count")
  ' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "Test 3b: What if user forgets suffixes? (should be wrong)"
$KELORA -f json /tmp/test2.jsonl \
  --exec '
    track_min("latency", e.latency);
    track_max("latency", e.latency);
    track_avg("latency", e.latency)
  ' \
  --metrics -q 2>&1 | grep -E "latency|^$"

echo ""
echo "=== Experiment 4: Check internal operation metadata ==="
echo "Let's see what the __op_ metadata looks like"
echo ""

# This would require modifying the code to expose internal state
# For now, we can infer from merge behavior

echo "Test 4: Create a custom script that shows the issue"
cat > /tmp/show-conflict.rhai << 'EOF'
// This should demonstrate the conflict
track_min("metric", 100);
print("After track_min");

track_max("metric", 50);
print("After track_max - what's the value now?");
EOF

$KELORA -f json /tmp/test.jsonl \
  --include /tmp/show-conflict.rhai \
  --metrics -q 2>&1 | grep -E "metric|After|^$" || true

echo ""
echo "=== Summary ==="
echo "1. Same key with different operations = last operation wins"
echo "2. Users MUST manually add suffixes (_min, _max, _avg)"
echo "3. Verbose and error-prone"
echo "4. Should we auto-suffix to prevent conflicts?"
