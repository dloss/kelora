# Verbose/Transparency Redesign: Separate Concerns

## Rationale

The current `-v/-vv/-vvv` flags serve a single purpose: error verbosity. While this works well for its intended use case, users have two additional needs that aren't currently addressed:

1. **Learning/Transparency**: "What is Kelora doing? What does my pipeline look like? Which stages are dropping events?"
2. **Targeted Debugging**: "Why isn't event #237 passing my filter? Why doesn't field X appear after my exec stage?"

The original verbose redesign attempted to solve all three problems (errors, learning, debugging) by overloading the `-v` levels with sampling and progress blocks. This created several issues:

- **Sampling gaps**: Showing only every 100th event means missing the specific event a user is debugging
- **Mixed concerns**: Combining error output, progress indicators, and pipeline transparency into `-v` levels makes each use case noisier
- **Unclear intent**: Does `-vv` mean "show me errors in detail" or "teach me how Kelora works" or "debug this specific event"?
- **Reinventing progress**: Kelora already supports SIGUSR1/CTRL-T for on-demand stats (zero-noise, proven UX pattern)

**Solution**: Separate the three concerns into orthogonal features:
- `-v/-vv/-vvv` = error verbosity (current behavior, proven and understood)
- `--explain` = learning/transparency (enhance SIGUSR1/CTRL-T with per-stage stats, show startup config)
- `--trace=<selector>` = targeted debugging (show specific events' pipeline journeys)

This allows users to compose what they need: `-v` for errors, `--explain` to understand the pipeline, `--trace=drops` to debug filtering logic. Progress checking uses the existing SIGUSR1/CTRL-T mechanism (no automatic output).

---

## Design

### 1. `-v/-vv/-vvv`: Error Verbosity (unchanged)

**Keep current behavior** - these flags control how much detail to show when errors occur:

- **`-v`**: Basic error messages (line number, error type, message)
- **`-vv`**: Errors + original line content (for parse errors)
- **`-vvv`**: Errors + line statistics (length, encoding details, control chars) + Rhai ExecutionTracer

**Why keep it:**
- Error handling is the most common debugging need
- Current behavior is well-understood and documented
- Natural volume control (errors are inherently bounded in well-formed logs)
- Works well in both sequential and parallel modes

**Interaction with new features:**
- Honors existing `--silent`, `--no-diagnostics`, `--no-emoji` flags
- Can be combined with `--explain` and/or `--trace`
- Independent of pipeline transparency and event tracing

---

### 2. `--explain`: Pipeline Transparency (new)

**Purpose**: Help users understand what Kelora is doing with their configuration.

**When to use:**
- Learning how Kelora interprets your config
- Understanding which stages are in the pipeline and in what order
- Seeing aggregate statistics: which filters drop events, which stages succeed/fail
- Debugging pipeline construction (not individual event behavior)
- Checking progress during long-running jobs (via SIGUSR1/CTRL-T)

**Progress checking**: Kelora already supports SIGUSR1 (all Unix) and SIGINFO/CTRL-T (BSD/macOS) for on-demand stats. When `--explain` is enabled, these signals show enhanced output including per-stage breakdowns.

#### Output: Startup Plan

Printed once at beginning, shows resolved configuration:

```
🔹 Kelora Pipeline Configuration
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Input:
  Format: json (auto-detected from content)
  Files: app.log (234 MB), api.log (89 MB)
  Multiline: none
  Timestamp: field 'timestamp', format ISO8601, timezone UTC

Pipeline Stages (execution order):
  #1 filter: e.level != "DEBUG"
  #2 exec: e.duration_ms = e.end_time - e.start_time
  #3 filter: e.duration_ms > 1000
  #4 emit

Output:
  Formatter: json
  Keys: level, message, duration_ms, timestamp
  Context: none
  Take limit: none

Mode: sequential (1 thread)
  Use --parallel for multi-core processing

Span tracking: disabled
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

**Details:**
- Show detected vs explicit format (e.g., "json (auto-detected)" vs "json (--format)")
- List all stages with stable IDs (#1, #2, etc.) matching runtime order
- Show truncated expressions (80 chars) for filters/exec/map stages
- Include key config: multiline strategy, timestamp parsing, take/head limits, context config
- Indicate sequential vs parallel mode and thread count
- Show span configuration if active

#### Output: Stage Statistics

Printed at end of processing, or on-demand via SIGUSR1/CTRL-T:

```
🔹 Pipeline Statistics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Input:
  Lines read: 45,234
  Events parsed: 45,102
  Parse errors: 132 (0.29%)

Stage Results:
  #1 filter (e.level != "DEBUG"):
     ✓ passed: 12,340 (27.4%)
     ✗ dropped: 32,762 (72.6%)

  #2 exec (e.duration_ms = ...):
     ✓ ok: 12,340 (100%)
     ⚠ errors: 0 (0%)

  #3 filter (e.duration_ms > 1000):
     ✓ passed: 1,523 (12.3%)
     ✗ dropped: 10,817 (87.7%)

  #4 emit:
     Events emitted: 1,523

Summary:
  Total events emitted: 1,523 (3.4% of parsed)
  Processing time: 2.3s
  Throughput: 19,609 events/sec
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

**Details:**
- Show stage-level pass/drop/error counts with percentages
- Percentages are relative to events entering that stage
- Include top 3 error types per stage if errors occurred
- Show final emit count and overall yield percentage
- Include basic throughput metrics (not detailed timing per stage)
- In parallel mode, aggregate across all workers

#### Implementation Notes

**Data collection:**
- Startup plan: gather from `PipelineConfig` and resolved settings (no hot-path overhead)
- Stage statistics: use existing `__kelora_stage_*` tracking infrastructure
  - Add `__kelora_stage_pass_<id>` counter
  - Add `__kelora_stage_drop_<id>` counter
  - Add `__kelora_stage_error_<id>` counter
  - Reducer sums these automatically in parallel mode
- Minimal hot-path cost: one counter increment per stage per event

**Suppression:**
- Honors `--silent` (suppress all output including startup plan and stats)
- Honors `-m` and `-s` (suppress startup plan, show final stats)
- Honors `--no-diagnostics` (suppress final stats, show startup plan only)
- Honors `--no-emoji` and `NO_EMOJI` env var

**Integration with SIGUSR1/CTRL-T:**
- **Without `--explain`** (current behavior): SIGUSR1/CTRL-T shows global stats (events, throughput, timestamps)
- **With `--explain`**: SIGUSR1/CTRL-T shows global stats PLUS per-stage breakdown
- Zero noise by default - stats only appear when user requests them (signal) or at end
- Useful for long-running jobs and slow streaming: "Press CTRL-T to check progress"

---

### 3. `--trace=<selector>`: Targeted Event Debugging (new)

**Purpose**: Show detailed pipeline journey for specific events you care about.

**When to use:**
- Debugging why a specific event is dropped/modified
- Understanding why expected fields don't appear after transformations
- Investigating filter or exec logic for particular cases
- Seeing exactly what happens to events matching certain criteria

#### Selector Syntax

```bash
# Trace specific input line numbers
--trace=line:1043
--trace=line:1000-2000

# Trace specific event IDs (shown in output with --explain)
--trace=event:237
--trace=event:100,237,1043

# Trace events matching a Rhai filter expression
--trace='e.status >= 500'
--trace='e.level == "ERROR" && e.user == "admin"'

# Trace all dropped events (events that don't reach emit)
--trace=drops

# Trace all events that encounter errors in any stage
--trace=errors

# Trace first K events and every Mth thereafter (sampling)
--trace=sample:5:100
```

#### Output Format

For each traced event, show its complete journey through the pipeline:

```
━━━ Event #237 (app.log:1043) ━━━
Input: {"level":"ERROR","status":500,"msg":"Database timeout","start":100,"end":null}

Stage #1 filter (e.level != "DEBUG"): PASS
  Condition: true
  → Event unchanged

Stage #2 exec (e.duration_ms = e.end_time - e.start_time): ERROR
  ⚠ Runtime error: Field 'end_time' is null
  → e.duration_ms not set

Stage #3 filter (e.duration_ms > 1000): FAIL
  Condition: false (duration_ms is null)
  → Event dropped

✗ Event dropped at stage #3 (will not be emitted)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

━━━ Event #238 (app.log:1044) ━━━
Input: {"level":"ERROR","status":500,"msg":"Connection refused","start":100,"end":150}

Stage #1 filter (e.level != "DEBUG"): PASS
  Condition: true
  → Event unchanged

Stage #2 exec (e.duration_ms = e.end_time - e.start_time): OK
  → e.duration_ms = 50

Stage #3 filter (e.duration_ms > 1000): FAIL
  Condition: false (50 > 1000)
  → Event dropped

✗ Event dropped at stage #3 (will not be emitted)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

**Details:**
- Show input line with full event content (subject to `--max-field-length` truncation)
- For each stage:
  - Stage ID + type + truncated expression (matches `--explain` output)
  - Outcome: PASS/FAIL/OK/ERROR/DROP
  - For filters: show boolean result and why
  - For exec/map: show what changed (added/removed/modified keys, up to 8 keys)
  - For errors: show error message
- Show final disposition: emitted or dropped (and which stage dropped it)
- In parallel mode: include worker tag (e.g., "Event #237 [worker 2]")

#### Detailed Stage Outcomes

**Filter stages:**
```
Stage #1 filter (e.level != "DEBUG"): PASS
  Condition: true
  → Event unchanged
```

**Exec stages (successful):**
```
Stage #2 exec (...): OK
  Added keys: duration_ms
  → e.duration_ms = 50
```

**Exec stages (with errors):**
```
Stage #2 exec (...): ERROR
  ⚠ Runtime error: Field 'end_time' is null
  Failed to set: duration_ms
  → Event unchanged (continues to next stage)
```

**Map stages:**
```
Stage #3 map (#{level: e.level, ...}): OK
  Event replaced with new map
  Keys: level, message, duration_ms (3 keys)
  → Output: {"level":"ERROR","message":"...","duration_ms":50}
```

**Drop stages:**
```
Stage #4 drop: DROPPED
  → Event removed from pipeline
```

**Emit stage:**
```
Stage #5 emit: EMITTED
  Formatter: json
  Output: {"level":"ERROR","message":"Database timeout","duration_ms":50}
```

#### Implementation Notes

**Selector parsing:**
- Parse selector at startup, validate syntax
- For filter expressions, compile Rhai expression once
- Store selector type (line, event, filter, drops, errors, sample)

**Hot-path overhead:**
- For each event, check if it matches selector (minimal cost)
- If matched, set `is_traced` flag on event metadata
- Each stage checks `is_traced` and collects stage outcome if true
- Stage outcome: enum { Pass, Fail, Ok, Error(msg), Dropped }
- Collect minimal data: outcome + changed keys (not full event copies)
- At end of pipeline, format and print trace for traced events

**Parallel mode:**
- Event IDs are global monotonic (assigned by coordinator)
- Workers check selector independently (deterministic from event ID/content)
- Traced events buffer stage outcomes locally
- Send complete trace to coordinator via stderr capture (ordered by event ID)
- Coordinator prints traces in event ID order for readability

**Suppression:**
- Honors `--silent` (suppress trace output)
- Honors `--no-diagnostics` (still show traces, only progress/stats suppressed)
- Honors `--no-emoji` in trace formatting
- NOT suppressed by `-q` (traces are debugging output, not event output)

**Volume control:**
- NO automatic sampling (sampling is only via explicit `--trace=sample:K:M`)
- User is responsible for choosing appropriate selector
- Warning if selector matches >10,000 events: "⚠️ Tracing 15,234 events. Output may be large. Use --trace=sample:K:M to limit."
- Can combine with `--head` or `--take` to limit input/output

---

## Progress Monitoring (Existing Feature)

Kelora already supports **on-demand progress checking** via Unix signals - no automatic output needed:

- **SIGUSR1** (all Unix): `kill -USR1 <pid>`
- **SIGINFO/CTRL-T** (BSD/macOS): Press `Ctrl-T` in terminal

**Current behavior** (without `--explain`):
```
📈 Stats:
Events created: 12340 total, 1523 output, 10817 filtered (87.7%)
Throughput: 19609 lines/s in 2.3s
Timestamp: timestamp (auto-detected); 12340 parsed, 0 missing
Time span: 2024-01-15 10:00:00 to 2024-01-15 10:05:00
```

**Enhanced behavior** (with `--explain`):
Shows the above PLUS per-stage breakdown:
```
📈 Stats:
Events created: 12340 total, 1523 output, 10817 filtered (87.7%)
Throughput: 19609 lines/s in 2.3s

Stage Results:
  #1 filter (e.level != "DEBUG"): ✓ 12340 passed, ✗ 32762 dropped (72.6%)
  #2 exec (duration calc): ✓ 12340 ok, ⚠ 0 errors
  #3 filter (e.duration_ms > 1000): ✓ 1523 passed, ✗ 10817 dropped (87.7%)

Timestamp: timestamp (auto-detected); 12340 parsed, 0 missing
Time span: 2024-01-15 10:00:00 to 2024-01-15 10:05:00
```

**Why this is better than automatic progress:**
- ✅ Zero noise - only prints when user requests it
- ✅ Works perfectly for slow streaming (`tail -f | kelora`)
- ✅ No complexity of periodic updates, buffering, or terminal detection
- ✅ Proven UX pattern (used by `dd`, `rsync`, database clients, etc.)
- ✅ Already implemented - just needs enhancement for `--explain`

**Use case: Slow streaming**
```bash
# Tailing a log that updates infrequently
tail -f /var/log/app.log | kelora --explain -f json --filter 'e.level == "ERROR"'

# Is it working or hung? Press CTRL-T to check:
# → Shows: "Events created: 47 total, 12 output, 35 filtered (74.5%)"
# → Proves it's alive and processing
```

---

## Feature Interactions

### Combining Flags

All three features are orthogonal and can be combined:

```bash
# Errors + pipeline transparency
kelora -v --explain app.log

# Errors + targeted debugging
kelora -v --trace='e.status >= 500' app.log

# Learn the pipeline + trace drops
kelora --explain --trace=drops app.log

# All three: errors, transparency, and debugging
kelora -v --explain --trace=errors app.log

# Check progress anytime with CTRL-T (or kill -USR1 <pid>)
kelora --explain large.log
# Press CTRL-T → see per-stage stats while processing
```

### Suppression Flags

| Flag | `-v` errors | `--explain` startup | `--explain` stats | `--trace` |
|------|-------------|---------------------|-------------------|-----------|
| `--silent` | ✗ | ✗ | ✗ | ✗ |
| `-m` / `--metrics` | ✗ | ✗ | ✓ | ✗ |
| `-s` / `--stats` | ✗ | ✗ | ✓ | ✗ |
| `--no-diagnostics` | ✓ | ✓ | ✗ | ✓ |
| `-q` / `--no-events` | ✓ | ✓ | ✓ | ✓ |

**Rationale:**
- `--silent`: Suppress everything except fatal errors and metrics files
- `-m`/`-s`: Only show final statistics
- `--no-diagnostics`: Suppress progress/stats but allow errors and traces
- `-q` / `--no-events`: Suppress event output but allow diagnostics/errors/traces

### Parallel Mode

All three features work in parallel mode:

- **`-v` errors**: Already works (ordered stderr capture)
- **`--explain`**: Startup plan printed by coordinator only; stats aggregated via reducer
- **`--trace`**: Traces buffered per-worker, sent to coordinator, printed in event ID order

---

## Migration from Current Behavior

**No breaking changes** - existing users see no difference unless they use new flags:

- `-v/-vv/-vvv` behavior unchanged
- New `--explain` flag is opt-in
- New `--trace` flag is opt-in
- All existing scripts and workflows continue working

**Help text updates:**
- Document `-v/-vv/-vvv` as "error verbosity levels"
- Add `--explain` to "Diagnostics" help heading
- Add `--trace` to "Diagnostics" help heading
- Update `--help-quick` to mention `--explain` for pipeline transparency

---

## Implementation Phases

### Phase 1: `--explain` (minimal hot-path changes)

1. Add `--explain` flag to CLI
2. Implement startup plan formatting (gather from `PipelineConfig`)
3. Add stage pass/drop/error counters to tracking infrastructure
4. Implement final statistics formatting (aggregate from reducer state)
5. Enhance SIGUSR1/CTRL-T handler to show per-stage stats when `--explain` active
6. Tests: verify stats accuracy, parallel aggregation, suppression flags, signal handling

**Estimated complexity:** Low (mostly formatting, uses existing tracking)

### Phase 2: `--trace=<selector>` (moderate hot-path impact)

1. Add `--trace` flag with selector parsing
2. Implement selector types: line, event, filter, drops, errors, sample
3. Add `is_traced` flag to event metadata
4. Collect stage outcomes for traced events (per-stage instrumentation)
5. Format and print traces at end of pipeline
6. Parallel mode: buffer traces, send via stderr capture, order by event ID
7. Tests: verify selector matching, stage outcome accuracy, parallel ordering

**Estimated complexity:** Medium (requires per-stage instrumentation)

### Phase 3: Polish

1. Add `--trace-stage=<id>` to focus on specific stage
2. Improve trace output formatting (syntax highlighting, better truncation)
3. Add progress indicator for trace volume (warn if >10k events)
4. Performance optimization: minimize allocations for non-traced events
5. Documentation: examples, common patterns, troubleshooting guide

**Estimated complexity:** Low (refinement)

---

## Alternatives Considered

### Alternative 1: Overload `-v` levels (original proposal)

**Approach:** `-v` = progress, `-vv` = sampled traces, `-vvv` = deep debug

**Rejected because:**
- Mixes error verbosity, progress, and debugging into one concept
- Sampling at `-vv` creates gaps for debugging specific events
- Changes meaning of `-v` (breaking change in behavior)
- Users can't get "just errors" or "just progress" independently

### Alternative 2: Single `--debug` flag with sub-options

**Approach:** `--debug=errors,pipeline,trace:drops`

**Rejected because:**
- More complex syntax (harder to discover, remember)
- Can't use simple boolean flags in config files
- Doesn't leverage familiar `-v` semantics for errors
- Harder to document in `--help` output

### Alternative 3: Keep `-v` for errors, add `--debug=<level>`

**Approach:** `-v` unchanged, `--debug=1/2/3` for transparency/tracing

**Rejected because:**
- Still mixes transparency and tracing into one concept
- Numeric levels are less intuitive than named flags
- Users still can't combine "learn pipeline" with "trace specific events" independently

---

## Open Questions

1. **Default trace output limit?** Should `--trace='e.level == "ERROR"'` warn/limit if it matches thousands of events? Or trust the user's selector?
   - **Recommendation:** Warn at 10,000 matches but don't auto-limit. User controls via selector or `--head`/`--take`.

2. **Trace output destination?** Always stderr? Or follow event output destination (stdout) when appropriate?
   - **Recommendation:** Always stderr (consistent with `-v` errors). Traces are diagnostic output, not event data.

3. **Should `--explain` show implicit stages?** E.g., auto-inserted multiline aggregation, implicit emit at end?
   - **Recommendation:** Yes, show ALL stages in execution order including implicit ones. This is educational.

4. **Trace format: structured or human-readable?** Could make traces machine-parseable (JSON lines)?
   - **Recommendation:** Start with human-readable. Add `--trace-format=json` later if needed for tooling.

5. **Should `--trace` work with `--parallel`?** Or warn/disable parallel mode due to ordering complexity?
   - **Recommendation:** Support parallel mode. Event ID ordering makes traces understandable. Important for debugging large files.

---

## Success Metrics

**How do we know this design is successful?**

1. **Users can answer "what is Kelora doing?"** with `--explain` alone
2. **Users can debug specific events** without sampling gaps using `--trace`
3. **Error verbosity is unchanged** - existing `-v` users are unaffected
4. **Features are discoverable** - help text clearly explains each flag's purpose
5. **Performance overhead is minimal** - `--explain` has near-zero cost, `--trace` only impacts traced events
6. **Parallel mode works correctly** - stats aggregate properly, traces are ordered

**User feedback to collect:**
- Is `--explain` output helpful for understanding pipeline configuration?
- Do stage statistics help identify filtering bottlenecks?
- Does `--trace=<selector>` provide enough detail to debug event-level issues?
- Are there common selectors we should add shortcuts for?

---

## Documentation Requirements

1. **`--help` text**: Add `--explain` and `--trace` with one-line descriptions
2. **`--help-quick`**: Mention `--explain` in "Understanding Your Pipeline" section
3. **Man page / website docs**:
   - Dedicated section on "Debugging and Transparency"
   - Examples of each flag's use cases
   - Table of common `--trace` selectors
4. **Error messages**: When stages drop events, suggest `--trace=drops` to investigate
5. **Examples directory**: Add example demonstrating `--explain` and `--trace` usage

---

## Summary

This design separates three distinct user needs:

1. **Error verbosity** (`-v/-vv/-vvv`): Preserve current behavior - show errors as they occur
2. **Pipeline transparency** (`--explain`): Help users understand what Kelora is doing
3. **Event debugging** (`--trace=<selector>`): Show specific events' journeys through stages

**Progress monitoring** uses existing SIGUSR1/CTRL-T signals - no automatic output needed. When `--explain` is active, signals show enhanced per-stage statistics.

Each feature is orthogonal, minimal in hot-path overhead, and composable with others. Users get clearer intent, more control, and no sampling gaps in debugging output.

**The key insights:**
- Verbosity is not one problem - it's three. Solve each separately.
- Progress is already solved - enhance it, don't reinvent it.
