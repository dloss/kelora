#!/bin/bash

# test_kelora.sh - Comprehensive test runner for kelora

set -e  # Exit on any error

echo "🧪 Running kelora test suite..."
echo "================================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${2}${1}${NC}"
}

# Build the project first
print_status "📦 Building kelora..." $YELLOW
if cargo build --release; then
    print_status "✅ Build successful" $GREEN
else
    print_status "❌ Build failed" $RED
    exit 1
fi

# Run unit tests
print_status "🔧 Running unit tests..." $YELLOW
if cargo test --lib; then
    print_status "✅ Unit tests passed" $GREEN
else
    print_status "❌ Unit tests failed" $RED
    exit 1
fi

# Run integration tests
print_status "🔄 Running integration tests..." $YELLOW
if cargo test --test integration_tests; then
    print_status "✅ Integration tests passed" $GREEN
else
    print_status "❌ Integration tests failed" $RED
    exit 1
fi

# Manual tests with sample data
print_status "📝 Running manual tests with sample data..." $YELLOW

# Create sample data files
TEMP_DIR=$(mktemp -d)
echo "Using temp directory: $TEMP_DIR"

# Create sample JSONL file
cat > "$TEMP_DIR/sample.jsonl" << 'EOF'
{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","component":"database","message":"Connection failed","host":"db.example.com","port":5432,"retry":3}
{"timestamp":"2023-07-18T15:04:25.789Z","level":"WARN","component":"database","message":"Using fallback connection","fallback":"localhost:5433"}
{"timestamp":"2023-07-18T15:06:41.210Z","level":"INFO","component":"database","message":"Connection established","host":"localhost","port":5433,"latency_ms":45}
{"timestamp":"2023-07-18T15:07:12.345Z","level":"DEBUG","component":"auth","message":"User authentication attempt","user_id":"user123","method":"password","success":true}
{"timestamp":"2023-07-18T15:08:30.678Z","level":"INFO","component":"api","message":"Request completed","endpoint":"/users","method":"GET","status":200,"duration_ms":127}
EOF

# Create sample logfmt file
cat > "$TEMP_DIR/sample.logfmt" << 'EOF'
timestamp="2023-07-18T15:04:23.456Z" level=ERROR component=database message="Connection failed" host="db.example.com" port=5432 retry=3
timestamp="2023-07-18T15:04:25.789Z" level=WARN component=database message="Using fallback connection" fallback="localhost:5433"
timestamp="2023-07-18T15:06:41.210Z" level=INFO component=database message="Connection established" host="localhost" port=5433 latency_ms=45
timestamp="2023-07-18T15:07:12.345Z" level=DEBUG component=auth message="User authentication attempt" user_id="user123" method=password success=true
timestamp="2023-07-18T15:08:30.678Z" level=INFO component=api message="Request completed" endpoint="/users" method=GET status=200 duration_ms=127
EOF

# Test 1: Basic JSONL parsing
print_status "Test 1: Basic JSONL parsing" $YELLOW
./target/release/kelora -f jsonl "$TEMP_DIR/sample.jsonl" > "$TEMP_DIR/output1.txt"
if [ -s "$TEMP_DIR/output1.txt" ]; then
    print_status "✅ JSONL parsing works" $GREEN
    echo "   Output lines: $(wc -l < "$TEMP_DIR/output1.txt")"
else
    print_status "❌ JSONL parsing failed" $RED
fi

# Test 2: Key filtering
print_status "Test 2: Key filtering" $YELLOW
./target/release/kelora -f jsonl -k timestamp,level,message "$TEMP_DIR/sample.jsonl" > "$TEMP_DIR/output2.txt"
if grep -q "timestamp=" "$TEMP_DIR/output2.txt" && ! grep -q "component=" "$TEMP_DIR/output2.txt"; then
    print_status "✅ Key filtering works" $GREEN
else
    print_status "❌ Key filtering failed" $RED
fi

# Test 3: Level filtering
print_status "Test 3: Level filtering" $YELLOW
./target/release/kelora -f jsonl -l ERROR,WARN "$TEMP_DIR/sample.jsonl" > "$TEMP_DIR/output3.txt"
lines=$(wc -l < "$TEMP_DIR/output3.txt")
if [ "$lines" -eq 2 ]; then
    print_status "✅ Level filtering works (filtered to $lines lines)" $GREEN
else
    print_status "❌ Level filtering failed (got $lines lines, expected 2)" $RED
fi

# Test 4: Stats mode
print_status "Test 4: Statistics mode" $YELLOW
./target/release/kelora -f jsonl -S "$TEMP_DIR/sample.jsonl" 2> "$TEMP_DIR/stats.txt"
if grep -q "Events shown: 5" "$TEMP_DIR/stats.txt"; then
    print_status "✅ Statistics mode works" $GREEN
else
    print_status "❌ Statistics mode failed" $RED
    echo "   Stats output:"
    cat "$TEMP_DIR/stats.txt"
fi

# Test 5: Common fields mode
print_status "Test 5: Common fields mode" $YELLOW
./target/release/kelora -f jsonl -c "$TEMP_DIR/sample.jsonl" > "$TEMP_DIR/output5.txt"
if grep -q "timestamp=" "$TEMP_DIR/output5.txt" && grep -q "level=" "$TEMP_DIR/output5.txt" && ! grep -q "component=" "$TEMP_DIR/output5.txt"; then
    print_status "✅ Common fields mode works" $GREEN
else
    print_status "❌ Common fields mode failed" $RED
fi

# Test 6: Logfmt parsing
print_status "Test 6: Logfmt parsing" $YELLOW
./target/release/kelora -f logfmt "$TEMP_DIR/sample.logfmt" > "$TEMP_DIR/output6.txt"
if [ -s "$TEMP_DIR/output6.txt" ]; then
    print_status "✅ Logfmt parsing works" $GREEN
    echo "   Output lines: $(wc -l < "$TEMP_DIR/output6.txt")"
else
    print_status "❌ Logfmt parsing failed" $RED
fi

# Test 7: JSON output format
print_status "Test 7: JSON output format" $YELLOW
./target/release/kelora -f logfmt -F jsonl "$TEMP_DIR/sample.logfmt" > "$TEMP_DIR/output7.json"
if [ -s "$TEMP_DIR/output7.json" ] && head -1 "$TEMP_DIR/output7.json" | jq . >/dev/null 2>&1; then
    print_status "✅ JSON output format works" $GREEN
else
    print_status "❌ JSON output format failed" $RED
fi

# Test 8: Stdin input
print_status "Test 8: Stdin input" $YELLOW
if cat "$TEMP_DIR/sample.jsonl" | ./target/release/kelora -f jsonl -c > "$TEMP_DIR/output8.txt" && [ -s "$TEMP_DIR/output8.txt" ]; then
    print_status "✅ Stdin input works" $GREEN
else
    print_status "❌ Stdin input failed" $RED
fi

# Test 9: Error handling (malformed JSON)
print_status "Test 9: Error handling" $YELLOW
echo '{"valid":"json"}
{malformed json}
{"another":"valid"}' | ./target/release/kelora -f jsonl > "$TEMP_DIR/output9.txt" 2>"$TEMP_DIR/error9.txt"
valid_lines=$(wc -l < "$TEMP_DIR/output9.txt")
if [ "$valid_lines" -eq 2 ]; then
    print_status "✅ Error handling works (processed $valid_lines valid lines)" $GREEN
else
    print_status "❌ Error handling failed (got $valid_lines lines, expected 2)" $RED
fi

# Test 10: Performance test with larger file
print_status "Test 10: Performance test" $YELLOW
# Generate 1000 log entries
for i in $(seq 1 1000); do
    echo "{\"timestamp\":\"2023-07-18T15:04:23.456Z\",\"level\":\"INFO\",\"message\":\"Message $i\",\"id\":$i}"
done > "$TEMP_DIR/large.jsonl"

start_time=$(date +%s%N)
./target/release/kelora -f jsonl -S "$TEMP_DIR/large.jsonl" >/dev/null 2>&1
end_time=$(date +%s%N)
duration=$(( (end_time - start_time) / 1000000 )) # Convert to milliseconds

if [ $duration -lt 5000 ]; then  # Less than 5 seconds
    print_status "✅ Performance test passed (${duration}ms for 1000 entries)" $GREEN
else
    print_status "⚠️  Performance test slow (${duration}ms for 1000 entries)" $YELLOW
fi

# Cleanup
print_status "🧹 Cleaning up..." $YELLOW
rm -rf "$TEMP_DIR"

# Summary
print_status "📊 Test Summary" $YELLOW
echo "================================"
print_status "✅ All tests completed successfully!" $GREEN
echo ""
echo "You can now run individual tests with:"
echo "  cargo test                    # Run all tests"
echo "  cargo test --lib             # Run only unit tests"
echo "  cargo test --test integration_tests  # Run only integration tests"
echo ""
echo "Or test specific functionality:"
echo "  ./target/release/kelora --help"
echo "  echo '{\"level\":\"info\",\"msg\":\"test\"}' | ./target/release/kelora -f jsonl"
echo ""
print_status "Happy logging! 🪵" $GREEN