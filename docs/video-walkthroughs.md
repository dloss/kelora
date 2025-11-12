# Video Walkthrough Screenplays

Collection of video demonstration scripts for Kelora, designed to showcase real features with actual example files.

---

## Video 1: "The 90-Second Hero Demo" ⚡
**Target:** Social media, attention-grabbing
**Length:** 90 seconds
**Files:** `examples/api_errors.jsonl`

### Script

```
[0:00-0:10] THE HOOK
Terminal shows: wc -l examples/api_errors.jsonl
Output: 1000 examples/api_errors.jsonl

Narration: "You need to find all API errors in this 1,000-line JSON log file."

[0:10-0:30] THE PROBLEM
Show trying with grep:
  $ grep ERROR examples/api_errors.jsonl
Output: Ugly, unparsed JSON blobs scrolling by

Show trying with jq:
  $ jq 'select(.level == "ERROR")' examples/api_errors.jsonl
Output: Works but verbose, slow, hard to customize

Narration: "grep doesn't parse structure. jq works but requires learning new syntax."

[0:30-0:70] THE SOLUTION
Type one Kelora command:
  $ kelora -j examples/api_errors.jsonl \
      --levels error \
      --exec 'e.msg = e.endpoint + " → " + e.error'

Output: Clean, readable, streaming results:
  🔹 /api/data → database timeout
  🔹 /api/export → service unavailable
  ...

Narration: "Kelora understands JSON. Filter by level. Transform with simple code.
           All in one command."

[0:70-0:90] THE KICKER
Quick montage showing 3 terminals side by side:
1. Same command with -f combined (web logs)
2. Adding --metrics for counting
3. Output to --levelmap for visual timeline

End screen: "kelora.dev • Programmable Log Analysis"
              "150+ Functions • JSON, Logfmt, Syslog, CSV & More"
```

---

## Video 2: "The Realistic Debugging Journey" 🔍
**Target:** Developers who work with logs daily
**Length:** 3-4 minutes
**Files:** `examples/api_errors.jsonl`, `examples/payments_latency.jsonl`

### Act 1: The Alert (0:00-0:45)

```
SCENE: You get a Slack notification
"⚠️ Payment API errors spiked 10 minutes ago"

Cut to terminal showing large log file
Narration: "Let's investigate. First, what does the error pattern look like?"

TYPE: kelora -j examples/api_errors.jsonl -F levelmap

OUTPUT: Beautiful color-coded timeline visualization:
  ████████░░░░░░░░░░░░░░████████████░░░░░░
  INFO  ████████████░░░░░░░░░░░░░░░░░░
  ERROR ░░░░░░░░░░░░░░░░████████████████

Narration: "There's our spike. Let's zoom in on those errors."
```

### Act 2: The Investigation (0:45-2:00)

```
TYPE: kelora -j examples/api_errors.jsonl \
  --levels error \
  --metrics \
  --exec 'track_count(e.endpoint)'

OUTPUT:
Metrics:
  endpoint
    /api/data    → 342
    /api/export  → 189
    /api/auth    → 12

Narration: "The /api/data endpoint had the most failures.
           What were the actual error messages?"

TYPE: kelora -j examples/api_errors.jsonl \
  --filter 'e.endpoint == "/api/data"' \
  --metrics \
  --exec 'track_count(e.error ?? "unknown")'

OUTPUT:
Metrics:
  error
    database timeout       → 287
    connection pool full   → 55

Narration: "Database timeouts. That's our smoking gun."
```

### Act 3: The Timeline (2:00-3:00)

```
Narration: "When exactly did this start?"

TYPE: kelora -j examples/api_errors.jsonl \
  --filter 'e.status >= 500' \
  --mark-gaps 5m

OUTPUT: Shows events with gap markers:
  🔹 2024-07-17T12:00:05Z ERROR /api/data → database timeout
  🔹 2024-07-17T12:00:20Z ERROR /api/export → service unavailable
  ━━━━━━━━━━━━━━━━━ 6m gap ━━━━━━━━━━━━━━━━━
  🔹 2024-07-17T12:06:33Z ERROR /api/data → database timeout
  🔹 2024-07-17T12:06:35Z ERROR /api/data → database timeout
  🔹 2024-07-17T12:06:37Z ERROR /api/data → database timeout

Narration: "The errors started clustering after a 6-minute gap.
           Let's correlate this with deployments."

[Show checking deployment logs - deployment happened at 12:06]

Narration: "Found it. The deployment at 12:06 introduced the issue.
           Rollback confirmed."
```

### Act 4: The Takeaway (3:00-3:30)

```
Split screen showing 4 commands used:
1. kelora -j logs.jsonl -F levelmap
   → Quick visualization

2. kelora -j logs.jsonl --metrics --exec 'track_count(field)'
   → Aggregation and grouping

3. kelora -j logs.jsonl --filter 'condition'
   → Structured filtering

4. kelora -j logs.jsonl --mark-gaps 5m
   → Timeline pattern detection

Narration: "From alert to root cause in 3 minutes.
            That's programmable log analysis."

End card: "kelora.dev • When logs tell stories"
          "Install: cargo install kelora"
```

---

## Video 3: "The Power User Tour" 🎓
**Target:** Technical audience wanting comprehensive overview
**Length:** 5-7 minutes
**Files:** Multiple from `examples/`

### Part 1: The Basics (0-90s)

```
Narration: "Kelora is a command-line log analyzer with embedded scripting."

[Demo 1: Auto-detection]
$ kelora examples/simple_json.jsonl
Shows: JSON auto-detected, formatted output

[Demo 2: Format specification]
$ kelora -f combined examples/web_access.log
Shows: Apache/Nginx combined log parsed

[Demo 3: Level filtering]
$ kelora -j examples/api_errors.jsonl --levels error,critical
Shows: Only ERROR and CRITICAL events

Narration: "It understands JSON, logfmt, syslog, CSV, and custom formats.
           Start simple, then add scripting when you need it."
```

### Part 2: Scripting Power (90s-4m)

```
[Demo 4: Field transformation]
$ kelora -j examples/payments_latency.jsonl \
    --exec 'e.slow = e.duration_ms > 1000' \
    --filter 'e.slow'

Shows: Adding computed fields and filtering

[Demo 5: Aggregation with metrics]
$ kelora -j examples/payments_latency.jsonl \
    --metrics \
    --exec 'track_unique("regions", e.region)' \
    --exec 'track_sum("total_ms", e.duration_ms)'

Shows: Metrics output table at end

[Demo 6: Window analysis]
$ kelora -j examples/payments_latency.jsonl \
    --window 100 \
    --end 'let times = window.pluck_as_nums("duration_ms");
           print("p95: " + times.percentile(95))'

Shows: Percentile calculation over sliding window

Narration: "Use --exec for transformations, --metrics for aggregation,
           --window for sliding window analysis. All with simple syntax."
```

### Part 3: Advanced Patterns (4m-6m)

```
[Demo 7: Span aggregation]
$ kelora -j examples/api_errors.jsonl \
    --span 100 \
    --span-close 'let errors = span.events.filter(|e| e.level == "ERROR");
                  if errors.len() > 10 {
                    print("Spike: " + errors.len() + " errors in span " + span.id)
                  }'

Shows: Detecting error bursts

[Demo 8: Multi-file enrichment]
$ kelora -j examples/api_errors.jsonl \
    --exec 'let metadata = read_file("examples/service_metadata.json").parse_json();
            let service = e.endpoint.extract_re(r"/api/(\w+)", 1);
            e.owner = metadata[service].owner ?? "unknown"'

Shows: Enriching events with external data

[Demo 9: Custom output formats]
$ kelora -j examples/api_errors.jsonl \
    --filter 'e.level == "ERROR"' \
    -F csv

Shows: CSV output with headers

Narration: "150+ built-in functions for parsing, transforming, aggregating.
           Read external files. Output to any format. All streaming."
```

### Part 4: Real-World Integration (6m-7m)

```
[Demo 10: Config aliases]
$ kelora --save-alias errors \
    --levels error \
    --metrics \
    --exec 'track_count(e.endpoint)'

$ kelora -j examples/api_errors.jsonl -a errors

Shows: Reusable command aliases

[Demo 11: Pipeline integration]
$ tail -f production.log | kelora -j --filter 'e.latency > 1000'

Shows: Live streaming analysis

[Demo 12: Exit codes for CI]
$ kelora -j examples/api_errors.jsonl --levels error -qq
$ echo $?
1

Shows: Exit code 1 when errors found

Narration: "Save command aliases. Integrate with existing tools.
           Use in CI/CD pipelines. Kelora fits your workflow."

End: "Get started: kelora.dev"
     "cargo install kelora"
```

---

## Video 4: "Kelora vs The Alternatives" ⚔️
**Target:** Users of existing tools
**Length:** 2-3 minutes
**Goal:** Side-by-side comparison

### Structure

Split screen showing the same task in three tools:

**Task 1: Find errors with high latency**
```
grep + jq:
  $ grep ERROR app.log | jq 'select(.latency > 1000)'
  (slow, requires two tools, fragile)

Kelora:
  $ kelora -j app.log --levels error --filter 'e.latency > 1000'
  (fast, one tool, readable)
```

**Task 2: Calculate 95th percentile response times**
```
jq (complex):
  $ jq -s '[.[].duration_ms] | sort | .[length * 0.95 | floor]' app.log
  (loads entire file into memory, hard to read)

Kelora:
  $ kelora -j app.log --window 1000 \
      --end 'print(window.pluck_as_nums("duration_ms").percentile(95))'
  (streaming, clear intent)
```

**Task 3: Group and count errors by endpoint**
```
awk + sort + uniq:
  $ grep ERROR app.log | awk -F'"' '{print $8}' | sort | uniq -c
  (fragile parsing, assumes structure)

jq:
  $ jq -s 'group_by(.endpoint) | map({endpoint: .[0].endpoint, count: length})'
  (loads entire file, verbose)

Kelora:
  $ kelora -j app.log --levels error --metrics \
      --exec 'track_count(e.endpoint)'
  (streaming, clear, built-in)
```

End with comparison table:
| Feature          | grep+awk | jq  | Kelora |
|-----------------|----------|-----|--------|
| Streaming       | ✓        | ✗   | ✓      |
| Structured data | ✗        | ✓   | ✓      |
| Readable syntax | ~        | ~   | ✓      |
| Aggregations    | Manual   | ✓   | Built-in |
| Multiple formats| ✗        | ✗   | ✓      |

---

## Production Notes

### Recording Setup
- Terminal: 1920x1080, high contrast theme
- Font: JetBrains Mono or Fira Code, 18pt minimum
- Shell prompt: Minimal (just `$`)
- Use `asciinema` for recording, convert to GIF/MP4
- Add subtle syntax highlighting with `bat` if needed

### Narration Guidelines
- Conversational, not robotic
- Focus on "why" not just "what"
- Pause after complex commands (let viewers read)
- Keep technical but accessible
- Energy level: Informative, not hyperactive

### Visual Enhancements
- Add text overlays for key concepts (fade in/out)
- Highlight changed parts of commands (color/underline)
- Use progress bars for long operations
- Add subtle sound effects for transitions
- End cards with clear CTAs

### Multiple Versions Strategy
1. **90-second cut** (Video 1) → Twitter, LinkedIn, Reddit
2. **3-minute cut** (Video 2) → YouTube, homepage hero
3. **Full 7-minute tour** (Video 3) → Documentation, tutorials
4. **Comparison cut** (Video 4) → Landing page, positioning

### Call to Action Hierarchy
- Primary: "Visit kelora.dev"
- Secondary: "cargo install kelora"
- Tertiary: Links to docs, GitHub

---

## Next Steps

1. **Create missing demo files** (if needed larger datasets)
2. **Record with asciinema** for authentic terminal feel
3. **Add narration** with clear, energetic voice
4. **Edit and polish** with fade transitions
5. **Export multiple formats** (GIF for docs, MP4 for YouTube)
6. **Test on mobile** to ensure readability
