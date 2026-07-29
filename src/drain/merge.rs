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
//! A merge that leaves a template with no literal tokens is refused
//! ([`MIN_LITERALS`]). This is what separates a parameter from an event name.
//! Position 0 frequently holds the name of the thing that happened —
//! `onStandStepChanged <num>`, `onReceive <num>`, a dozen more in one Android or
//! HealthApp log — and those are a large variant family by R2's test. Merging
//! them yields `<*> <num>`, which identifies nothing and silently swallows a
//! dozen distinct events. The same guard blocks `<*> <path> <num>` from eating
//! every HTTP method.
//!
//! A token counts as literal when it carries text of its own: `Invalid`, `user`
//! and `uid=<num>` do, while a bare placeholder (`<num>`, `<ipv4>`, `<*>`) does
//! not — it is a position the masker already emptied of meaning.
//!
//! Rounds repeat to a fixpoint (bounded by [`MAX_ROUNDS`]) because a merge
//! creates a new wildcard sibling, which can make R1 apply where it did not
//! before: fifty user names collapse, and the result then absorbs the near-miss
//! that only R1 could see.

use super::{is_bare_placeholder, Finalized, MaskedToken};
use std::collections::HashMap;

/// Identifies a variant family: token count, the position that varies, and the
/// shared skeleton (the template with that position wildcarded).
type FamilyKey = (usize, usize, Vec<MaskedToken>);

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
/// nothing; see the module docs on the event-name failure mode.
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
    let mut families: HashMap<FamilyKey, Vec<usize>> = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        for position in 0..entry.tokens.len() {
            let skeleton = skeleton_at(&entry.tokens, position);
            families
                .entry((entry.tokens.len(), position, skeleton))
                .or_default()
                .push(idx);
        }
    }

    // Largest families first so the most confident merge claims its members
    // before a smaller overlapping one can; ties break on the family key for a
    // deterministic result regardless of HashMap order.
    let mut candidates: Vec<(&FamilyKey, &Vec<usize>)> = families.iter().collect();
    candidates.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| a.0 .1.cmp(&b.0 .1))
            .then_with(|| super::render_template(&a.0 .2).cmp(&super::render_template(&b.0 .2)))
    });

    let mut claimed = vec![false; entries.len()];
    // Merged results, and the members that went into each.
    let mut merges: Vec<(Vec<MaskedToken>, Vec<usize>)> = Vec::new();
    for ((_, position, skeleton), members) in candidates {
        if members.len() < 2 || members.iter().any(|idx| claimed[*idx]) {
            continue;
        }
        let has_wildcard_sibling = members
            .iter()
            .any(|idx| matches!(entries[*idx].tokens[*position], MaskedToken::WildCard));
        let rule_applies = has_wildcard_sibling || members.len() >= MIN_VARIANTS;
        if !rule_applies || literal_count(skeleton) < MIN_LITERALS {
            continue;
        }
        for idx in members {
            claimed[*idx] = true;
        }
        merges.push((skeleton.clone(), members.clone()));
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

/// `tokens` with `position` replaced by the wildcard.
fn skeleton_at(tokens: &[MaskedToken], position: usize) -> Vec<MaskedToken> {
    let mut out = tokens.to_vec();
    out[position] = MaskedToken::WildCard;
    out
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
