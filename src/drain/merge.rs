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
//! # The two rules
//!
//! Both act on a *variant family*: templates in one length bucket that agree
//! everywhere except position `i`.
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
//! Rounds repeat to a fixpoint (bounded by [`MAX_ROUNDS`]) because a merge
//! creates a new wildcard sibling, which can make R1 apply where it did not
//! before: fifty user names collapse, and the result then absorbs the near-miss
//! that only R1 could see.

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

/// Literal tokens a merged template must retain. Below this it identifies
/// nothing at all.
///
/// Deliberately 1, not 2: raising it neither fixes the event-name shape nor pays
/// for itself on the suite. See the module docs under "What the guard does not
/// do" for the measurements behind that.
const MIN_LITERALS: usize = 1;

/// Fixpoint iteration bound. Each round strictly reduces the template count, so
/// this only guards against a future rule that does not.
const MAX_ROUNDS: usize = 8;

/// Merge variant families in `entries`, returning the finished set.
///
/// Order of the result is unspecified; the caller sorts.
pub(super) fn merge_variants(mut entries: Vec<Finalized>) -> Vec<Finalized> {
    for _ in 0..MAX_ROUNDS {
        let before = entries.len();
        entries = one_round(entries);
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
        let has_wildcard_sibling = members
            .iter()
            .any(|idx| matches!(entries[*idx].tokens[position], MaskedToken::WildCard));
        let rule_applies = has_wildcard_sibling || members.len() >= MIN_VARIANTS;
        if !rule_applies {
            continue;
        }
        let skeleton = family.skeleton();
        if literal_count(&skeleton) < MIN_LITERALS {
            continue;
        }
        for idx in members {
            claimed[*idx] = true;
        }
        merges.push((skeleton, members.clone()));
    }

    if merges.is_empty() {
        return entries;
    }

    // Rebuild: merged families first, then whatever was left untouched. Two
    // families can collapse onto the same skeleton, so fold by template.
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
            MaskedToken::WildCard => false,
            MaskedToken::Val(s) => !s.is_empty() && !is_bare_placeholder(s),
        })
        .count()
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
            .map(|t| {
                if t == "<*>" {
                    MaskedToken::WildCard
                } else {
                    MaskedToken::Val(t.to_string())
                }
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
}
