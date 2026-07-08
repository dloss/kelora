//! Projection pushdown: materialize only the fields the pipeline can observe.
//!
//! When a query names a bounded set of fields (`-k time,level,msg`), most of a
//! wide record's fields are parsed into `Dynamic` values only to be thrown away
//! by `KeyFilterStage`. A [`Projection`] lets a parser skip building the
//! `Dynamic` (and the `FieldMap` insert) for fields that provably nothing
//! downstream can read.
//!
//! ## Safety model
//!
//! Projection is invisible: it may only avoid materializing fields that
//! *provably* nothing observes. The invariant that makes this sound is that
//! [`Projection::Only`] always keeps a **superset** of what `KeyFilterStage`
//! would keep, plus every field any earlier stage reads:
//!
//! - `-k` keys — kept, then narrowed to exactly `-k` (in order) by
//!   `KeyFilterStage`, which always runs when `-k` is present.
//! - the level field names, when a level filter is present.
//! - the timestamp candidate fields, so `parsed_ts` (and `_ts` output, and the
//!   result-time span in `--stats`) is unchanged.
//! - the `--drain` target field.
//!
//! Every stage declares its demands through [`Demand`] (see
//! `ScriptStage::field_demands`), and the default is [`Demand::All`] so a future
//! stage that forgets to think about projection fails safe. Any single `All`
//! demand — a Rhai stage, an exclude-only key filter, `--stats`/`--discover`,
//! an unsupported parser — collapses the whole projection to [`Projection::All`]
//! and the pipeline behaves exactly as before.

use std::collections::HashSet;

/// Field-name set backing [`Projection::Only`]. Uses ahash (same fast,
/// non-cryptographic hasher as [`crate::event::FieldMap`]) because `wants` is
/// probed once per field per line on the hot parse path — the std SipHash
/// default measurably regresses narrow records, where the per-field lookup can
/// outweigh the value materialization the projection saves.
pub type FieldNameSet = HashSet<String, ahash::RandomState>;

/// The set of top-level field names the pipeline may need to materialize.
#[derive(Debug, Clone)]
pub enum Projection {
    /// Materialize every field (no pushdown). The safe default.
    All,
    /// Materialize only these top-level field names; skip building `Dynamic`
    /// values for anything else.
    Only(FieldNameSet),
}

impl Projection {
    /// Whether a parser should materialize the value for `key`.
    #[inline]
    pub fn wants(&self, key: &str) -> bool {
        match self {
            Projection::All => true,
            Projection::Only(set) => set.contains(key),
        }
    }

    /// True when no pushdown applies (materialize everything).
    #[inline]
    pub fn is_all(&self) -> bool {
        matches!(self, Projection::All)
    }
}

/// A stage's declared demand on event fields, used to compute the pipeline-wide
/// [`Projection`] at construction time.
#[derive(Debug, Clone)]
pub enum Demand {
    /// Observes arbitrary fields (e.g. a Rhai stage that can index by variable,
    /// iterate `e.keys()`, etc.). Forces the projection to [`Projection::All`].
    /// This is the fail-safe default for any stage that does not override
    /// `field_demands`.
    All,
    /// Observes exactly these top-level field names and nothing else.
    Fields(Vec<String>),
    /// Observes no event fields at all. Reserved for stages that act only on
    /// event metadata (line number, timestamp already parsed, etc.) rather than
    /// field contents; no current stage needs it, but it keeps the demand model
    /// complete so such a stage would not have to fall back to `All`.
    #[allow(dead_code)]
    Nothing,
}
