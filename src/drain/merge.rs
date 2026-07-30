//! Merging clusters after mining, before anything reads the template set.
//!
//! The tree groups a line the moment it arrives, from one keyed prefix and a
//! similarity score against the clusters that happen to exist at that point. It
//! is a streaming decision made with partial information, and it fragments in a
//! predictable way: a value in a position the tree keys on, or one that varies
//! too much for the similarity bar, yields a family of templates identical but
//! for a single position — `Invalid user admin from <ipv4>`,
//! `Invalid user oracle from <ipv4>`, fifty more.
//!
//! By end of input that partial information is gone. There are a few hundred
//! templates rather than millions of lines, the whole family is visible at once,
//! and the question "is this position a parameter?" can be answered from
//! evidence instead of guessed from a prefix. That is what this pass does, and it
//! is why both CLI modes read the model here rather than mid-stream: `--drain`
//! prints at end of input, and `--drain-diff` already counts every value against
//! the frozen final set.
//!
//! Fragmentation is not merely untidy. `--drain-diff` computes per-template
//! statistics, so a burst split across fifty templates puts every fragment under
//! the reporting floor: the mode's headline finding — one message becoming six
//! times more frequent — was reported as fifty separate NEW templates and no
//! volume shift at all.
//!
//! # The rules
//!
//! Two same-length rules act on a *variant family*: templates in one length
//! bucket that agree everywhere except position `i`. Two cross-length rules
//! (R3, R4 below) repair the fragmentation the same-length rules cannot see —
//! a parameter whose *width* varies (`lifetime 00:03` / `lifetime <1 sec`, an
//! optional `(1.13 KB)` segment, a trailing list), which token-count bucketing
//! re-mines as one template per width forever.
//!
//! **R1, wildcard sibling.** If some member already holds `<*>` at `i`, the
//! family merges into it. This is the strongest evidence available and it costs
//! nothing to trust: the tree itself concluded that this position varies, for
//! some subset of the data. The rest of the family is the same conclusion
//! reached too late.
//!
//! **R2, long thin tail.** Otherwise, a family of at least [`MIN_VARIANTS`]
//! members merges, on the grounds that a position holding that many distinct
//! values across otherwise-identical messages is a parameter. A keyword is not
//! like that: `session opened` versus `session closed` is a family of two, and
//! two fat alternatives is what a keyword looks like.
//!
//! # The guard
//!
//! A merge that leaves a template with **no literal tokens at all** is refused
//! ([`MIN_LITERALS`]). Such a template identifies nothing: `<*> <num>` would
//! swallow every one-value-plus-number message in the log, and `<*> <path>
//! <num>` would eat every HTTP method.
//!
//! A token counts as literal when it carries text of its own: `Invalid`, `user`
//! and `uid=<num>` do, while a bare placeholder (`<num>`, `<ipv4>`, `<*>`) does
//! not — it is a position the masker already emptied of meaning.
//!
//! ## What the guard does not do
//!
//! It does **not** separate a parameter from an event name in general, and it
//! cannot: the two are the same shape. A family of twelve `onReceive succeeded`
//! / `onStandStepChanged succeeded` messages merges to `<*> succeeded`, which
//! names no event — and a family of twelve `starting nginx` / `starting redis`
//! merges to `starting <*>`, which is exactly right. Nothing local to the
//! template tells them apart.
//!
//! Tightening it was measured against the loghub suite and rejected. Two
//! candidates, both worse than living with `<*> succeeded`:
//!
//! * **Refuse a merge that consumes the template's leading literal.** This is
//!   the rule that catches the event-name case, and it is backwards on real
//!   data: **Proxifier 47.8% -> 0.0%** (mean -3.0pp, every other dataset
//!   unchanged to the decimal). Proxifier's leading token is
//!   `proxy.cse.cuhk.edu.hk:5070` -> `<fqdn>:<num>`, which counts as a literal
//!   because it is only partially masked — so it *is* the leading literal, and
//!   it *is* a parameter. Merging it is the whole of that dataset's gain.
//! * **Require two surviving literals.** Cheap on the suite (-0.1pp) but it does
//!   not fix the shape — `<*> handler done` keeps two literals and still merges —
//!   while refusing the legitimate two-token `starting <*>`.
//!
//! And the shape does not occur in the corpus: across all 16 datasets (32,000
//! annotated lines) exactly one merge-produced template leads with `<*>`, and it
//! is Proxifier's correct one. So `MIN_LITERALS = 1` stands, and the residual
//! over-merge is a known, tested limitation rather than an oversight — see
//! `merges_a_family_that_may_be_event_names`. Distinguishing the two would take
//! evidence the template alone does not carry, such as whether the varying value
//! also appears in other families (a hostname does, an event name does not).
//!
//! # The cross-length rules
//!
//! **R3, aligned pair** ([`pair_merges`]): two templates of different token
//! counts merge when aligning them (longest common subsequence over tokens)
//! matches at least [`CROSS_LEN_SIMILARITY`] of the longer one, and every
//! divergence window between matched runs is **value-dominated**: at least
//! half its tokens, pooled from both sides, carry a masked value. The matched
//! skeleton keeps the shared tokens and puts a gap ([`MaskedToken::Gap`],
//! matching zero or more tokens) at each divergence.
//!
//! The window guard is what separates a parameter whose width varies from an
//! extra phrase, and each of its clauses earns its keep on real data:
//! `<num> bytes sent` / `<num> bytes (<size_kb>) sent` has window
//! `[(<size_kb>)]` — pure value, merge. `Failed password for root from
//! <ipv4> …` / `Failed password for invalid user <*> from <ipv4> …` pools
//! `[root]` against `[invalid, user, <*>]` — no attested value at all,
//! refused, and merging it would collapse a distinction (valid vs. invalid
//! account) a security reader cannot recover even though it clears any
//! plausible similarity bar (8 of 11). `corrected` / `corrected over
//! <duration>` pools `[over, <duration>]` — a value tied with a literal is an
//! optional phrase, refused (loghub keeps those apart). And `… ruser=
//! rhost=<ipv4>` / `… ruser= <*> <*>` is why a bare wildcard is not evidence:
//! the second template's `<*>`s are a laundered `user=root`.
//!
//! **R4, edge-window family** ([`edge_window_families`]): templates sharing a
//! literal anchor at one end whose windows at the other end are **entirely
//! variable tokens** merge into anchor + gap when at least two window widths
//! occur. This is R1's evidence applied to width: the masker or tree already
//! emptied the stretch of literal text, so only how many tokens it spans is in
//! question. There is deliberately **no** count-based (R2-style) evidence path
//! here: sharing a short anchor with a dozen different tails is the signature
//! of a subsystem prefix, not of one message — measured on loghub, a
//! [`MIN_VARIANTS`]-based variant of this rule collapsed 24 distinct `ciod:`
//! messages (BGL) and 23 distinct `[instance: <uuid>]` messages (OpenStack)
//! into one template each, the unrecoverable direction of error.
//!
//! Rounds repeat to a fixpoint (bounded by [`MAX_ROUNDS`]) because a merge
//! creates a new wildcard sibling, which can make R1 apply where it did not
//! before: fifty user names collapse, and the result then absorbs the near-miss
//! that only R1 could see. The cross-length rules chain the same way: R3
//! merges each template with at most one partner per round, so a family
//! fragmented across several optional segments converges over a few rounds,
//! and a gap R3/R4 produced counts as a wildcard sibling for R1 (kept as a
//! gap, since narrowing zero-or-more to exactly-one would orphan the lines the
//! earlier merge absorbed).

use super::{is_bare_placeholder, Finalized, MaskedToken};
use std::collections::HashMap;

/// Identifies a variant family, without materializing the skeleton it names.
///
/// The obvious key — the position plus an owned copy of the template with that
/// position wildcarded — costs one vector and one string clone per token per
/// template per position, which at the 10k-cluster cap is millions of
/// allocations for a set that mostly does not merge. This borrows the template
/// instead and substitutes the wildcard while hashing and comparing, so building
/// the index allocates nothing beyond the map itself. Only families that actually
/// merge get an owned skeleton, and there are few of those.
///
/// `skip` is part of the identity, not just a detail of how the key is read: two
/// templates already holding a wildcard can produce the same token sequence from
/// different positions (`x <*> c` at 0 and `<*> y c` at 1 both read
/// `<*> <*> c`), and those are different families.
#[derive(Debug, Clone, Copy)]
struct FamilyKey<'a> {
    tokens: &'a [MaskedToken],
    skip: usize,
}

impl FamilyKey<'_> {
    /// The token at `position` as the skeleton has it.
    fn token(&self, position: usize) -> &MaskedToken {
        if position == self.skip {
            &MaskedToken::WildCard
        } else {
            &self.tokens[position]
        }
    }

    /// The skeleton as an owned template, built only for a family that merges.
    fn skeleton(&self) -> Vec<MaskedToken> {
        let mut out = self.tokens.to_vec();
        out[self.skip] = MaskedToken::WildCard;
        out
    }
}

impl PartialEq for FamilyKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.skip == other.skip
            && self.tokens.len() == other.tokens.len()
            && (0..self.tokens.len()).all(|i| self.token(i) == other.token(i))
    }
}

impl Eq for FamilyKey<'_> {}

impl std::hash::Hash for FamilyKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must agree with `eq`: same length, same skip, same substituted tokens.
        self.tokens.len().hash(state);
        self.skip.hash(state);
        for i in 0..self.tokens.len() {
            self.token(i).hash(state);
        }
    }
}

/// Distinct values at one position, across otherwise-identical templates, taken
/// as evidence that the position is a parameter rather than a keyword.
///
/// High enough that sets of keywords do not qualify — HTTP methods, log levels,
/// open/close pairs, the handful of verbs a subsystem logs — and low enough to
/// catch the fragmentation that actually occurs, which runs to dozens of
/// variants. Only R2 consults it; R1 needs no threshold because it is acting on
/// the tree's own conclusion.
const MIN_VARIANTS: usize = 12;

/// Minimum aligned fraction — tokens matched by the alignment, over the longer
/// template's non-gap token count — for a cross-length pair to merge (R3).
///
/// The cross-length analogue of the tree's similarity bar: high enough that
/// messages sharing only a head or tail stay apart (`Connection closed` /
/// `Connection reset by peer` align 1 of 4), low enough that optional segments
/// in a long message clear it (`… <num> bytes sent, …` /
/// `… <num> bytes (<size_kb>) sent, …` aligns 8 of 9).
const CROSS_LEN_SIMILARITY: f64 = 0.7;

/// Template-set size past which the cross-length rules (R3/R4) are skipped.
///
/// Their pair scan is quadratic when many templates share an edge token, and
/// the cluster cap admits 10,000 — a set that large is far past reading, and
/// past the point where the field should have been templated at all (the cap's
/// own reasoning). The same-length rules, which index rather than pair, still
/// run. Generous next to any template set a coherent field produces: the
/// largest in the loghub suite is Mac's 341.
const CROSS_LEN_MAX_TEMPLATES: usize = 1_000;

/// Literal tokens a merged template must retain. Below this it identifies
/// nothing at all.
///
/// Deliberately 1, not 2: raising it neither fixes the event-name shape nor pays
/// for itself on the suite. See the module docs under "What the guard does not
/// do" for the measurements behind that.
const MIN_LITERALS: usize = 1;

/// Fixpoint iteration bound. Each round strictly reduces the template count, so
/// this only guards against a future rule that does not. Higher than the
/// same-length rules alone would need: R3 merges one pair per family per round,
/// so a family fragmented across many optional segments converges by halving.
const MAX_ROUNDS: usize = 16;

/// Merge variant families in `entries`, returning the finished set.
///
/// Order of the result is unspecified; the caller sorts.
pub(super) fn merge_variants(mut entries: Vec<Finalized>) -> Vec<Finalized> {
    for _ in 0..MAX_ROUNDS {
        let before = entries.len();
        entries = one_round(entries);
        if entries.len() <= CROSS_LEN_MAX_TEMPLATES {
            entries = edge_window_families(entries);
            entries = pair_merges(entries);
        }
        if entries.len() == before {
            break;
        }
    }
    entries
}

fn one_round(entries: Vec<Finalized>) -> Vec<Finalized> {
    // Index by (token count, differing position, skeleton) so every variant
    // family is found in one pass. The skeleton is the template with that
    // position wildcarded, which is also exactly what a merged member becomes —
    // so a member that already holds `<*>` there lands in its own family.
    let mut families: HashMap<FamilyKey<'_>, Vec<usize>> = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        for position in 0..entry.tokens.len() {
            families
                .entry(FamilyKey {
                    tokens: &entry.tokens,
                    skip: position,
                })
                .or_default()
                .push(idx);
        }
    }

    // Families of two or more are the only candidates, and most sets have few, so
    // filter before sorting. Largest first, so the most confident merge claims its
    // members before a smaller overlapping one can; ties break on the family
    // itself for a result independent of HashMap order.
    let mut candidates: Vec<(&FamilyKey<'_>, &Vec<usize>)> =
        families.iter().filter(|(_, m)| m.len() > 1).collect();
    candidates.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| a.0.skip.cmp(&b.0.skip))
            .then_with(|| {
                super::render_template(&a.0.skeleton())
                    .cmp(&super::render_template(&b.0.skeleton()))
            })
    });

    let mut claimed = vec![false; entries.len()];
    // Merged results, and the members that went into each.
    let mut merges: Vec<(Vec<MaskedToken>, Vec<usize>)> = Vec::new();
    for (family, members) in candidates {
        if members.iter().any(|idx| claimed[*idx]) {
            continue;
        }
        let position = family.skip;
        // A Gap counts as a wildcard sibling: a previous cross-length merge
        // already concluded the position varies.
        let has_wildcard_sibling = members.iter().any(|idx| {
            matches!(
                entries[*idx].tokens[position],
                MaskedToken::WildCard | MaskedToken::Gap
            )
        });
        let rule_applies = has_wildcard_sibling || members.len() >= MIN_VARIANTS;
        if !rule_applies {
            continue;
        }
        let mut skeleton = family.skeleton();
        // A member holding a Gap keeps it: narrowing zero-or-more to
        // exactly-one would orphan the lines its earlier merge absorbed.
        if members
            .iter()
            .any(|idx| matches!(entries[*idx].tokens[position], MaskedToken::Gap))
        {
            skeleton[position] = MaskedToken::Gap;
        }
        if literal_count(&skeleton) < MIN_LITERALS {
            continue;
        }
        for idx in members {
            claimed[*idx] = true;
        }
        merges.push((skeleton, members.clone()));
    }

    rebuild(entries, merges, &claimed)
}

/// Apply `merges` (skeleton, member indices) to `entries`: merged families
/// first, then whatever was left unclaimed. Two families can collapse onto the
/// same skeleton, so fold by template.
fn rebuild(
    entries: Vec<Finalized>,
    merges: Vec<(Vec<MaskedToken>, Vec<usize>)>,
    claimed: &[bool],
) -> Vec<Finalized> {
    if merges.is_empty() {
        return entries;
    }

    let mut by_template: HashMap<Vec<MaskedToken>, Finalized> = HashMap::new();
    for (tokens, members) in merges {
        let merged = members
            .iter()
            .map(|idx| &entries[*idx])
            .fold(None::<Finalized>, |acc, entry| Some(combine(acc, entry)))
            .map(|mut folded| {
                folded.tokens = tokens.clone();
                folded
            })
            .expect("a family has at least two members");
        absorb_into(&mut by_template, merged);
    }
    for (idx, entry) in entries.into_iter().enumerate() {
        if !claimed[idx] {
            absorb_into(&mut by_template, entry);
        }
    }
    by_template.into_values().collect()
}

/// Tokens carrying text of their own, as opposed to a bare placeholder. See the
/// module docs on the guard.
fn literal_count(tokens: &[MaskedToken]) -> usize {
    tokens
        .iter()
        .filter(|token| match token {
            MaskedToken::WildCard | MaskedToken::Gap => false,
            MaskedToken::Val(s) => !s.is_empty() && !is_bare_placeholder(s),
        })
        .count()
}

/// Whether a (non-empty) window is variable throughout — every token a
/// wildcard, a gap, or a bare placeholder — *and* anchored by at least one
/// named placeholder. The cross-length analogue of R1's wildcard sibling: the
/// masker already emptied the stretch of literal text, so only its width is in
/// question.
///
/// The named-placeholder requirement is the same lesson as
/// [`attested_value`]: a window of nothing but `<*>` is the tree's
/// disagreement, not the masker's evidence, and trusting it merges `… ruser=
/// rhost=<ipv4>` (as `… ruser= <*>`) with its `user=root` sibling (as `…
/// ruser= <*> <*>`).
fn all_variable(window: &[MaskedToken]) -> bool {
    !window.is_empty()
        && window.iter().all(|token| match token {
            MaskedToken::WildCard | MaskedToken::Gap => true,
            MaskedToken::Val(s) => is_bare_placeholder(s),
        })
        && window
            .iter()
            .any(|token| matches!(token, MaskedToken::Val(s) if is_bare_placeholder(s)))
}

/// Whether a token carries a *masker-attested* value: text the masker put a
/// named placeholder into (`<num>:<num>`, `(<version>`, `uid=<num>`).
///
/// Deliberately **not** true for a bare wildcard. A `<*>` is the tree giving
/// up on a position, and trusting it as value evidence launders keywords: an
/// extra `user=root` / `user=news` key-value pair generalizes to `<*>` inside
/// its cluster, and a guard that then reads `<*>` as "a value varied here"
/// merges `… ruser= rhost=<ipv4>` with `… ruser= rhost=<ipv4> user=<*>` — two
/// messages loghub's ground truth (and any security reader) keeps apart. The
/// masker's named placeholders carry no such ambiguity: they were values on
/// every line, not disagreements between lines.
/// A `key=value` token is not value evidence either, however masked its value:
/// the key names a field, and which fields a message carries is part of what
/// distinguishes it (`… rhost=<ipv4>` versus `… rhost=<ipv4> user=<*>` are two
/// pam_unix messages). This is the masker's own doctrine — `mask_token` keeps
/// keys verbatim because "the key is the discriminating part" — applied to the
/// merge layer.
fn attested_value(token: &MaskedToken) -> bool {
    match token {
        MaskedToken::WildCard | MaskedToken::Gap => false,
        MaskedToken::Val(s) => {
            let key_like = s.split_once('=').is_some_and(|(key, _)| {
                !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
            });
            !key_like && s.contains('<') && s.contains('>')
        }
    }
}

/// R3's alignment of two templates: the matched token count and the merged
/// skeleton, or `None` when some divergence window fails the value guard.
///
/// Alignment is a longest-common-subsequence over tokens, so a pair may
/// diverge in several windows at once — the shape two independent optional
/// segments produce, which no single prefix/suffix split can see. Gaps never
/// match (each is a zero-or-more stretch from an earlier merge, not a token),
/// so they fall into the windows and collapse into the new skeleton's gaps.
///
/// Every divergence window must be value-dominated, judged per side: a side
/// that contributes literal-bearing tokens must hold at least one
/// masker-attested value (see [`attested_value`]) and strictly more values
/// than literals. Strict, because a tie is an optional *phrase*, not a value:
/// loghub keeps `corrected` and `corrected over <duration>` apart (one
/// literal, one value). Per side, because pooling lets values on one side
/// outvote a decisive literal on the other: `… 12) *<num>` and `… 12)
/// *<num>, disabled.` pool to two values against one literal, and merging
/// them erases `disabled.`. The windows this rule exists for —
/// `(<size_kb>)`, `<num>:<num>` — are values with at most decoration.
///
/// A side holding no literal-bearing token at all is neutral and passes: an
/// empty side is an optional segment's absence, a wildcard side is a position
/// some earlier merge already generalized (`<*>` alone is not *evidence* — see
/// [`attested_value`] — but it need not be re-litigated), and a gap side is a
/// stretch this pass created itself, so two skeletons differing only in where
/// their gaps sit still unify.
/// One divergence window's evidence, per side (0 = the pair's first template,
/// 1 = its second). See [`align_pair`]'s docs for the guard it feeds.
#[derive(Default)]
struct WindowEvidence {
    values: [usize; 2],
    literals: [usize; 2],
    /// Whether the window held anything at all, including wildcards and gaps —
    /// an untouched window emits no gap, and an all-wildcard/gap window is
    /// vacuously value-dominated.
    saw_any: bool,
}

impl WindowEvidence {
    fn value_dominated(&self) -> bool {
        (0..2).all(|side| {
            let (values, literals) = (self.values[side], self.literals[side]);
            // A side with no literal-bearing tokens is neutral.
            values + literals == 0 || (values >= 1 && values > literals)
        })
    }
}

fn align_pair(a: &[MaskedToken], b: &[MaskedToken]) -> Option<AlignedPair> {
    // dp[i][j] = tokens matched aligning a[i..] with b[j..].
    let (la, lb) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; lb + 1]; la + 1];
    for i in (0..la).rev() {
        for j in (0..lb).rev() {
            let matches = a[i] == b[j] && !matches!(a[i], MaskedToken::Gap);
            dp[i][j] = if matches {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    // Walk the alignment, emitting matched tokens and folding each divergence
    // window into one gap — checking the window guard as it goes.
    let mut skeleton: Vec<MaskedToken> = Vec::with_capacity(la.max(lb));
    let matched = dp[0][0] as usize;
    let (mut i, mut j) = (0, 0);
    let mut window = WindowEvidence::default();
    let close_window = |skeleton: &mut Vec<MaskedToken>, window: &mut WindowEvidence| -> bool {
        if !window.saw_any {
            return true;
        }
        let ok = window.value_dominated();
        *window = WindowEvidence::default();
        if ok && !matches!(skeleton.last(), Some(MaskedToken::Gap)) {
            skeleton.push(MaskedToken::Gap);
        }
        ok
    };
    while i < la || j < lb {
        if i < la
            && j < lb
            && a[i] == b[j]
            && !matches!(a[i], MaskedToken::Gap)
            && dp[i][j] == dp[i + 1][j + 1] + 1
        {
            if !close_window(&mut skeleton, &mut window) {
                return None;
            }
            skeleton.push(a[i].clone());
            i += 1;
            j += 1;
            continue;
        }
        // Inside a divergence window: consume whichever side the DP says.
        let (token, side) = if i < la && (j >= lb || dp[i + 1][j] >= dp[i][j + 1]) {
            i += 1;
            (&a[i - 1], 0)
        } else {
            j += 1;
            (&b[j - 1], 1)
        };
        if matches!(token, MaskedToken::Gap) {
            // A gap from an earlier merge is a stretch, not a token: it widens
            // the window without weighing on either side's evidence, and it
            // survives into the skeleton even when the window contributes no
            // countable tokens.
            if !matches!(skeleton.last(), Some(MaskedToken::Gap)) {
                skeleton.push(MaskedToken::Gap);
            }
        } else {
            window.saw_any = true;
            match token {
                MaskedToken::WildCard => {}
                token if attested_value(token) => window.values[side] += 1,
                _ => window.literals[side] += 1,
            }
        }
    }
    if !close_window(&mut skeleton, &mut window) {
        return None;
    }
    let skeleton = collapse_gaps(skeleton);
    let windows = skeleton
        .iter()
        .filter(|t| matches!(t, MaskedToken::Gap))
        .count();
    Some(AlignedPair {
        matched,
        windows,
        skeleton,
    })
}

/// What [`align_pair`] found: the matched token count, how many divergence
/// windows the skeleton folded into gaps, and the skeleton itself.
struct AlignedPair {
    matched: usize,
    windows: usize,
    skeleton: Vec<MaskedToken>,
}

/// A template's token count with gaps excluded — its minimum width, and the
/// denominator R3's similarity is computed over.
fn non_gap_len(tokens: &[MaskedToken]) -> usize {
    tokens
        .iter()
        .filter(|t| !matches!(t, MaskedToken::Gap))
        .count()
}

/// Collapse adjacent gaps left by skeleton construction: `<gap> <gap>` matches
/// exactly what one gap matches.
fn collapse_gaps(tokens: Vec<MaskedToken>) -> Vec<MaskedToken> {
    let mut out: Vec<MaskedToken> = Vec::with_capacity(tokens.len());
    for token in tokens {
        if matches!(token, MaskedToken::Gap) && matches!(out.last(), Some(MaskedToken::Gap)) {
            continue;
        }
        out.push(token);
    }
    out
}

/// Which end of a template an R4 window sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Edge {
    /// Fixed prefix, variable-width tail.
    Tail,
    /// Fixed suffix, variable-width head.
    Head,
}

/// **R4, edge-window family**: templates sharing a fixed run of tokens at one
/// end (the anchor) whose windows at the other end are entirely variable
/// tokens, differing only in width.
///
/// R1's evidence applied to width: the masker or tree already emptied the
/// window of literal text on every member, so only how many tokens the value
/// spans is in question (`queued jobs <num>` / `queued jobs <num> <num>`).
/// Templates with any literal text in the window never join a family — see
/// the module docs for why a count-based evidence path was measured and
/// rejected here. Requiring at least two distinct widths keeps this rule off
/// R2's turf, where a same-width family is decided position by position.
fn edge_window_families(entries: Vec<Finalized>) -> Vec<Finalized> {
    let (merges, claimed) = {
        // Anchor -> member indices, over qualifying members only: a template
        // joins a family exactly when one end matches the anchor verbatim and
        // everything past it is variable.
        let mut families: HashMap<(Edge, &[MaskedToken]), Vec<usize>> = HashMap::new();
        for (idx, entry) in entries.iter().enumerate() {
            let len = entry.tokens.len();
            for split in 1..len {
                if all_variable(&entry.tokens[split..]) {
                    families
                        .entry((Edge::Tail, &entry.tokens[..split]))
                        .or_default()
                        .push(idx);
                }
                if all_variable(&entry.tokens[..split]) {
                    families
                        .entry((Edge::Head, &entry.tokens[split..]))
                        .or_default()
                        .push(idx);
                }
            }
        }

        let mut candidates: Vec<(Edge, &[MaskedToken], &Vec<usize>)> = families
            .iter()
            .filter(|((_, anchor), members)| {
                if members.len() < 2 || literal_count(anchor) < MIN_LITERALS {
                    return false;
                }
                // The window is everything past the anchor, on either edge.
                let widths: std::collections::HashSet<usize> = members
                    .iter()
                    .map(|idx| entries[*idx].tokens.len() - anchor.len())
                    .collect();
                widths.len() >= 2
            })
            .map(|((edge, anchor), members)| (*edge, *anchor, members))
            .collect();
        // Longest anchor first, so the most specific family claims its members
        // before a shorter anchor generalizes them further than the evidence
        // requires; the rest breaks ties deterministically.
        candidates.sort_by(|a, b| {
            b.1.len()
                .cmp(&a.1.len())
                .then_with(|| b.2.len().cmp(&a.2.len()))
                .then_with(|| super::render_template(a.1).cmp(&super::render_template(b.1)))
                .then_with(|| ((a.0 == Edge::Head) as u8).cmp(&((b.0 == Edge::Head) as u8)))
        });

        let mut claimed = vec![false; entries.len()];
        let mut merges: Vec<(Vec<MaskedToken>, Vec<usize>)> = Vec::new();
        for (edge, anchor, members) in candidates {
            if members.iter().any(|idx| claimed[*idx]) {
                continue;
            }
            let mut skeleton: Vec<MaskedToken> = Vec::with_capacity(anchor.len() + 1);
            match edge {
                Edge::Tail => {
                    skeleton.extend_from_slice(anchor);
                    skeleton.push(MaskedToken::Gap);
                }
                Edge::Head => {
                    skeleton.push(MaskedToken::Gap);
                    skeleton.extend_from_slice(anchor);
                }
            }
            let skeleton = collapse_gaps(skeleton);
            for idx in members {
                claimed[*idx] = true;
            }
            merges.push((skeleton, members.clone()));
        }
        (merges, claimed)
    };
    rebuild(entries, merges, &claimed)
}

/// **R3, aligned pair**: two templates of different widths that align on most
/// of their tokens — the shape optional segments mine as (`<num> bytes sent` /
/// `<num> bytes (<size_kb>) sent`). See the module docs for the rule and
/// [`align_pair`] for the guard.
///
/// Each template merges with at most one partner per round, closest first; the
/// fixpoint loop chains the rest.
fn pair_merges(entries: Vec<Finalized>) -> Vec<Finalized> {
    let mut candidates: Vec<(f64, usize, usize, Vec<MaskedToken>)> = {
        // A qualifying pair aligns ≥70% of the longer template, so it must
        // share the first or last non-gap token — unless one of the pair has a
        // wildcard or gap at that end, which matches any head or tail.
        // Bucketing by the edge tokens, with the wildcard-edged templates in
        // one bucket paired against everything, keeps the scan off the full
        // quadratic (wildcard-edged templates are few: each is itself the
        // product of a merge).
        let mut by_first: HashMap<&MaskedToken, Vec<usize>> = HashMap::new();
        let mut by_last: HashMap<&MaskedToken, Vec<usize>> = HashMap::new();
        let mut wild_edged: Vec<usize> = Vec::new();
        for (idx, entry) in entries.iter().enumerate() {
            // The raw edge tokens, not the first non-gap ones: a template
            // ending in a gap can align its literal tail into the middle of a
            // partner, so it belongs to every bucket.
            match (entry.tokens.first(), entry.tokens.last()) {
                (Some(first @ MaskedToken::Val(_)), Some(last @ MaskedToken::Val(_))) => {
                    by_first.entry(first).or_default().push(idx);
                    by_last.entry(last).or_default().push(idx);
                }
                _ => wild_edged.push(idx),
            }
        }
        // The wildcard-edged bucket pairs within itself and against everything.
        let all: Vec<usize> = (0..entries.len()).collect();
        let wild_pairs = wild_edged
            .iter()
            .flat_map(|&i| all.iter().map(move |&j| (i, j)).filter(|(i, j)| i != j));

        let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
        let mut candidates: Vec<(f64, usize, usize, Vec<MaskedToken>)> = Vec::new();
        let bucket_pairs = by_first
            .values()
            .chain(by_last.values())
            .flat_map(|bucket| {
                bucket
                    .iter()
                    .enumerate()
                    .flat_map(move |(pos, &i)| bucket[pos + 1..].iter().map(move |&j| (i, j)))
            });
        for (i, j) in bucket_pairs.chain(wild_pairs) {
            let (a, b) = (&entries[i].tokens, &entries[j].tokens);
            if !seen.insert((i.min(j), i.max(j))) {
                continue;
            }
            let (wa, wb) = (non_gap_len(a), non_gap_len(b));
            let Some(aligned) = align_pair(a, b) else {
                continue;
            };
            // A same-length gap-free pair differing in ONE window is R2's
            // turf: a single-position family, decided with count evidence
            // this pairwise rule does not have. Everything else is R3's —
            // different widths, positions that do not line up because of a
            // gap, or several windows at once (which R2's one-position model
            // cannot express, however many variants accumulate).
            let gap_free = a.len() == wa && b.len() == wb;
            if wa == wb && gap_free && aligned.windows <= 1 {
                continue;
            }
            let sim = aligned.matched as f64 / wa.max(wb) as f64;
            if sim < CROSS_LEN_SIMILARITY {
                continue;
            }
            if literal_count(&aligned.skeleton) < MIN_LITERALS {
                continue;
            }
            candidates.push((sim, i, j, aligned.skeleton));
        }
        candidates
    };

    // Closest pair first: where one template could merge with several
    // neighbours, it goes to the one it shares most with, and the fixpoint
    // loop revisits the rest. Ties break on the templates for determinism.
    candidates.sort_by(|x, y| {
        y.0.partial_cmp(&x.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| entries[x.1].template().cmp(&entries[y.1].template()))
            .then_with(|| entries[x.2].template().cmp(&entries[y.2].template()))
    });

    let mut claimed = vec![false; entries.len()];
    let mut merges: Vec<(Vec<MaskedToken>, Vec<usize>)> = Vec::new();
    for (_, i, j, skeleton) in candidates {
        if claimed[i] || claimed[j] {
            continue;
        }
        claimed[i] = true;
        claimed[j] = true;
        merges.push((skeleton, vec![i, j]));
    }
    rebuild(entries, merges, &claimed)
}

/// Fold one family member into the running merge: counts add, and the line range
/// and sample come from the earliest member, so `--drain=full` still shows a real
/// line and the first place the message appeared.
fn combine(acc: Option<Finalized>, entry: &Finalized) -> Finalized {
    let Some(acc) = acc else {
        return entry.clone();
    };
    let earliest_is_acc = match (acc.first_line, entry.first_line) {
        (Some(a), Some(b)) => a <= b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    };
    let (sample, first_line) = if earliest_is_acc {
        (acc.sample, acc.first_line)
    } else {
        (entry.sample.clone(), entry.first_line)
    };
    Finalized {
        tokens: acc.tokens,
        count: acc.count.saturating_add(entry.count),
        sample,
        first_line,
        last_line: acc.last_line.max(entry.last_line),
    }
}

/// Add `entry` to `by_template`, folding into an equal template if present.
fn absorb_into(by_template: &mut HashMap<Vec<MaskedToken>, Finalized>, entry: Finalized) {
    match by_template.get_mut(&entry.tokens) {
        Some(existing) => {
            let folded = combine(Some(existing.clone()), &entry);
            *existing = folded;
        }
        None => {
            by_template.insert(entry.tokens.clone(), entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(template: &str, count: usize, first_line: usize) -> Finalized {
        let tokens = template
            .split(' ')
            .map(|t| match t {
                "<*>" => MaskedToken::WildCard,
                // Test-only spelling; a real gap renders as `<*>` too.
                "<gap>" => MaskedToken::Gap,
                _ => MaskedToken::Val(t.to_string()),
            })
            .collect();
        Finalized {
            tokens,
            count,
            sample: format!("sample of {template}"),
            first_line: Some(first_line),
            last_line: Some(first_line + 10),
        }
    }

    fn templates(entries: Vec<Finalized>) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = entries
            .into_iter()
            .map(|e| (e.template(), e.count))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn absorbs_a_family_into_its_wildcard_sibling() {
        // The tree already generalized this position for some subset, so two
        // stragglers merge in however few they are.
        let merged = merge_variants(vec![
            entry("Invalid user <*> from <ipv4>", 15, 1),
            entry("Invalid user admin from <ipv4>", 5, 4),
            entry("Invalid user oracle from <ipv4>", 3, 9),
        ]);
        assert_eq!(
            templates(merged),
            vec![("Invalid user <*> from <ipv4>".to_string(), 23)]
        );
    }

    #[test]
    fn merges_a_long_thin_tail_with_no_wildcard_sibling() {
        let family: Vec<Finalized> = (0..MIN_VARIANTS)
            .map(|i| entry(&format!("Invalid user u{i} from <ipv4>"), 2, i + 1))
            .collect();
        let merged = merge_variants(family);
        assert_eq!(
            templates(merged),
            vec![("Invalid user <*> from <ipv4>".to_string(), 2 * MIN_VARIANTS)]
        );
    }

    #[test]
    fn keeps_a_small_family_of_keywords_apart() {
        // Two fat alternatives at one position is what a keyword looks like, and
        // neither side ever generalized, so no rule fires.
        let merged = merge_variants(vec![
            entry("session opened for user <*>", 122, 1),
            entry("session closed for user <*>", 123, 2),
        ]);
        assert_eq!(
            templates(merged),
            vec![
                ("session closed for user <*>".to_string(), 123),
                ("session opened for user <*>".to_string(), 122),
            ]
        );
    }

    #[test]
    fn refuses_a_merge_that_would_leave_no_literal() {
        // Event names in position 0: a big family by R2's count, but merging
        // yields `<*> <num>`, which names nothing. This is the guard's whole
        // reason for existing.
        let family: Vec<Finalized> = (0..MIN_VARIANTS + 4)
            .map(|i| entry(&format!("onEvent{i} <num>"), 20, i + 1))
            .collect();
        let expected = family.len();
        let merged = merge_variants(family);
        assert_eq!(merged.len(), expected, "nothing should merge");
    }

    #[test]
    fn merges_a_family_that_may_be_event_names() {
        // Pins a known limitation rather than an intended behaviour: with one
        // literal surviving, R2 merges a family whose varying position may hold
        // event names, so `<*> succeeded` names no event.
        //
        // It stays this way because every structural fix measured worse. The
        // rule that catches this — refusing to consume the leading literal —
        // takes Proxifier from 47.8% to 0.0%, because that dataset's leading
        // token is a partially-masked hostname that really is a parameter.
        // Requiring two literals does not catch it at all (`<*> handler done`
        // survives with two) and refuses the correct `starting <*>` below.
        // See the module docs for the full numbers before changing this.
        let family: Vec<Finalized> = (0..MIN_VARIANTS)
            .map(|i| entry(&format!("onEvent{i} succeeded"), 30, i + 1))
            .collect();
        assert_eq!(
            templates(merge_variants(family)),
            vec![("<*> succeeded".to_string(), 30 * MIN_VARIANTS)]
        );

        // The same shape, where merging is what a reader wants. Structurally
        // indistinguishable from the case above, which is the point.
        let services: Vec<Finalized> = (0..MIN_VARIANTS)
            .map(|i| entry(&format!("starting svc{i}"), 30, i + 1))
            .collect();
        assert_eq!(
            templates(merge_variants(services)),
            vec![("starting <*>".to_string(), 30 * MIN_VARIANTS)]
        );
    }

    #[test]
    fn a_partially_masked_token_counts_as_a_literal() {
        // `uid=<num>` carries text of its own, so a skeleton keeping only that
        // still names the message and the merge is allowed.
        let family: Vec<Finalized> = (0..MIN_VARIANTS)
            .map(|i| entry(&format!("<num> uid=<num> s{i}"), 3, i + 1))
            .collect();
        let merged = merge_variants(family);
        assert_eq!(
            templates(merged),
            vec![("<num> uid=<num> <*>".to_string(), 3 * MIN_VARIANTS)]
        );
    }

    #[test]
    fn merging_reaches_a_fixpoint_across_rounds() {
        // The R2 merge of the numbered family creates a wildcard sibling, which
        // R1 then uses to absorb the straggler a single round could not.
        let mut family: Vec<Finalized> = (0..MIN_VARIANTS)
            .map(|i| entry(&format!("job j{i} finished with <num> errors"), 2, i + 1))
            .collect();
        family.push(entry("job late finished with <num> errors", 7, 99));
        let merged = merge_variants(family);
        assert_eq!(
            templates(merged),
            vec![(
                "job <*> finished with <num> errors".to_string(),
                2 * MIN_VARIANTS + 7
            )]
        );
    }

    #[test]
    fn only_one_position_merges_per_family() {
        // Templates differing in two positions are not a variant family, so they
        // stay apart rather than collapsing into an all-wildcard template.
        let merged = merge_variants(vec![entry("alpha one x", 5, 1), entry("bravo two x", 5, 2)]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn keeps_the_earliest_sample_and_widens_the_line_range() {
        let merged = merge_variants(vec![
            entry("user <*> in", 1, 50),
            entry("user later in", 1, 90),
            entry("user early in", 1, 7),
        ]);
        assert_eq!(merged.len(), 1);
        let only = &merged[0];
        assert_eq!(only.count, 3);
        assert_eq!(only.first_line, Some(7));
        assert_eq!(only.last_line, Some(100));
        assert_eq!(only.sample, "sample of user early in");
    }

    #[test]
    fn an_unmergeable_set_is_returned_unchanged() {
        let merged = merge_variants(vec![entry("a b c", 3, 1), entry("x y z", 4, 2)]);
        assert_eq!(
            templates(merged),
            vec![("a b c".to_string(), 3), ("x y z".to_string(), 4)]
        );
    }

    #[test]
    fn merges_an_optional_segment_across_lengths() {
        // Proxifier's shape: the same message with and without a "(1.13 KB)"
        // segment (masked to one `(<size_kb>)` token by the spaced-size
        // pre-pass). Token counts differ, so no same-length rule can see it.
        let merged = merge_variants(vec![
            entry(
                "<fqdn>:<num> close, <num> bytes sent, lifetime <num>:<num>",
                500,
                1,
            ),
            entry(
                "<fqdn>:<num> close, <num> bytes (<size_kb>) sent, lifetime <num>:<num>",
                400,
                2,
            ),
        ]);
        assert_eq!(
            templates(merged),
            vec![(
                "<fqdn>:<num> close, <num> bytes <*> sent, lifetime <num>:<num>".to_string(),
                900
            )]
        );
    }

    #[test]
    fn an_optional_phrase_does_not_pair_merge() {
        // A value tied with a literal is an optional phrase, not a decorated
        // value: loghub keeps `corrected` and `corrected over N seconds`
        // apart (BGL), and the strict-majority clause of the window guard is
        // what enforces that.
        let merged = merge_variants(vec![
            entry("L3 EDRAM <function> detected and corrected", 30, 1),
            entry(
                "L3 EDRAM <function> detected and corrected over <duration>",
                20,
                2,
            ),
        ]);
        assert_eq!(merged.len(), 2, "got {:?}", templates(merged));
    }

    #[test]
    fn chains_pair_merges_over_several_optional_segments() {
        // Two independent optional segments -> four length variants. One R3
        // round merges nearest pairs; the fixpoint loop finishes the job.
        let merged = merge_variants(vec![
            entry("host close, <num> b sent, <num> b recv, life <num>", 10, 1),
            entry(
                "host close, <num> b (<size_kb>) sent, <num> b recv, life <num>",
                10,
                2,
            ),
            entry(
                "host close, <num> b sent, <num> b (<size_mb>) recv, life <num>",
                10,
                3,
            ),
            entry(
                "host close, <num> b (<size_kb>) sent, <num> b (<size_mb>) recv, life <num>",
                10,
                4,
            ),
        ]);
        assert_eq!(
            templates(merged),
            vec![(
                "host close, <num> b <*> sent, <num> b <*> recv, life <num>".to_string(),
                40
            )]
        );
    }

    #[test]
    fn a_purely_literal_window_does_not_pair_merge() {
        // High overlap, but the extra tokens carry no masked value: that is
        // what an extra keyword looks like, and keywords distinguish events.
        let merged = merge_variants(vec![
            entry("connection to backend closed cleanly now", 5, 1),
            entry("connection to backend closed cleanly right now", 5, 2),
        ]);
        assert_eq!(merged.len(), 2, "got {:?}", templates(merged));
    }

    #[test]
    fn keeps_a_cross_length_keyword_pair_apart() {
        // Shares only the first token: nowhere near the similarity bar.
        let merged = merge_variants(vec![
            entry("Connection closed", 40, 1),
            entry("Connection reset by peer <num>", 40, 2),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn a_literal_window_family_stays_apart_however_big() {
        // A fixed prefix followed by literal tails of varying width and
        // content. Merging on member count alone was measured and rejected:
        // the same shape is a subsystem prefix (`ciod: <*>` would swallow 24
        // distinct BGL messages), and over-merging cannot be recovered from
        // the output. See the module docs on R4.
        let family: Vec<Finalized> = (0..MIN_VARIANTS + 4)
            .map(|i| {
                // Tails of two different widths, so only the literal content
                // stands between this family and a merge.
                let tail = if i % 2 == 0 {
                    format!("x{i} y{i}")
                } else {
                    format!("x{i} y{i} z{i}")
                };
                entry(&format!("failed to resolve source name {tail}"), 3, i + 1)
            })
            .collect();
        let expected = family.len();
        let merged = merge_variants(family);
        assert_eq!(merged.len(), expected, "nothing should merge");
    }

    #[test]
    fn a_small_tail_family_needs_an_all_variable_window() {
        // Two members are no evidence by count, but a window the masker already
        // emptied of literals is R1's evidence: only the width varies.
        let merged = merge_variants(vec![
            entry("queued jobs <num>", 5, 1),
            entry("queued jobs <num> <num>", 5, 2),
        ]);
        assert_eq!(templates(merged), vec![("queued jobs <*>".to_string(), 10)]);

        // The same two-member family with literal text in the window stays
        // apart: width evidence alone does not overrule a possible keyword.
        let merged = merge_variants(vec![
            entry("queued jobs <num> now", 5, 1),
            entry("queued jobs <num> right now", 5, 2),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn a_gap_counts_as_a_wildcard_sibling_and_survives_the_merge() {
        // R1 with a gap sibling: the straggler differs from the gap template at
        // exactly the gap's position, so it merges in — and the skeleton keeps
        // the gap (zero-or-more) rather than narrowing it to one token, which
        // would orphan the lines the gap's earlier merge absorbed.
        let merged = merge_variants(vec![
            entry("session for <gap> closed cleanly", 10, 1),
            entry("session for admin closed cleanly", 3, 5),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].template(), "session for <*> closed cleanly");
        assert_eq!(merged[0].count, 13);
        assert!(
            merged[0].tokens.contains(&MaskedToken::Gap),
            "the merged skeleton must keep the gap: {:?}",
            merged[0].tokens
        );
    }
}

#[cfg(test)]
mod gap_edge_tests {
    use super::*;

    fn entry(template: &str, count: usize) -> Finalized {
        let tokens = template
            .split(' ')
            .map(|t| match t {
                "<*>" => MaskedToken::WildCard,
                "<gap>" => MaskedToken::Gap,
                _ => MaskedToken::Val(t.to_string()),
            })
            .collect();
        Finalized {
            tokens,
            count,
            sample: String::new(),
            first_line: Some(1),
            last_line: Some(1),
        }
    }

    #[test]
    fn a_same_length_pair_differing_in_two_value_windows_merges() {
        // Same raw token count, no gaps on one side — but the two divergences
        // (block id, size token vs. an already-generalized position) are both
        // value windows, which R2's one-position model cannot express however
        // many variants accumulate. R3 takes same-length pairs once they
        // diverge in more than one window; with exactly one it defers to R2.
        let merged = merge_variants(vec![
            entry(
                "Block rdd_<num>_<num> stored as bytes in memory (estimated size <size_b>, free <size_kb>)",
                75,
            ),
            entry(
                "Block broadcast_<num>_piece0 stored as bytes in memory (estimated size <gap> free <size_kb>)",
                37,
            ),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].template(),
            "Block <*> stored as bytes in memory (estimated size <*> free <size_kb>)"
        );
        assert_eq!(merged[0].count, 112);
    }

    #[test]
    fn a_decisive_literal_on_one_side_refuses_the_window() {
        // Pooled evidence would let the two `*<num>` values outvote
        // `disabled.`; judged per side, the second side is a value-literal tie
        // and the pair stays apart (loghub keeps enabled and disabled PCI
        // links separate).
        let merged = merge_variants(vec![
            entry("PCI Link [LNKA] (IRQs <num> <num>) *<num>", 3),
            entry("PCI Link [LNKD] (IRQs <num> <num>) *<num>, disabled.", 5),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn a_gap_edged_skeleton_pairs_beyond_its_own_bucket() {
        // R3's pair scan buckets templates by their raw edge tokens; a
        // skeleton that begins and ends in a gap shares neither edge with the
        // stragglers it must absorb (its literal `close,` is not its first
        // token), so it has to be paired against every bucket. Skipping the
        // gaps when picking the bucket key put this exact pair — Proxifier's
        // final unification — out of reach.
        let merged = merge_variants(vec![
            entry(
                "<gap> close, <num> bytes <gap> sent, <num> bytes <gap> received, lifetime <gap>",
                945,
            ),
            entry(
                "www.<num>.com:<num> close, <num> bytes sent, <num> bytes received, lifetime <num>:<num>",
                1,
            ),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].template(),
            "<*> close, <num> bytes <*> sent, <num> bytes <*> received, lifetime <*>"
        );
        assert_eq!(merged[0].count, 946);
    }
}
