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
