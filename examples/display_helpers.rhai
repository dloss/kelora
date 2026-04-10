// display_helpers.rhai — Visual and formatting helpers for terminal output
//
// Demonstrates: map.keep(), map.drop(), sample_prob(), dt.ceil_to(),
//   format_decimals(), format_percent(), human_bytes(), human_bytes_si(),
//   ljust(), rjust(), shorten(), bold(), dim(), red(), green(),
//   bar(), sparkline()
//
// ── Usage ────────────────────────────────────────────────────────────────────
//
// Slim output (noisy debug fields stripped):
//   kelora -j examples/api_logs.jsonl --exec-file examples/display_helpers.rhai
//
// Statistical sample — keep ~5% of events (useful for very large files):
//   kelora -j examples/api_logs.jsonl --filter 'sample_prob(0.05)'
//
// Project to a fixed set of fields only:
//   kelora -j examples/api_logs.jsonl \
//     --exec 'e.keep(["service", "level", "status", "response_time", "path"])'
//
// Format byte sizes from a storage or transfer field:
//   kelora -j examples/api_logs.jsonl \
//     --exec 'e.size_fmt = human_bytes(e.get_path("content_length", 0))'
//
// Full service health table (paste the --end block from SUMMARY below):
//   kelora -j examples/api_logs.jsonl -q -m \
//     --exec-file examples/display_helpers.rhai \
//     --end '...'

// ── Field projection ──────────────────────────────────────────────────────────
// Drop verbose debug fields; emit everything else.
// Swap for e.keep([...]) to whitelist specific fields instead.
e = e.drop(["stack_trace", "referer", "metadata"]);

// ── Time bucketing ────────────────────────────────────────────────────────────
// Bin each event into 5-minute windows and track volume per window.
// dt.ceil_to() rounds up to the next boundary of the given interval.
let ts = e.timestamp.parse_timestamp();
if ts != () {
    let w = ts.ceil_to("5m").to_iso();
    e.window = w;
    track_count("win:" + w);
}

// ── Metric tracking ───────────────────────────────────────────────────────────
// Per-service request counts and response-time stats, consumed in --end below.
let svc = e.service ?? "unknown";
track_count(svc + ":reqs");
if e.level == "ERROR" { track_count(svc + ":errs"); }
track_stats("rt:" + svc, e.get_path("response_time", 0.0));

// ── SUMMARY: colour-coded service health table ────────────────────────────────
//
// Paste this block into --end to print a formatted table after all events.
//
// Uses:
//   ljust / rjust       — fixed-width column alignment
//   bar(v, max, width)  — block bar scaled to max; set max=1.0 for ratios
//   sparkline(array)    — one-line Unicode trend from an array of numbers
//   red / green / bold / dim — ANSI colour/style (no-ops when colour is off)
//   format_percent      — convert 0.0–1.0 ratio to "42.0%" string
//   format_decimals     — fixed decimal places, e.g. "0.347"
//
// kelora -j examples/api_logs.jsonl -q -m \
//   --exec-file examples/display_helpers.rhai \
//   --end '
//     let SLA  = 1.0;                             // p95 target in seconds
//     let SVCS = ["api-gateway", "auth-service"];
//
//     print(("Service".ljust(16) + "  Reqs".rjust(7) + "  Errs".rjust(7)
//            + "  Err%".rjust(7) + "  p95 vs 2×SLA").bold());
//
//     for svc in SVCS {
//       let reqs = metrics[svc + ":reqs"] ?? 0;
//       let errs = metrics[svc + ":errs"] ?? 0;
//       let p95  = metrics["rt:" + svc + "_p95"] ?? 0.0;
//       let pct  = if reqs > 0 { errs.to_float() / reqs.to_float() } else { 0.0 };
//
//       // bar spans 0 to 2×SLA; half-fill = at SLA, full-fill = twice the budget
//       let vis = bar(p95, SLA * 2.0, 14) + " " + format_decimals(p95, 3) + "s";
//       let row = svc.ljust(16)
//               + reqs.to_string().rjust(7)
//               + errs.to_string().rjust(7)
//               + format_percent(pct, 1).rjust(7)
//               + "  " + vis;
//
//       if pct > 0.05 || p95 > SLA {
//         print(("✗ " + row).red())
//       } else {
//         print(("✓ " + row).green())
//       }
//     }
//
//     // Sparkline of request volume across 5-minute windows
//     let wins = [];
//     for k in metrics.keys() {
//       if k.starts_with("win:") { wins.push(k); }
//     }
//     wins.sort();
//     let counts = [];
//     for k in wins { counts.push(metrics[k] ?? 0); }
//     print("");
//     print(("Volume/5min: " + sparkline(counts)).dim());
//   '
