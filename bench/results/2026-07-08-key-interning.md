# Field-name small-strings / parser allocation hygiene — results

Spec: "Field-name small-strings and parser allocation hygiene." Baseline in
`2026-07-08-key-interning-baseline.md`. Corpus: `{logfmt,json}_1m_1pct.log`,
1M lines each (`bench/gen_corpus.py --lines 1000000`).

**Method.** No `hyperfine` in this environment. Wall clock is an *interleaved*
A/B: baseline and candidate binaries run back-to-back, 8 rounds (3 for the slow
`--exec` case), warm page cache; min and median reported. The box is shared and
noisy — the baseline drifts ±2–3% between sessions — so only the interleaved
min/median gap is load-bearing. Allocs come from the `--features bench-alloc`
counting allocator (`alloc`+`realloc`+`alloc_zeroed`), `÷1_000_000` (the harness
prints `lines=0` in pass-through since `lines_read` is only set under `-s`). The
report is gated on `KELORA_BENCH_ALLOC=1` so a `--features bench-alloc` build
stays silent on normal/`cargo test --all-features` runs; set the env var when
measuring.

## Outcome

Two changes were specified: **Phase 1** — change the `FieldMap` key type from
`String` to an inline small-string (`smartstring`); **Phase 2** — parser
allocation hygiene (byte-slice logfmt parse, JSON value reuse).

**Phase 1 was measured and rejected. Phase 2 shipped, on `String` keys.**

### Phase 1 (smartstring keys) — rejected per §2.4 kill criterion

Isolated, the representation change **regressed wall clock ~9–12%** while cutting
allocs only ~11%:

| Scenario | Δ wall (smartstring vs baseline) | Δ allocs/line |
|----------|---------------------------------:|--------------:|
| A logfmt → logfmt | **+11.7% slower** | −10.7% |
| B json → json     | **+8.9% slower**  | −10.7% |

Even *combined with* Phase 2, smartstring keys stayed a net loss on JSON:

| Scenario | smartstring + Phase 2 | **String + Phase 2 (shipped)** |
|----------|----------------------:|-------------------------------:|
| A logfmt → logfmt | +6.0% faster, −32% allocs | **+13.0% faster, −23% allocs** |
| B json → json     | **−6.0% slower**, −18% allocs | **+2.9% faster**, −7% allocs |

Root cause. The spec's premise ("the hot path is allocation-bound") predates the
mimalloc switch that `Cargo.toml` already documents. Under mimalloc these small
key allocations are *already cheap*, so eliminating them buys little, while
`smartstring` adds real per-operation cost: an inline/heap discriminant branch on
every construction, and a `Deref` on every hash and every comparison (the field
map is probed ~44×/line by `ordered_fields` for the ts/level/msg priority
ordering). On JSON it is strictly worse — serde can *move* a parsed `String` key
into a `String`-keyed map, whereas a small-string forces a byte-copy into the
inline buffer with no compensating allocation saved.

Per the spec's kill criterion (§2.4: "<8% wall clock on its target scenario →
report; keep the phase only if it also reduces allocs/line by ≥30%, else
revert") and safety-gate framing, Phase 1 fails: negative wall clock, <30%
allocs. It is **not** shipped. The type stays `FieldMap = IndexMap<String, …>`.

### Phase 2 (parser allocation hygiene, on `String` keys) — shipped

- **logfmt** rewritten to scan byte slices and borrow key/value spans from the
  line, inserting directly into the event. Gone: the per-line
  `Vec<(String, String)>`, the char-by-char `String::push` loops, and the
  `String`→`ImmutableString` double allocation on string values (now one
  `ImmutableString` built from the borrowed span). Unquoted and escape-free
  quoted values allocate no intermediate `String`; only an escaped quoted value
  builds one unescape buffer. Boolean coercion uses `eq_ignore_ascii_case`
  instead of an allocating `to_lowercase`. The field map is pre-sized with a
  SIMD `memchr` count of `=` so it never grows mid-parse.
- **JSON** string values build one `ImmutableString` from the borrowed span
  (`visit_str`) instead of `String`-then-`ImmutableString`.
- `Event::set_field` takes `impl Into<String>` so the parser can hand it borrowed
  `&str` spans.

| ID | Scenario | Wall Δ (min / median) | allocs/line base → final | Δ allocs |
|----|----------|----------------------:|-------------------------:|---------:|
| A  | logfmt → logfmt | **+13.0% / +13.3%** | 158.2 → 122.0 | −22.9% |
| A' | logfmt → json   | **+10.1% / +10.5%** | 160.2 → 124.0 | −22.6% |
| B  | json → json     | **+2.9% / +3.1%**   | 140.0 → 130.0 | −7.1% |
| B' | json → logfmt   | (≈B)                | 138.0 → 128.0 | −7.2% |
| D  | logfmt `--exec` per-line | **+7.3%**  | — | — (Dynamic bridge untouched) |

All wins, no regressions. Differential matrix (`bench/diff_check.sh`,
26 cases) byte-identical vs baseline; full suite (3153 tests) green.

## vs the spec's targets (§6/§7)

- A wall: spec wanted ≥20% combined; **got +13%**. B wall: spec wanted ≥8%;
  **got +3%**. Both are real wins but short of target — because wall clock is
  *not* allocation-bound here (see Phase 1 root cause). Removing 23% of logfmt
  allocs yields 13% wall; the rest of the per-line cost is CPU (UTF-8 scanning,
  number/bool coercion, output formatting) and output-side allocations outside
  the parser's scope.
- allocs: spec wanted phase-1 ≥30% / combined ≥60%; **got −23% (logfmt), −7%
  (json)**. The gap is dominated by allocations the parser spec did not target
  (see below). This is the "documented analysis / revised target" path the spec
  explicitly allows (§7.2).

## Top remaining allocation sites (guides the next spec)

Measured on `logfmt → logfmt` (122 allocs/line, ~15 fields/line):

1. **Per-field key `String`** (parser, ~15/line). The one allocation the
   small-string idea was meant to remove — but doing so via `smartstring`
   regresses wall clock (above). A per-parser key **interner** (a
   `HashMap<u64, Rc<str>>` of the ~dozens of distinct field names in a stream)
   would remove these with a cheap hash+lookup instead of an alloc, and keep
   `String`/`&str` ergonomics. This is the most promising next step.
2. **`sanitize_logfmt_key` `String`** (logfmt *output*, ~15/line). Returns an
   owned `String` per key even when no sanitization is needed. A `Cow<str>`
   fast-path (return `Borrowed` when the key has no space/`=`) removes almost
   all of these. Output-side, so out of this spec's parser scope.
3. **`Event.original_line` `String`** (1/line) and the **`FieldMap`** backing
   allocation (1/line) — structural, hard to remove without a columnar rewrite
   (§8 non-goal).
4. **Timestamp string** materialized in `identify_timestamp_field` when the ts
   field is stored as a string (1/line on this corpus).
5. **Output buffer / serde_json map** (JSON output builds a `serde_json::Map`
   with `String` keys — ~15/line — before serializing).

Sites 1 and 2 together are ~30 allocs/line and are the clear next targets; both
are addressable without the `smartstring` representation change.
