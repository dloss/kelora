//! The prefix tree that groups masked lines into clusters.
//!
//! kelora's own implementation of the Drain tree
//! (<https://arxiv.org/pdf/1806.04356.pdf>), replacing the vendored `drain-rs`
//! fork. Three reasons it is worth owning rather than patching:
//!
//! 1. **The fork had already stopped being upstream.** Two behavioural patches
//!    deep (a cluster's template was never generalized; clusters gained a stable
//!    id), the "diffable against 0.3.0" goal it was kept for was gone.
//!
//! 2. **The crate has to guess which tokens vary; kelora knows.** `drain-rs`
//!    keys the tree on "token contains a digit" because that is all a generic
//!    implementation can see. By the time a line reaches this tree, kelora's
//!    [`Masker`](super::Masker) has already replaced every value it recognized
//!    with a named placeholder, so "is this position variable?" is answered by
//!    *what the masker did* — see [`Tree::key_token`].
//!
//! 3. **Two defects were structural, not one-liners.** The crate keys the tree
//!    on `max_depth` leading tokens *and* on the token at the final level, and
//!    it terminates at `depth == len - 1`, so every token of a line no longer
//!    than `max_depth + 1` became part of the routing key. A line whose tokens
//!    are all keys can never generalize: no position is left for the algorithm's
//!    defining step. That silently broke every short message —
//!    `session closed for user cyrus/news/test` stayed three templates each
//!    naming a user that 60% of its events did not have — and it made `depth`
//!    mean something different from the literature (`depth=4` keyed on 5 tokens
//!    where the reference implementation keys on 1).
//!
//! Here `depth` means exactly what it says: **the number of leading tokens used
//! as routing keys**, and never more than `len - 1`, so some position is always
//! free to generalize ([`Tree::key_count`]).
//!
//! Memory is bounded: a stream with unbounded template variety evicts the
//! least-recently-matched cluster past `max_clusters` rather than growing
//! forever, and reports that it did (`--drain` on `tail -f` had no cap at all).

use super::MaskedToken;
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;

/// Tuning for one [`Tree`]. Mirrors the user-facing options of
/// [`DrainConfig`](super::DrainConfig), already sanitized.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TreeConfig {
    /// Leading tokens used as routing keys (see the module docs).
    pub depth: usize,
    /// Distinct literal keys allowed at one tree node before further values
    /// route through the wildcard child, bounding fan-out on a position that
    /// turns out to be variable after all.
    pub max_children: usize,
    /// Minimum fraction of exactly-matching token positions for a line to join
    /// an existing cluster instead of starting a new one.
    pub min_similarity: f64,
    /// Clusters held before the least-recently-matched one is evicted.
    pub max_clusters: usize,
}

/// One mined cluster: a template plus how many lines it has matched.
#[derive(Debug)]
pub(super) struct Cluster {
    /// The template, in the same token form a masked line has. A position the
    /// cluster's members disagree on holds [`MaskedToken::WildCard`].
    pub tokens: Vec<MaskedToken>,
    pub count: u64,
    /// Where this cluster hangs, so eviction can unlink it without searching.
    route: Route,
}

impl Cluster {
    /// The template as displayed. See [`super::render_template`].
    pub fn template(&self) -> String {
        super::render_template(&self.tokens)
    }

    /// Fold `line` into this cluster, generalizing every position the two
    /// disagree on. This is Drain's defining step.
    ///
    /// Any disagreement generalizes, in either direction — unlike `drain-rs`,
    /// which skipped the comparison when the *incoming* token was already a
    /// wildcard and so left the cluster claiming a literal for a line that never
    /// said it. That is the same class of lie as the bug this replaces.
    fn absorb(&mut self, line: &[MaskedToken]) {
        for (slot, token) in self.tokens.iter_mut().zip(line.iter()) {
            if slot != token {
                *slot = MaskedToken::WildCard;
            }
        }
        self.count += 1;
    }

    /// Similarity to `line`, Drain's ordering: fraction of positions matching
    /// exactly, with the number of wildcard positions breaking ties (a cluster
    /// that has to generalize less is the better home).
    fn similarity(&self, line: &[MaskedToken]) -> (f64, usize) {
        let mut exact = 0usize;
        let mut wild = 0usize;
        for (slot, token) in self.tokens.iter().zip(line.iter()) {
            if slot == token {
                exact += 1;
            } else if matches!(slot, MaskedToken::WildCard) {
                wild += 1;
            }
        }
        (exact as f64 / self.tokens.len() as f64, wild)
    }
}

/// A cluster's position in the tree: token count, then one key per keyed
/// position. Short by construction (`depth` is small), so cloning it per
/// inserted cluster is cheap.
type Route = (usize, Vec<MaskedToken>);

/// What [`Tree::add`] did with a line.
#[derive(Debug, Clone, Copy)]
pub(super) struct Match {
    pub id: u64,
    pub count: u64,
    /// True when this line created the cluster.
    pub is_new: bool,
}

#[derive(Debug)]
enum Node {
    /// Keyed children. [`MaskedToken::WildCard`] is the overflow child.
    Inner(HashMap<MaskedToken, Node>),
    /// Candidate clusters at this route, most recently created last.
    Leaf(Vec<u64>),
}

#[derive(Debug)]
pub(super) struct Tree {
    cfg: TreeConfig,
    /// One tree per token count, as in Drain: lines of different length are
    /// never the same message, and bucketing first keeps every comparison
    /// between equal-length token vectors.
    roots: HashMap<usize, Node>,
    /// Clusters by id, in least-recently-matched order for eviction.
    clusters: LruCache<u64, Cluster>,
    next_id: u64,
    /// Clusters dropped to stay under `max_clusters`; surfaced as a warning,
    /// since their counts are missing from the output.
    evicted: u64,
}

impl Tree {
    pub(super) fn new(cfg: TreeConfig) -> Self {
        let cap = NonZeroUsize::new(cfg.max_clusters.max(1)).expect("max_clusters >= 1");
        Self {
            cfg,
            roots: HashMap::new(),
            clusters: LruCache::new(cap),
            next_id: 0,
            evicted: 0,
        }
    }

    /// How many leading positions are routing keys for a line of `len` tokens.
    ///
    /// Capped at `len - 1` so at least one position is always left for the
    /// cluster to generalize. Without that cap a line no longer than `depth` has
    /// every position keyed, which routes each distinct value to its own leaf and
    /// makes a `<*>` impossible — the defect that made every short message mine
    /// one template per value.
    fn key_count(&self, len: usize) -> usize {
        self.cfg.depth.min(len.saturating_sub(1))
    }

    /// The routing key for `token`: its literal text, or the wildcard when the
    /// masker has already established that this position holds a value.
    ///
    /// This is the step a generic Drain has to approximate. `drain-rs` keys on
    /// "the token contains a digit", which throws away `worker-3` and `HTTP/1.1`
    /// while keeping the placeholder `<ipv4>` apart from `<fqdn>`. kelora knows
    /// which spans were values because it masked them, so a token that is
    /// *nothing but* a placeholder keys as the wildcard, while a token that
    /// merely contains one (`uid=<num>`, `HTTP/<version>`) keys on its literal
    /// text — that text is exactly what names the message.
    fn key_token(token: &MaskedToken) -> MaskedToken {
        match token {
            MaskedToken::WildCard => MaskedToken::WildCard,
            MaskedToken::Val(s) if is_bare_placeholder(s) => MaskedToken::WildCard,
            MaskedToken::Val(s) => MaskedToken::Val(s.clone()),
        }
    }

    /// Add a masked line, returning the cluster it belongs to.
    ///
    /// `route_scratch` is caller-owned scratch space for the routing key, reused
    /// across lines so the common path (an existing cluster) allocates only the
    /// key tokens themselves.
    pub(super) fn add(
        &mut self,
        line: &[MaskedToken],
        route_scratch: &mut Vec<MaskedToken>,
    ) -> Match {
        let keys = self.key_count(line.len());
        route_scratch.clear();

        // Walk down, creating nodes as needed, and collect the effective route
        // (which can differ from the line's own tokens once a node overflows
        // into its wildcard child).
        let max_children = self.cfg.max_children;
        let mut node = self
            .roots
            .entry(line.len())
            .or_insert_with(|| Node::new_at(0, keys));
        for (depth, token) in line.iter().enumerate().take(keys) {
            let wanted = Self::key_token(token);
            let Node::Inner(children) = node else {
                // Reached a leaf early: only possible if `keys` shrank, which it
                // cannot for a fixed length bucket. Stop routing and use it.
                break;
            };
            let key = if children.contains_key(&wanted) || children.len() < max_children {
                wanted
            } else {
                MaskedToken::WildCard
            };
            route_scratch.push(key.clone());
            node = children
                .entry(key)
                .or_insert_with(|| Node::new_at(depth + 1, keys));
        }

        let Node::Leaf(candidates) = node else {
            // An inner node at the end of the walk means `keys` positions were
            // consumed without reaching a leaf, which `Node::new_at` prevents.
            unreachable!("routing ended on an inner node");
        };

        // Best candidate in this leaf, if any clears the similarity bar.
        let mut best: Option<(f64, usize, u64)> = None;
        for id in candidates.iter() {
            let Some(cluster) = self.clusters.peek(id) else {
                continue;
            };
            let (exact, wild) = cluster.similarity(line);
            if best.is_none_or(|(bx, bw, _)| exact > bx || (exact == bx && wild > bw)) {
                best = Some((exact, wild, *id));
            }
        }

        if let Some((exact, _, id)) = best {
            if exact >= self.cfg.min_similarity {
                // `get_mut` also marks the cluster most-recently-used.
                let cluster = self.clusters.get_mut(&id).expect("peeked cluster exists");
                cluster.absorb(line);
                return Match {
                    id,
                    count: cluster.count,
                    is_new: false,
                };
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        candidates.push(id);
        let cluster = Cluster {
            tokens: line.to_vec(),
            count: 1,
            route: (line.len(), route_scratch.clone()),
        };
        // `push` returns whatever it had to evict to stay within capacity.
        if let Some((_, dropped)) = self.clusters.push(id, cluster) {
            self.evicted += 1;
            self.unlink(&dropped);
        }
        Match {
            id,
            count: 1,
            is_new: true,
        }
    }

    /// Remove an evicted cluster's id from the leaf that held it, and prune the
    /// leaf if it is now empty, so a long-running stream does not accumulate
    /// dead routing structure alongside its bounded cluster set.
    fn unlink(&mut self, dropped: &Cluster) {
        let (len, route) = &dropped.route;
        let Some(root) = self.roots.get_mut(len) else {
            return;
        };
        // Walk to the leaf, remembering the path so empty nodes can be pruned.
        let mut node = root;
        for key in route {
            match node {
                Node::Inner(children) => match children.get_mut(key) {
                    Some(child) => node = child,
                    None => return,
                },
                Node::Leaf(_) => break,
            }
        }
        if let Node::Leaf(ids) = node {
            // The evicted id is gone from `clusters`, so drop it here too; other
            // ids in this leaf stay valid.
            ids.retain(|id| self.clusters.contains(id));
        }
    }

    /// Every live cluster. Order is the LRU's (most recently matched first),
    /// which is deterministic for a given input — callers sort for display.
    pub(super) fn clusters(&self) -> impl Iterator<Item = (u64, &Cluster)> {
        self.clusters.iter().map(|(id, cluster)| (*id, cluster))
    }

    pub(super) fn cluster(&self, id: u64) -> Option<&Cluster> {
        self.clusters.peek(&id)
    }

    /// Clusters dropped to stay under `max_clusters`.
    pub(super) fn evicted(&self) -> u64 {
        self.evicted
    }
}

impl Node {
    /// A node at `depth`, which is a leaf once every keyed position is consumed.
    fn new_at(depth: usize, keys: usize) -> Node {
        if depth >= keys {
            Node::Leaf(Vec::new())
        } else {
            Node::Inner(HashMap::new())
        }
    }
}

/// Whether `token` is nothing but a placeholder — `<num>`, `<ipv4>`, `<*>` — as
/// opposed to a token that merely contains one (`uid=<num>`, `HTTP/<version>`),
/// whose literal text still names the message.
pub(super) fn is_bare_placeholder(token: &str) -> bool {
    token.len() > 2
        && token.starts_with('<')
        && token.ends_with('>')
        && !token[1..token.len() - 1].contains(['<', '>'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(line: &str) -> Vec<MaskedToken> {
        line.split(' ')
            .map(|t| {
                if t == "<*>" {
                    MaskedToken::WildCard
                } else {
                    MaskedToken::Val(t.to_string())
                }
            })
            .collect()
    }

    fn tree(depth: usize) -> Tree {
        Tree::new(TreeConfig {
            depth,
            max_children: 100,
            min_similarity: 0.4,
            max_clusters: 1000,
        })
    }

    fn mine(tree: &mut Tree, lines: &[&str]) -> Vec<String> {
        let mut scratch = Vec::new();
        for line in lines {
            tree.add(&tokens(line), &mut scratch);
        }
        let mut out: Vec<String> = tree.clusters().map(|(_, c)| c.template()).collect();
        out.sort();
        out
    }

    #[test]
    fn generalizes_the_position_members_disagree_on() {
        let mut t = tree(4);
        let templates = mine(
            &mut t,
            &[
                "session closed for user cyrus",
                "session closed for user news",
                "session closed for user test",
            ],
        );
        assert_eq!(templates, vec!["session closed for user <*>"]);
    }

    #[test]
    fn a_short_line_still_leaves_a_position_to_generalize() {
        // Every token would be a routing key under `drain-rs`'s termination
        // rule, so these mined one template per value however `depth` was set.
        let mut t = tree(4);
        assert_eq!(
            mine(&mut t, &["started service alpha", "started service bravo"]),
            vec!["started service <*>"]
        );
        let mut t = tree(4);
        assert_eq!(mine(&mut t, &["up ok", "up bad"]), vec!["up <*>"]);
    }

    #[test]
    fn a_single_token_line_is_never_generalized_away() {
        // With no position to spare, distinct one-token lines stay distinct
        // rather than collapsing to a bare `<*>` that identifies nothing.
        let mut t = tree(4);
        assert_eq!(
            mine(&mut t, &["started", "stopped"]),
            vec!["started", "stopped"]
        );
    }

    #[test]
    fn keys_on_literal_text_but_not_on_a_bare_placeholder() {
        // `<ipv4>` and `<fqdn>` at a keyed position must not split the cluster:
        // the masker already established that the position holds a value.
        let mut t = tree(4);
        assert_eq!(
            mine(&mut t, &["connect <ipv4> ok now", "connect <fqdn> ok now"],),
            vec!["connect <*> ok now"]
        );
        // A token that only *contains* a placeholder keys on its text, so two
        // different keys stay two templates.
        let mut t = tree(4);
        assert_eq!(
            mine(
                &mut t,
                &["auth uid=<num> done now", "auth gid=<num> done now"]
            ),
            vec!["auth gid=<num> done now", "auth uid=<num> done now"]
        );
    }

    #[test]
    fn different_token_counts_never_share_a_cluster() {
        let mut t = tree(4);
        assert_eq!(
            mine(&mut t, &["a b c d", "a b c"]),
            vec!["a b c", "a b c d"]
        );
    }

    #[test]
    fn a_dissimilar_line_starts_its_own_cluster() {
        let mut t = tree(1);
        // One key token, so both reach the same leaf; similarity 1/4 is below
        // the 0.4 bar, so they do not merge into an all-wildcard template.
        assert_eq!(
            mine(&mut t, &["x alpha bravo charlie", "x delta echo foxtrot"]),
            vec!["x alpha bravo charlie", "x delta echo foxtrot"]
        );
    }

    #[test]
    fn overflowing_a_node_routes_through_the_wildcard_child() {
        let mut t = Tree::new(TreeConfig {
            depth: 2,
            max_children: 2,
            min_similarity: 0.4,
            max_clusters: 1000,
        });
        let mut scratch = Vec::new();
        for name in ["alpha", "bravo", "charlie", "delta"] {
            t.add(&tokens(&format!("job {name} finished ok")), &mut scratch);
        }
        // The first two keys get a child each; the rest overflow into the
        // wildcard child and cluster together there.
        let mut templates: Vec<String> = t.clusters().map(|(_, c)| c.template()).collect();
        templates.sort();
        assert_eq!(
            templates,
            vec![
                "job <*> finished ok",
                "job alpha finished ok",
                "job bravo finished ok",
            ]
        );
        let overflow = t
            .clusters()
            .find(|(_, c)| c.template() == "job <*> finished ok")
            .expect("wildcard cluster");
        assert_eq!(overflow.1.count, 2, "charlie and delta share it");
    }

    #[test]
    fn evicts_the_least_recently_matched_cluster() {
        let mut t = Tree::new(TreeConfig {
            depth: 2,
            max_children: 100,
            min_similarity: 0.9,
            max_clusters: 2,
        });
        let mut scratch = Vec::new();
        t.add(&tokens("one alpha x y"), &mut scratch);
        t.add(&tokens("two bravo x y"), &mut scratch);
        // Touch the first so the second becomes least-recently-matched.
        t.add(&tokens("one alpha x y"), &mut scratch);
        t.add(&tokens("three charlie x y"), &mut scratch);

        let mut templates: Vec<String> = t.clusters().map(|(_, c)| c.template()).collect();
        templates.sort();
        assert_eq!(
            templates,
            vec!["one alpha x y", "three charlie x y"],
            "the untouched cluster is the one evicted"
        );
        assert_eq!(t.evicted(), 1);
        // The evicted id is unlinked, so its leaf does not keep pointing at it.
        let mut scratch2 = Vec::new();
        let m = t.add(&tokens("two bravo x y"), &mut scratch2);
        assert!(
            m.is_new,
            "the evicted cluster is gone, not silently rejoined"
        );
    }

    #[test]
    fn is_bare_placeholder_only_matches_a_whole_placeholder() {
        assert!(is_bare_placeholder("<num>"));
        assert!(is_bare_placeholder("<*>"));
        assert!(!is_bare_placeholder("uid=<num>"));
        assert!(!is_bare_placeholder("HTTP/<version>"));
        assert!(!is_bare_placeholder("<num>,<num>"));
        assert!(!is_bare_placeholder("<>"));
        assert!(!is_bare_placeholder("plain"));
    }
}
