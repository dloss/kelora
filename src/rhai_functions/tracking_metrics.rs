use super::merge::{
    deserialize_hll, deserialize_tdigest, merge_numeric, new_hll, new_hll_with_error,
    record_operation_metadata, serialize_hll, serialize_tdigest,
};
use super::with_user_tracking;
use rhai::Dynamic;
use std::collections::HashSet;
use tdigests::TDigest;

pub(super) fn track_cardinality_impl<V: std::hash::Hash>(key: &str, value: &V) {
    with_user_tracking(|state| {
        let mut hll = if let Some(existing) = state.get(key) {
            if let Ok(bytes) = existing.clone().into_blob() {
                deserialize_hll(&bytes).unwrap_or_else(new_hll)
            } else {
                new_hll()
            }
        } else {
            new_hll()
        };

        hll.insert(value);

        let bytes = serialize_hll(&hll);
        state.insert(key.to_string(), Dynamic::from_blob(bytes));
    });

    record_operation_metadata(key, "cardinality");
}

pub(super) fn track_cardinality_with_error_impl<V: std::hash::Hash>(
    key: &str,
    value: &V,
    error_rate: f64,
) {
    let error_rate = error_rate.clamp(0.001, 0.26);

    with_user_tracking(|state| {
        let mut hll = if let Some(existing) = state.get(key) {
            if let Ok(bytes) = existing.clone().into_blob() {
                deserialize_hll(&bytes).unwrap_or_else(|| new_hll_with_error(error_rate))
            } else {
                new_hll_with_error(error_rate)
            }
        } else {
            new_hll_with_error(error_rate)
        };

        hll.insert(value);

        let bytes = serialize_hll(&hll);
        state.insert(key.to_string(), Dynamic::from_blob(bytes));
    });

    record_operation_metadata(key, "cardinality");
}

pub(super) fn track_percentiles_impl(
    key: &str,
    value: f64,
    percentiles: rhai::Array,
) -> Result<(), Box<rhai::EvalAltResult>> {
    if percentiles.is_empty() {
        return Err("track_percentiles requires a non-empty array of percentiles".into());
    }

    let mut valid_percentiles = Vec::new();
    let mut seen = HashSet::new();

    for p in percentiles {
        let percentile = if p.is_int() {
            p.as_int().map_err(|_| -> Box<rhai::EvalAltResult> {
                "track_percentiles percentile must be a number".into()
            })? as f64
        } else if p.is_float() {
            p.as_float().map_err(|_| -> Box<rhai::EvalAltResult> {
                "track_percentiles percentile must be a number".into()
            })?
        } else {
            return Err("track_percentiles percentile must be a number".into());
        };

        if !(0.0..=1.0).contains(&percentile) {
            return Err(format!(
                "track_percentiles percentile must be in range [0.0, 1.0], got {}",
                percentile
            )
            .into());
        }

        if !seen.contains(&percentile.to_bits()) {
            seen.insert(percentile.to_bits());
            valid_percentiles.push(percentile);
        }
    }

    if !value.is_finite() {
        return Ok(());
    }

    for percentile in valid_percentiles {
        let percentage = percentile * 100.0;
        let percentile_str = if percentage.fract() == 0.0 {
            format!("p{}", percentage as i64)
        } else {
            let formatted = format!("{:.10}", percentage);
            let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
            format!("p{}", trimmed)
        };

        let metric_key = format!("{}_{}", key, percentile_str);

        with_user_tracking(|state| {
            let new_digest = TDigest::from_values(vec![value]);
            let digest = if let Some(existing) = state.get(&metric_key) {
                if let Ok(bytes) = existing.clone().into_blob() {
                    if let Some(existing_digest) = deserialize_tdigest(&bytes) {
                        existing_digest.merge(&new_digest)
                    } else {
                        new_digest
                    }
                } else {
                    new_digest
                }
            } else {
                new_digest
            };

            let bytes = serialize_tdigest(&digest);
            state.insert(metric_key.clone(), Dynamic::from_blob(bytes));
        });

        record_operation_metadata(&metric_key, "percentiles");
    }

    Ok(())
}

pub(super) fn track_stats_impl(
    key: &str,
    value: f64,
    percentiles: rhai::Array,
) -> Result<(), Box<rhai::EvalAltResult>> {
    if !value.is_finite() {
        return Ok(());
    }

    let min_key = format!("{}_min", key);
    with_user_tracking(|state| {
        let current = state
            .get(&min_key)
            .cloned()
            .unwrap_or(Dynamic::from(f64::INFINITY));
        let current_val = if current.is_int() {
            current.as_int().unwrap_or(i64::MAX) as f64
        } else {
            current.as_float().unwrap_or(f64::INFINITY)
        };
        if value < current_val {
            state.insert(min_key.clone(), Dynamic::from(value));
        }
    });
    record_operation_metadata(&min_key, "min");

    let max_key = format!("{}_max", key);
    with_user_tracking(|state| {
        let current = state
            .get(&max_key)
            .cloned()
            .unwrap_or(Dynamic::from(f64::NEG_INFINITY));
        let current_val = if current.is_int() {
            current.as_int().unwrap_or(i64::MIN) as f64
        } else {
            current.as_float().unwrap_or(f64::NEG_INFINITY)
        };
        if value > current_val {
            state.insert(max_key.clone(), Dynamic::from(value));
        }
    });
    record_operation_metadata(&max_key, "max");

    let avg_key = format!("{}_avg", key);
    with_user_tracking(|state| {
        let current = state.get(&avg_key).cloned();
        let (new_sum, new_count) = if let Some(existing) = current {
            if let Some(map) = existing.try_cast::<rhai::Map>() {
                let existing_sum = map
                    .get("sum")
                    .and_then(|v| {
                        if v.is_float() {
                            v.as_float().ok()
                        } else if v.is_int() {
                            v.as_int().ok().map(|i| i as f64)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                let existing_count = map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                (existing_sum + value, existing_count + 1)
            } else {
                (value, 1)
            }
        } else {
            (value, 1)
        };

        let mut map = rhai::Map::new();
        map.insert("sum".into(), Dynamic::from(new_sum));
        map.insert("count".into(), Dynamic::from(new_count));
        state.insert(avg_key.clone(), Dynamic::from(map));
    });
    record_operation_metadata(&avg_key, "avg");

    let count_key = format!("{}_count", key);
    with_user_tracking(|state| {
        let updated = merge_numeric(state.get(&count_key).cloned(), Dynamic::from(1_i64));
        state.insert(count_key.clone(), updated);
    });
    record_operation_metadata(&count_key, "count");

    let sum_key = format!("{}_sum", key);
    with_user_tracking(|state| {
        let updated = merge_numeric(state.get(&sum_key).cloned(), Dynamic::from(value));
        state.insert(sum_key.clone(), updated);
    });
    record_operation_metadata(&sum_key, "sum");

    track_percentiles_impl(key, value, percentiles)?;

    Ok(())
}
