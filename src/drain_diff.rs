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
//! Thresholds are hardcoded by design (the spec's zero-config stance):
//! templates with a combined count below 2 are ignored, and NEW templates are
//! exempt from the floor — a template appearing even once only in the target
//! is exactly the incident signal the mode exists for. Anyone needing
//! different cutoffs uses `--drain-diff=json` and filters downstream.
//!
//! Volume shifts (templates present on both sides) must clear two bars: the
//! move has to be **bigger than sampling noise** for the number of events on
//! each side (a two-proportion z-test with continuity correction, at
//! `Z_CRITICAL`), *and* it has to be **big enough to act on** — either
//! `MIN_DELTA_PP` percentage points of share, or a `MIN_RATE_RATIO`-fold
//! change in rate.
//!
//! Significance alone is not enough. A fixed pp threshold misfires as noise on
//! small samples (a single event moving a 20-event side by several points of
//! share), which the z-test fixes; but the z-test alone reports operationally
//! meaningless rows on huge samples, because with 50k events per side a 0.06pp
//! wobble is already "significant". Per-template testing has no multiplicity
//! control either, so on a log with hundreds of templates a handful of such
//! rows appear by chance. The effect-size bar filters exactly those (they are
//! false precisely because they are tiny) without making one template's
//! verdict depend on how many *other* templates happened to be mined, the way
//! a Bonferroni-style correction would.
//!
//! The two effect-size conditions are an OR because they cover opposite ends
//! of the share range: a dominant template matters in absolute pp (60% → 50%
//! is 10pp of traffic), while a rare one matters as a multiple (5 → 500
//! occurrences in a 100k log is under 0.5pp but a 100-fold explosion — the
//! same incident signal NEW's floor exemption exists to protect).
//!
//! This gate applies only to the shifted category — NEW's floor exemption
//! and VANISHED's fixed floor are untouched, since scaling those away would
//! risk silencing rare-but-real incident signals the mode exists to surface.

use std::cell::RefCell;
use std::collections::HashMap;

/// Which side of the comparison an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Baseline,
    Target,
}

/// Two-proportion z-test critical value gating volume shifts (~95%
/// two-sided confidence; compared against |z|). Applied only to templates
/// present on both sides; NEW/VANISHED classification is unaffected.
const Z_CRITICAL: f64 = 1.96;

/// Effect-size floor #1: absolute share move, in percentage points. Catches
/// shifts in high-share templates, where a modest multiple is a lot of traffic.
const MIN_DELTA_PP: f64 = 0.5;

/// Effect-size floor #2: fold change in rate (share ratio, either direction).
/// Catches shifts in low-share templates, where a large multiple is still a
/// small slice of the whole.
const MIN_RATE_RATIO: f64 = 1.5;

/// A |Δpp| below this rounds to `0.0pp` in the report, so a template moving
/// less than this is not described as having "moved" in the within-noise note.
const DISPLAY_EPSILON_PP: f64 = 0.05;

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
    /// Events excluded because the mined field carried no text (absent, or
    /// present but empty).
    excluded_no_field: u64,
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

/// Record an event dropped from the comparison because the mined field carried
/// no text — absent from the event, or present with an empty value. A partial
/// count is a warning (heterogeneous logs are normal); *every* event excluded
/// this way means the field name never matched anything, which the caller turns
/// into an error rather than a diff over zero events.
pub fn record_excluded_no_field() {
    DIFF_STATE.with(|state| {
        state.borrow_mut().excluded_no_field += 1;
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
    /// Signed two-proportion z-test statistic for templates present on both
    /// sides (the `shifted` category), positive when the target share is the
    /// larger one; `None` for NEW/VANISHED, which are gated by the fixed noise
    /// floor instead. See `Z_CRITICAL`.
    pub z_score: Option<f64>,
}

/// Two-proportion z-test with continuity correction, signed to match the
/// direction of the move (positive = grew in the target) so JSON consumers get
/// direction and magnitude from one field. Callers must ensure
/// `baseline_total > 0 && target_total > 0` (guaranteed in the `finalize`
/// branch that calls this, since `b > 0` and `t > 0` there imply both side
/// totals are positive).
fn two_proportion_z(b: u64, t: u64, baseline_total: u64, target_total: u64) -> f64 {
    let (b, t) = (b as f64, t as f64);
    let (n1, n2) = (baseline_total as f64, target_total as f64);
    let p_pool = (b + t) / (n1 + n2);
    let se = (p_pool * (1.0 - p_pool) * (1.0 / n1 + 1.0 / n2)).sqrt();
    if se == 0.0 {
        return 0.0;
    }
    let diff = t / n2 - b / n1;
    let cc = 0.5 * (1.0 / n1 + 1.0 / n2);
    let magnitude = (diff.abs() - cc).max(0.0) / se;
    // Guard the sign flip on zero so the report never prints `-0.0`.
    if diff < 0.0 && magnitude > 0.0 {
        -magnitude
    } else {
        magnitude
    }
}

/// Whether a both-sides template's move is worth a row: bigger than sampling
/// noise *and* big enough to act on. See the module docs for why both bars are
/// needed and why the effect-size test is an OR.
fn is_reportable_shift(
    z_score: f64,
    delta_pp: f64,
    baseline_share: f64,
    target_share: f64,
) -> bool {
    if z_score.abs() < Z_CRITICAL {
        return false;
    }
    if delta_pp.abs() >= MIN_DELTA_PP {
        return true;
    }
    // Both shares are positive here (the caller's branch has b > 0 && t > 0,
    // which also makes both side totals positive), so the ratio is finite.
    let ratio = target_share / baseline_share;
    ratio >= MIN_RATE_RATIO || ratio <= 1.0 / MIN_RATE_RATIO
}

#[derive(Debug, Clone)]
pub struct DiffReport {
    pub new: Vec<DiffEntry>,
    pub vanished: Vec<DiffEntry>,
    pub shifted: Vec<DiffEntry>,
    /// Templates present on both sides whose move did not clear the reporting
    /// bars — either indistinguishable from sampling noise or too small to act
    /// on. "Within noise", not literally unchanged.
    pub unchanged_count: usize,
    /// How many of those were rejected as noise (not as too-small an effect)
    /// while moving by a visible amount — a |Δpp| that does not round to zero
    /// in the report. Drives the explanatory note, so a suppressed move never
    /// reads as a contradiction.
    pub within_noise_moved: usize,
    /// Largest |Δpp| among those, 0.0 when none moved.
    pub within_noise_max_delta_pp: f64,
    pub baseline_total: u64,
    pub target_total: u64,
    /// Events whose field value matched no frozen template in pass 2. Should
    /// be impossible by construction (every value was added in pass 1); a
    /// nonzero count guards against future drain behavior changes.
    pub unmatched_events: u64,
    pub excluded_no_timestamp: u64,
    /// Events excluded because the mined field carried no text. See
    /// `record_excluded_no_field`.
    pub excluded_no_field: u64,
}

impl DiffReport {
    /// Events that actually contributed to the comparison. Zero means the
    /// report says nothing about change whatever its sections claim, so callers
    /// must not present it as "nothing changed".
    pub fn compared_events(&self) -> u64 {
        self.baseline_total + self.target_total
    }
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
        let mut within_noise_moved = 0usize;
        let mut within_noise_max_delta_pp = 0.0f64;

        for (_, (template, counts)) in per_template {
            let (b, t) = (counts[0], counts[1]);
            let z_score = if b > 0 && t > 0 {
                Some(two_proportion_z(b, t, baseline_total, target_total))
            } else {
                None
            };
            let entry = DiffEntry {
                template_id: crate::drain::generate_template_id(&template),
                template,
                baseline_count: b,
                target_count: t,
                baseline_share: share(b, baseline_total),
                target_share: share(t, target_total),
                delta_pp: (share(t, target_total) - share(b, baseline_total)) * 100.0,
                z_score,
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
                let z = z_score.expect("computed above for b > 0 && t > 0");
                if is_reportable_shift(z, entry.delta_pp, entry.baseline_share, entry.target_share) {
                    shifted.push(entry);
                } else {
                    unchanged_count += 1;
                    let moved = entry.delta_pp.abs();
                    // Only noise rejections feed the note, so its wording stays
                    // literally true. An effect-size rejection is by definition
                    // below MIN_DELTA_PP, so it can never be the conspicuously
                    // bigger move a reader would notice going missing.
                    if moved >= DISPLAY_EPSILON_PP && z.abs() < Z_CRITICAL {
                        within_noise_moved += 1;
                        within_noise_max_delta_pp = within_noise_max_delta_pp.max(moved);
                    }
                }
            }
        }

        // Sorting replaces threshold flags: counts descending for new/vanished,
        // |Δ share| descending for shifts; template string breaks ties so the
        // output is deterministic across runs. Shifts sort by magnitude rather
        // than by z because magnitude is what an operator acts on — the gate
        // has already removed everything that is only noise.
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
            within_noise_moved,
            within_noise_max_delta_pp,
            baseline_total,
            target_total,
            unmatched_events: unmatched,
            excluded_no_timestamp: state.excluded_no_timestamp,
            excluded_no_field: state.excluded_no_field,
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

/// How much more (or less) of the log this template is, as a plain multiple:
/// `14× more frequent`. Computed from *shares*, not raw counts, so sides of
/// different sizes compare fairly — the same quantity `MIN_RATE_RATIO` gates.
///
/// This is what the shift line shows instead of the change in percentage
/// points, because the two shares printed beside it already carry that: the eye
/// subtracts 1.8% from 25.0% for free, but it does not divide.
fn rate_multiple(entry: &DiffEntry) -> String {
    // Everything in the shifted list has counts on both sides, so both shares
    // are positive. Defensive fallback only, in case that ever changes.
    if entry.baseline_share <= 0.0 || entry.target_share <= 0.0 {
        return signed_pp(entry.delta_pp);
    }
    let ratio = entry.target_share / entry.baseline_share;
    let (factor, direction) = if ratio >= 1.0 {
        (ratio, "more")
    } else {
        (1.0 / ratio, "less")
    };
    // A tenth matters near the 1.5x bar, where "1.5x" and "2x" are different
    // claims; past 10x it is noise, and "14x" reads better than "13.8x".
    if factor >= 10.0 {
        format!("{:.0}\u{d7} {} frequent", factor, direction)
    } else {
        format!("{:.1}\u{d7} {} frequent", factor, direction)
    }
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
                "    baseline: {} ({})  \u{2192}  target: {} ({})   {}{}{}\n",
                entry.baseline_count,
                pct(entry.baseline_share),
                entry.target_count,
                pct(entry.target_share),
                delta_color,
                rate_multiple(entry),
                reset,
            ));
        }
    }

    // Without this, a report can look self-contradictory: a template that moved
    // by a visible amount would silently sit in the totals line as if it had
    // not moved. Only fires in that case, so ordinary reports stay quiet.
    if let Some(note) = within_noise_note(report) {
        out.push_str(&format!("  {}{}{}\n", gray, note, reset));
    }
    out.push('\n');

    out.push_str(&format!(
        "totals: baseline {} events, target {} events, {} shared {} within noise",
        report.baseline_total,
        report.target_total,
        report.unchanged_count,
        plural(report.unchanged_count),
    ));

    out
}

/// The explanation for suppressed moves, phrased for someone who did not come
/// here for statistics: event counts, and the implied fix (collect more).
/// `None` — the common case — when nothing held back moved by enough for its
/// absence to read as an omission.
fn within_noise_note(report: &DiffReport) -> Option<String> {
    /// A suppressed move this large registers on the report's own scale (a full
    /// point of the side's traffic), so leaving it unexplained invites "where
    /// did my drop go?". Below it, nobody is counting.
    const NOTE_FLOOR_PP: f64 = 1.0;

    if report.within_noise_moved == 0 || report.within_noise_max_delta_pp < NOTE_FLOOR_PP {
        return None;
    }
    let n = report.within_noise_moved;
    Some(format!(
        "{} {}{} moved, but {}/{} events is too few to be sure {} real",
        n,
        // "2 more templates" reads wrong when no shifts were listed above it.
        if report.shifted.is_empty() {
            ""
        } else {
            "more "
        },
        if n == 1 { "template" } else { "templates" },
        report.baseline_total,
        report.target_total,
        if n == 1 { "it's" } else { "they're" },
    ))
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
                    "z_score": e.z_score,
                })
            })
            .collect::<Vec<_>>(),
        "unchanged_count": report.unchanged_count,
        "baseline_events": report.baseline_total,
        "target_events": report.target_total,
        "unmatched_events": report.unmatched_events,
        "excluded_no_timestamp": report.excluded_no_timestamp,
        "excluded_no_field": report.excluded_no_field,
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
        let z_score = if b > 0 && t > 0 {
            Some(two_proportion_z(b, t, b_total, t_total))
        } else {
            None
        };
        DiffEntry {
            template: template.to_string(),
            template_id: crate::drain::generate_template_id(template),
            baseline_count: b,
            target_count: t,
            baseline_share: bs,
            target_share: ts,
            delta_pp: (ts - bs) * 100.0,
            z_score,
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
    fn small_sample_single_count_wobble_is_not_a_shift() {
        // Mirrors a real small log (~20 events/side): several templates each
        // move by exactly one occurrence, which used to clear the old fixed
        // 1.0pp threshold on every single one of them. None of these should
        // be statistically distinguishable from noise at this sample size.
        reset_all();
        for _ in 0..2 {
            mine_and_record(
                "GET /api/x completed in 12ms with status 200",
                DiffSide::Baseline,
            );
        }
        for _ in 0..2 {
            mine_and_record(
                "GET /api/x completed in 12ms with status 200",
                DiffSide::Target,
            );
        }
        for _ in 0..21 {
            mine_and_record("heartbeat ok", DiffSide::Baseline);
        }
        for _ in 0..16 {
            mine_and_record("heartbeat ok", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        assert!(
            report.shifted.is_empty(),
            "single-occurrence wobble on ~20-event sides must not read as a shift: {:?}",
            report.shifted
        );
        assert_eq!(report.unchanged_count, 2);
    }

    #[test]
    fn large_sample_shift_still_fires_under_z_test() {
        // Mirrors the deploy_before/after.jsonl example: baseline 1.8% share
        // growing to 25% in the target. Overwhelmingly significant, and must
        // still be reported under the z-test gate, not just the old pp one.
        reset_all();
        for _ in 0..2 {
            mine_and_record("upstream payments returned 503", DiffSide::Baseline);
        }
        for _ in 0..108 {
            mine_and_record("client connected ok", DiffSide::Baseline);
        }
        for _ in 0..30 {
            mine_and_record("upstream payments returned 503", DiffSide::Target);
        }
        for _ in 0..90 {
            mine_and_record("client connected ok", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        let up = report
            .shifted
            .iter()
            .find(|e| e.template.contains("upstream"))
            .expect("large, real shift must be reported");
        assert!(up.z_score.expect("z_score set for shifted entries") > Z_CRITICAL);
    }

    /// Convenience for the effect-size table below: classify a both-sides
    /// template straight from counts, the way `finalize` does.
    fn classify(b: u64, t: u64, n1: u64, n2: u64) -> (f64, bool) {
        let e = entry("t <num>", b, t, n1, n2);
        let z = e.z_score.expect("both sides present");
        (
            z,
            is_reportable_shift(z, e.delta_pp, e.baseline_share, e.target_share),
        )
    }

    #[test]
    fn significant_but_trivial_move_is_not_a_shift() {
        // Two 50k-event logs drawn from the same distribution: with samples
        // this large, pure sampling wobble clears the z-test (there is no
        // multiplicity control across templates), so the effect-size bar is
        // what keeps 0.06pp rows out of the report.
        let (z, reported) = classify(106, 138, 50_000, 50_000);
        assert!(z.abs() >= Z_CRITICAL, "significant by z alone: {}", z);
        assert!(!reported, "0.06pp is not worth a row at any sample size");

        let (z, reported) = classify(109, 79, 50_000, 50_000);
        assert!(z.abs() >= Z_CRITICAL, "significant by z alone: {}", z);
        assert!(!reported);
    }

    #[test]
    fn rare_template_multiplying_is_reported_below_the_pp_floor() {
        // 5 -> 500 occurrences in a 100k log is +0.495pp — under MIN_DELTA_PP,
        // but a 100-fold explosion. Exactly the incident signal NEW's floor
        // exemption exists to protect, so the rate ratio has to catch it.
        let e = entry("t <num>", 5, 500, 100_000, 100_000);
        assert!(e.delta_pp.abs() < MIN_DELTA_PP, "below the pp floor");
        let (_, reported) = classify(5, 500, 100_000, 100_000);
        assert!(reported, "a 100x rate change must be reported");
    }

    #[test]
    fn high_share_drift_is_reported_below_the_rate_floor() {
        // 10% -> 10.9% at 10k events per side: only a 1.09x rate change, but
        // +0.86pp of traffic, and statistically solid. The pp floor catches it.
        let e = entry("t <num>", 1000, 1090, 10_000, 10_000);
        let ratio = e.target_share / e.baseline_share;
        assert!(ratio < MIN_RATE_RATIO, "below the rate floor: {}", ratio);
        let (_, reported) = classify(1000, 1090, 10_000, 10_000);
        assert!(reported, "+0.86pp of traffic must be reported");
    }

    #[test]
    fn z_score_is_signed_and_zero_when_variance_vanishes() {
        let (grew, _) = classify(2, 30, 110, 120);
        assert!(grew > 0.0, "growth is positive: {}", grew);
        let (shrank, _) = classify(30, 2, 120, 110);
        assert!(shrank < 0.0, "decline is negative: {}", shrank);
        assert!(
            (grew + shrank).abs() < 1e-9,
            "swapping the sides only flips the sign"
        );
        // A log with a single template: every event is that template on both
        // sides, so the pooled proportion is 1 and there is no variance left.
        assert_eq!(two_proportion_z(40, 60, 40, 60), 0.0);
    }

    #[test]
    fn new_and_vanished_carry_no_z_score() {
        reset_all();
        for _ in 0..40 {
            mine_and_record("steady state ok", DiffSide::Baseline);
            mine_and_record("steady state ok", DiffSide::Target);
        }
        for _ in 0..5 {
            mine_and_record("cache warmed in 12ms", DiffSide::Baseline);
            mine_and_record("OOM killer invoked for process 4242", DiffSide::Target);
        }
        let report = finalize().expect("finalize");
        assert!(report.new.iter().all(|e| e.z_score.is_none()));
        assert!(report.vanished.iter().all(|e| e.z_score.is_none()));
        let json: serde_json::Value =
            serde_json::from_str(&format_report_json(&report)).expect("valid JSON");
        // The z-less categories omit the key rather than emitting null.
        assert!(json["new"][0].get("z_score").is_none());
        assert!(json["vanished"][0].get("z_score").is_none());
    }

    #[test]
    fn within_noise_note_explains_a_suppressed_bigger_move() {
        // The deploy example: a +23.2pp rise reported, and its complementary
        // -13.6pp drop just under the bar. Without the note the report would
        // look self-contradictory.
        let report = DiffReport {
            new: vec![],
            vanished: vec![],
            shifted: vec![entry("upstream <fqdn> returned <num>", 2, 30, 110, 120)],
            unchanged_count: 2,
            within_noise_moved: 2,
            within_noise_max_delta_pp: 13.6,
            baseline_total: 110,
            target_total: 120,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
        };
        let text = format_report_text(&report, false);
        assert!(
            text.contains(
                "2 more templates moved, but 110/120 events is too few to be sure they're real"
            ),
            "text: {}",
            text
        );

        // Nothing held back moved by a full point of traffic: nothing to explain.
        let quiet = DiffReport {
            within_noise_max_delta_pp: 0.6,
            ..report.clone()
        };
        assert!(!format_report_text(&quiet, false).contains("too few to be sure"));

        // Same with no shifts shown at all — the note is not a zero-match nag.
        let quiet = DiffReport {
            shifted: vec![],
            within_noise_max_delta_pp: 0.9,
            ..report.clone()
        };
        assert!(!format_report_text(&quiet, false).contains("too few to be sure"));

        // Nothing reported and a large move held back: "more" would be wrong.
        let lone = DiffReport {
            shifted: vec![],
            within_noise_moved: 1,
            within_noise_max_delta_pp: 4.0,
            ..report
        };
        let text = format_report_text(&lone, false);
        assert!(
            text.contains("1 template moved, but 110/120 events is too few to be sure it's real"),
            "text: {}",
            text
        );
    }

    #[test]
    fn rate_multiple_reads_as_a_plain_factor() {
        // 1.8% -> 25.0% of the side's lines. Past 10x the tenth is noise.
        assert_eq!(
            rate_multiple(&entry("t <num>", 2, 30, 110, 120)),
            "14\u{d7} more frequent"
        );
        // Declines invert the ratio rather than printing a fraction.
        assert_eq!(
            rate_multiple(&entry("t <num>", 30, 2, 120, 110)),
            "14\u{d7} less frequent"
        );
        // Near the MIN_RATE_RATIO bar the tenth is the whole claim.
        assert_eq!(
            rate_multiple(&entry("t <num>", 100, 160, 1000, 1000)),
            "1.6\u{d7} more frequent"
        );
        // Raw counts would say 2x here; shares say the rate held steady.
        assert_eq!(
            rate_multiple(&entry("t <num>", 100, 200, 1000, 2000)),
            "1.0\u{d7} more frequent"
        );
        // Defensive fallback for a degenerate entry (not reachable via finalize).
        assert_eq!(rate_multiple(&entry("t <num>", 0, 30, 110, 120)), "+25.0pp");
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
    fn excluded_no_field_is_carried_into_the_report() {
        // Every event the stage could not mine is counted, so the caller can
        // tell "no templates differ" from "nothing was ever compared".
        reset_all();
        mine_and_record("worker started", DiffSide::Baseline);
        mine_and_record("worker started", DiffSide::Target);
        record_excluded_no_field();
        record_excluded_no_field();
        let report = finalize().expect("finalize");
        assert_eq!(report.excluded_no_field, 2);
        assert_eq!(report.compared_events(), 2);
    }

    #[test]
    fn compared_events_is_zero_when_every_event_lacked_the_field() {
        reset_all();
        for _ in 0..5 {
            record_excluded_no_field();
        }
        let report = finalize().expect("finalize");
        assert_eq!(report.compared_events(), 0, "nothing was comparable");
        assert_eq!(report.excluded_no_field, 5);
        // The sections read as "no change" — which is exactly why the caller
        // must refuse this report rather than print it.
        assert!(report.new.is_empty() && report.vanished.is_empty() && report.shifted.is_empty());
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
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 9014,
            target_total: 14903,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
        };
        let text = format_report_text(&report, false);
        assert!(text.contains("NEW in target (1 template):"));
        assert!(text.contains("3412  OOM killer invoked for process <num>"));
        assert!(text.contains("VANISHED from target (1 template):"));
        assert!(text.contains("(baseline count)"));
        assert!(text.contains("VOLUME SHIFTS (1 template):"));
        // 2.1% of the baseline's lines -> 14.8% of the target's: 7.1x the rate.
        assert!(text.contains("7.1\u{d7} more frequent"), "text: {}", text);
        assert!(text.contains(
            "totals: baseline 9014 events, target 14903 events, 41 shared templates within noise"
        ));
        // Nothing suppressed moved, so the explanatory note stays out.
        assert!(!text.contains("too few to be sure"));
    }

    #[test]
    fn empty_sections_print_single_lines() {
        let report = DiffReport {
            new: vec![],
            vanished: vec![],
            shifted: vec![],
            unchanged_count: 5,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 100,
            target_total: 100,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
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
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 10,
            target_total: 20,
            unmatched_events: 0,
            excluded_no_timestamp: 2,
            excluded_no_field: 0,
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
        assert!(json["shifted"][0]["z_score"].is_number());
    }
}
