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

thread_local! {
    /// Per-window extremes for `track_min`/`track_max` (and the min/max parts
    /// of `track_stats`) while a span consumer is active (#380).
    ///
    /// A global extreme cannot be un-merged back to a window's extreme, so the
    /// span processor opens a fresh map when a span opens and takes it when the
    /// span closes. `None` whenever no span is being reported on, which keeps
    /// the non-span path to one thread-local check per call.
    static WINDOW_EXTREMES: RefCell<Option<HashMap<String, Dynamic>>> = const { RefCell::new(None) };
}

/// Start recording per-window extremes for a newly opened span.
pub fn begin_window_extremes() {
    WINDOW_EXTREMES.with(|w| *w.borrow_mut() = Some(HashMap::new()));
}

/// Stop recording and return the closing span's extremes (empty if none were
/// tracked or recording was never started).
pub fn take_window_extremes() -> HashMap<String, Dynamic> {
    WINDOW_EXTREMES.with(|w| w.borrow_mut().take().unwrap_or_default())
}

/// Fold one value into the open window's extreme for `key`, if a window is
/// being recorded. `stored` is what is kept (the caller's int or float);
/// `value` is its comparable form.
pub(super) fn record_window_extreme(key: &str, stored: Dynamic, value: f64, is_min: bool) {
    WINDOW_EXTREMES.with(|w| {
        let mut guard = w.borrow_mut();
        let Some(map) = guard.as_mut() else {
            return;
        };
        // The global update never stores NaN (every comparison with it is
        // false); match that rather than seeding a window with it.
        if value.is_nan() {
            return;
        }
        let better = match map.get(key) {
            None => true,
            Some(current) => {
                let current = if current.is_int() {
                    current.as_int().map(|i| i as f64).unwrap_or(value)
                } else {
                    current.as_float().unwrap_or(value)
                };
                if is_min {
                    value < current
                } else {
                    value > current
                }
            }
        };
        if better {
            map.insert(key.to_string(), stored);
        }
    });
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
