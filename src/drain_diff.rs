//! `--drain-diff`: template-level comparison of a baseline and a target log.
//!
//! Two logical passes over one shared drain instance:
//!
//! **Pass 1 (streaming)** — every event's mined field is added to the joint
//! drain tree (`crate::drain::drain_record`, learning mode) while this module
//! counts occurrences of each *unique* field value per side. Logs are
//! repetitive, so the unique-value map is far smaller than the event stream;
//! it doubles as the buffered corpus for pass 2, which makes stdin and
//! `--cut` first-class (no re-reading of inputs, no seekability concerns).
//!
//! **Pass 2 (finalize)** — the template set is frozen
//! (`crate::drain::frozen_template_set`) and each unique value is matched
//! against it read-only (no learning), multiplying by its per-side count.
//! Counting against the *final* template set makes results independent of
//! when a template stabilized mid-stream.
//!
//! Memory is bounded by `MAX_UNIQUE_VALUES`; exceeding it refuses the report
//! rather than emitting a diff computed from a truncated corpus.
//!
//! Thresholds are hardcoded by design (the spec's zero-config stance): volume
//! shifts are reported at |Δ share| ≥ 1.0 percentage points, templates with a
//! combined count below 2 are ignored, and NEW templates are exempt from the
//! floor — a template appearing even once only in the target is exactly the
//! incident signal the mode exists for. Anyone needing different cutoffs uses
//! `--drain-diff=json` and filters downstream.

use std::cell::RefCell;
use std::collections::HashMap;

/// Which side of the comparison an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Baseline,
    Target,
}

/// Volume shifts are reported at |Δ share| ≥ this many percentage points.
const SHIFT_THRESHOLD_PP: f64 = 1.0;

/// Templates with baseline+target count below this are ignored (NEW exempt).
const NOISE_FLOOR_COMBINED: u64 = 2;

/// Cap on distinct mined field values held in memory for pass 2. Logs are
/// repetitive, so legitimate inputs sit far below this; a field where nearly
/// every value is unique produces useless templates anyway. ~1M entries of
/// typical log-message size is on the order of a few hundred MB worst case.
const MAX_UNIQUE_VALUES: usize = 1_000_000;

#[derive(Debug, Default)]
struct DiffState {
    /// Unique mined field value -> [baseline count, target count].
    counts: HashMap<String, [u64; 2]>,
    /// True once `counts` refused an insertion; the report is then invalid.
    cap_exceeded: bool,
    /// Events excluded in --cut mode because they carry no parseable timestamp.
    excluded_no_timestamp: u64,
}

thread_local! {
    static DIFF_STATE: RefCell<DiffState> = RefCell::new(DiffState::default());
}

pub fn reset() {
    DIFF_STATE.with(|state| {
        *state.borrow_mut() = DiffState::default();
    });
}

/// Record one event's mined field value on the given side (pass 1 counting;
/// the caller separately feeds the joint drain tree via `drain_record`).
pub fn record(text: &str, side: DiffSide) {
    DIFF_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let idx = match side {
            DiffSide::Baseline => 0,
            DiffSide::Target => 1,
        };
        if let Some(entry) = state.counts.get_mut(text) {
            entry[idx] += 1;
        } else if state.counts.len() >= MAX_UNIQUE_VALUES {
            state.cap_exceeded = true;
        } else {
            let mut entry = [0u64; 2];
            entry[idx] = 1;
            state.counts.insert(text.to_string(), entry);
        }
    });
}

/// Record an event dropped from the comparison in --cut mode because it has
/// no parseable timestamp. Surfaced as a warning with the report.
pub fn record_excluded_no_timestamp() {
    DIFF_STATE.with(|state| {
        state.borrow_mut().excluded_no_timestamp += 1;
    });
}

/// One template's numbers in the diff report. Shares are fractions of the
/// side's total events (0.021 = 2.1%); `delta_pp` is target share minus
/// baseline share in percentage points.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub template: String,
    pub template_id: String,
    pub baseline_count: u64,
    pub target_count: u64,
    pub baseline_share: f64,
    pub target_share: f64,
    pub delta_pp: f64,
}

#[derive(Debug, Clone)]
pub struct DiffReport {
    pub new: Vec<DiffEntry>,
    pub vanished: Vec<DiffEntry>,
    pub shifted: Vec<DiffEntry>,
    /// Templates present on both sides whose share moved less than the
    /// reporting threshold.
    pub unchanged_count: usize,
    pub baseline_total: u64,
    pub target_total: u64,
    /// Events whose field value matched no frozen template in pass 2. Should
    /// be impossible by construction (every value was added in pass 1); a
    /// nonzero count guards against future drain behavior changes.
    pub unmatched_events: u64,
    pub excluded_no_timestamp: u64,
}

/// Pass 2: freeze the jointly-mined template set, match every unique field
/// value against it, and classify templates into NEW / VANISHED / shifted.
///
/// Errors when the unique-value cap was exceeded during pass 1 — a diff over a
/// truncated corpus would be silently wrong, so it is refused instead.
pub fn finalize() -> Result<DiffReport, String> {
    DIFF_STATE.with(|state| {
        let state = state.borrow();
        if state.cap_exceeded {
            return Err(format!(
                "--drain-diff exceeded the cap of {} distinct field values; the field is too high-cardinality to template meaningfully. Normalize it first (e.g. --exec to strip IDs) or --filter the stream down.",
                MAX_UNIQUE_VALUES
            ));
        }

        // Per-template per-side counts, keyed on the normalized template
        // string (the same identity generate_template_id hashes).
        let mut per_template: HashMap<String, (String, [u64; 2])> = HashMap::new();
        let mut unmatched: u64 = 0;
        let mut baseline_total: u64 = 0;
        let mut target_total: u64 = 0;

        let frozen = crate::drain::frozen_template_set();
        for (text, counts) in &state.counts {
            baseline_total += counts[0];
            target_total += counts[1];
            match frozen.match_text(text) {
                Some(template) => {
                    let key = normalize_key(template);
                    let entry = per_template
                        .entry(key)
                        .or_insert_with(|| (template.to_string(), [0u64; 2]));
                    entry.1[0] += counts[0];
                    entry.1[1] += counts[1];
                }
                None => unmatched += counts[0] + counts[1],
            }
        }

        let share = |count: u64, total: u64| -> f64 {
            if total == 0 {
                0.0
            } else {
                count as f64 / total as f64
            }
        };

        let mut new = Vec::new();
        let mut vanished = Vec::new();
        let mut shifted = Vec::new();
        let mut unchanged_count = 0usize;

        for (_, (template, counts)) in per_template {
            let (b, t) = (counts[0], counts[1]);
            let entry = DiffEntry {
                template_id: crate::drain::generate_template_id(&template),
                template,
                baseline_count: b,
                target_count: t,
                baseline_share: share(b, baseline_total),
                target_share: share(t, target_total),
                delta_pp: (share(t, target_total) - share(b, baseline_total)) * 100.0,
            };
            if b == 0 && t > 0 {
                // NEW templates bypass the noise floor down to count 1.
                new.push(entry);
            } else if t == 0 && b > 0 {
                if b >= NOISE_FLOOR_COMBINED {
                    vanished.push(entry);
                }
                // A baseline singleton that vanished is below the noise floor.
            } else if b > 0 && t > 0 {
                if entry.delta_pp.abs() >= SHIFT_THRESHOLD_PP {
                    shifted.push(entry);
                } else {
                    unchanged_count += 1;
                }
            }
        }

        // Sorting replaces threshold flags: counts descending for new/vanished,
        // |Δ share| descending for shifts; template string breaks ties so the
        // output is deterministic across runs.
        new.sort_by(|a, b| {
            b.target_count
                .cmp(&a.target_count)
                .then_with(|| a.template.cmp(&b.template))
        });
        vanished.sort_by(|a, b| {
            b.baseline_count
                .cmp(&a.baseline_count)
                .then_with(|| a.template.cmp(&b.template))
        });
        shifted.sort_by(|a, b| {
            b.delta_pp
                .abs()
                .partial_cmp(&a.delta_pp.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.template.cmp(&b.template))
        });

        Ok(DiffReport {
            new,
            vanished,
            shifted,
            unchanged_count,
            baseline_total,
            target_total,
            unmatched_events: unmatched,
            excluded_no_timestamp: state.excluded_no_timestamp,
        })
    })
}

/// Same whitespace normalization drain.rs keys metadata on; duplicated here
/// only in behavior (via the public template id) — templates whose normalized
/// forms agree must aggregate into one diff row.
fn normalize_key(template: &str) -> String {
    template.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn pct(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

fn signed_pp(delta: f64) -> String {
    format!(
        "{}{:.1}pp",
        if delta >= 0.0 { "+" } else { "-" },
        delta.abs()
    )
}

/// Format the report as the three-section human-readable table. Sections with
/// zero entries print a single line rather than disappearing — absence of
/// change is information.
pub fn format_report_text(report: &DiffReport, use_colors: bool) -> String {
    let (red, gray, green, bold, reset) = if use_colors {
        ("\x1b[31m", "\x1b[90m", "\x1b[32m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };

    let plural = |n: usize| if n == 1 { "template" } else { "templates" };
    let count_width = |counts: &mut dyn Iterator<Item = u64>| -> usize {
        counts.map(|c| c.to_string().len()).max().unwrap_or(1)
    };

    let mut out = String::new();

    if report.new.is_empty() {
        out.push_str(&format!(
            "{}NEW in target:{} no new templates\n",
            bold, reset
        ));
    } else {
        out.push_str(&format!(
            "{}NEW in target ({} {}):{}\n",
            bold,
            report.new.len(),
            plural(report.new.len()),
            reset
        ));
        let width = count_width(&mut report.new.iter().map(|e| e.target_count));
        for entry in &report.new {
            out.push_str(&format!(
                "  {:>w$}  {}{}{}\n",
                entry.target_count,
                red,
                entry.template,
                reset,
                w = width
            ));
        }
    }
    out.push('\n');

    if report.vanished.is_empty() {
        out.push_str(&format!(
            "{}VANISHED from target:{} no vanished templates\n",
            bold, reset
        ));
    } else {
        out.push_str(&format!(
            "{}VANISHED from target ({} {}):{}\n",
            bold,
            report.vanished.len(),
            plural(report.vanished.len()),
            reset
        ));
        let width = count_width(&mut report.vanished.iter().map(|e| e.baseline_count));
        for entry in &report.vanished {
            out.push_str(&format!(
                "  {:>w$}  {}{}{}          (baseline count)\n",
                entry.baseline_count,
                gray,
                entry.template,
                reset,
                w = width
            ));
        }
    }
    out.push('\n');

    if report.shifted.is_empty() {
        out.push_str(&format!(
            "{}VOLUME SHIFTS:{} no volume shifts\n",
            bold, reset
        ));
    } else {
        out.push_str(&format!(
            "{}VOLUME SHIFTS ({} {}):{}\n",
            bold,
            report.shifted.len(),
            plural(report.shifted.len()),
            reset
        ));
        for entry in &report.shifted {
            let delta_color = if entry.delta_pp >= 0.0 { red } else { green };
            out.push_str(&format!("  {}\n", entry.template));
            out.push_str(&format!(
                "    baseline: {} ({})  \u{2192}  target: {} ({})   \u{0394} {}{}{}\n",
                entry.baseline_count,
                pct(entry.baseline_share),
                entry.target_count,
                pct(entry.target_share),
                delta_color,
                signed_pp(entry.delta_pp),
                reset,
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "totals: baseline {} events, target {} events, {} shared {} unchanged",
        report.baseline_total,
        report.target_total,
        report.unchanged_count,
        plural(report.unchanged_count),
    ));

    out
}

/// Format the report as one JSON object for scripting and agent use. Shares
/// are emitted in percent so they share units with `delta_pp`.
pub fn format_report_json(report: &DiffReport) -> String {
    let entry_new_vanished = |e: &DiffEntry, count: u64, share: f64| {
        serde_json::json!({
            "template": e.template,
            "template_id": e.template_id,
            "count": count,
            "share_pct": share * 100.0,
        })
    };

    let json = serde_json::json!({
        "new": report
            .new
            .iter()
            .map(|e| entry_new_vanished(e, e.target_count, e.target_share))
            .collect::<Vec<_>>(),
        "vanished": report
            .vanished
            .iter()
            .map(|e| entry_new_vanished(e, e.baseline_count, e.baseline_share))
            .collect::<Vec<_>>(),
        "shifted": report
            .shifted
            .iter()
            .map(|e| {
                serde_json::json!({
                    "template": e.template,
                    "template_id": e.template_id,
                    "baseline_count": e.baseline_count,
                    "target_count": e.target_count,
                    "baseline_pct": e.baseline_share * 100.0,
                    "target_pct": e.target_share * 100.0,
                    "delta_pp": e.delta_pp,
                })
            })
            .collect::<Vec<_>>(),
        "unchanged_count": report.unchanged_count,
        "baseline_events": report.baseline_total,
        "target_events": report.target_total,
        "unmatched_events": report.unmatched_events,
        "excluded_no_timestamp": report.excluded_no_timestamp,
    });
    serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(template: &str, b: u64, t: u64, b_total: u64, t_total: u64) -> DiffEntry {
        let bs = if b_total == 0 {
            0.0
        } else {
            b as f64 / b_total as f64
        };
        let ts = if t_total == 0 {
            0.0
        } else {
            t as f64 / t_total as f64
        };
        DiffEntry {
            template: template.to_string(),
            template_id: crate::drain::generate_template_id(template),
            baseline_count: b,
            target_count: t,
            baseline_share: bs,
            target_share: ts,
            delta_pp: (ts - bs) * 100.0,
        }
    }

    /// Drive record()/finalize() through the same thread-local machinery the
    /// pipeline uses. Tests touching this must reset both drain states.
    fn reset_all() {
        crate::drain::reset();
        reset();
    }

    fn mine_and_record(text: &str, side: DiffSide) {
        crate::drain::drain_record(text, None, None).expect("drain_record");
        record(text, side);
    }

    #[test]
    fn self_diff_is_empty() {
        reset_all();
        for side in [DiffSide::Baseline, DiffSide::Target] {
            mine_and_record("connect to 10.0.0.1", side);
            mine_and_record("connect to 10.0.0.2", side);
            mine_and_record("worker started", side);
        }
        let report = finalize().expect("finalize");
        assert!(report.new.is_empty());
        assert!(report.vanished.is_empty());
        assert!(report.shifted.is_empty());
        assert_eq!(report.unchanged_count, 2);
        assert_eq!(report.baseline_total, 3);
        assert_eq!(report.target_total, 3);
        assert_eq!(report.unmatched_events, 0);
    }

    #[test]
    fn injected_novel_line_is_new_with_exact_count() {
        reset_all();
        for i in 0..50 {
            mine_and_record(
                &format!("request from 10.0.0.{}", i % 8),
                DiffSide::Baseline,
            );
            mine_and_record(&format!("request from 10.0.0.{}", i % 8), DiffSide::Target);
        }
        for _ in 0..3 {
            mine_and_record("OOM killer invoked for process 4242", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        assert_eq!(report.new.len(), 1, "exactly one NEW template");
        assert_eq!(report.new[0].target_count, 3);
        assert!(report.vanished.is_empty());
        assert_eq!(report.unmatched_events, 0);
    }

    #[test]
    fn new_singleton_bypasses_noise_floor_but_vanished_singleton_does_not() {
        reset_all();
        // Shared bulk so totals are non-trivial.
        for side in [DiffSide::Baseline, DiffSide::Target] {
            for _ in 0..10 {
                mine_and_record("heartbeat ok", side);
            }
        }
        mine_and_record(
            "certificate for a.example.com expired 3d ago",
            DiffSide::Target,
        );
        mine_and_record("one lonely baseline-only message", DiffSide::Baseline);
        let report = finalize().expect("finalize");
        assert_eq!(report.new.len(), 1, "NEW singleton must be reported");
        assert_eq!(report.new[0].target_count, 1);
        assert!(
            report.vanished.is_empty(),
            "vanished singleton is below the noise floor"
        );
    }

    #[test]
    fn proportional_sides_produce_no_shifts() {
        reset_all();
        // Target is 3x the baseline volume with identical proportions.
        for _ in 0..20 {
            mine_and_record("user alice@example.com logged in", DiffSide::Baseline);
        }
        for _ in 0..10 {
            mine_and_record("upstream a.example.com returned 502", DiffSide::Baseline);
        }
        for _ in 0..60 {
            mine_and_record("user alice@example.com logged in", DiffSide::Target);
        }
        for _ in 0..30 {
            mine_and_record("upstream a.example.com returned 502", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        assert!(report.new.is_empty());
        assert!(report.vanished.is_empty());
        assert!(
            report.shifted.is_empty(),
            "share math must not flag proportionally identical sides: {:?}",
            report.shifted
        );
        assert_eq!(report.unchanged_count, 2);
    }

    #[test]
    fn volume_shift_detected_with_sign() {
        reset_all();
        for _ in 0..90 {
            mine_and_record("heartbeat ok", DiffSide::Baseline);
        }
        for _ in 0..10 {
            mine_and_record("upstream a.example.com returned 502", DiffSide::Baseline);
        }
        for _ in 0..50 {
            mine_and_record("heartbeat ok", DiffSide::Target);
        }
        for _ in 0..50 {
            mine_and_record("upstream a.example.com returned 502", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        assert_eq!(report.shifted.len(), 2);
        let up = report
            .shifted
            .iter()
            .find(|e| e.template.contains("upstream"))
            .expect("upstream shift");
        assert!(up.delta_pp > 0.0, "grew from 10% to 50%");
        assert!((up.delta_pp - 40.0).abs() < 0.01);
        let down = report
            .shifted
            .iter()
            .find(|e| e.template.contains("heartbeat"))
            .expect("heartbeat shift");
        assert!(down.delta_pp < 0.0, "shrank from 90% to 50%");
    }

    #[test]
    fn empty_sides_do_not_divide_by_zero() {
        reset_all();
        mine_and_record("only baseline has data", DiffSide::Baseline);
        mine_and_record("only baseline has data", DiffSide::Baseline);
        let report = finalize().expect("finalize");
        assert_eq!(report.target_total, 0);
        assert_eq!(report.vanished.len(), 1);
        assert!(report.vanished[0].target_share == 0.0);
    }

    #[test]
    fn cap_exceeded_refuses_report() {
        reset_all();
        DIFF_STATE.with(|state| state.borrow_mut().cap_exceeded = true);
        let err = finalize().expect_err("capped state must refuse the report");
        assert!(err.contains("distinct field values"));
    }

    #[test]
    fn text_output_has_three_sections_and_totals() {
        let report = DiffReport {
            new: vec![entry(
                "OOM killer invoked for process <num>",
                0,
                3412,
                9014,
                14903,
            )],
            vanished: vec![entry(
                "Connection pool recycled for <fqdn>",
                438,
                0,
                9014,
                14903,
            )],
            shifted: vec![entry(
                "Upstream <fqdn> returned <num>",
                187,
                2205,
                9014,
                14903,
            )],
            unchanged_count: 41,
            baseline_total: 9014,
            target_total: 14903,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
        };
        let text = format_report_text(&report, false);
        assert!(text.contains("NEW in target (1 template):"));
        assert!(text.contains("3412  OOM killer invoked for process <num>"));
        assert!(text.contains("VANISHED from target (1 template):"));
        assert!(text.contains("(baseline count)"));
        assert!(text.contains("VOLUME SHIFTS (1 template):"));
        assert!(text.contains("\u{0394} +12.7pp"));
        assert!(text.contains(
            "totals: baseline 9014 events, target 14903 events, 41 shared templates unchanged"
        ));
    }

    #[test]
    fn empty_sections_print_single_lines() {
        let report = DiffReport {
            new: vec![],
            vanished: vec![],
            shifted: vec![],
            unchanged_count: 5,
            baseline_total: 100,
            target_total: 100,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
        };
        let text = format_report_text(&report, false);
        assert!(text.contains("NEW in target: no new templates"));
        assert!(text.contains("VANISHED from target: no vanished templates"));
        assert!(text.contains("VOLUME SHIFTS: no volume shifts"));
    }

    #[test]
    fn json_output_round_trips() {
        let report = DiffReport {
            new: vec![entry("a new <num>", 0, 7, 10, 20)],
            vanished: vec![entry("an old <num>", 5, 0, 10, 20)],
            shifted: vec![entry("a shared <num>", 5, 13, 10, 20)],
            unchanged_count: 3,
            baseline_total: 10,
            target_total: 20,
            unmatched_events: 0,
            excluded_no_timestamp: 2,
        };
        let json: serde_json::Value =
            serde_json::from_str(&format_report_json(&report)).expect("valid JSON");
        assert_eq!(json["new"][0]["count"], 7);
        assert_eq!(json["vanished"][0]["count"], 5);
        assert_eq!(json["shifted"][0]["baseline_count"], 5);
        assert_eq!(json["unchanged_count"], 3);
        assert_eq!(json["baseline_events"], 10);
        assert_eq!(json["target_events"], 20);
        assert_eq!(json["excluded_no_timestamp"], 2);
        // share_pct and delta_pp share units (percent).
        assert!((json["new"][0]["share_pct"].as_f64().unwrap() - 35.0).abs() < 0.01);
    }
}
