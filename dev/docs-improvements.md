# Documentation Improvements

Review completed: 2025-11-11

## Executive Summary

**Overall Rating:** 7.5/10 - Excellent foundation, needs new-user onboarding polish

**Strengths:**
- Excellent Diátaxis structure (Tutorials, How-To, Concepts, Reference)
- Comprehensive coverage with executable examples
- Logical learning path and good cross-referencing
- High-quality tutorial content

**Gaps:**
- Written for users who already understand log processing concepts
- Terminology used before definition
- Stage ordering confusion (filter/exec/levels)
- Missing scaffolding for complete newcomers (glossary, diagrams, FAQ)

---

## High Priority Fixes (Do First)

### 1. ✅ Add a Glossary
**Problem:** Terms like "event", "field", "span", "stage", "window" used throughout but never formally defined up front.

**Solution:** Create `docs/glossary.md` with clear definitions and link prominently from:
- Landing page
- Quickstart
- Basics tutorial
- Navigation footer

**Impact:** Reduces cognitive load for new users trying to understand examples.

---

### 2. Fix Title/Navigation Inconsistencies
**Problem:** `pipeline-model.md` has title "Processing Architecture" but nav shows "Pipeline Model"

**Solution:** Pick one name and use consistently. Recommend "Processing Architecture" as it's more descriptive.

**Files to update:**
- `docs/concepts/pipeline-model.md` (title)
- `mkdocs.yml` (nav entry)
- All cross-references

---

### 3. Add Prominent Stage Ordering Callout
**Problem:** New users will put `--filter 'e.computed_field > 10'` before the `--exec` that creates it.

**Solution:** Add a prominent admonition box in `docs/tutorials/basics.md` (Part 2.5 or Part 3):

```markdown
!!! warning "Stage Order Matters"
    Filters and transforms run in the order you specify them on the CLI.
    Create fields with `--exec` BEFORE filtering on them with `--filter`.

    ❌ Wrong: `kelora --filter 'e.duration_s > 1' -e 'e.duration_s = e.duration_ms / 1000'`

    ✅ Right: `kelora -e 'e.duration_s = e.duration_ms / 1000' --filter 'e.duration_s > 1'`
```

**Also add to:**
- `docs/intro-to-rhai.md` (Step 7)
- `docs/quickstart.md` (after command 3)

---

### 4. Create Quick Reference Page
**Problem:** Users need to search through multiple pages for common patterns.

**Solution:** Create `docs/reference/quick-reference.md` with one-page cheatsheet:
- Common flags and what they do
- Common patterns (filter errors, extract fields, export CSV)
- Stage ordering rules
- Format selection shortcuts
- Debugging tips

**Link from:**
- Navigation (Reference section)
- Landing page footer
- README

---

### 5. Add "Common Issues" Section
**Problem:** New users hit predictable errors with no guidance.

**Solution:** Add FAQ or "Common Issues" section covering:

**In Quickstart or as separate `docs/troubleshooting.md`:**

```markdown
## Common Issues

### "Field not found" or empty output
- Check format is specified: `-j` for JSON, `-f logfmt`, etc.
- Default is `-f line` (plain text)
- Use `-F inspect` to see field names and types

### Filter not working
- Fields must exist before filtering
- Create fields with `--exec` BEFORE filtering on them
- Use `--verbose` to see errors

### Type comparison errors
- String "500" ≠ Integer 500
- Use `.to_int_or(0)` to convert safely
- Use `--filter 'e.status.to_int() >= 500'` for string fields

### No events match filter
- Check level filtering: `-l error` only shows errors
- Use `--stats` to see how many events filtered out
- Verify timestamp filtering with `--since`/`--until`
```

---

## Medium Priority

### 6. Consolidate Performance Pages
**Problem:** Three separate performance pages create navigation overhead.

**Current:**
- `concepts/performance-model.md`
- `concepts/performance-comparisons.md`
- `concepts/benchmark-results.md`

**Solution:** Merge into single `concepts/performance.md` with sections:
1. **When to Optimize** - Decision tree for sequential vs parallel
2. **Performance Model** - Streaming, memory usage, throughput
3. **Benchmarks** - Real-world numbers
4. **Comparisons** - vs jq, awk, grep
5. **Tuning Guide** - Batch size, threads, optimization tips

---

### 7. Add Visual Pipeline Diagram
**Problem:** Text-only explanation of pipeline is hard to grasp.

**Solution:** Add a simple flowchart to landing page and pipeline-model page:

```
┌─────────┐    ┌────────┐    ┌──────────┐    ┌────────┐
│  Input  │ -> │ Parse  │ -> │  Stages  │ -> │ Output │
│ (files) │    │ (json) │    │ (filter) │    │ (json) │
└─────────┘    └────────┘    └──────────┘    └────────┘
                                    |
                              ┌─────┴─────┐
                              │   --exec  │
                              │  --filter │
                              │  --levels │
                              └───────────┘
```

Create simple ASCII or mermaid diagram showing data flow.

---

### 8. Split Dense Pipeline Model Page
**Problem:** 588 lines covering everything is overwhelming.

**Solution:** Split into two pages:
- `concepts/pipeline-basics.md` - Input → Parse → Filter → Output (core concepts)
- `concepts/pipeline-advanced.md` - Spans, parallel mode, batching, context lines

**Or:** Add executive summary/TL;DR at top with jump links.

---

### 9. Add "When NOT to Use Kelora" Section
**Problem:** Users waste time trying to use tool for wrong use cases.

**Solution:** Add to landing page or Concepts:

```markdown
## When NOT to Use Kelora

Kelora excels at streaming log analysis with custom logic, but isn't ideal for:

- **Quick grep/awk jobs** - Use grep/awk for simple pattern matching
- **SQL-style joins** - Use DuckDB, ClickHouse, or SQLite for relational queries
- **Real-time dashboards** - Use Grafana, Kibana, or Datadog for visualization
- **Log storage** - Use Elasticsearch, Loki, or S3 for archival
- **Binary log formats** - Kelora works with text-based formats only

**Best for:** Ad-hoc analysis, ETL pipelines, streaming transforms, custom parsing
```

---

### 10. Create Troubleshooting Guide
**Problem:** No systematic debugging approach for users stuck on errors.

**Solution:** Create `docs/troubleshooting.md` with:

**Debugging Workflow:**
1. Use `-F inspect` to see field types
2. Use `--verbose` to see error messages
3. Use `--stats` to see processing summary
4. Test with `--take 10` for quick iteration
5. Use `--strict` to fail fast on errors

**Common Error Patterns:**
- Parse errors → Check format selection
- Filter errors → Check field existence with `e.has("field")`
- Type errors → Use safe conversions like `.to_int_or(0)`
- Performance issues → Consider `--parallel` for large files
- Empty output → Check level filtering, format detection

**Exit Codes:**
- 0 = Success
- 1 = Parse/runtime errors
- 2 = Invalid CLI usage

---

## Low Priority (Nice to Have)

### 11. Comparison Table with Other Tools
Add to landing page or concepts:

| Tool | Best For | Kelora Advantage |
|------|----------|------------------|
| grep | Pattern matching | Structured field access, transformations |
| awk | Column processing | Type-aware, nested fields, metrics |
| jq | JSON manipulation | Multi-format, filtering, aggregation |
| Python | Complex logic | No setup, streaming, CLI-first |
| Logstash | ETL pipelines | Lightweight, no JVM, Rhai scripting |

---

### 12. Video/GIF Walkthroughs
Create short screencasts for:
- Quickstart (5 min)
- Error triage workflow (3 min)
- Custom parsing (3 min)

---

### 13. Migration Guides
Create guides for users coming from:
- jq users → Kelora equivalents
- awk users → Kelora patterns
- grep users → Filtering approaches

---

### 14. Extended Examples Repository
Separate from how-tos, create `docs/examples/` with:
- One-liners for common tasks
- Script snippets library
- Copy-paste templates

---

## Content Issues to Address

### Landing Page (index.md)

**Problem:** "What It Does" section overlaps heavily with Quickstart.

**Solution Options:**
1. **Remove section** - Link directly to Quickstart instead
2. **Tell a story** - Make three examples show progression: Problem → Detection → Solution
3. **Keep one example** - Show most compelling use case, link to Quickstart for more

**Recommendation:** Keep one compelling example, link prominently to Quickstart.

---

### Core Concepts Introduced Too Late

**Problem:** "Event" mentioned everywhere but not explained until tutorial Part 2.5.

**Solution:** Add brief explanation on landing page:

```markdown
## How It Works

Kelora parses each log line into an **event** - a structured object (map) with fields you can access and manipulate using Rhai scripts.

For example, after parsing this JSON:
\`\`\`json
{"timestamp": "...", "level": "ERROR", "message": "..."}
\`\`\`

You can filter with `--filter 'e.level == "ERROR"'` where `e` is the event and `e.level` accesses the level field.
```

---

### Format Flag Inconsistency

**Problem:** Examples use `-j`, `-f json`, `--input-format json` interchangeably without stating equivalence.

**Solution:** Add to Quickstart after first command:

```markdown
!!! tip "Format Shortcuts"
    `-j` is shorthand for `-f json`. These are equivalent:
    - `kelora -j app.log`
    - `kelora -f json app.log`
    - `kelora --input-format json app.log`

    This guide uses `-j` for brevity.
```

---

### Terminology Consistency

**Current inconsistencies:**
- "150+ functions" vs "150+ built-in functions" vs "150+ built-in Rhai functions"
- "Pipeline Model" vs "Processing Architecture"
- Mix of short/long flag notation

**Recommendation:**
- Use "150+ built-in functions" everywhere
- Pick one: "Processing Architecture" (clearer for new users)
- Short flags in examples, long flags in prose: "Use `-e` (short for `--exec`) to transform events"

---

### Development Approach Section

**Problem:** Appears on both README and landing page (index.md).

**Solution Options:**
1. Remove from landing page, keep in README only
2. Move to separate "About" page
3. Move to footer with link "About This Project"

**Recommendation:** Keep on landing page but move to bottom (after License). It's important context but shouldn't be in "Get Started" path.

---

## Missing Documentation

### For New Users
1. ✅ Glossary (HIGH PRIORITY)
2. Visual pipeline diagram
3. "When NOT to use Kelora"
4. Common error messages with solutions
5. FAQ section

### For Learning
6. Quick reference card (HIGH PRIORITY)
7. Comparison table with other tools
8. Common gotchas page
9. Troubleshooting guide (MEDIUM PRIORITY)

### For Operations
10. Performance tuning guide (part of consolidated performance page)
11. Debugging workflow (part of troubleshooting guide)
12. Exit code reference (exists but needs better linking)

### For Reference
13. Complete CLI flag table (single page with all flags)
14. Rhai script snippets library

---

## Structural Improvements

### Navigation Improvements

**Add to mkdocs.yml nav:**
```yaml
- Getting Started:
    - Home: index.md
    - Quickstart: quickstart.md
    - Glossary: glossary.md
    - Troubleshooting: troubleshooting.md
```

**Add footer links:**
- Quick Reference
- Glossary
- FAQ
- Troubleshooting

---

### Learning Path Improvements

**Current flow:** Home → Quickstart → Basics → Intro to Rhai → ...

**Enhancement:** Add "learning checkpoint" boxes at end of each tutorial:

```markdown
## ✓ You've Learned

- [ ] Specify formats with `-f` and `-j`
- [ ] Filter levels with `-l error,warn`
- [ ] Select fields with `-k field1,field2`
- [ ] Export with `-F csv` or `-J`

**Next:** Learn to write custom filters and transforms in [Introduction to Rhai](intro-to-rhai.md)
```

---

### Cross-Reference Improvements

Add consistent "See Also" sections with:
- Related concepts
- Related how-tos
- Related reference pages

Format:
```markdown
## See Also

**Concepts:**
- [Events and Fields](../concepts/events-and-fields.md) - Event structure details

**How-To:**
- [Triage Production Errors](../how-to/find-errors-in-logs.md) - Apply these basics

**Reference:**
- [CLI Reference](../reference/cli-reference.md) - Complete flag list
```

---

## Specific Page Improvements

### Quickstart
- Add explanation of "why each step matters" between three commands
- Add "Format Shortcuts" tip box
- Add "Common Issues" section at end
- Add learning checkpoint

### Basics Tutorial
- Add stage ordering warning prominently
- Add "Understanding Events" earlier (currently Part 2.5, should be Part 1.5)
- Add learning checkpoint at end

### Intro to Rhai Tutorial
- Emphasize type awareness earlier
- Add more examples of type conversion failures
- Add debugging workflow section

### Pipeline Model (Processing Architecture)
- Add TL;DR section at top
- Add visual diagram
- Consider splitting into basics/advanced

### How-To Guides
- Add "Prerequisites" section to each (what you need to know)
- Add "Time to complete" estimate
- Add "Common variations" for each recipe

### Reference Pages
- CLI Reference: Add single-table overview of all flags
- Functions Reference: Already excellent, no changes needed
- Add Quick Reference page (new)

---

## Implementation Order

### Phase 1: Critical Fixes (Week 1)
1. Create glossary.md
2. Fix title/nav inconsistencies
3. Add stage ordering callouts
4. Add common issues to Quickstart
5. Fix terminology consistency

### Phase 2: New Content (Week 2)
6. Create quick-reference.md
7. Create troubleshooting.md
8. Consolidate performance pages
9. Add visual pipeline diagram
10. Split pipeline-model.md

### Phase 3: Polish (Week 3)
11. Add "When NOT to use" section
12. Add learning checkpoints
13. Improve cross-references
14. Add comparison table
15. Update navigation structure

### Phase 4: Nice-to-Have (Future)
16. Video walkthroughs
17. Migration guides
18. Extended examples repository
19. FAQ page

---

## Success Metrics

**How to measure improvement:**
1. **Time to first success** - Can a new user run a successful filter in < 10 minutes?
2. **Concept clarity** - Do users understand "event", "stage", "format" before using them?
3. **Error recovery** - Can users debug common issues without external help?
4. **Feature discovery** - Can users find advanced features when needed?

**Before/After Test:**
- Have 3-5 new users try Quickstart
- Note where they get stuck
- Measure completion time
- Collect confusion points
- Repeat after improvements

---

## Notes

- Documentation is already very good - these are polish improvements
- Executable examples (markdown-exec) are a huge strength
- Tutorial quality is high - just needs better scaffolding
- Main gap is onboarding for complete newcomers
- Structure (Diátaxis) is excellent and should be preserved
