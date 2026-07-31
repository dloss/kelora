use rhai::Dynamic;
use std::cell::RefCell;
use std::collections::HashMap;

/// Snapshot of tracking state separated into user-visible metrics and internal-only data.
#[derive(Debug, Clone, Default)]
pub struct TrackingSnapshot {
    pub user: HashMap<String, Dynamic>,
    pub internal: HashMap<String, Dynamic>,
}

impl TrackingSnapshot {
    pub fn from_parts(user: HashMap<String, Dynamic>, internal: HashMap<String, Dynamic>) -> Self {
        Self { user, internal }
    }
}

thread_local! {
    pub static THREAD_TRACKING_STATE: RefCell<TrackingSnapshot> = RefCell::new(TrackingSnapshot::default());
    static UNDO_JOURNAL: RefCell<UndoJournal> = const { RefCell::new(UndoJournal::new()) };
}

/// How to undo one `track_*` mutation.
///
/// A script stage is atomic: if it errors, the pipeline falls back to the state
/// it had before the stage ran — the event keeps its old fields, emitted events
/// and queued file writes are dropped, and metrics recorded by that stage are
/// taken back with them. Undoing by *inverting each mutation* is what makes that
/// affordable: keeping a pre-image of the metrics instead would mean copying
/// every accumulated value on every event, which is the cost this whole design
/// exists to avoid.
///
/// Growing containers therefore get a targeted inverse (decrement this count,
/// pop this element); scalars and serialized sketches, whose size does not grow
/// with the data, simply keep their previous value.
pub(crate) enum Undo {
    /// Put a whole metric back the way it was — `None` meaning "it did not
    /// exist". For scalars, `{sum, count}` pairs and sketch blobs.
    Value { key: String, prior: Option<Dynamic> },
    /// Put one entry of a frequency table back.
    MapCount {
        key: String,
        entry: String,
        prior: Option<i64>,
    },
    /// Undo a push onto a unique-values array.
    ArrayPush { key: String },
    /// Put one element of a ranked array back — `None` meaning it was appended.
    ArrayEntry {
        key: String,
        index: usize,
        prior: Option<Dynamic>,
    },
}

struct UndoJournal {
    /// Only true while a script stage is running. Outside one — the parallel
    /// merge, `--end`, direct calls in tests — there is no stage to roll back
    /// to, so nothing is recorded and no entry is built.
    recording: bool,
    entries: Vec<Undo>,
}

impl UndoJournal {
    const fn new() -> Self {
        Self {
            recording: false,
            entries: Vec::new(),
        }
    }
}

/// Record how to undo a mutation, if a stage is running.
///
/// The entry is built lazily: outside a stage the closure never runs, so
/// nothing is cloned to describe a mutation nobody can roll back.
pub(crate) fn journal_undo(entry: impl FnOnce() -> Undo) {
    UNDO_JOURNAL.with(|journal| {
        let mut journal = journal.borrow_mut();
        if journal.recording {
            journal.entries.push(entry());
        }
    });
}

/// Set a metric to a new value, remembering how to put the old one back.
///
/// The right helper for any metric whose value does not grow with the data —
/// scalars, `{sum, count}` pairs, serialized sketches. Growing containers
/// (frequency tables, unique-value and ranked arrays) must journal a targeted
/// inverse instead of a whole-value pre-image; see [`Undo`].
pub(super) fn set_metric(state: &mut HashMap<String, Dynamic>, key: &str, value: Dynamic) {
    let prior = state.insert(key.to_string(), value);
    journal_undo(|| Undo::Value {
        key: key.to_string(),
        prior,
    });
}

/// Start recording undo entries for one script stage.
fn begin_stage_journal() {
    UNDO_JOURNAL.with(|journal| {
        let mut journal = journal.borrow_mut();
        journal.entries.clear();
        journal.recording = true;
    });
}

/// The stage succeeded: its mutations stand.
fn commit_stage_journal() {
    UNDO_JOURNAL.with(|journal| {
        let mut journal = journal.borrow_mut();
        journal.entries.clear();
        journal.recording = false;
    });
}

/// The stage failed: undo its mutations, newest first.
fn rollback_stage_journal() {
    let entries = UNDO_JOURNAL.with(|journal| {
        let mut journal = journal.borrow_mut();
        journal.recording = false;
        std::mem::take(&mut journal.entries)
    });

    if entries.is_empty() {
        return;
    }

    with_user_tracking(|state| {
        for entry in entries.into_iter().rev() {
            apply_undo(state, entry);
        }
    });
}

fn apply_undo(state: &mut HashMap<String, Dynamic>, entry: Undo) {
    match entry {
        Undo::Value { key, prior } => match prior {
            Some(value) => {
                state.insert(key, value);
            }
            None => {
                state.remove(&key);
            }
        },
        Undo::MapCount { key, entry, prior } => {
            let emptied = match state
                .get_mut(&key)
                .and_then(|v| v.write_lock::<rhai::Map>())
            {
                Some(mut map) => {
                    match prior {
                        Some(count) => {
                            map.insert(entry.into(), Dynamic::from(count));
                        }
                        None => {
                            map.remove(entry.as_str());
                        }
                    }
                    map.is_empty()
                }
                None => false,
            };
            // A metric whose only entry was the one just undone never really
            // existed; leaving an empty table behind would print as a metric
            // with no rows.
            if emptied {
                state.remove(&key);
            }
        }
        Undo::ArrayPush { key } => {
            let emptied = match state
                .get_mut(&key)
                .and_then(|v| v.write_lock::<rhai::Array>())
            {
                Some(mut arr) => {
                    arr.pop();
                    arr.is_empty()
                }
                None => false,
            };
            if emptied {
                state.remove(&key);
            }
        }
        Undo::ArrayEntry { key, index, prior } => {
            let emptied = match state
                .get_mut(&key)
                .and_then(|v| v.write_lock::<rhai::Array>())
            {
                Some(mut arr) => {
                    match prior {
                        Some(value) if index < arr.len() => arr[index] = value,
                        Some(_) => {}
                        None => arr.truncate(index),
                    }
                    arr.is_empty()
                }
                None => false,
            };
            if emptied {
                state.remove(&key);
            }
        }
    }
}

pub fn get_thread_snapshot() -> TrackingSnapshot {
    THREAD_TRACKING_STATE.with(|state| state.borrow().clone())
}

pub fn with_user_tracking<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, Dynamic>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        f(&mut snapshot.user)
    })
}

pub fn with_internal_tracking<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, Dynamic>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        f(&mut snapshot.internal)
    })
}

/// Hand the caller's tracking state to the thread-local for the duration of one
/// script stage, leaving the caller's *user* map empty.
///
/// The user half — the `track_*` metrics themselves — is **moved**, and that is
/// the point. The pipeline hands the state over before every stage of every
/// event, so copying it would cost one deep clone of every metric's accumulated
/// value per event: for a frequency table, the whole value→count map, which
/// made a high-cardinality `--freq` cost O(events × distinct values). A move is
/// pointer-sized no matter how much the metrics hold.
///
/// The internal half — `__op_` metadata, error/gate counters, skip tallies — is
/// copied instead, because unlike user metrics it is also written *between*
/// stages: the pipeline's error handlers record into the thread-local after the
/// engine has returned (see `tracking::errors`). Keeping the context's copy
/// authoritative at install time preserves that arrangement. It is cheap: the
/// internal map holds a few small scalars per metric, not per distinct value.
///
/// Pair every call with [`take_thread_tracking_state`];
/// [`crate::engine::RhaiEngine`] does that with a guard, so the state comes back
/// on error paths too.
pub fn install_thread_tracking_state(
    user: &mut HashMap<String, Dynamic>,
    internal: &HashMap<String, Dynamic>,
) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        debug_assert!(
            snapshot.user.is_empty(),
            "tracking state is already installed: script stages must not nest"
        );
        snapshot.user = std::mem::take(user);
        snapshot.internal = internal.clone();
    });
    begin_stage_journal();
}

/// Move the thread-local user metrics back to the caller and copy the internal
/// half back, leaving the thread-local user map empty. Counterpart to
/// [`install_thread_tracking_state`].
///
/// `outcome` decides what happens to the metrics the stage recorded: a stage
/// that completed keeps them, a stage that errored has them undone, so a failed
/// stage leaves the metrics where they were before it ran. The internal half —
/// error and gate counters, skip tallies — is never rolled back: it exists to
/// record that the failure happened.
pub fn take_thread_tracking_state(
    user: &mut HashMap<String, Dynamic>,
    internal: &mut HashMap<String, Dynamic>,
    outcome: StageOutcome,
) {
    match outcome {
        StageOutcome::Completed => commit_stage_journal(),
        StageOutcome::Failed => rollback_stage_journal(),
    }

    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        *user = std::mem::take(&mut snapshot.user);
        internal.clone_from(&snapshot.internal);
    });
}

/// Whether a script stage ran to completion, which decides whether the metrics
/// it recorded are kept or undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    Completed,
    Failed,
}

/// Drop whatever the thread-local holds. Used by the parallel workers once a
/// batch's metric deltas have been sent on.
pub fn clear_thread_tracking_state() {
    THREAD_TRACKING_STATE.with(|state| {
        state.borrow_mut().user.clear();
    });
}

pub fn set_thread_tracking_state(metrics: &HashMap<String, Dynamic>) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        snapshot.user = metrics.clone();
    });
}

pub fn get_thread_tracking_state() -> HashMap<String, Dynamic> {
    THREAD_TRACKING_STATE.with(|state| state.borrow().user.clone())
}

pub fn set_thread_internal_state(metrics: &HashMap<String, Dynamic>) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        snapshot.internal = metrics.clone();
    });
}

pub fn get_thread_internal_state() -> HashMap<String, Dynamic> {
    THREAD_TRACKING_STATE.with(|state| state.borrow().internal.clone())
}
