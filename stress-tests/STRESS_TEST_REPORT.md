# Kelora Stress Test Report

**Date**: 2026-01-01
**Version**: 1.3.0
**Tester**: Automated QA

## Executive Summary

Stress testing identified **1 critical bug**, **2 moderate issues**, and several **minor observations**. The tool handles most edge cases gracefully, but has a significant issue with Rhai infinite loops that can hang the process indefinitely.

---

## Critical Issues

### 1. Infinite Loops in Rhai Scripts Cannot Be Terminated (CRITICAL)

**Severity**: Critical
**Impact**: Denial of Service / Process Hang

**Description**: Rhai scripts containing infinite loops (`loop {}` or `while true {}`) cannot be terminated even with SIGTERM. The process displays "Received SIGTERM, shutting down gracefully..." but continues running indefinitely.

**Reproduction**:
```bash
echo '{"msg": "test"}' | timeout 3 ./target/release/kelora -f json --exec 'loop { }'
# Process hangs, ignores timeout/SIGTERM
```

**Expected Behavior**: Process should terminate within reasonable time after receiving SIGTERM.

**Suggested Fix**:
- Add Rhai execution timeout (e.g., `Engine::set_max_operations()`)
- Implement cooperative cancellation check in long-running scripts
- Add `--script-timeout` CLI option

---

## Moderate Issues

### 2. Window Size Off-by-One Behavior

**Severity**: Moderate
**Impact**: Confusing UX

**Description**: `--window N` appears to keep N+1 events in the window, not N events.

**Reproduction**:
```bash
printf '{"val": 1}\n{"val": 2}\n{"val": 3}\n{"val": 4}\n{"val": 5}' > /tmp/test.json
./target/release/kelora -f json /tmp/test.json --window 3 --exec 'e.w = len(window)'
# Output shows w=4 for later events, not w=3
```

**Expected**: `--window 3` should maintain a window of exactly 3 events.

### 3. Invalid Regex Silently Returns Empty Result

**Severity**: Moderate
**Impact**: Silent failures in scripts

**Description**: When `extract_regex()` is called with an invalid regex pattern, it silently returns an empty result instead of raising an error.

**Reproduction**:
```bash
echo '{"msg": "test"}' | ./target/release/kelora -f json --exec 'extract_regex(e.msg, "[invalid")'
# No error, just continues with empty result
```

**Expected**: Should report a regex compilation error.

---

## Minor Observations / Potential Improvements

### 4. null + string Concatenation Coerces to String

```bash
echo '{"msg": null}' | ./target/release/kelora -f json --exec 'e.msg = e.msg + "test"'
# Output: msg='test' (null becomes empty string)
```
This may be intentional, but could be surprising. Consider documenting this behavior.

### 5. Accessing Deeply Nested Nonexistent Properties Errors

```bash
echo '{"a":1}' | ./target/release/kelora -f json --filter 'e.nonexistent.deeply.nested == 1'
# Error: Unknown property 'deeply' - a getter is not registered for type '()'
```
This is correct error handling. Consider adding a `get_path()` or safe navigation operator for optional deep access.

### 6. CSV Output Requires --keys (Good Validation)

```bash
echo '{"a":1}' | ./target/release/kelora -f json -F csv
# Error: CSV output format requires --keys to specify field order
```
This is proper validation. No issue here.

### 7. Integer Overflow Properly Caught

```bash
echo '{"val": 9223372036854775807}' | ./target/release/kelora -f json --exec 'e.result = e.val + 1'
# Error: Addition overflow: 9223372036854775807 + 1
```
Good error handling.

### 8. Division by Zero Properly Caught

```bash
echo '{"val": 0}' | ./target/release/kelora -f json --exec 'e.result = 10 / e.val'
# Error: Division by zero: 10 / 0
```
Good error handling.

---

## Positive Findings (Robustness Confirmed)

The following edge cases are handled correctly:

| Test Case | Result |
|-----------|--------|
| Empty input | Exits cleanly with code 0 |
| Malformed JSON in stream | Continues processing, reports error count |
| 100-level nested JSON | Parses successfully |
| 1MB string value | Handles correctly |
| 10,000 JSON keys | Handles correctly |
| 100,000 element array | Handles correctly |
| Unicode (emoji, CJK, RTL) | Handles correctly |
| Null bytes in JSON string | Rejected with clear error |
| 100 parallel threads | Works correctly |
| Corrupted gzip file | Proper error message |
| Symlink loop | Proper error (os error 40) |
| Non-existent input file | Proper error message |
| Non-existent output directory | Proper error with suggestion |
| CSV with unclosed quotes | Parses gracefully |
| Syslog with invalid values | Rejected with clear error |
| --strict mode | Stops on first error as expected |
| SIGTERM during normal operation | Shuts down gracefully |

---

## Test Environment

- Platform: Linux 4.4.0
- Rust: release build
- Input methods: file, stdin
- Tested parsers: json, csv, logfmt, syslog, cef, regex, line

---

## Recommendations

1. **Priority 1**: Fix Rhai infinite loop termination issue
2. **Priority 2**: Clarify or fix window size semantics
3. **Priority 3**: Add proper error handling for invalid regex patterns
4. **Consider**: Add `--script-timeout` option for untrusted scripts
5. **Consider**: Add `--max-line-length` option to prevent memory issues with extremely long lines
6. **Consider**: Document null coercion behavior in scripting guide

---

## Appendix: Commands Used

```bash
# Build
cargo build --release

# Infinite loop test (CRITICAL BUG)
echo '{"msg": "test"}' | timeout 3 ./target/release/kelora -f json --exec 'loop { }'

# Edge case tests
echo '{"a":1}' | ./target/release/kelora -f json --filter 'e.nonexistent.deeply.nested == 1'
python3 -c "..." | ./target/release/kelora -f json  # Various large input tests

# Format parser tests
echo 'CEF:0|...' | ./target/release/kelora -f cef
echo '<999>...' | ./target/release/kelora -f syslog

# Parallel processing
seq 1 100 | sed 's/.*/{"n": &}/' | ./target/release/kelora -f json --parallel --threads 100
```
