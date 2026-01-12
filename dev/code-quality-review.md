# Code Quality Review - Pending Items

Quick identification of remaining issues needing attention.

## 🟠 Serious Issues

### 1. Monolithic Files

- `src/rhai_functions/strings.rs`: **6,550 lines** (3x recommended max)
- `src/parallel.rs`: 4,011 lines
- `src/engine.rs`: 3,203 lines
- `src/main.rs`: 3,109 lines

**Recommendation:** Split `strings.rs` into modules (basic, search, transform, parsing, format)

**Effort:** 8-12h

---

### 2. Too Many Parameters (Remaining)

**Remaining work:**
- `src/parallel.rs:1642` - `file_aware_batcher_thread` (13 params)
- `src/parallel.rs:2602, 2811` (8+ params each)
- `src/main.rs:1120` (12+ params)
- `src/rhai_functions/tracking.rs:159` (9+ params - has justifying comment)

**Recommendation:** Create context/config structs following patterns established for `handle_file_aware_line` and `batcher_thread`.

**Effort:** 4-6h

---

### 3. Excessive Cloning

**Stats:**
- Total codebase: 1,094 `.clone()` calls
- `strings.rs`: 189 clones
- `parallel.rs`: 143 clones
- `tracking.rs`: 106 clones

**Action:** Profile with `cargo flamegraph`, then optimize hot paths.

**Effort:** 8-16h (depends on profiling)

---

## 🟡 Moderate Issues

### 4. Global Mutable State

`src/rhai_functions/file_ops.rs:38-52` - 5 global statics make testing difficult. Also `CSV_FORMATTER_HEADER_REGISTRY` in `formatters.rs:1667`.

**Fix:** Use dependency injection instead of globals.

**Effort:** 6-10h

---

### 5. Integration Tests

Missing tests for:
- Mutex poisoning recovery
- Thread panic scenarios
- OOM handling
- Non-UTF-8 paths

**Effort:** 8-12h

---

## ✅ Positive Findings

- 52 source files with `#[test]` markers
- No TODO/FIXME in source
- Active clippy linting
- Comprehensive --help system
- Security audits (cargo audit/deny)

---

## 📋 Recommended Order

**Phase 1 (Maintainability):**
1. Reduce remaining function parameters (4-6h)
2. Split large files (8-12h)

**Phase 2 (Performance):**
3. Profile and optimize clones (8-16h)

**Phase 3 (Quality):**
4. Global state refactoring (6-10h)
5. Integration tests (8-12h)

**Total:** 34-56 hours (~1-2 months part-time)

---

## Recent Fixes (Completed)

- ✅ Mutex poison handling with graceful recovery
- ✅ Thread join error context improvements
- ✅ Clone-on-array-length optimization
- ✅ Top/bottom duplication elimination
- ✅ Complex merge function extraction
- ✅ Panic as control flow fixed (state.rs)
- ✅ Edge case unwrap safety (timestamp.rs)
- ✅ Function parameter reduction for `handle_file_aware_line` and `batcher_thread`
