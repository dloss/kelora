use super::merge::record_operation_metadata;
use super::with_user_tracking;
use rhai::Dynamic;

pub(super) fn track_unique_string_impl(key: &str, value: &str) {
    let updated = with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let value_dynamic = Dynamic::from(value.to_string());
            if !arr
                .iter()
                .any(|v| v.clone().into_string().unwrap_or_default() == value)
            {
                arr.push(value_dynamic);
            }
            state.insert(key.to_string(), Dynamic::from(arr));
            true
        } else {
            false
        }
    });
    if updated {
        record_operation_metadata(key, "unique");
    }
}

pub(super) fn track_unique_i64_impl(key: &str, value: i64) {
    let updated = with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let value_dynamic = Dynamic::from(value);
            if !arr.iter().any(|v| v.as_int().unwrap_or(i64::MIN) == value) {
                arr.push(value_dynamic);
            }
            state.insert(key.to_string(), Dynamic::from(arr));
            true
        } else {
            false
        }
    });
    if updated {
        record_operation_metadata(key, "unique");
    }
}

pub(super) fn track_unique_f64_impl(key: &str, value: f64) {
    let updated = with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            let value_dynamic = Dynamic::from(value);
            if !arr
                .iter()
                .any(|v| v.as_float().unwrap_or(f64::NAN) == value)
            {
                arr.push(value_dynamic);
            }
            state.insert(key.to_string(), Dynamic::from(arr));
            true
        } else {
            false
        }
    });
    if updated {
        record_operation_metadata(key, "unique");
    }
}

pub(super) fn track_bucket_impl(key: &str, bucket: &str) {
    let updated = with_user_tracking(|state| {
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

        if let Some(mut map) = current.try_cast::<rhai::Map>() {
            let count = map.get(bucket).cloned().unwrap_or(Dynamic::from(0i64));
            let new_count = count.as_int().unwrap_or(0) + 1;
            map.insert(bucket.into(), Dynamic::from(new_count));
            state.insert(key.to_string(), Dynamic::from(map));
            true
        } else {
            false
        }
    });
    if updated {
        record_operation_metadata(key, "bucket");
    }
}

pub(super) fn track_top_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
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
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "top");
    }
    Ok(())
}

pub(super) fn track_bottom_count_impl(
    key: &str,
    item_key: &str,
    n: i64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
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
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "bottom");
    }
    Ok(())
}

pub(super) fn track_top_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
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
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "top");
    }
    Ok(())
}

pub(super) fn track_bottom_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
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
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "bottom");
    }
    Ok(())
}
