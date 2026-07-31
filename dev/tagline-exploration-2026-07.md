# Tagline: why "messy logs" underperforms, and what to say instead

Exploration of the top-line positioning (2026-07, v2.x). The line in question
appears in exactly two places:

- `README.md:8` — **One command for messy logs.**
- `docs/index.md:3` — same line, plus "and your own custom formats"

Plus a separate, unrelated descriptor in `Cargo.toml:6`:
"A command-line log analysis tool with embedded Rhai scripting".

Verdict up front: **"One command" is the good half — keep it. "for messy logs"
is the weak half — replace it.** The problem isn't that the phrase is
inaccurate; it's that it describes a property of the reader's *data* instead of
naming the work Kelora saves them, and it now lags the product by a full
release cycle.

---

## 1. Five concrete problems with "messy"

### 1.1 It's an adjective about the input, not a promise about the output

Every strong CLI tagline names either what the tool *replaces* (Miller: "like
awk, sed, cut, join, and sort for CSV, TSV and JSON") or what it *does*
(ripgrep: "recursively searches directories for a regex pattern"). "Messy" does
neither — it characterizes the file you already have. The reader still has to
infer the benefit.

### 1.2 It disqualifies readers who shouldn't be disqualified

A team shipping clean structured JSON reads "messy logs" and concludes "not for
me." But that reader is squarely in the target audience: `--discover`,
`--drain`, `--freq`, `--span`, and the scripting layer are just as valuable on
immaculate JSON. The current line filters out qualified readers on a premise
that doesn't matter.

### 1.3 It's mildly accusatory

"Messy" is a judgment about the reader's logging setup — often one they
inherited and can't change. Positioning that starts by grading someone's
infrastructure is a small friction, but it's friction in the very first
sentence.

### 1.4 The word is already crowded and does nothing for search

"Messy data" is claimed territory: pandas tutorials, OpenRefine, csvkit,
Miller, every ETL blog post. It carries no differentiation and no search value
— nobody types "messy logs" into a search bar. They type "parse json logs
cli", "command line log analysis", "jq for logs", "group similar log lines".
None of those strings appear in the tagline slot.

### 1.5 It frames Kelora as remedial when the product has become analytical

This is the real cost. "Messy logs" says *cleanup tool* — normalize the
garbage, move on. But look at what the README tour actually demonstrates, in
order:

1. `--discover` — you don't know what's in the file yet
2. `-f json,line` — mixed formats in one pass
3. `--drain` — 742 near-duplicate lines collapse into 4 patterns
4. `--span 1m --freq level` — when did this happen

Only #2 is about mess. The other three are *investigation*: figuring out what a
log stream contains and what's breaking in it. Same for `--describe`, `--card`,
and the tracker/metrics layer. The tagline is describing v1 while the product
ships v2-and-heading-to-v3. That mismatch is worth fixing on its own.

---

## 2. The tagline slot is doing two jobs badly

The single line currently tries to be both:

| Job | Where it's read | What it needs |
|---|---|---|
| **Identify** | crates.io, GitHub "About", search results, `apt search` | Literal, keyword-bearing, boring on purpose |
| **Motivate** | README H1 subtitle, docs landing page | Why this instead of grep/jq/a script |

These want different sentences. Kelora already *has* two slots (`Cargo.toml`
description and the README/docs line) but they aren't split along this seam —
the Cargo one leads with "Rhai", a word no prospective user has heard, and the
README one leads with a mood.

Recommendation: make the split explicit. Descriptive line in `Cargo.toml`,
motivating line in `README.md` / `docs/index.md`.

---

## 3. What Kelora actually is, relative to the neighbors

| Tool | Occupies | Kelora's gap |
|---|---|---|
| grep/ripgrep | text search | logs as typed events, not lines |
| jq | JSON only | any format, and mixed formats in one file |
| Miller | CSV/TSV/JSON, own DSL | log-native (ts/level/msg), stateful scripting |
| angle-grinder | query language, no general scripting | full embedded language |
| lnav | interactive TUI viewer | pipeable, scriptable, CI-friendly |
| Vector / fluent-bit | shipping and routing | local analysis, not transport |
| Loki / ELK / Splunk | indexed platform | zero infrastructure, one binary |

The one-word differentiator that survives all seven rows: **programmable**. The
one-sentence differentiator, already written in `docs/index.md:52` and buried
three screens down the README:

> Reach for Kelora when you'd otherwise be writing a throwaway Python script.
> It's the middle ground between "grep is enough" and "I need a real
> observability platform."

That is the best positioning sentence in the repository, and it's below the
fold. Whatever the tagline becomes, that line should move up.

---

## 4. Candidates, with the case against each

### Direction A — Replace the script you'd otherwise write

- **One command instead of a throwaway script.**
- One command, no throwaway script.
- The log tool you reach for instead of writing Python.

*For:* Names Kelora's true competitor — not jq, but the reader's own
forty-minute Saturday script. Universally recognized pain. Keeps "one command,"
which the README then proves four times. Says nothing about whose data is bad.

*Against:* Defines by negation. "Python" narrows it (some readers write awk or
Perl); "throwaway script" is the version that stays neutral.

### Direction B — Unknown file to answer

- **From an unknown log file to an answer, in one command.**
- Point it at a log file. Find out what's in it.

*For:* Matches the README tour's actual arc, starting where `--discover`
starts. Describes the reader's *situation* ("I don't know what this is") rather
than judging their data. Covers auto-detection, drain, freq, and spans under one
umbrella.

*Against:* "Answer" is vague. Slightly long for a subtitle. Undersells the
scripting layer, which is the deep half of the product.

### Direction C — Logs are data, not text

- **Logs are data, not text.**

*For:* It's the genuine philosophy (already in `dev/pitch.md`), short, quotable.

*Against:* A premise, not a benefit — and one jq, Miller, and every
observability vendor also assert. Doesn't imply CLI, doesn't hint at discover or
drain. Better as a section heading than a tagline.

### Direction D — Locate it on the known map

- **For logs that outgrew grep but don't need Grafana.**
- Between grep and Grafana.

*For:* Instantly places the tool using two anchors every reader already has.

*Against:* Grafana is the wrong upper anchor (it visualizes; Loki/Splunk store).
Defines Kelora entirely by other products, and says nothing about what it does.
Strong as the *second* line, weak as the first.

### Direction E — Miller-style substitution list

- **grep, jq, awk, and a throwaway script — one command, for logs.**

*For:* Miller proved this format. Answers "what is this" in one read.

*Against:* Four tool names is a mouthful, and it invites the "swiss army knife"
smell that makes reviewers assume nothing is done well.

### Direction F — Plain descriptive (for the identify slot)

- **A programmable log processor for the command line.**
- Parse, filter, and analyze logs in any format, from the command line.

*For:* Searchable, honest, ages well, nothing to regret. "Programmable" is the
differentiator that survives the whole neighbor table.

*Against:* Zero spark. Won't carry a launch post. Correct for `Cargo.toml`, too
flat to lead the README.

---

## 5. Recommendation

**Split the slots, and swap the tagline's payload from the data to the work.**

`README.md:8` and `docs/index.md:3`:

> **One command instead of a throwaway script.** Parse, filter, transform, and
> summarize logs across JSON, logfmt, syslog, CSV, and plain text — with
> embedded [Rhai](https://rhai.rs) scripting when simple filters aren't enough.
>
> The middle ground between "grep is enough" and "I need a real observability
> platform."

`Cargo.toml:6`:

> description = "Programmable log processor for the command line — parse, filter, and analyze any log format"

Why this combination:

- Keeps the strongest existing asset ("one command") and spends the rest of the
  sentence on the reader's labor rather than their data quality.
- Excludes nobody: the clean-JSON reader still writes throwaway scripts.
- Promotes the best sentence in the docs from screen three to screen one, where
  it does the grep/Grafana positioning work that Direction D does — without
  spending the tagline on it.
- Fixes the crates.io descriptor, which currently leads with "Rhai" (unsearchable)
  instead of "log processor" (searchable) and omits "programmable" entirely.

Runner-up, if "throwaway script" reads as too self-deprecating for the front
door: **"From an unknown log file to an answer, in one command."**

Not recommended as the lead: Direction C (premise, not benefit) and Direction E
(swiss-army smell).

### Keep "messy" in body copy

The word is fine where it's making a specific, demonstrated claim rather than
setting the frame — `README.md:103` / `docs/index.md:55` ("Messy formats parse
cleanly. Mixed JSON and plaintext in the same file…") earns it with evidence on
the same line, as does `docs/reference/exit-codes.md:7` ("exits non-zero when it
couldn't do the job you asked — not because the data was messy"), which is one
of the sharper lines in the docs. The problem is "messy" in the *first*
sentence, not "messy" anywhere.
