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

pub(super) fn track_top_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "top")?;
    with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            if let Some(idx) = found_idx {
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let count = map.get("count").cloned().unwrap_or(Dynamic::from(0i64));
                    let new_count = count.as_int().unwrap_or(0) + 1;
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("count".into(), Dynamic::from(new_count));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("count".into(), Dynamic::from(1i64));
                arr.push(Dynamic::from(new_map));
            }

            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_count = a_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    let b_count = b_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    match b_count.cmp(&a_count) {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
        }
    });

    Ok(())
}

pub(super) fn track_bottom_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "bottom")?;
    with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            if let Some(idx) = found_idx {
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let count = map.get("count").cloned().unwrap_or(Dynamic::from(0i64));
                    let new_count = count.as_int().unwrap_or(0) + 1;
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("count".into(), Dynamic::from(new_count));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("count".into(), Dynamic::from(1i64));
                arr.push(Dynamic::from(new_map));
            }

            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_count = a_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    let b_count = b_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    match a_count.cmp(&b_count) {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
        }
    });

    Ok(())
}

pub(super) fn track_top_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "top_by")?;
    with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            if let Some(idx) = found_idx {
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let current_val = map
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let new_val = value.max(current_val);
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("value".into(), Dynamic::from(new_val));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("value".into(), Dynamic::from(value));
                arr.push(Dynamic::from(new_map));
            }

            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_val = a_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let b_val = b_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    match b_val
                        .partial_cmp(&a_val)
                        .unwrap_or(std::cmp::Ordering::Equal)
                    {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
        }
    });

    Ok(())
}

pub(super) fn track_bottom_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    ensure_operation_metadata(key, "bottom_by")?;
    with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            if let Some(idx) = found_idx {
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let current_val = map
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let new_val = value.min(current_val);
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("value".into(), Dynamic::from(new_val));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("value".into(), Dynamic::from(value));
                arr.push(Dynamic::from(new_map));
            }

            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_val = a_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let b_val = b_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    match a_val
                        .partial_cmp(&b_val)
                        .unwrap_or(std::cmp::Ordering::Equal)
                    {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
        }
    });

    Ok(())
}
