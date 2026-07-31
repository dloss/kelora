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
}

/// Move the thread-local user metrics back to the caller and copy the internal
/// half back, leaving the thread-local user map empty. Counterpart to
/// [`install_thread_tracking_state`].
pub fn take_thread_tracking_state(
    user: &mut HashMap<String, Dynamic>,
    internal: &mut HashMap<String, Dynamic>,
) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        *user = std::mem::take(&mut snapshot.user);
        internal.clone_from(&snapshot.internal);
    });
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
