use super::merge::ensure_operation_metadata;
use super::with_user_tracking;
use rhai::Dynamic;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Past this many distinct values, warn once that the set lives entirely in
/// memory. track_unique stays unbounded by design (the operator knows their
/// data and their RAM); the warning exists so an eventual OOM kill is
/// attributable to the right metric.
pub(crate) const TRACK_UNIQUE_WARN_THRESHOLD: usize = 100_000;

/// Whether tracking runtime warnings may be printed (mirrors the
/// `--silent` / `--no-diagnostics` gate; set once at startup from config).
static RUNTIME_WARNINGS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Keys already warned about, shared across threads so the sequential path,
/// worker threads, and the parallel merge thread warn at most once per metric.
static UNIQUE_SIZE_WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub fn set_tracking_warnings_enabled(enabled: bool) {
    RUNTIME_WARNINGS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// If a track_unique set just grew past the threshold (and we haven't warned
/// for this metric yet), return the formatted warning. Emission is left to the
/// caller: worker threads route it through the captured-stderr channel, while
/// the parallel merge thread (whose captures are never drained) prints
/// directly.
pub(crate) fn unique_size_warning(key: &str, len: usize) -> Option<String> {
    if len < TRACK_UNIQUE_WARN_THRESHOLD || !RUNTIME_WARNINGS_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let first_time = {
        let mut warned = UNIQUE_SIZE_WARNED.lock().unwrap_or_else(|e| e.into_inner());
        warned
            .get_or_insert_with(HashSet::new)
            .insert(key.to_string())
    };
    if !first_time {
        return None;
    }
    Some(crate::config::format_warning_message_auto(&format!(
        "track_unique(\"{}\") now holds {}+ values, all kept in memory; for a unique count use track_cardinality() instead",
        key, TRACK_UNIQUE_WARN_THRESHOLD
    )))
}

/// Warn (once per metric) that a track_unique set has grown past the
/// threshold, from the per-event insert path.
fn warn_unique_size(key: &str, len: usize) {
    if let Some(message) = unique_size_warning(key, len) {
        if crate::rhai_functions::strings::is_parallel_mode() {
            crate::rhai_functions::strings::capture_stderr(message);
        } else {
            eprintln!("{}", message);
        }
    }
}

/// Append `value` to the unique-value array for `key` if absent, mutating the
/// stored array in place (per-event clones of the whole set would make
/// high-cardinality tracking O(n) per event in allocation alone). Returns the
/// array length for the size warning.
fn track_unique_insert(
    key: &str,
    value: Dynamic,
    already_present: impl Fn(&Dynamic) -> bool,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "unique")?;
    let len = with_user_tracking(|state| {
        if !state.contains_key(key) {
            state.insert(key.to_string(), Dynamic::from(rhai::Array::new()));
        }
        let mut arr = match state
            .get_mut(key)
            .and_then(|v| v.write_lock::<rhai::Array>())
        {
            Some(arr) => arr,
            None => return 0,
        };
        if !arr.iter().any(&already_present) {
            arr.push(value);
        }
        arr.len()
    });
    warn_unique_size(key, len);
    Ok(())
}

pub(super) fn track_unique_string_impl(
    key: &str,
    value: &str,
) -> Result<(), Box<rhai::EvalAltResult>> {
    track_unique_insert(key, Dynamic::from(value.to_string()), |v| {
        v.clone().into_string().unwrap_or_default() == value
    })
}

pub(super) fn track_unique_i64_impl(key: &str, value: i64) -> Result<(), Box<rhai::EvalAltResult>> {
    track_unique_insert(key, Dynamic::from(value), |v| {
        v.as_int().unwrap_or(i64::MIN) == value
    })
}

pub(super) fn track_unique_f64_impl(key: &str, value: f64) -> Result<(), Box<rhai::EvalAltResult>> {
    track_unique_insert(key, Dynamic::from(value), |v| {
        v.as_float().unwrap_or(f64::NAN) == value
    })
}

/// Count one occurrence of `category` under the metric `key`.
/// Storage shape: `{key → {category → count}}`. The stored map is mutated in
/// place: this is the flagship counting function, and cloning the whole
/// category map per event would be O(distinct categories) per event.
/// (The internal op id is still "bucket"; the public function was renamed
/// from track_bucket to track_count in kelora 2.0.)
pub(super) fn track_count_impl(key: &str, category: &str) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "bucket")?;
    with_user_tracking(|state| {
        if !state.contains_key(key) {
            state.insert(key.to_string(), Dynamic::from(rhai::Map::new()));
        }
        if let Some(mut map) = state.get_mut(key).and_then(|v| v.write_lock::<rhai::Map>()) {
            let count = map.get(category).and_then(|v| v.as_int().ok()).unwrap_or(0);
            map.insert(category.into(), Dynamic::from(count + 1));
        }
    });
    Ok(())
}

/// Build a `{key, count}` entry map for a ranked-count metric.
fn make_count_entry(item_key: &str, count: i64) -> Dynamic {
    let mut map = rhai::Map::new();
    map.insert("key".into(), Dynamic::from(item_key.to_string()));
    map.insert("count".into(), Dynamic::from(count));
    Dynamic::from(map)
}

/// Build a `{key, value}` entry map for a ranked-by-score metric.
fn make_value_entry(item_key: &str, value: f64) -> Dynamic {
    let mut map = rhai::Map::new();
    map.insert("key".into(), Dynamic::from(item_key.to_string()));
    map.insert("value".into(), Dynamic::from(value));
    Dynamic::from(map)
}

/// Find the entry for `item_key` in a ranked array (`[{key, count|value}]`).
/// Linear scan, like track_unique: ranked metrics now keep one entry per
/// distinct item and rank/truncate at format time, so the array is no longer
/// bounded to N here.
fn find_item_index(arr: &rhai::Array, item_key: &str) -> Option<usize> {
    arr.iter().position(|elem| {
        elem.read_lock::<rhai::Map>()
            .and_then(|m| m.get("key").and_then(|k| k.clone().into_string().ok()))
            .as_deref()
            == Some(item_key)
    })
}

/// Record the requested N for a ranked metric in user tracking. It lives in
/// user state (not internal) so it survives parallel worker merges, which only
/// propagate the full user map plus `__op_` metadata; it is filtered from all
/// metric output by its `__kelora_` prefix.
fn record_rank_n(state: &mut std::collections::HashMap<String, Dynamic>, key: &str, n: i64) {
    state.insert(format!("{}{}", super::TOPN_PREFIX, key), Dynamic::from(n));
}

/// Shared per-event accumulation for track_top / track_bottom. Direction only
/// affects ordering (applied at format time), so both keep an identical full
/// `{key → count}` tally, like track_count. This is the fix for the old bug
/// where truncating to N after every event silently dropped heavy hitters that
/// first appeared once the N slots were already full.
fn rank_count_insert(key: &str, item_key: &str, n: i64) {
    with_user_tracking(|state| {
        record_rank_n(state, key, n);
        let entry = state
            .entry(key.to_string())
            .or_insert_with(|| Dynamic::from(rhai::Array::new()));
        let Some(mut arr) = entry.write_lock::<rhai::Array>() else {
            return;
        };
        match find_item_index(&arr, item_key) {
            Some(idx) => {
                let count = arr[idx]
                    .read_lock::<rhai::Map>()
                    .and_then(|m| m.get("count").and_then(|v| v.as_int().ok()))
                    .unwrap_or(0)
                    + 1;
                arr[idx] = make_count_entry(item_key, count);
            }
            None => arr.push(make_count_entry(item_key, 1)),
        }
    });
}

/// Shared per-event accumulation for track_top_by / track_bottom_by. Keeps the
/// extreme score (max for top, min for bottom) per distinct item; ranking and
/// truncation to N happen at format time.
fn rank_weighted_insert(key: &str, item_key: &str, n: i64, value: f64, is_top: bool) {
    with_user_tracking(|state| {
        record_rank_n(state, key, n);
        let entry = state
            .entry(key.to_string())
            .or_insert_with(|| Dynamic::from(rhai::Array::new()));
        let Some(mut arr) = entry.write_lock::<rhai::Array>() else {
            return;
        };
        match find_item_index(&arr, item_key) {
            Some(idx) => {
                let current = arr[idx]
                    .read_lock::<rhai::Map>()
                    .and_then(|m| m.get("value").and_then(|v| v.as_float().ok()))
                    .unwrap_or(if is_top {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    });
                let merged = if is_top {
                    value.max(current)
                } else {
                    value.min(current)
                };
                arr[idx] = make_value_entry(item_key, merged);
            }
            None => arr.push(make_value_entry(item_key, value)),
        }
    });
}

pub(super) fn track_top_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "top")?;
    rank_count_insert(key, item_key, n);
    Ok(())
}

pub(super) fn track_bottom_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "bottom")?;
    rank_count_insert(key, item_key, n);
    Ok(())
}

pub(super) fn track_top_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "top_by")?;
    rank_weighted_insert(key, item_key, n, value, true);
    Ok(())
}

pub(super) fn track_bottom_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "bottom_by")?;
    rank_weighted_insert(key, item_key, n, value, false);
    Ok(())
}
