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

#[cfg(test)]
pub fn get_thread_snapshot() -> TrackingSnapshot {
    THREAD_TRACKING_STATE.with(|state| state.borrow().clone())
}

/// One recorded change to the user metrics, enough to reverse it.
///
/// Script stages are transactional: a stage that fails on an event rolls back
/// that event's `track_*` calls (`--help-rhai`, "Rolled back on error"). The
/// state used to be copied before each event to make that possible, which cost
/// O(all tracked values) per event — a 50k-value `--freq` copied 50k entries
/// twice for every line. Instead each change logs what it overwrote, and only
/// a failed stage pays to replay the log.
enum Undo {
    /// The metric's previous value (`None`: it did not exist).
    Key { key: String, old: Option<Dynamic> },
    /// One entry of a map-valued metric (`track_freq`).
    MapEntry {
        key: String,
        item: String,
        old: Option<Dynamic>,
    },
    /// Length of an array-valued metric before a push.
    ArrayLen { key: String, len: usize },
    /// One replaced element of an array-valued metric.
    ArrayElem {
        key: String,
        index: usize,
        old: Dynamic,
    },
}

thread_local! {
    /// The undo log of the script stage running on this thread; `None` outside
    /// a stage, where changes are not logged.
    static UNDO_LOG: RefCell<Option<Vec<Undo>>> = const { RefCell::new(None) };
}

/// Start logging changes to the user metrics for one script stage on one event.
pub fn begin_tracking_transaction() {
    UNDO_LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
}

/// Keep the stage's changes and stop logging.
pub fn commit_tracking_transaction() {
    UNDO_LOG.with(|log| *log.borrow_mut() = None);
}

/// Reverse the stage's changes, newest first, and stop logging.
pub fn rollback_tracking_transaction() {
    let Some(entries) = UNDO_LOG.with(|log| log.borrow_mut().take()) else {
        return;
    };
    THREAD_TRACKING_STATE.with(|state| {
        let user = &mut state.borrow_mut().user;
        for undo in entries.into_iter().rev() {
            match undo {
                Undo::Key {
                    key,
                    old: Some(old),
                } => {
                    user.insert(key, old);
                }
                Undo::Key { key, old: None } => {
                    user.remove(&key);
                }
                Undo::MapEntry { key, item, old } => {
                    if let Some(mut map) =
                        user.get_mut(&key).and_then(|v| v.write_lock::<rhai::Map>())
                    {
                        match old {
                            Some(old) => {
                                map.insert(item.into(), old);
                            }
                            None => {
                                map.remove(item.as_str());
                            }
                        }
                    }
                }
                Undo::ArrayLen { key, len } => {
                    if let Some(mut arr) = user
                        .get_mut(&key)
                        .and_then(|v| v.write_lock::<rhai::Array>())
                    {
                        arr.truncate(len);
                    }
                }
                Undo::ArrayElem { key, index, old } => {
                    if let Some(mut arr) = user
                        .get_mut(&key)
                        .and_then(|v| v.write_lock::<rhai::Array>())
                    {
                        if let Some(slot) = arr.get_mut(index) {
                            *slot = old;
                        }
                    }
                }
            }
        }
    });
}

/// The user metrics as the `track_*` functions see them: every change goes
/// through a method that records how to undo it while a stage is running.
pub struct UserMetrics<'a> {
    map: &'a mut HashMap<String, Dynamic>,
    log: Option<&'a mut Vec<Undo>>,
}

impl UserMetrics<'_> {
    pub fn get(&self, key: &str) -> Option<&Dynamic> {
        self.map.get(key)
    }

    /// Set a metric, logging the value it replaces (moved, not copied).
    pub fn insert(&mut self, key: String, value: Dynamic) {
        let old = self.map.insert(key.clone(), value);
        if let Some(log) = self.log.as_deref_mut() {
            log.push(Undo::Key { key, old });
        }
    }

    /// Mutable access to a whole metric, logging a copy of it first. Use only
    /// for small values; collections have their own methods below.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Dynamic> {
        if let Some(log) = self.log.as_deref_mut() {
            if let Some(current) = self.map.get(key) {
                log.push(Undo::Key {
                    key: key.to_string(),
                    old: Some(current.clone()),
                });
            }
        }
        self.map.get_mut(key)
    }

    /// Add one to `item` in the map-valued metric `key`, creating either.
    pub fn increment_map_entry(&mut self, key: &str, item: &str) {
        self.add_to_map_entry(key, item, Dynamic::from(1_i64));
    }

    /// Add `amount` (an int or float) to the entry `item` of the map-valued
    /// metric `key`, in place: an integer total stays an integer until a float
    /// is added.
    pub fn add_to_map_entry(&mut self, key: &str, item: &str, amount: Dynamic) {
        if !self.map.contains_key(key) {
            self.insert(key.to_string(), Dynamic::from(rhai::Map::new()));
        }
        let Some(mut map) = self
            .map
            .get_mut(key)
            .and_then(|v| v.write_lock::<rhai::Map>())
        else {
            return;
        };
        let old = map.get(item).cloned();
        map.insert(
            item.into(),
            super::merge::merge_numeric(old.clone(), amount),
        );
        if let Some(log) = self.log.as_deref_mut() {
            log.push(Undo::MapEntry {
                key: key.to_string(),
                item: item.to_string(),
                old,
            });
        }
    }

    /// Run `f` on the array-valued metric `key` (created empty if absent),
    /// giving it an [`ArrayEdit`] that logs pushes and replacements. Returns
    /// `None` if the metric holds something other than an array.
    pub fn edit_array<R>(
        &mut self,
        key: &str,
        f: impl FnOnce(&mut ArrayEdit<'_>) -> R,
    ) -> Option<R> {
        if !self.map.contains_key(key) {
            self.insert(key.to_string(), Dynamic::from(rhai::Array::new()));
        }
        let mut arr = self.map.get_mut(key)?.write_lock::<rhai::Array>()?;
        let mut edit = ArrayEdit {
            key,
            arr: &mut arr,
            log: self.log.as_deref_mut(),
        };
        Some(f(&mut edit))
    }
}

/// An array-valued metric being changed inside [`UserMetrics::edit_array`].
pub struct ArrayEdit<'a> {
    key: &'a str,
    arr: &'a mut rhai::Array,
    log: Option<&'a mut Vec<Undo>>,
}

impl ArrayEdit<'_> {
    pub fn items(&self) -> &rhai::Array {
        self.arr
    }

    pub fn push(&mut self, value: Dynamic) {
        if let Some(log) = self.log.as_deref_mut() {
            log.push(Undo::ArrayLen {
                key: self.key.to_string(),
                len: self.arr.len(),
            });
        }
        self.arr.push(value);
    }

    pub fn set(&mut self, index: usize, value: Dynamic) {
        let old = std::mem::replace(&mut self.arr[index], value);
        if let Some(log) = self.log.as_deref_mut() {
            log.push(Undo::ArrayElem {
                key: self.key.to_string(),
                index,
                old,
            });
        }
    }
}

pub fn with_user_tracking<F, R>(f: F) -> R
where
    F: FnOnce(&mut UserMetrics<'_>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        UNDO_LOG.with(|log| {
            let mut log = log.borrow_mut();
            let mut metrics = UserMetrics {
                map: &mut snapshot.user,
                log: log.as_mut(),
            };
            f(&mut metrics)
        })
    })
}

/// Read-only access to the user metrics, without copying them.
pub fn with_user_tracking_ref<F, R>(f: F) -> R
where
    F: FnOnce(&HashMap<String, Dynamic>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| f(&state.borrow().user))
}

/// Insert or replace user metrics outside any script stage (merging parallel
/// results before `--end`). Not logged.
pub fn insert_user_tracking(entries: impl IntoIterator<Item = (String, Dynamic)>) {
    THREAD_TRACKING_STATE.with(|state| state.borrow_mut().user.extend(entries));
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
