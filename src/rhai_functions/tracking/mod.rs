use rhai::{Dynamic, Engine};
use std::collections::HashMap;

mod errors;
mod format;
mod merge;
mod metrics;
mod rank;
mod state;
#[cfg(test)]
use errors::format_error_location;
pub use errors::{
    extract_error_summary_from_tracking, format_fatal_error_line, has_errors_in_tracking,
    track_error,
};
pub use format::{format_metrics_json, format_metrics_output};
use merge::{
    deserialize_hll, deserialize_tdigest, is_hll_blob, merge_numeric, record_operation_metadata,
};
use metrics::{
    track_avg_impl, track_cardinality_impl, track_cardinality_with_error_impl, track_max_impl,
    track_min_impl, track_percentiles_impl, track_stats_impl,
};
use rank::{
    track_bottom_count_impl, track_bottom_weighted_impl, track_bucket_impl, track_top_count_impl,
    track_top_weighted_impl, track_unique_f64_impl, track_unique_i64_impl,
    track_unique_string_impl,
};
pub use state::{
    get_thread_internal_state, get_thread_snapshot, get_thread_tracking_state,
    set_thread_internal_state, set_thread_tracking_state, with_internal_tracking,
    with_user_tracking, TrackingSnapshot,
};

pub fn register_functions(engine: &mut Engine) {
    // Track functions using thread-local storage - clean user API
    // Store operation metadata for proper parallel merging
    engine.register_fn(
        "track_count",
        |key: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            let type_name = key.type_name().to_string();
            let key = key.into_string().map_err(|_| -> Box<rhai::EvalAltResult> {
                format!(
                    "track_count requires a string key; got {}. Hint: use to_string() for numbers (e.g. track_count(e.status.to_string()))",
                    type_name
                )
                .into()
            })?;

            with_user_tracking(|state| {
                let updated =
                    merge_numeric(state.get(key.as_str()).cloned(), Dynamic::from(1_i64));
                state.insert(key.to_string(), updated);
            });
            record_operation_metadata(&key, "count");
            Ok(())
        },
    );

    engine.register_fn("track_sum", |key: &str, value: i64| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: i32| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: f64| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: f32| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_sum", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_avg overloads for different number types
    // Stores both sum and count as a map for proper averaging in parallel mode
    engine.register_fn("track_avg", |key: &str, value: i64| {
        track_avg_impl(key, value as f64);
    });

    engine.register_fn("track_avg", |key: &str, value: i32| {
        track_avg_impl(key, value as f64);
    });

    engine.register_fn("track_avg", |key: &str, value: f64| {
        track_avg_impl(key, value);
    });

    engine.register_fn("track_avg", |key: &str, value: f32| {
        track_avg_impl(key, value as f64);
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_avg", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_min overloads for different number types
    engine.register_fn("track_min", |key: &str, value: i64| {
        track_min_impl(key, Dynamic::from(value), value as f64);
    });

    engine.register_fn("track_min", |key: &str, value: i32| {
        track_min_impl(key, Dynamic::from(value), value as f64);
    });

    engine.register_fn("track_min", |key: &str, value: f64| {
        track_min_impl(key, Dynamic::from(value), value);
    });

    engine.register_fn("track_min", |key: &str, value: f32| {
        track_min_impl(key, Dynamic::from(value), value as f64);
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_min", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_max overloads for different number types
    engine.register_fn("track_max", |key: &str, value: i64| {
        track_max_impl(key, Dynamic::from(value), value as f64);
    });

    engine.register_fn("track_max", |key: &str, value: i32| {
        track_max_impl(key, Dynamic::from(value), value as f64);
    });

    engine.register_fn("track_max", |key: &str, value: f64| {
        track_max_impl(key, Dynamic::from(value), value);
    });

    engine.register_fn("track_max", |key: &str, value: f32| {
        track_max_impl(key, Dynamic::from(value), value as f64);
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_max", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_percentiles - streaming percentile estimation using t-digest
    // Auto-suffixes metric names with percentile (e.g., "latency_p95", "latency_p99")
    // This is the ONLY track_* function that auto-suffixes because percentiles are always multi-valued

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: i64, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: i32, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: f64, percentiles: rhai::Array| {
            track_percentiles_impl(key, value, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: f32, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_percentiles",
        |_key: &str,
         _value: (),
         _percentiles: rhai::Array|
         -> Result<(), Box<rhai::EvalAltResult>> {
            // Silently ignore Unit values - no tracking occurs
            Ok(())
        },
    );

    // Default percentiles overloads (when no array provided, use [0.50, 0.95, 0.99])
    engine.register_fn("track_percentiles", |key: &str, value: i64| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_percentiles_impl(key, value as f64, default_percentiles)
    });

    engine.register_fn("track_percentiles", |key: &str, value: i32| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_percentiles_impl(key, value as f64, default_percentiles)
    });

    engine.register_fn("track_percentiles", |key: &str, value: f64| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_percentiles_impl(key, value, default_percentiles)
    });

    engine.register_fn("track_percentiles", |key: &str, value: f32| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_percentiles_impl(key, value as f64, default_percentiles)
    });

    // Unit overload for default percentiles
    engine.register_fn("track_percentiles", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_stats - comprehensive statistics tracking (min, max, avg, count, sum, percentiles)
    // Auto-suffixes metric names with _min, _max, _avg, _count, _sum, _pXX
    // This is a convenience function that combines track_min, track_max, track_avg, and track_percentiles

    engine.register_fn(
        "track_stats",
        |key: &str, value: i64, percentiles: rhai::Array| {
            track_stats_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_stats",
        |key: &str, value: i32, percentiles: rhai::Array| {
            track_stats_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_stats",
        |key: &str, value: f64, percentiles: rhai::Array| track_stats_impl(key, value, percentiles),
    );

    engine.register_fn(
        "track_stats",
        |key: &str, value: f32, percentiles: rhai::Array| {
            track_stats_impl(key, value as f64, percentiles)
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_stats",
        |_key: &str,
         _value: (),
         _percentiles: rhai::Array|
         -> Result<(), Box<rhai::EvalAltResult>> {
            // Silently ignore Unit values - no tracking occurs
            Ok(())
        },
    );

    // Default percentiles overloads (when no array provided, use [0.50, 0.95, 0.99])
    engine.register_fn("track_stats", |key: &str, value: i64| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_stats_impl(key, value as f64, default_percentiles)
    });

    engine.register_fn("track_stats", |key: &str, value: i32| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_stats_impl(key, value as f64, default_percentiles)
    });

    engine.register_fn("track_stats", |key: &str, value: f64| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_stats_impl(key, value, default_percentiles)
    });

    engine.register_fn("track_stats", |key: &str, value: f32| {
        let default_percentiles = vec![
            Dynamic::from(0.50_f64),
            Dynamic::from(0.95_f64),
            Dynamic::from(0.99_f64),
        ];
        track_stats_impl(key, value as f64, default_percentiles)
    });

    // Unit overload for default percentiles
    engine.register_fn("track_stats", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    engine.register_fn("track_unique", |key: &str, value: &str| {
        track_unique_string_impl(key, value);
    });

    engine.register_fn("track_unique", |key: &str, value: i64| {
        track_unique_i64_impl(key, value);
    });

    engine.register_fn("track_unique", |key: &str, value: i32| {
        track_unique_i64_impl(key, value as i64);
    });

    engine.register_fn("track_unique", |key: &str, value: f64| {
        track_unique_f64_impl(key, value);
    });

    engine.register_fn("track_unique", |key: &str, value: f32| {
        track_unique_f64_impl(key, value as f64);
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_unique", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_cardinality - probabilistic cardinality estimation using HyperLogLog
    // Uses ~12KB of memory regardless of cardinality, with ~1% standard error
    // For exact counts of small sets, use track_unique instead

    engine.register_fn("track_cardinality", |key: &str, value: &str| {
        let s = value.to_string();
        track_cardinality_impl(key, &s);
    });

    engine.register_fn("track_cardinality", |key: &str, value: i64| {
        track_cardinality_impl(key, &value);
    });

    engine.register_fn("track_cardinality", |key: &str, value: i32| {
        let v = value as i64;
        track_cardinality_impl(key, &v);
    });

    engine.register_fn("track_cardinality", |key: &str, value: f64| {
        // Convert to bits for consistent hashing of floats
        let bits = value.to_bits();
        track_cardinality_impl(key, &bits);
    });

    engine.register_fn("track_cardinality", |key: &str, value: f32| {
        let bits = (value as f64).to_bits();
        track_cardinality_impl(key, &bits);
    });

    // Overloads with custom error rate (third parameter)
    engine.register_fn(
        "track_cardinality",
        |key: &str, value: &str, error_rate: f64| {
            let s = value.to_string();
            track_cardinality_with_error_impl(key, &s, error_rate);
        },
    );

    engine.register_fn(
        "track_cardinality",
        |key: &str, value: i64, error_rate: f64| {
            track_cardinality_with_error_impl(key, &value, error_rate);
        },
    );

    engine.register_fn(
        "track_cardinality",
        |key: &str, value: i32, error_rate: f64| {
            let v = value as i64;
            track_cardinality_with_error_impl(key, &v, error_rate);
        },
    );

    engine.register_fn(
        "track_cardinality",
        |key: &str, value: f64, error_rate: f64| {
            let bits = value.to_bits();
            track_cardinality_with_error_impl(key, &bits, error_rate);
        },
    );

    engine.register_fn(
        "track_cardinality",
        |key: &str, value: f32, error_rate: f64| {
            let bits = (value as f64).to_bits();
            track_cardinality_with_error_impl(key, &bits, error_rate);
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_cardinality", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    engine.register_fn(
        "track_cardinality",
        |_key: &str, _value: (), _error_rate: f64| {
            // Silently ignore Unit values - no tracking occurs
        },
    );

    engine.register_fn("track_bucket", |key: &str, bucket: &str| {
        track_bucket_impl(key, bucket);
    });

    engine.register_fn("track_bucket", |key: &str, bucket: i64| {
        track_bucket_impl(key, &bucket.to_string());
    });

    engine.register_fn("track_bucket", |key: &str, bucket: i32| {
        track_bucket_impl(key, &bucket.to_string());
    });

    engine.register_fn("track_bucket", |key: &str, bucket: f64| {
        track_bucket_impl(key, &bucket.to_string());
    });

    engine.register_fn("track_bucket", |key: &str, bucket: f32| {
        track_bucket_impl(key, &bucket.to_string());
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_bucket", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_top - Count mode: Most frequent items
    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }
            track_top_count_impl(key, item_key, n)
        },
    );

    // Unit overload - no-op for missing/empty item keys
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64| -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );

    // track_top - Weighted mode: Highest values
    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    // Unit overload - no-op for missing/empty item keys (weighted mode)
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64, _value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64, _value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64, _value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64, _value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_top",
        |_key: &str,
         _item_key: &str,
         _n: i64,
         _value: ()|
         -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );
    engine.register_fn(
        "track_top",
        |_key: &str, _item_key: (), _n: i64, _value: ()| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );

    // track_bottom - Count mode: Least frequent items
    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }
            track_bottom_count_impl(key, item_key, n)
        },
    );

    // Unit overload - no-op for missing/empty item keys
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64| -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );

    // track_bottom - Weighted mode: Lowest values
    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    // Unit overload - no-op for missing/empty item keys (weighted mode)
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64, _value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64, _value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64, _value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64, _value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_bottom",
        |_key: &str,
         _item_key: &str,
         _n: i64,
         _value: ()|
         -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );
    engine.register_fn(
        "track_bottom",
        |_key: &str, _item_key: (), _n: i64, _value: ()| -> Result<(), Box<rhai::EvalAltResult>> {
            Ok(())
        },
    );
}

/// Merge thread-local tracking state into context tracker for sequential mode
pub fn merge_thread_tracking_to_context(ctx: &mut crate::pipeline::PipelineContext) {
    let snapshot = get_thread_snapshot();
    for (key, value) in snapshot.user {
        ctx.tracker.insert(key, value);
    }
    for (key, value) in snapshot.internal {
        ctx.internal_tracker.insert(key, value);
    }
}

/// Finalize a single tracking value for exposure to user-visible Rhai scopes
/// (`--end`, `--span-close`, etc.).
///
/// Internal tracking state stores average progress as a `{sum, count}` map
/// and stores t-digest / HLL sketches as opaque blobs. `--metrics` formatting
/// deserializes these on the fly, but when the `metrics` map is handed to a
/// user script it needs to contain plain numeric values so that scripts can
/// render them with helpers like `format_decimals` and `bar`. This helper
/// returns the finalized `Dynamic` for a metric key/value pair.
fn finalize_metric_value(key: &str, value: &Dynamic) -> Dynamic {
    // Average tracking: {sum, count} map → mean as f64
    if let Some(map) = value.clone().try_cast::<rhai::Map>() {
        if map.contains_key("sum") && map.contains_key("count") {
            let sum = map
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
            let count = map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
            let avg = if count > 0 { sum / count as f64 } else { 0.0 };
            return Dynamic::from(avg);
        }
    }

    // Sketch blobs: HyperLogLog (cardinality) or t-digest (percentile)
    if let Ok(blob) = value.clone().into_blob() {
        if is_hll_blob(&blob) {
            if let Some(hll) = deserialize_hll(&blob) {
                return Dynamic::from(hll.len() as i64);
            }
        }
        if let Some(digest) = deserialize_tdigest(&blob) {
            if let Some(p_pos) = key.rfind("_p") {
                if let Ok(percentile) = key[p_pos + 2..].parse::<f64>() {
                    let quantile = percentile / 100.0;
                    return Dynamic::from(digest.estimate_quantile(quantile));
                }
            }
        }
    }

    // Everything else (counts, min/max, sum, track_unique / top / bottom / bucket
    // arrays, user-set values) is already script-friendly.
    value.clone()
}

/// Build a finalized `rhai::Map` suitable for exposing as the `metrics`
/// global inside `--end` / `--span-close` / other post-processing stages.
/// Internal bookkeeping keys (`__op_*`, `__kelora_stats_*`, `__kelora_error_*`)
/// are filtered out, and sketch-backed metrics are finalized to plain numbers.
pub fn finalize_metrics_for_script(metrics: &HashMap<String, Dynamic>) -> rhai::Map {
    let mut out = rhai::Map::new();
    for (key, value) in metrics.iter() {
        if key.starts_with("__op_")
            || key.starts_with("__kelora_stats_")
            || key.starts_with("__kelora_error_")
        {
            continue;
        }
        out.insert(key.clone().into(), finalize_metric_value(key, value));
    }
    out
}

#[allow(dead_code)] // Retained for potential future CLI summary output
pub fn extract_error_summary(
    metrics: &std::collections::HashMap<String, Dynamic>,
) -> Option<String> {
    format::extract_error_summary(metrics)
}

#[cfg(test)]
mod tests {
    use super::state::THREAD_TRACKING_STATE;
    use super::*;
    use crate::stats::ProcessingStats;
    use rhai::Dynamic;
    use std::collections::HashMap;

    // Helper to clear thread-local state between tests
    fn clear_tracking_state() {
        THREAD_TRACKING_STATE.with(|state| {
            let mut snapshot = state.borrow_mut();
            snapshot.user.clear();
            snapshot.internal.clear();
        });
    }

    #[test]
    fn test_get_set_thread_tracking_state() {
        clear_tracking_state();

        let mut metrics = HashMap::new();
        metrics.insert("test_key".to_string(), Dynamic::from(123i64));

        set_thread_tracking_state(&metrics);

        let retrieved = get_thread_tracking_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved.get("test_key").unwrap().as_int().unwrap(), 123);

        clear_tracking_state();
    }

    #[test]
    fn test_get_set_thread_internal_state() {
        clear_tracking_state();

        let mut internal = HashMap::new();
        internal.insert("internal_key".to_string(), Dynamic::from(456i64));

        set_thread_internal_state(&internal);

        let retrieved = get_thread_internal_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(
            retrieved.get("internal_key").unwrap().as_int().unwrap(),
            456
        );

        clear_tracking_state();
    }

    #[test]
    fn test_with_user_tracking() {
        clear_tracking_state();

        with_user_tracking(|state| {
            state.insert("key1".to_string(), Dynamic::from(100i64));
            state.insert("key2".to_string(), Dynamic::from(200i64));
        });

        let retrieved = get_thread_tracking_state();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved.get("key1").unwrap().as_int().unwrap(), 100);
        assert_eq!(retrieved.get("key2").unwrap().as_int().unwrap(), 200);

        clear_tracking_state();
    }

    #[test]
    fn test_with_internal_tracking() {
        clear_tracking_state();

        with_internal_tracking(|state| {
            state.insert("__internal1".to_string(), Dynamic::from(999i64));
        });

        let retrieved = get_thread_internal_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved.get("__internal1").unwrap().as_int().unwrap(), 999);

        clear_tracking_state();
    }

    #[test]
    fn test_record_operation_metadata() {
        clear_tracking_state();

        record_operation_metadata("test_key", "count");

        let internal = get_thread_internal_state();
        assert!(internal.contains_key("__op_test_key"));
        assert_eq!(
            internal
                .get("__op_test_key")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "count"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_tracking_snapshot_from_parts() {
        let mut user = HashMap::new();
        user.insert("user_key".to_string(), Dynamic::from(1i64));

        let mut internal = HashMap::new();
        internal.insert("internal_key".to_string(), Dynamic::from(2i64));

        let snapshot = TrackingSnapshot::from_parts(user.clone(), internal.clone());

        assert_eq!(snapshot.user.len(), 1);
        assert_eq!(snapshot.internal.len(), 1);
        assert_eq!(snapshot.user.get("user_key").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            snapshot
                .internal
                .get("internal_key")
                .unwrap()
                .as_int()
                .unwrap(),
            2
        );
    }

    #[test]
    fn test_get_thread_snapshot() {
        clear_tracking_state();

        with_user_tracking(|state| {
            state.insert("user_data".to_string(), Dynamic::from(111i64));
        });

        with_internal_tracking(|state| {
            state.insert("internal_data".to_string(), Dynamic::from(222i64));
        });

        let snapshot = get_thread_snapshot();
        assert_eq!(snapshot.user.len(), 1);
        assert_eq!(snapshot.internal.len(), 1);
        assert_eq!(
            snapshot.user.get("user_data").unwrap().as_int().unwrap(),
            111
        );
        assert_eq!(
            snapshot
                .internal
                .get("internal_data")
                .unwrap()
                .as_int()
                .unwrap(),
            222
        );

        clear_tracking_state();
    }

    #[test]
    fn test_has_errors_in_tracking_no_errors() {
        let snapshot = TrackingSnapshot::default();
        assert!(!has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_has_errors_in_tracking_with_errors() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(5i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        assert!(has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_has_errors_in_tracking_zero_count() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(0i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        assert!(!has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_format_error_location_single_file() {
        let input_files = vec!["test.log".to_string()];
        let location = format_error_location(Some(42), Some("test.log"), &input_files);
        assert_eq!(location, "line 42");
    }

    #[test]
    fn test_format_error_location_stdin() {
        let input_files: Vec<String> = vec![];
        let location = format_error_location(Some(10), None, &input_files);
        assert_eq!(location, "line 10");
    }

    #[test]
    fn test_format_error_location_multiple_files_no_conflict() {
        let input_files = vec!["file1.log".to_string(), "file2.log".to_string()];
        let location = format_error_location(Some(100), Some("file1.log"), &input_files);
        assert_eq!(location, "file1.log:100");
    }

    #[test]
    fn test_format_error_location_multiple_files_with_conflict() {
        let input_files = vec![
            "/path/to/file.log".to_string(),
            "/other/path/file.log".to_string(),
        ];
        let location = format_error_location(Some(50), Some("/path/to/file.log"), &input_files);
        assert_eq!(location, "/path/to/file.log:50");
    }

    #[test]
    fn test_format_error_location_no_line_number() {
        let input_files = vec!["test.log".to_string()];
        let location = format_error_location(None, Some("test.log"), &input_files);
        assert_eq!(location, "unknown");
    }

    #[test]
    fn test_extract_error_summary_from_tracking_no_errors() {
        let snapshot = TrackingSnapshot::default();
        let summary = extract_error_summary_from_tracking(&snapshot, 0, None, None);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_error_summary_from_tracking_with_errors() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(3i64),
        );

        // Create sample error objects
        let mut sample_obj = rhai::Map::new();
        sample_obj.insert("error_type".into(), Dynamic::from("parse"));
        sample_obj.insert("line_num".into(), Dynamic::from(42i64));
        sample_obj.insert("message".into(), Dynamic::from("Test error"));
        sample_obj.insert("filename".into(), Dynamic::from("test.log"));

        let samples = vec![Dynamic::from(sample_obj)];
        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(samples),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = extract_error_summary_from_tracking(&snapshot, 0, None, None);

        assert!(summary.is_some());
        let text = summary.unwrap();
        assert!(text.contains("Parse errors: 3 total"));
        assert!(text.contains("Test error"));
    }

    #[test]
    fn test_extract_error_summary_adds_yearless_warning() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(2i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);

        let stats = ProcessingStats {
            yearless_timestamps: 5,
            ..Default::default()
        };

        let summary =
            extract_error_summary_from_tracking(&snapshot, 0, Some(&stats), None).unwrap();

        assert!(summary.contains("Year-less timestamp format detected"));
        assert!(summary.contains("5 parse"));
    }

    #[test]
    fn test_format_fatal_error_line_includes_filename_for_first_parse_error() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(18i64),
        );

        let mut sample_obj = rhai::Map::new();
        sample_obj.insert("error_type".into(), Dynamic::from("parse"));
        sample_obj.insert("line_num".into(), Dynamic::from(98i64));
        sample_obj.insert("message".into(), Dynamic::from("invalid JSON"));
        sample_obj.insert("filename".into(), Dynamic::from("filename.log"));

        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(vec![Dynamic::from(sample_obj)]),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = format_fatal_error_line(&snapshot);

        assert_eq!(summary, "18 parse errors (first at: filename.log:98)");
    }

    #[test]
    fn test_format_fatal_error_line_falls_back_to_line_without_filename() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(18i64),
        );

        let mut sample_obj = rhai::Map::new();
        sample_obj.insert("error_type".into(), Dynamic::from("parse"));
        sample_obj.insert("line_num".into(), Dynamic::from(98i64));
        sample_obj.insert("message".into(), Dynamic::from("invalid JSON"));

        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(vec![Dynamic::from(sample_obj)]),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = format_fatal_error_line(&snapshot);

        assert_eq!(summary, "18 parse errors (first at: line 98)");
    }

    #[test]
    fn test_format_fatal_error_line_for_few_errors_uses_file_aware_locations() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(2i64),
        );

        let mut first = rhai::Map::new();
        first.insert("error_type".into(), Dynamic::from("parse"));
        first.insert("line_num".into(), Dynamic::from(10i64));
        first.insert("filename".into(), Dynamic::from("a.log"));

        let mut second = rhai::Map::new();
        second.insert("error_type".into(), Dynamic::from("parse"));
        second.insert("line_num".into(), Dynamic::from(22i64));

        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(vec![Dynamic::from(first), Dynamic::from(second)]),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = format_fatal_error_line(&snapshot);

        assert_eq!(summary, "2 parse errors at a.log:10, line 22");
    }

    #[test]
    fn test_extract_error_summary_from_tracking_includes_filename_in_examples() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(1i64),
        );

        let mut sample_obj = rhai::Map::new();
        sample_obj.insert("error_type".into(), Dynamic::from("parse"));
        sample_obj.insert("line_num".into(), Dynamic::from(7i64));
        sample_obj.insert("message".into(), Dynamic::from("bad event"));
        sample_obj.insert("filename".into(), Dynamic::from("events.log"));

        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(vec![Dynamic::from(sample_obj)]),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = extract_error_summary_from_tracking(&snapshot, 0, None, None).unwrap();

        assert!(summary.contains("events.log:7: bad event"));
    }

    #[test]
    fn test_thread_tracking_isolation() {
        clear_tracking_state();

        // Set initial state
        with_user_tracking(|state| {
            state.insert("test".to_string(), Dynamic::from(1i64));
        });

        // Verify state is set
        let state1 = get_thread_tracking_state();
        assert_eq!(state1.get("test").unwrap().as_int().unwrap(), 1);

        // Clear and verify empty
        clear_tracking_state();
        let state2 = get_thread_tracking_state();
        assert!(state2.is_empty());
    }

    #[test]
    fn test_track_top_count_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some items
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "cherry", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 3);

        // Check first item (apple: 3)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "apple"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 3);

        // Check second item (banana: 1 or cherry: 1, sorted alphabetically on tie)
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(second.get("count").unwrap().as_int().unwrap(), 1);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_n_limit() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track 5 items but limit to top 2
        engine
            .eval::<()>(r#"track_top("test", "item1", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item2", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item2", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item3", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item4", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item5", 2)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // Should only have top 2
        assert_eq!(result.len(), 2);

        // First should be item2 (count=2)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "item2"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 2);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom values (latency)
        engine
            .eval::<()>(r#"track_top("slow", "/api/users", 2, 150)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("slow", "/api/products", 2, 50)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("slow", "/api/users", 2, 200)"#)
            .unwrap(); // Should update to max (200)
        engine
            .eval::<()>(r#"track_top("slow", "/api/orders", 2, 75)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("slow").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 2);

        // First should be /api/users with value=200 (max)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "/api/users"
        );
        assert_eq!(first.get("value").unwrap().as_float().unwrap(), 200.0);

        // Second should be /api/orders (75) or /api/products (50) - orders is higher
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "/api/orders"
        );
        assert_eq!(second.get("value").unwrap().as_float().unwrap(), 75.0);

        clear_tracking_state();
    }

    #[test]
    fn test_track_bottom_count_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track items with different frequencies
        // apple: 3 times, banana: 2 times, cherry: 1 time, date: 1 time
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "cherry", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "date", 3)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // Should have bottom 3 (by count ascending, then alphabetically)
        assert_eq!(result.len(), 3);

        // First and second should both have count=1 (cherry and date, alphabetically)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "cherry"
        );

        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(second.get("count").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "date"
        );

        // Third should be banana with count=2
        let third = result[2].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(third.get("count").unwrap().as_int().unwrap(), 2);
        assert_eq!(
            third.get("key").unwrap().clone().into_string().unwrap(),
            "banana"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_bottom_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom values (latency - bottom = fastest)
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/users", 2, 150.5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/products", 2, 30.0)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/users", 2, 100.0)"#)
            .unwrap(); // Should update to min (100)
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/orders", 2, 75.0)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("fast").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 2);

        // First should be /api/products with value=30.0 (min)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "/api/products"
        );
        assert_eq!(first.get("value").unwrap().as_float().unwrap(), 30.0);

        // Second should be /api/orders (75.0) or /api/users (100.0) - orders is smaller
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "/api/orders"
        );
        assert_eq!(second.get("value").unwrap().as_float().unwrap(), 75.0);

        clear_tracking_state();
    }

    #[test]
    fn test_track_bottom_unit_item() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit item keys should be silently ignored
        engine.eval::<()>(r#"track_bottom("test", (), 3)"#).unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", (), 3, 10)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        // Should not have created any entry or should be empty
        assert!(
            !state.contains_key("test")
                || state
                    .get("test")
                    .unwrap()
                    .clone()
                    .into_array()
                    .unwrap()
                    .is_empty()
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_invalid_n() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // N must be >= 1
        let result = engine.eval::<()>(r#"track_top("test", "item", 0)"#);
        assert!(result.is_err());

        let result = engine.eval::<()>(r#"track_top("test", "item", -1)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_track_top_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored
        engine
            .eval::<()>(r#"track_top("test", "apple", 3, ())"#)
            .unwrap();

        let state = get_thread_tracking_state();
        // Should not have created any entry or should be empty
        assert!(
            !state.contains_key("test")
                || state
                    .get("test")
                    .unwrap()
                    .clone()
                    .into_array()
                    .unwrap()
                    .is_empty()
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_unit_item() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit item keys should be silently ignored
        engine.eval::<()>(r#"track_top("test", (), 3)"#).unwrap();
        engine
            .eval::<()>(r#"track_top("test", (), 3, 10)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        // Should not have created any entry or should be empty
        assert!(
            !state.contains_key("test")
                || state
                    .get("test")
                    .unwrap()
                    .clone()
                    .into_array()
                    .unwrap()
                    .is_empty()
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_stable_sort() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Create ties - all have count=1
        engine
            .eval::<()>(r#"track_top("test", "zebra", 5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "mango", 5)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // All have same count, so should be sorted alphabetically
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        let third = result[2].clone().try_cast::<rhai::Map>().unwrap();

        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "apple"
        );
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "mango"
        );
        assert_eq!(
            third.get("key").unwrap().clone().into_string().unwrap(),
            "zebra"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_basic() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some values
        engine
            .eval::<()>(r#"track_percentiles("latency", 100, [0.50, 0.95, 0.99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 200, [0.50, 0.95, 0.99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 150, [0.50, 0.95, 0.99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 300, [0.50, 0.95, 0.99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 250, [0.50, 0.95, 0.99])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that all three percentile metrics were created
        assert!(state.contains_key("latency_p50"));
        assert!(state.contains_key("latency_p95"));
        assert!(state.contains_key("latency_p99"));

        // Check that they're blobs (serialized t-digest)
        assert!(state.get("latency_p50").unwrap().is_blob());
        assert!(state.get("latency_p95").unwrap().is_blob());
        assert!(state.get("latency_p99").unwrap().is_blob());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_single() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track single percentile
        engine
            .eval::<()>(r#"track_percentiles("response_time", 123.45, [0.95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        assert!(state.contains_key("response_time_p95"));
        assert!(state.get("response_time_p95").unwrap().is_blob());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_dedup() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with duplicate percentiles
        engine
            .eval::<()>(r#"track_percentiles("test", 100, [0.95, 0.95, 0.99, 0.95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Should only create two metrics (deduped)
        assert!(state.contains_key("test_p95"));
        assert!(state.contains_key("test_p99"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_invalid_percentile() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Out of range percentile
        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [1.01])"#);
        assert!(result.is_err());

        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [-0.5])"#);
        assert!(result.is_err());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_empty_array() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Empty percentiles array
        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [])"#);
        assert!(result.is_err());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored (with array)
        engine
            .eval::<()>(r#"track_percentiles("test", (), [0.95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // No metric should be created for unit value
        assert!(!state.contains_key("test_p95"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_default() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Use default percentiles [0.50, 0.95, 0.99]
        engine
            .eval::<()>(r#"track_percentiles("latency", 100)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 200)"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that default percentiles were created
        assert!(state.contains_key("latency_p50"));
        assert!(state.contains_key("latency_p95"));
        assert!(state.contains_key("latency_p99"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_decimal_suffix() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test decimal percentiles (0.999 → p99.9)
        engine
            .eval::<()>(r#"track_percentiles("latency", 100, [0.999, 0.9999])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check decimal suffixes
        assert!(state.contains_key("latency_p99.9"));
        assert!(state.contains_key("latency_p99.99"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_unit_value_default() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored (default)
        engine
            .eval::<()>(r#"track_percentiles("test", ())"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // No metrics should be created
        assert!(!state.contains_key("test_p50"));
        assert!(!state.contains_key("test_p95"));
        assert!(!state.contains_key("test_p99"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_stats_basic() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some stats with default percentiles
        engine
            .eval::<()>(r#"track_stats("response_time", 100)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_stats("response_time", 200)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_stats("response_time", 150)"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that all metrics were created with proper suffixes
        assert!(state.contains_key("response_time_min"));
        assert!(state.contains_key("response_time_max"));
        assert!(state.contains_key("response_time_avg"));
        assert!(state.contains_key("response_time_count"));
        assert!(state.contains_key("response_time_sum"));
        assert!(state.contains_key("response_time_p50"));
        assert!(state.contains_key("response_time_p95"));
        assert!(state.contains_key("response_time_p99"));

        // Verify min/max values
        assert_eq!(
            state
                .get("response_time_min")
                .unwrap()
                .as_float()
                .unwrap_or(0.0),
            100.0
        );
        assert_eq!(
            state
                .get("response_time_max")
                .unwrap()
                .as_float()
                .unwrap_or(0.0),
            200.0
        );

        // Verify count and sum
        assert_eq!(
            state
                .get("response_time_count")
                .unwrap()
                .as_int()
                .unwrap_or(0),
            3
        );
        assert_eq!(
            state
                .get("response_time_sum")
                .unwrap()
                .as_float()
                .unwrap_or(0.0),
            450.0
        );

        // Verify avg (stored as map with sum and count)
        let avg_map = state
            .get("response_time_avg")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(avg_map.get("sum").unwrap().as_float().unwrap_or(0.0), 450.0);
        assert_eq!(avg_map.get("count").unwrap().as_int().unwrap_or(0), 3);

        clear_tracking_state();
    }

    #[test]
    fn test_track_stats_custom_percentiles() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom percentiles
        engine
            .eval::<()>(r#"track_stats("latency", 100, [0.50, 0.90, 0.99, 0.999])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_stats("latency", 200, [0.50, 0.90, 0.99, 0.999])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that custom percentiles were created
        assert!(state.contains_key("latency_p50"));
        assert!(state.contains_key("latency_p90"));
        assert!(state.contains_key("latency_p99"));
        assert!(state.contains_key("latency_p99.9"));

        // Also check basic stats
        assert!(state.contains_key("latency_min"));
        assert!(state.contains_key("latency_max"));
        assert!(state.contains_key("latency_avg"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_stats_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored
        engine.eval::<()>(r#"track_stats("test", ())"#).unwrap();

        let state = get_thread_tracking_state();

        // No metrics should be created
        assert!(!state.contains_key("test_min"));
        assert!(!state.contains_key("test_max"));
        assert!(!state.contains_key("test_avg"));
        assert!(!state.contains_key("test_count"));
        assert!(!state.contains_key("test_sum"));
        assert!(!state.contains_key("test_p50"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_stats_multiple_types() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with different numeric types
        engine.eval::<()>(r#"track_stats("mixed", 100)"#).unwrap(); // i64
        engine.eval::<()>(r#"track_stats("mixed", 150.5)"#).unwrap(); // f64

        let state = get_thread_tracking_state();

        // Verify count
        assert_eq!(state.get("mixed_count").unwrap().as_int().unwrap_or(0), 2);

        // Verify sum (should handle mixed int/float)
        let sum = state.get("mixed_sum").unwrap().as_float().unwrap_or(0.0);
        assert!((sum - 250.5).abs() < 0.001);

        clear_tracking_state();
    }

    #[test]
    fn test_track_cardinality_basic() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some unique string values
        engine
            .eval::<()>(r#"track_cardinality("users", "alice")"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_cardinality("users", "bob")"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_cardinality("users", "charlie")"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_cardinality("users", "alice")"#) // duplicate
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that the metric was created
        assert!(state.contains_key("users"));

        // Check that it's a blob (serialized HLL)
        let value = state.get("users").unwrap();
        assert!(value.is_blob());

        // Deserialize and check the cardinality estimate
        let blob = value.clone().into_blob().unwrap();
        assert!(is_hll_blob(&blob));
        let hll = deserialize_hll(&blob).unwrap();
        let cardinality = hll.len();

        // HLL should estimate ~3 unique values (with some margin for error)
        assert!((2.0..=4.0).contains(&cardinality));

        clear_tracking_state();
    }

    #[test]
    fn test_track_cardinality_integers() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track integer values
        for i in 1..=100 {
            engine
                .eval::<()>(&format!(r#"track_cardinality("numbers", {})"#, i))
                .unwrap();
        }

        let state = get_thread_tracking_state();
        let blob = state.get("numbers").unwrap().clone().into_blob().unwrap();
        let hll = deserialize_hll(&blob).unwrap();
        let cardinality = hll.len();

        // HLL should estimate close to 100 (within ~5% for default error rate)
        assert!((90.0..=110.0).contains(&cardinality));

        clear_tracking_state();
    }

    #[test]
    fn test_track_cardinality_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored
        engine
            .eval::<()>(r#"track_cardinality("test", ())"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // No metric should be created for unit values only
        assert!(!state.contains_key("test"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_cardinality_with_custom_error_rate() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom error rate (0.5% for higher precision)
        for i in 1..=100 {
            engine
                .eval::<()>(&format!(r#"track_cardinality("precise", {}, 0.005)"#, i))
                .unwrap();
        }

        let state = get_thread_tracking_state();
        let blob = state.get("precise").unwrap().clone().into_blob().unwrap();
        let hll = deserialize_hll(&blob).unwrap();
        let cardinality = hll.len();

        // With 0.5% error rate, should be very close to 100
        assert!((95.0..=105.0).contains(&cardinality));

        clear_tracking_state();
    }

    #[test]
    fn test_track_cardinality_operation_metadata() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine
            .eval::<()>(r#"track_cardinality("ips", "192.168.1.1")"#)
            .unwrap();

        let internal = get_thread_internal_state();

        // Check that operation metadata was recorded
        assert!(internal.contains_key("__op_ips"));
        assert_eq!(
            internal
                .get("__op_ips")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "cardinality"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_finalize_metrics_for_script_stats() {
        // Regression test: inside `--end`, scripts must see finalized numeric
        // values for track_stats percentile / average metrics, not the raw
        // internal t-digest blob or {sum,count} map.
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        for v in [100.0_f64, 150.0, 200.0, 250.0, 300.0] {
            engine
                .eval::<()>(&format!(
                    "track_stats(\"latency\", {}, [0.50, 0.95, 0.99])",
                    v
                ))
                .unwrap();
        }

        let state = get_thread_tracking_state();

        // Raw state keeps internal representations (this is intentional).
        assert!(state.get("latency_p50").unwrap().is_blob());
        assert!(state.get("latency_p95").unwrap().is_blob());
        assert!(state.get("latency_avg").unwrap().is::<rhai::Map>());

        // Finalized view exposes plain numbers.
        let finalized = finalize_metrics_for_script(&state);

        let p50 = finalized
            .get("latency_p50")
            .expect("latency_p50 missing")
            .as_float()
            .expect("latency_p50 should be a float");
        assert!((100.0..=300.0).contains(&p50), "p50 out of range: {}", p50);

        let p99 = finalized
            .get("latency_p99")
            .expect("latency_p99 missing")
            .as_float()
            .expect("latency_p99 should be a float");
        assert!(p99 >= p50, "p99 ({}) should be >= p50 ({})", p99, p50);

        let avg = finalized
            .get("latency_avg")
            .expect("latency_avg missing")
            .as_float()
            .expect("latency_avg should be a float");
        assert!((avg - 200.0).abs() < 1e-9, "avg should be 200, got {}", avg);

        // Scalar fields (count, min, max, sum) pass through unchanged.
        let count = finalized
            .get("latency_count")
            .expect("latency_count missing")
            .as_int()
            .expect("latency_count should be int");
        assert_eq!(count, 5);

        let min = finalized
            .get("latency_min")
            .expect("latency_min missing")
            .as_float()
            .unwrap();
        let max = finalized
            .get("latency_max")
            .expect("latency_max missing")
            .as_float()
            .unwrap();
        assert!((min - 100.0).abs() < 1e-9);
        assert!((max - 300.0).abs() < 1e-9);

        clear_tracking_state();
    }

    #[test]
    fn test_finalize_metrics_for_script_cardinality() {
        // track_cardinality() stores an HLL blob; finalize should expose the
        // cardinality estimate as a plain integer.
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        for v in ["a", "b", "c", "a", "d", "b"] {
            engine
                .eval::<()>(&format!("track_cardinality(\"uniq\", \"{}\")", v))
                .unwrap();
        }

        let state = get_thread_tracking_state();
        assert!(state.get("uniq").unwrap().is_blob());

        let finalized = finalize_metrics_for_script(&state);
        let cardinality = finalized
            .get("uniq")
            .expect("uniq missing")
            .as_int()
            .expect("uniq should be int after finalize");
        assert_eq!(
            cardinality, 4,
            "expected 4 distinct values, got {}",
            cardinality
        );

        clear_tracking_state();
    }

    #[test]
    fn test_finalize_metrics_for_script_filters_internal_keys() {
        // Internal bookkeeping keys must not leak into user-visible scripts.
        clear_tracking_state();

        with_user_tracking(|state| {
            state.insert("visible".to_string(), Dynamic::from(1_i64));
            state.insert("__op_hidden".to_string(), Dynamic::from(1_i64));
            state.insert("__kelora_stats_hidden".to_string(), Dynamic::from(1_i64));
            state.insert("__kelora_error_hidden".to_string(), Dynamic::from(1_i64));
        });

        let state = get_thread_tracking_state();
        let finalized = finalize_metrics_for_script(&state);

        assert!(finalized.contains_key("visible"));
        assert!(!finalized.contains_key("__op_hidden"));
        assert!(!finalized.contains_key("__kelora_stats_hidden"));
        assert!(!finalized.contains_key("__kelora_error_hidden"));

        clear_tracking_state();
    }
}
