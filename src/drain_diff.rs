//! `--drain-diff`: template-level comparison of a baseline and a target log.
//!
//! Two logical passes over one shared drain instance:
//!
//! **Pass 1 (streaming)** — every event's mined field is added to the joint
//! drain tree (`crate::drain::drain_record`, learning mode) while this module
//! counts occurrences of each *unique* field value per side. Logs are
//! repetitive, so the unique-value map is far smaller than the event stream;
//! it doubles as the buffered corpus for pass 2, which makes stdin and
//! `--cut-at` first-class (no re-reading of inputs, no seekability concerns).
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

use crate::text_width::{display_width, pad_left_display, truncate_for_display, Glyphs};
use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::collections::HashMap;

/// Which side of the comparison an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Baseline,
    Target,
}

impl DiffSide {
    /// Report-facing name, used in the split echo and the one-sided refusal.
    pub fn label(self) -> &'static str {
        match self {
            DiffSide::Baseline => "baseline",
            DiffSide::Target => "target",
        }
    }
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
    /// Events excluded in --cut-at mode because they carry no parseable timestamp.
    excluded_no_timestamp: u64,
    /// Events excluded because the mined field carried no text (absent, or
    /// present but empty).
    excluded_no_field: u64,
    /// Observed timestamp span (earliest, latest) of the events counted on each
    /// side, indexed like `counts`. `None` when no event on that side carried a
    /// parseable timestamp, which is the only case where the split echo is
    /// omitted — two-file mode fills these in too whenever the logs are
    /// timestamped, and benefits from the same at-a-glance confirmation.
    /// Powers the split echo and the one-sided refusal message.
    spans: [Option<(DateTime<Utc>, DateTime<Utc>)>; 2],
    /// Events where the `--cut-before`/`--cut-after` predicate failed to evaluate *while the
    /// boundary was still undecided*. Any such failure invalidates the whole
    /// split: the event might have been the boundary, so every event after it may
    /// be on the wrong side. Unlike a `--filter` error, which drops one event,
    /// this silently relabels the rest of the log, so the report is refused.
    cut_predicate_errors: u64,
    /// Whether the predicate ever matched. Distinguishes the two ways a
    /// `--cut-after` split can starve the target — never matching, versus
    /// matching the final event so nothing lands after the boundary — which need
    /// opposite fixes.
    cut_predicate_matched: bool,
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
///
/// `ts` is the event's parsed timestamp when it has one; it only widens the
/// side's observed span for the report, and never affects the counts — an event
/// already assigned a side is counted whether or not it is timestamped.
pub fn record(text: &str, side: DiffSide, ts: Option<DateTime<Utc>>) {
    DIFF_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let idx = match side {
            DiffSide::Baseline => 0,
            DiffSide::Target => 1,
        };
        if let Some(ts) = ts {
            state.spans[idx] = Some(match state.spans[idx] {
                Some((first, last)) => (first.min(ts), last.max(ts)),
                None => (ts, ts),
            });
        }
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

/// Record that the predicate matched, wherever the boundary ended up landing.
pub fn record_cut_predicate_matched() {
    DIFF_STATE.with(|state| {
        state.borrow_mut().cut_predicate_matched = true;
    });
}

/// Record a predicate-split failure that left the boundary undecided.
/// See `DiffState::cut_predicate_errors`.
pub fn record_cut_predicate_error() {
    DIFF_STATE.with(|state| {
        state.borrow_mut().cut_predicate_errors += 1;
    });
}

/// Record an event dropped from the comparison in --cut-at mode because it has
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
    /// Distinct templates each side contributed at least one event to. Counted
    /// directly rather than derived from the sections, because `vanished` is
    /// filtered by the noise floor and so undercounts baseline-only templates.
    /// Used by [`DiffReport::undersized_side`].
    pub baseline_templates: usize,
    pub target_templates: usize,
    /// Events whose field value matched no frozen template in pass 2. Should
    /// be impossible by construction (every value was added in pass 1); a
    /// nonzero count guards against future drain behavior changes.
    pub unmatched_events: u64,
    pub excluded_no_timestamp: u64,
    /// Events excluded because the mined field carried no text. See
    /// `record_excluded_no_field`.
    pub excluded_no_field: u64,
    /// Observed timestamp span of the events counted on each side, when they
    /// carried parseable timestamps. See `DiffState::spans`.
    pub baseline_span: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub target_span: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Predicate-split evaluation failures that left the boundary undecided; any
    /// nonzero count invalidates the split. See
    /// `DiffState::cut_predicate_errors`.
    pub cut_predicate_errors: u64,
    /// Whether the predicate ever matched. See
    /// `DiffState::cut_predicate_matched`.
    pub cut_predicate_matched: bool,
}

impl DiffReport {
    /// Events that actually contributed to the comparison. Zero means the
    /// report says nothing about change whatever its sections claim, so callers
    /// must not present it as "nothing changed".
    pub fn compared_events(&self) -> u64 {
        self.baseline_total + self.target_total
    }

    /// The empty side of a lopsided comparison: one side got every event and
    /// the other got none. Such a report is not a comparison — every template
    /// lands on a `+` or `-` row by construction — so callers refuse it rather
    /// than print sections that read as a dramatic finding.
    ///
    /// `None` when both sides have events, and also when *neither* does: that
    /// case is vacuous for a different reason and has its own handling, so
    /// keeping it out here leaves one cause per message.
    pub fn empty_side(&self) -> Option<DiffSide> {
        match (self.baseline_total, self.target_total) {
            (0, t) if t > 0 => Some(DiffSide::Baseline),
            (b, 0) if b > 0 => Some(DiffSide::Target),
            _ => None,
        }
    }

    /// The side too small to have exhibited the other's message variety, if
    /// either is.
    ///
    /// [`Self::empty_side`] refuses the case where a comparison is *undefined*
    /// — a side with no events at all. This names the softer one next to it: a
    /// side with events, so shares and z-scores compute and the report is
    /// well-formed, but so few of them that the other side's templates land on
    /// `+` or `-` rows because the small side never had room for them rather
    /// than because anything changed. That is the shape a boundary landing at
    /// the edge of the log produces — a `--cut-before` marker matching the last
    /// event, a `--cut-at` one event inside the range, a target file with one
    /// line — and every one of those reported the whole log as changed, exit 0,
    /// with nothing said.
    ///
    /// The test is "fewer events than the other side has distinct templates",
    /// which is derived rather than tuned: a side cannot show N message types
    /// with fewer than N events, so below that the section length is bounded by
    /// the sample size instead of by any change. It deliberately does *not* fire
    /// on a homogeneous-but-well-sampled side — 500 events of one template
    /// against five templates is a real finding, not an artifact — which is why
    /// the count of events is compared against templates rather than a side's
    /// template count being tested on its own.
    ///
    /// Advisory only: the report still prints. Callers warn (🔸) rather than
    /// refuse, matching how the other compromised-corpus cases (cluster-cap
    /// eviction, partial field exclusion, zero compared events) are handled.
    pub fn undersized_side(&self) -> Option<DiffSide> {
        if self.empty_side().is_some() {
            // Already refused upstream, and a zero side would trip the test
            // below for a reason that has its own, better message.
            return None;
        }
        if self.baseline_total < self.target_templates as u64 {
            Some(DiffSide::Baseline)
        } else if self.target_total < self.baseline_templates as u64 {
            Some(DiffSide::Target)
        } else {
            None
        }
    }

    /// The observed timestamp span across both sides, for the "pick a cut
    /// inside this range" half of the one-sided refusal message.
    pub fn overall_span(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match (self.baseline_span, self.target_span) {
            (Some((b0, b1)), Some((t0, t1))) => Some((b0.min(t0), b1.max(t1))),
            (Some(span), None) | (None, Some(span)) => Some(span),
            (None, None) => None,
        }
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

        let mut baseline_templates = 0usize;
        let mut target_templates = 0usize;

        for (_, (template, counts)) in per_template {
            let (b, t) = (counts[0], counts[1]);
            if b > 0 {
                baseline_templates += 1;
            }
            if t > 0 {
                target_templates += 1;
            }
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
            baseline_templates,
            target_templates,
            unmatched_events: unmatched,
            excluded_no_timestamp: state.excluded_no_timestamp,
            excluded_no_field: state.excluded_no_field,
            baseline_span: state.spans[0],
            target_span: state.spans[1],
            cut_predicate_errors: state.cut_predicate_errors,
            cut_predicate_matched: state.cut_predicate_matched,
        })
    })
}

/// Same whitespace normalization drain.rs keys metadata on; duplicated here
/// only in behavior (via the public template id) — templates whose normalized
/// forms agree must aggregate into one diff row.
fn normalize_key(template: &str) -> String {
    template.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn signed_pp(delta: f64) -> String {
    format!(
        "{}{:.1}pp",
        if delta >= 0.0 { "+" } else { "-" },
        delta.abs()
    )
}

/// How much more (or less) of the log this template is, as a plain multiple:
/// `14× more`. Computed from *shares*, not raw counts, so sides of different
/// sizes compare fairly — the same quantity `MIN_RATE_RATIO` gates.
///
/// A multiple rather than the change in percentage points because it is the
/// number a reader cannot recover for themselves: given two shares the eye
/// subtracts 1.8% from 25.0% for free, but it does not divide.
fn rate_multiple(entry: &DiffEntry, glyphs: &Glyphs) -> String {
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
        format!("{:.0}{} {}", factor, glyphs.times, direction)
    } else {
        format!("{:.1}{} {}", factor, glyphs.times, direction)
    }
}

/// Rows shown per marker group (`+`, `-`, `*`) before the rest collapse into a
/// single line.
///
/// A group is only as readable as it is short. High-cardinality fields produce
/// long `+` groups however good the clustering is — a burst of one message
/// under 85 distinct user names is 85 rows of the same finding — and past a
/// screenful the reader has lost the ranking the sort exists to provide. Each
/// group is ordered by what an operator acts on (count for `+`/`-`, |Δ share|
/// for `*`), so the head is the part worth showing.
///
/// Never a silent cut: the note names how many rows and how many events were
/// held back, and where to get all of them.
const MAX_ROWS_PER_SECTION: usize = 20;

/// Smallest template column we will render. Below this a row says nothing, so
/// a very narrow terminal gets an overlong line rather than a useless one.
const MIN_TEMPLATE_WIDTH: usize = 24;

/// The "and the rest" line for a capped group, or `None` when everything fit.
/// Carries its group's marker so it stays self-locating in a flat row list.
fn truncation_note(
    marker: char,
    shown: usize,
    entries: &[DiffEntry],
    count: impl Fn(&DiffEntry) -> u64,
    glyphs: &Glyphs,
) -> Option<String> {
    let hidden = entries.len().checked_sub(shown).filter(|n| *n > 0)?;
    let events: u64 = entries.iter().skip(shown).map(count).sum();
    Some(format!(
        "  {} {} {} more {} ({} {}) not shown; --drain-diff=json lists every one",
        marker,
        glyphs.ellipsis,
        hidden,
        if hidden == 1 { "template" } else { "templates" },
        events,
        if events == 1 { "event" } else { "events" },
    ))
}

/// How each side is named on its `---`/`+++` header line. Built by the caller,
/// which is the only place that knows the filenames and which splitter produced
/// them.
#[derive(Debug, Clone)]
pub struct DiffSideLabels {
    pub baseline: String,
    pub target: String,
}

/// Everything the text report needs beyond the numbers.
#[derive(Debug, Clone)]
pub struct TextReportOptions {
    pub labels: DiffSideLabels,
    /// The field named by `-k`, echoed in the footer so a pasted report says
    /// what it actually compared.
    pub mined_field: Option<String>,
    pub use_colors: bool,
    /// False under `--no-emoji`: punctuation falls back to ASCII.
    pub use_unicode: bool,
    /// Column budget. Templates are truncated to fit; a wrapped row would
    /// destroy the alignment the marker column exists to provide.
    pub width: usize,
}

impl TextReportOptions {
    /// Plain, uncolored, generously wide — for tests and for callers that only
    /// want the body text.
    #[cfg(test)]
    pub fn plain(baseline: &str, target: &str) -> Self {
        Self {
            labels: DiffSideLabels {
                baseline: baseline.to_string(),
                target: target.to_string(),
            },
            mined_field: None,
            use_colors: false,
            use_unicode: true,
            width: crate::text_width::REDIRECTED_TABLE_WIDTH,
        }
    }
}

/// One rendered row, before column widths are known.
struct DiffRow<'a> {
    marker: char,
    color: &'a str,
    annotation: String,
    template: &'a str,
}

/// `--- <label>  <n> events  <first> .. <last>`, the line that answers "which
/// side is which" in syntax every engineer already reads. The span is the same
/// form `--cut-at` accepts back, so a header line doubles as the lookup for the
/// next invocation.
fn side_header(label: &str, total: u64, span: Option<(DateTime<Utc>, DateTime<Utc>)>) -> String {
    let mut out = format!(
        "{}  {} event{}",
        label,
        total,
        if total == 1 { "" } else { "s" }
    );
    if let Some((first, last)) = span {
        out.push_str(&format!(
            "  {} .. {}",
            format_instant(first),
            format_instant(last)
        ));
    }
    out
}

/// Format the report as a diff: two header lines naming the sides, then one
/// row per changed template marked `+` (only in the target), `-` (only in the
/// baseline), or `*` (present in both, at a materially different rate).
///
/// Templates whose frequency did not meaningfully change are the diff's context
/// lines — counted in the footer, not printed. Long groups are capped at
/// [`MAX_ROWS_PER_SECTION`] with a line stating what was held back.
pub fn format_report_text(report: &DiffReport, opts: &TextReportOptions) -> String {
    let glyphs = Glyphs::new(opts.use_unicode);
    let colors = crate::colors::DiffColors::new(opts.use_colors);
    let plural = |n: usize| if n == 1 { "template" } else { "templates" };

    // Colors open and close within one line: an escape left open across a
    // newline survives neither a pager nor a line-oriented filter. An uncolored
    // row emits no reset either, so `--no-color` output stays byte-clean.
    let line = |color: &str, text: &str| {
        if color.is_empty() {
            format!("{}\n", text)
        } else {
            format!("{}{}{}\n", color, text, colors.reset)
        }
    };

    let mut out = String::new();
    out.push_str(&line(
        "",
        &format!(
            "--- {}",
            side_header(
                &opts.labels.baseline,
                report.baseline_total,
                report.baseline_span
            )
        ),
    ));
    out.push_str(&line(
        "",
        &format!(
            "+++ {}",
            side_header(&opts.labels.target, report.target_total, report.target_span)
        ),
    ));
    out.push('\n');

    let new_shown = report.new.len().min(MAX_ROWS_PER_SECTION);
    let vanished_shown = report.vanished.len().min(MAX_ROWS_PER_SECTION);
    let shifted_shown = report.shifted.len().min(MAX_ROWS_PER_SECTION);

    let mut rows: Vec<DiffRow> = Vec::new();
    for entry in &report.new[..new_shown] {
        rows.push(DiffRow {
            marker: '+',
            color: colors.added,
            annotation: entry.target_count.to_string(),
            template: &entry.template,
        });
    }
    for entry in &report.vanished[..vanished_shown] {
        rows.push(DiffRow {
            marker: '-',
            color: colors.removed,
            annotation: entry.baseline_count.to_string(),
            template: &entry.template,
        });
    }
    for entry in &report.shifted[..shifted_shown] {
        rows.push(DiffRow {
            marker: '*',
            // Uncolored on purpose: growth is not a verdict. See `DiffColors`.
            color: "",
            annotation: rate_multiple(entry, &glyphs),
            template: &entry.template,
        });
    }

    if rows.is_empty() {
        out.push_str(&line("", "no template differences"));
    } else {
        // One annotation column across all three markers, so the templates
        // start at the same column whatever mix of rows a run produces.
        let annotation_width = rows
            .iter()
            .map(|row| display_width(&row.annotation))
            .max()
            .unwrap_or(1);
        // "  " + marker + " " + annotation + "  "
        let prefix = 2 + 1 + 1 + annotation_width + 2;
        let template_width = opts.width.saturating_sub(prefix).max(MIN_TEMPLATE_WIDTH);

        let emit = |rows: &[DiffRow], note: Option<String>, out: &mut String| {
            for row in rows {
                out.push_str(&line(
                    row.color,
                    &format!(
                        "  {} {}  {}",
                        row.marker,
                        pad_left_display(&row.annotation, annotation_width),
                        truncate_for_display(row.template, template_width, glyphs.ellipsis),
                    ),
                ));
            }
            if let Some(note) = note {
                out.push_str(&line("", &note));
            }
        };

        let (added, rest) = rows.split_at(new_shown);
        let (removed, changed) = rest.split_at(vanished_shown);
        emit(
            added,
            truncation_note('+', new_shown, &report.new, |e| e.target_count, &glyphs),
            &mut out,
        );
        emit(
            removed,
            truncation_note(
                '-',
                vanished_shown,
                &report.vanished,
                |e| e.baseline_count,
                &glyphs,
            ),
            &mut out,
        );
        emit(
            changed,
            // Frequency changes count events on both sides; the note reports the
            // target count, matching the direction `+` rows use.
            truncation_note(
                '*',
                shifted_shown,
                &report.shifted,
                |e| e.target_count,
                &glyphs,
            ),
            &mut out,
        );
    }

    let mut footer = Vec::new();
    if report.unchanged_count > 0 {
        footer.push(format!(
            "{} {} unchanged in frequency",
            report.unchanged_count,
            plural(report.unchanged_count)
        ));
    }
    if let Some(field) = &opts.mined_field {
        footer.push(format!("field: {}", field));
    }
    if !footer.is_empty() {
        out.push('\n');
        out.push_str(&line("", &footer.join(" | ")));
    }

    // Without this, a report can look self-contradictory: a template that moved
    // by a visible amount would sit silently in the unchanged tally as if it had
    // not moved at all. Only fires in that case, so ordinary reports stay quiet.
    if let Some(note) = within_noise_note(report) {
        if footer.is_empty() {
            out.push('\n');
        }
        out.push_str(&line("", &note));
    }

    // The caller's `writeln` terminates the last line.
    out.trim_end_matches('\n').to_string()
}

/// Render an instant in the form `--cut-at`/`--since` accept back verbatim, so a
/// span printed in the report can be copied straight into the next invocation.
pub fn format_instant(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The explanation for suppressed changes, phrased for someone who did not come
/// here for statistics: event counts, and the implied fix (collect more).
/// `None` — the common case — when nothing held back changed by enough for its
/// absence to read as an omission.
///
/// The two totals are spelled out rather than joined with a slash: `110/120`
/// reads as "110 out of 120", which is not what it means.
fn within_noise_note(report: &DiffReport) -> Option<String> {
    /// A suppressed change this large registers on the report's own scale (a
    /// full point of the side's traffic), so leaving it unexplained invites
    /// "where did my drop go?". Below it, nobody is counting.
    const NOTE_FLOOR_PP: f64 = 1.0;

    if report.within_noise_moved == 0 || report.within_noise_max_delta_pp < NOTE_FLOOR_PP {
        return None;
    }
    let n = report.within_noise_moved;
    Some(format!(
        "{} of them changed a little, but {} and {} events are too few to tell that from random variation",
        n, report.baseline_total, report.target_total,
    ))
}

/// Format the report as one JSON object for scripting and agent use. Shares
/// are emitted in percent so they share units with `delta_pp`.
///
/// The three arrays are named for the same three things the table marks — `new`
/// (`+`), `gone` (`-`), `freq_changed` (`*`) — and every entry names the side
/// each number belongs to, so `count`/`share_pct` never leave a consumer asking
/// "which side is this?". `freq_changed` rather than `changed`: the message
/// wording is exactly what this cannot see, and a bare `changed` would promise
/// it.
pub fn format_report_json(report: &DiffReport) -> String {
    let one_sided = |e: &DiffEntry, count_key: &str, pct_key: &str, count: u64, share: f64| {
        serde_json::json!({
            "template": e.template,
            "template_id": e.template_id,
            count_key: count,
            pct_key: share * 100.0,
        })
    };

    let json = serde_json::json!({
        "new": report
            .new
            .iter()
            .map(|e| one_sided(e, "target_count", "target_pct", e.target_count, e.target_share))
            .collect::<Vec<_>>(),
        "gone": report
            .vanished
            .iter()
            .map(|e| one_sided(e, "baseline_count", "baseline_pct", e.baseline_count, e.baseline_share))
            .collect::<Vec<_>>(),
        "freq_changed": report
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
        // Templates present on both sides that did not clear the reporting bars
        // — the diff's context lines. Named for the axis it is about, like the
        // array above it.
        "freq_unchanged_count": report.unchanged_count,
        // How many of those did move visibly, and by how much, so a consumer can
        // reproduce the table's explanatory note instead of inferring it.
        "freq_unchanged_moved": report.within_noise_moved,
        "freq_unchanged_max_delta_pp": report.within_noise_max_delta_pp,
        "baseline_events": report.baseline_total,
        "target_events": report.target_total,
        "unmatched_events": report.unmatched_events,
        "excluded_no_timestamp": report.excluded_no_timestamp,
        "excluded_no_field": report.excluded_no_field,
        // Null unless timestamps reached the diff (i.e. --cut-at mode). Same
        // purpose as the table's header lines: let a consumer confirm the split
        // landed where it meant it to, without a second pass over the input.
        "baseline_span": span_json(report.baseline_span),
        "target_span": span_json(report.target_span),
    });
    serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
}

/// Kind column for the machine formats, matching the JSON array names and the
/// table's `+` / `-` / `*` markers.
const KIND_NEW: &str = "new";
const KIND_GONE: &str = "gone";
const KIND_FREQ_CHANGED: &str = "freq_changed";

/// Format the report as one tab-separated record per changed template:
///
/// ```text
/// change  template_id  baseline_count  target_count  baseline_pct  target_pct  delta_pp  z_score  template
/// ```
///
/// No header row and no surrounding report, matching `-m --metrics=tsv`: a
/// record stream is written verbatim so `head`/`sort`/`awk` see only data. The
/// column count is fixed, so `new` and `gone` rows carry a `0` for the side they
/// are absent from and an empty `z_score` (they are gated by the noise floor,
/// not the z-test). The template goes last because it is the only free-form
/// field; tabs and newlines inside it are flattened to spaces so one record
/// stays one line.
///
/// Percentages are rounded to four decimals rather than emitted at full f64
/// precision: enough to distinguish any two templates in a realistic log, and
/// stable enough to diff two runs of this command.
///
/// The row cap that keeps the table readable does not apply — a machine format
/// with silently missing rows is worse than a long one — and neither do the
/// header/footer, which carry no per-template data. Use `=json` when the totals,
/// spans and exclusion counts matter.
pub fn format_report_tsv(report: &DiffReport) -> String {
    use crate::rhai_functions::tracking::tsv_sanitize;

    let mut out = String::new();
    let mut row = |kind: &str, e: &DiffEntry| {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\n",
            kind,
            tsv_sanitize(&e.template_id),
            e.baseline_count,
            e.target_count,
            e.baseline_share * 100.0,
            e.target_share * 100.0,
            e.delta_pp,
            match e.z_score {
                Some(z) => format!("{:.4}", z),
                None => String::new(),
            },
            tsv_sanitize(&e.template),
        ));
    };

    for entry in &report.new {
        row(KIND_NEW, entry);
    }
    for entry in &report.vanished {
        row(KIND_GONE, entry);
    }
    for entry in &report.shifted {
        row(KIND_FREQ_CHANGED, entry);
    }

    // The caller's `writeln` terminates the last record.
    out.trim_end_matches('\n').to_string()
}

fn span_json(span: Option<(DateTime<Utc>, DateTime<Utc>)>) -> serde_json::Value {
    match span {
        Some((first, last)) => serde_json::json!({
            "first": format_instant(first),
            "last": format_instant(last),
        }),
        None => serde_json::Value::Null,
    }
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
        mine_and_record_at(text, side, None);
    }

    fn mine_and_record_at(text: &str, side: DiffSide, ts: Option<DateTime<Utc>>) {
        crate::drain::drain_record(text, None, None).expect("drain_record");
        record(text, side, ts);
    }

    /// A report whose only meaningful content is the per-side event totals, for
    /// exercising the vacuity predicates that read nothing else.
    fn report_with_totals(baseline_total: u64, target_total: u64) -> DiffReport {
        DiffReport {
            new: vec![],
            vanished: vec![],
            shifted: vec![],
            unchanged_count: 0,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total,
            target_total,
            // Zero, so the derived-from-templates predicates read as "nothing
            // to be undersized against"; tests that care set them explicitly.
            baseline_templates: 0,
            target_templates: 0,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        }
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
            baseline_templates: 3,
            target_templates: 3,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let opts = TextReportOptions::plain("before.log", "after.log");
        let text = format_report_text(&report, &opts);
        assert!(
            text.contains(
                "2 of them changed a little, but 110 and 120 events are too few to tell that from random variation"
            ),
            "text: {}",
            text
        );

        // Nothing held back changed by a full point of traffic: nothing to explain.
        let quiet = DiffReport {
            within_noise_max_delta_pp: 0.6,
            ..report.clone()
        };
        assert!(!format_report_text(&quiet, &opts).contains("too few to tell"));

        // Same with no frequency changes shown at all — not a zero-match nag.
        let quiet = DiffReport {
            shifted: vec![],
            within_noise_max_delta_pp: 0.9,
            ..report.clone()
        };
        assert!(!format_report_text(&quiet, &opts).contains("too few to tell"));

        // Nothing reported at all, and a large change held back: the note has to
        // stand on its own rather than reading as a follow-on to listed rows.
        let lone = DiffReport {
            shifted: vec![],
            within_noise_moved: 1,
            within_noise_max_delta_pp: 4.0,
            ..report
        };
        let text = format_report_text(&lone, &opts);
        assert!(
            text.contains("1 of them changed a little, but 110 and 120 events"),
            "text: {}",
            text
        );
    }

    #[test]
    fn rate_multiple_reads_as_a_plain_factor() {
        let g = Glyphs::new(true);
        // 1.8% -> 25.0% of the side's lines. Past 10x the tenth is noise.
        assert_eq!(
            rate_multiple(&entry("t <num>", 2, 30, 110, 120), &g),
            "14\u{d7} more"
        );
        // Declines invert the ratio rather than printing a fraction.
        assert_eq!(
            rate_multiple(&entry("t <num>", 30, 2, 120, 110), &g),
            "14\u{d7} less"
        );
        // Near the MIN_RATE_RATIO bar the tenth is the whole claim.
        assert_eq!(
            rate_multiple(&entry("t <num>", 100, 160, 1000, 1000), &g),
            "1.6\u{d7} more"
        );
        // Raw counts would say 2x here; shares say the rate held steady.
        assert_eq!(
            rate_multiple(&entry("t <num>", 100, 200, 1000, 2000), &g),
            "1.0\u{d7} more"
        );
        // Defensive fallback for a degenerate entry (not reachable via finalize).
        assert_eq!(
            rate_multiple(&entry("t <num>", 0, 30, 110, 120), &g),
            "+25.0pp"
        );
        // --no-emoji falls back to ASCII rather than emitting a multiplication sign.
        assert_eq!(
            rate_multiple(&entry("t <num>", 2, 30, 110, 120), &Glyphs::new(false)),
            "14x more"
        );
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
    fn text_output_is_a_diff_with_named_sides() {
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
            baseline_templates: 43,
            target_templates: 43,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let text = format_report_text(
            &report,
            &TextReportOptions {
                mined_field: Some("msg".to_string()),
                ..TextReportOptions::plain("before.log", "after.log")
            },
        );
        // The two sides are named in unified-diff syntax, with their own totals.
        assert!(
            text.contains("--- before.log  9014 events"),
            "text: {}",
            text
        );
        assert!(
            text.contains("+++ after.log  14903 events"),
            "text: {}",
            text
        );
        // One row per changed template, marked by direction and annotated with
        // the number that matters for that direction.
        let row = |template: &str| -> String {
            text.lines()
                .find(|line| line.contains(template))
                .unwrap_or_else(|| panic!("no row for {}: {}", template, text))
                .to_string()
        };
        let added = row("OOM killer invoked for process <num>");
        assert!(
            added.starts_with("  + ") && added.contains("3412"),
            "{}",
            added
        );
        let removed = row("Connection pool recycled for <fqdn>");
        assert!(
            removed.starts_with("  - ") && removed.contains("438"),
            "{}",
            removed
        );
        // 2.1% of the baseline's lines -> 14.8% of the target's: 7.1x the rate.
        let changed = row("Upstream <fqdn> returned <num>");
        assert!(
            changed.starts_with("  * ") && changed.contains("7.1\u{d7} more"),
            "{}",
            changed
        );
        assert!(text.contains("41 templates unchanged in frequency | field: msg"));
        // Nothing suppressed changed, so the explanatory note stays out.
        assert!(!text.contains("too few to tell"));
        // No section headers, no blank-line padding between groups.
        assert!(!text.contains("NEW in target"));
        assert!(!text.contains("VOLUME SHIFTS"));
    }

    #[test]
    fn rows_share_one_annotation_column_and_templates_are_truncated_to_width() {
        let long =
            "circuit breaker <num> opened for downstream service <fqdn> after <num> failures";
        let report = DiffReport {
            new: vec![entry(long, 0, 400, 800, 800)],
            vanished: vec![],
            shifted: vec![entry("steady heartbeat <num>", 100, 300, 800, 800)],
            unchanged_count: 0,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 800,
            target_total: 800,
            baseline_templates: 1,
            target_templates: 2,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let text = format_report_text(
            &report,
            &TextReportOptions {
                width: 60,
                ..TextReportOptions::plain("a.log", "b.log")
            },
        );
        let rows: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("  +") || line.starts_with("  *"))
            .collect();
        assert_eq!(rows.len(), 2, "text: {}", text);
        // The widest annotation ("3.0x more") sets one column for both markers,
        // so every template starts at the same offset.
        // Templates carry no double space, so the last run of two is the gap
        // between the annotation column and the template column.
        // Measured in display columns, not bytes: the `×` in a rate multiple is
        // two bytes wide and one column, which is exactly the confusion the
        // shared width helpers exist to prevent.
        let starts: Vec<usize> = rows
            .iter()
            .map(|row| {
                let gap = row.rfind("  ").expect("annotation gap");
                display_width(&row[..gap + 2])
            })
            .collect();
        assert_eq!(starts[0], starts[1], "rows: {:?}", rows);
        // Nothing wraps: a long template is cut to fit, never folded.
        for row in &rows {
            assert!(display_width(row) <= 60, "row overflows width: {:?}", row);
        }
        assert!(text.contains('\u{2026}'), "long template must be elided");
    }

    #[test]
    fn empty_side_names_the_starved_side_and_ignores_the_both_empty_case() {
        let mut report = report_with_totals(0, 0);
        assert_eq!(
            report.empty_side(),
            None,
            "neither side having events is a different diagnosis, handled elsewhere"
        );

        report.baseline_total = 0;
        report.target_total = 5;
        assert_eq!(report.empty_side(), Some(DiffSide::Baseline));

        report.baseline_total = 5;
        report.target_total = 0;
        assert_eq!(report.empty_side(), Some(DiffSide::Target));

        report.target_total = 1;
        assert_eq!(
            report.empty_side(),
            None,
            "a single event on a side is lopsided, not vacuous — still a comparison"
        );
    }

    #[test]
    fn undersized_side_catches_a_boundary_that_landed_at_the_edge() {
        // The `--cut-before` marker matched the last event: one event on the
        // target, and the baseline's whole template set reads as VANISHED.
        let mut report = report_with_totals(2390, 1);
        report.baseline_templates = 5;
        report.target_templates = 1;
        assert_eq!(report.undersized_side(), Some(DiffSide::Target));

        // Mirror image: `--cut-after` matching the first event.
        let mut report = report_with_totals(1, 2389);
        report.baseline_templates = 1;
        report.target_templates = 5;
        assert_eq!(report.undersized_side(), Some(DiffSide::Baseline));
    }

    #[test]
    fn undersized_side_ignores_a_well_sampled_or_vacuous_comparison() {
        // Ordinary comparison: both sides have far more events than either has
        // templates.
        let mut report = report_with_totals(1050, 1340);
        report.baseline_templates = 6;
        report.target_templates = 6;
        assert_eq!(report.undersized_side(), None);

        // A homogeneous baseline is not undersized. 500 events of one template
        // against five templates is a real finding, so testing a side's template
        // count on its own would fire here and be wrong.
        let mut report = report_with_totals(500, 400);
        report.baseline_templates = 1;
        report.target_templates = 5;
        assert_eq!(
            report.undersized_side(),
            None,
            "well-sampled side, however few templates it holds"
        );

        // An empty side is refused upstream with a message of its own; this
        // predicate stays quiet so the two do not both fire.
        let mut report = report_with_totals(0, 400);
        report.baseline_templates = 0;
        report.target_templates = 5;
        assert_eq!(report.undersized_side(), None);
    }

    #[test]
    fn side_header_says_one_event_not_one_events() {
        let mut report = report_with_totals(2389, 1);
        report.baseline_templates = 5;
        report.target_templates = 1;
        let text = format_report_text(&report, &TextReportOptions::plain("a.log", "b.log"));
        assert!(text.contains("--- a.log  2389 events"), "text: {}", text);
        assert!(text.contains("+++ b.log  1 event"), "text: {}", text);
    }

    #[test]
    fn overall_span_unions_the_sides_and_survives_a_missing_one() {
        let ts = |h: u32, m: u32| {
            chrono::DateTime::parse_from_rfc3339(&format!("2026-07-24T{:02}:{:02}:00Z", h, m))
                .expect("fixture parses")
                .with_timezone(&Utc)
        };

        let mut report = report_with_totals(5, 5);
        report.baseline_span = Some((ts(10, 0), ts(11, 0)));
        report.target_span = Some((ts(14, 0), ts(15, 0)));
        assert_eq!(report.overall_span(), Some((ts(10, 0), ts(15, 0))));

        // Out-of-order sides must still yield the true outer bounds.
        report.baseline_span = Some((ts(14, 0), ts(15, 0)));
        report.target_span = Some((ts(10, 0), ts(11, 0)));
        assert_eq!(report.overall_span(), Some((ts(10, 0), ts(15, 0))));

        // The refusal path always has one side empty, so exactly one span is
        // typically present — that side still has to produce a range.
        report.target_span = None;
        assert_eq!(report.overall_span(), Some((ts(14, 0), ts(15, 0))));

        report.baseline_span = None;
        assert_eq!(report.overall_span(), None);
    }

    #[test]
    fn record_widens_the_side_span_without_touching_the_other() {
        let ts = |h: u32| {
            chrono::DateTime::parse_from_rfc3339(&format!("2026-07-24T{:02}:00:00Z", h))
                .expect("fixture parses")
                .with_timezone(&Utc)
        };
        reset_all();

        // Deliberately out of order: the span is min/max, not first/last seen.
        mine_and_record_at("worker started", DiffSide::Baseline, Some(ts(11)));
        mine_and_record_at("worker started", DiffSide::Baseline, Some(ts(9)));
        mine_and_record_at("worker started", DiffSide::Baseline, Some(ts(10)));
        // An untimestamped event still counts, it just cannot widen the span.
        mine_and_record_at("worker started", DiffSide::Target, None);
        mine_and_record_at("worker started", DiffSide::Target, Some(ts(15)));

        let report = finalize().expect("finalize");
        assert_eq!(report.baseline_span, Some((ts(9), ts(11))));
        assert_eq!(report.target_span, Some((ts(15), ts(15))));
        assert_eq!(report.baseline_total, 3);
        assert_eq!(
            report.target_total, 2,
            "the untimestamped event still counts"
        );
    }

    #[test]
    fn a_report_with_no_differences_says_so_once() {
        let report = DiffReport {
            new: vec![],
            vanished: vec![],
            shifted: vec![],
            unchanged_count: 5,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 100,
            target_total: 100,
            baseline_templates: 5,
            target_templates: 5,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let text = format_report_text(&report, &TextReportOptions::plain("a.log", "b.log"));
        // One statement, not three "no X" lines: absence of change is one fact.
        assert!(text.contains("no template differences"), "text: {}", text);
        assert_eq!(text.matches("no template differences").count(), 1);
        // The sides and the unchanged tally still report — a null result has to
        // show it compared something.
        assert!(text.contains("--- a.log  100 events"));
        assert!(text.contains("+++ b.log  100 events"));
        assert!(text.contains("5 templates unchanged in frequency"));
    }

    #[test]
    fn colors_open_and_close_within_each_line() {
        let report = DiffReport {
            new: vec![entry("a new <num>", 0, 7, 10, 20)],
            vanished: vec![entry("an old <num>", 5, 0, 10, 20)],
            shifted: vec![],
            unchanged_count: 1,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 10,
            target_total: 20,
            baseline_templates: 1,
            target_templates: 1,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let text = format_report_text(
            &report,
            &TextReportOptions {
                use_colors: true,
                ..TextReportOptions::plain("a.log", "b.log")
            },
        );
        // An escape left open across a newline survives neither a pager nor a
        // line-oriented filter, so every colored line must close its own.
        for line in text.lines() {
            let escapes = line.matches('\u{1b}').count();
            if escapes == 0 {
                continue;
            }
            assert_eq!(escapes, 2, "line is not exactly open+reset: {:?}", line);
            assert!(line.ends_with("\u{1b}[0m"), "line leaks color: {:?}", line);
        }
        // Diff convention: additions green, removals red.
        assert!(text.contains("\u{1b}[32m  + "), "text: {:?}", text);
        assert!(text.contains("\u{1b}[31m  - "), "text: {:?}", text);
        // ...and nothing else is colored. Only the two markers that carry a
        // direction earn a color; the scaffolding around them stays plain.
        for line in text.lines() {
            if line.starts_with("\u{1b}[32m") || line.starts_with("\u{1b}[31m") {
                continue;
            }
            assert!(
                !line.contains('\u{1b}'),
                "only + and - rows may be colored: {:?}",
                line
            );
        }
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
            baseline_templates: 5,
            target_templates: 5,
            unmatched_events: 0,
            excluded_no_timestamp: 2,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let json: serde_json::Value =
            serde_json::from_str(&format_report_json(&report)).expect("valid JSON");
        // The array names match the table's markers, and every count and percent
        // names the side it belongs to rather than leaving that to be inferred.
        assert_eq!(json["new"][0]["target_count"], 7);
        assert_eq!(json["gone"][0]["baseline_count"], 5);
        assert_eq!(json["freq_changed"][0]["baseline_count"], 5);
        assert_eq!(json["freq_changed"][0]["target_count"], 13);
        assert_eq!(json["freq_unchanged_count"], 3);
        assert_eq!(json["baseline_events"], 10);
        assert_eq!(json["target_events"], 20);
        assert_eq!(json["excluded_no_timestamp"], 2);
        // Percent keys and delta_pp share units.
        assert!((json["new"][0]["target_pct"].as_f64().unwrap() - 35.0).abs() < 0.01);
        assert!((json["gone"][0]["baseline_pct"].as_f64().unwrap() - 50.0).abs() < 0.01);
        assert!(json["freq_changed"][0]["z_score"].is_number());
        // The old names are gone rather than aliased: one vocabulary per report.
        assert!(json.get("vanished").is_none());
        assert!(json.get("shifted").is_none());
        assert!(json.get("unchanged_count").is_none());
        assert!(json["new"][0].get("count").is_none());
        assert!(json["new"][0].get("share_pct").is_none());
    }

    #[test]
    fn tsv_is_one_fixed_width_record_per_changed_template() {
        let report = DiffReport {
            new: vec![entry("a new <num>", 0, 7, 10, 20)],
            vanished: vec![entry("an old <num>", 5, 0, 10, 20)],
            shifted: vec![entry("a shared <num>", 5, 13, 10, 20)],
            unchanged_count: 3,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 10,
            target_total: 20,
            baseline_templates: 2,
            target_templates: 2,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let tsv = format_report_tsv(&report);
        let rows: Vec<Vec<&str>> = tsv.lines().map(|l| l.split('\t').collect()).collect();
        assert_eq!(rows.len(), 3, "tsv: {}", tsv);
        // Fixed column count, so awk positions hold whatever the row kind is.
        for row in &rows {
            assert_eq!(row.len(), 9, "row: {:?}", row);
        }
        assert_eq!(rows[0][0], "new");
        assert_eq!(rows[1][0], "gone");
        assert_eq!(rows[2][0], "freq_changed");
        // The absent side is a real zero, and only both-sides rows carry a z.
        assert_eq!(rows[0][2], "0", "a new template has no baseline count");
        assert_eq!(rows[1][3], "0", "a gone template has no target count");
        assert_eq!(
            rows[0][7], "",
            "new/gone are gated by the floor, not the z-test"
        );
        assert!(!rows[2][7].is_empty(), "row: {:?}", rows[2]);
        // Template last, so the only free-form field cannot shift the columns.
        assert_eq!(rows[0][8], "a new <num>");
        // No header row: a record stream is data only.
        assert!(!tsv.starts_with("change"), "tsv: {}", tsv);
    }

    #[test]
    fn tsv_flattens_tabs_inside_a_template() {
        // Templates come from log text, which can contain tabs; one record has
        // to stay one line with a fixed field count.
        let report = DiffReport {
            new: vec![entry("a\tnew\nline <num>", 0, 7, 10, 20)],
            vanished: vec![],
            shifted: vec![],
            unchanged_count: 0,
            within_noise_moved: 0,
            within_noise_max_delta_pp: 0.0,
            baseline_total: 10,
            target_total: 20,
            baseline_templates: 0,
            target_templates: 1,
            unmatched_events: 0,
            excluded_no_timestamp: 0,
            excluded_no_field: 0,
            baseline_span: None,
            target_span: None,
            cut_predicate_errors: 0,
            cut_predicate_matched: false,
        };
        let tsv = format_report_tsv(&report);
        assert_eq!(tsv.lines().count(), 1, "tsv: {:?}", tsv);
        assert_eq!(tsv.split('\t').count(), 9, "tsv: {:?}", tsv);
        assert!(tsv.ends_with("a new line <num>"), "tsv: {:?}", tsv);
    }

    #[test]
    fn tsv_of_an_unchanged_comparison_is_empty() {
        // A record stream with no records writes nothing, rather than putting a
        // blank line or a prose sentence into a pipe.
        let report = report_with_totals(100, 100);
        assert_eq!(format_report_tsv(&report), "");
    }
}
