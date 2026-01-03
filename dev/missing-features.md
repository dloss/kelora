# UX & CLI Improvement Ideas

Collected from first-time user experience testing. These are polish items - the core tool works well.

## 1. Fuzzy Function Suggestions (High Value) ✅ IMPLEMENTED

**Problem:** When a user types a wrong function name like `track_p99()`, they get:
```
Function not found: track_p99 (&str | ImmutableString | String, f64)
```

**Improvement:** Suggest similar functions:
```
Function not found: track_p99
Did you mean: track_percentiles() or track_stats()?
Run 'kelora --help-functions | grep percentile' for details.
```

**Implementation:** Levenshtein distance matching against registered function names. Could use `strsim` crate or simple edit distance. ~50-80 lines in error handling.

---

## 2. Shell Completions (Low Effort, High Value) ✅ IMPLEMENTED

**Problem:** No tab completion for flags, formats, or file arguments.

**Improvement:** Generate completions for bash/zsh/fish:
```bash
kelora --generate-completion bash > /etc/bash_completion.d/kelora
kelora --generate-completion zsh > ~/.zfunc/_kelora
kelora --generate-completion fish > ~/.config/fish/completions/kelora.fish
```

**Implementation:** Clap has built-in support via `clap_complete`. ~20-30 lines to wire up.

---

## 3. Dry-Run / Explain Mode (Medium Effort)

**Problem:** Hard to test complex pipelines on large files without processing everything.

**Improvement:** A `--explain` or `--dry-run` flag:
```bash
kelora huge.log.gz -f json --filter 'e.status >= 500' --explain
# Output:
# Format: json (auto-detected)
# Filter: e.status >= 500
# Sample (first 10 events): 3 would match
# Estimated: ~30% of events pass filter
```

**Implementation:** Run pipeline on first N events, extrapolate. ~100-150 lines. Could reuse `--stats` infrastructure.

---

## 4. Quick Field Discovery (Low Effort, Nice to Have)

**Problem:** To see available fields, you need `--stats` which shows extra info.

**Improvement:** A `--keys` or `--fields` shortcut:
```bash
kelora api.jsonl --keys
# endpoint,error,level,request_id,status,timestamp,token,user

kelora api.jsonl --keys --json
# ["endpoint","error","level","request_id","status","timestamp","token","user"]
```

**Implementation:** Subset of `--stats` output. ~20 lines.

---

## Priority Summary

| Feature | Effort | User Value | Status |
|---------|--------|------------|--------|
| Fuzzy function suggestions | Medium | High | ✅ Done |
| Shell completions | Low | High | ✅ Done |
| `--explain` dry-run | Medium | Medium | ❌ Skip - unreliable estimates |
| `--keys` shortcut | Low | Low | ❌ Skip - convenience bloat |

## Notes

These suggestions came from a first-time user walkthrough. The tool's error messages are already helpful - these would make them even better.
