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
#[cfg(test)]
pub use errors::has_errors_in_tracking;
pub use errors::{
    extract_error_summary_from_tracking, format_fatal_error_line,
    has_errors_in_tracking_with_policy, record_stage_success, stage_failed_completely, track_error,
};
pub use format::{format_metrics_json, format_metrics_output};
pub(crate) use merge::op_display_name;
use merge::{
    deserialize_hll, deserialize_tdigest, ensure_operation_metadata, is_hll_blob, merge_numeric,
    record_skipped_unit,
};
use metrics::{
    track_avg_impl, track_cardinality_impl, track_cardinality_with_error_impl, track_max_impl,
    track_min_impl, track_percentiles_impl, track_stats_impl,
};
pub use rank::set_tracking_warnings_enabled;
pub(crate) use rank::unique_size_warning;
use rank::{
    track_bottom_count_impl, track_bottom_weighted_impl, track_count_impl, track_top_count_impl,
    track_top_weighted_impl, track_unique_f64_impl, track_unique_i64_impl,
    track_unique_string_impl,
};
pub use state::{
    get_thread_internal_state, get_thread_snapshot, get_thread_tracking_state,
    set_thread_internal_state, set_thread_tracking_state, with_internal_tracking,
    with_user_tracking, TrackingSnapshot,
};

/// Default N for track_top / track_bottom / track_top_by / track_bottom_by.
const DEFAULT_RANK_N: i64 = 10;

/// Convert a categorical argument (track_count category, track_top/track_bottom
/// item, track_unique bool value) to the string form used as a map key.
/// Unit `()` means "missing field" and yields `None` so callers can skip the
/// event (recording the skip for diagnostics).
fn categorical_to_string(
    fn_name: &str,
    arg_name: &str,
    value: &Dynamic,
) -> Result<Option<String>, Box<rhai::EvalAltResult>> {
    if value.is_unit() {
        return Ok(None);
    }
    if let Ok(s) = value.clone().into_string() {
        return Ok(Some(s));
    }
    // Floats use Rust's Display (200.0 → "200"), matching the labels 1.x
    // track_bucket produced; Rhai's Dynamic Display would render "200.0",
    // silently changing every float category key across the 2.0 migration.
    if value.is_float() {
        return Ok(Some(value.as_float().unwrap_or_default().to_string()));
    }
    if let Some(f) = value.clone().try_cast::<f32>() {
        return Ok(Some(f.to_string()));
    }
    if value.is_int() || value.is::<bool>() || value.is::<char>() || value.is::<i32>() {
        return Ok(Some(value.to_string()));
    }
    Err(format!(
        "{} {} must be a string, number, or bool; got {}",
        fn_name,
        arg_name,
        value.type_name()
    )
    .into())
}

#[derive(Clone, Copy)]
enum NumericArg {
    Int(i64),
    Float(f64),
}

impl NumericArg {
    fn as_f64(self) -> f64 {
        match self {
            NumericArg::Int(i) => i as f64,
            NumericArg::Float(f) => f,
        }
    }

    fn into_dynamic(self) -> Dynamic {
        match self {
            NumericArg::Int(i) => Dynamic::from(i),
            NumericArg::Float(f) => Dynamic::from(f),
        }
    }
}

/// Convert a numeric argument, preserving int vs float. Unit `()` yields
/// `None` (missing value → skip); non-numeric types are an error.
fn numeric_arg(
    fn_name: &str,
    arg_name: &str,
    value: &Dynamic,
) -> Result<Option<NumericArg>, Box<rhai::EvalAltResult>> {
    if value.is_unit() {
        return Ok(None);
    }
    if let Ok(i) = value.as_int() {
        return Ok(Some(NumericArg::Int(i)));
    }
    if let Ok(f) = value.as_float() {
        return Ok(Some(NumericArg::Float(f)));
    }
    if let Some(i) = value.clone().try_cast::<i32>() {
        return Ok(Some(NumericArg::Int(i as i64)));
    }
    if let Some(f) = value.clone().try_cast::<f32>() {
        return Ok(Some(NumericArg::Float(f as f64)));
    }
    Err(format!(
        "{} {} must be a number; got {}",
        fn_name,
        arg_name,
        value.type_name()
    )
    .into())
}

fn default_percentiles() -> rhai::Array {
    vec![
        Dynamic::from(0.50_f64),
        Dynamic::from(0.95_f64),
        Dynamic::from(0.99_f64),
    ]
}

fn track_rank_count(
    fn_name: &str,
    key: &str,
    item: &Dynamic,
    n: i64,
    is_top: bool,
) -> Result<(), Box<rhai::EvalAltResult>> {
    if n < 1 {
        return Err(format!("{} requires n >= 1, got {}", fn_name, n).into());
    }
    match categorical_to_string(fn_name, "item", item)? {
        Some(item_key) => {
            if is_top {
                track_top_count_impl(key, &item_key, n)
            } else {
                track_bottom_count_impl(key, &item_key, n)
            }
        }
        None => {
            record_skipped_unit(key);
            Ok(())
        }
    }
}

fn track_rank_by(
    fn_name: &str,
    key: &str,
    item: &Dynamic,
    score: &Dynamic,
    n: i64,
    is_top: bool,
) -> Result<(), Box<rhai::EvalAltResult>> {
    if n < 1 {
        return Err(format!("{} requires n >= 1, got {}", fn_name, n).into());
    }
    let Some(item_key) = categorical_to_string(fn_name, "item", item)? else {
        record_skipped_unit(key);
        return Ok(());
    };
    let Some(score) = numeric_arg(fn_name, "score", score)? else {
        record_skipped_unit(key);
        return Ok(());
    };
    if is_top {
        track_top_weighted_impl(key, &item_key, n, score.as_f64())
    } else {
        track_bottom_weighted_impl(key, &item_key, n, score.as_f64())
    }
}

fn track_cardinality_dispatch(
    key: &str,
    value: &Dynamic,
    error_rate: Option<f64>,
) -> Result<(), Box<rhai::EvalAltResult>> {
    fn insert<V: std::hash::Hash>(
        key: &str,
        v: &V,
        error_rate: Option<f64>,
    ) -> Result<(), Box<rhai::EvalAltResult>> {
        match error_rate {
            Some(rate) => track_cardinality_with_error_impl(key, v, rate),
            None => track_cardinality_impl(key, v),
        }
    }

    if value.is_unit() {
        record_skipped_unit(key);
        return Ok(());
    }
    if let Ok(i) = value.as_int() {
        return insert(key, &i, error_rate);
    }
    if let Ok(f) = value.as_float() {
        // Hash float bit patterns for consistent hashing
        return insert(key, &f.to_bits(), error_rate);
    }
    if let Some(i) = value.clone().try_cast::<i32>() {
        return insert(key, &(i as i64), error_rate);
    }
    if let Some(f) = value.clone().try_cast::<f32>() {
        return insert(key, &((f as f64).to_bits()), error_rate);
    }
    if let Ok(s) = value.clone().into_string() {
        return insert(key, &s, error_rate);
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        let s = if b { "true" } else { "false" }.to_string();
        return insert(key, &s, error_rate);
    }
    Err(format!(
        "track_cardinality value must be a string, number, or bool; got {}",
        value.type_name()
    )
    .into())
}

pub fn register_functions(engine: &mut Engine) {
    // Track functions using thread-local storage - clean user API.
    // Operation metadata (`__op_*`) is recorded per metric key; it drives the
    // parallel merge strategy and doubles as a conflict check so that one
    // metric name cannot be shared by different track functions.
    //
    // Common conventions across the family:
    // - Categorical arguments (category, item) accept strings, numbers, and
    //   bools; they are stringified into map keys.
    // - Unit `()` values (missing fields) are skipped, and the skip is counted
    //   for `--diagnostics`.

    // track_count(name, category): count occurrences of each category value
    // under the metric `name`. Result shape: {name → {category → count}}.
    engine.register_fn(
        "track_count",
        |name: &str, category: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match categorical_to_string("track_count", "category", &category)? {
                Some(cat) => track_count_impl(name, &cat),
                None => {
                    record_skipped_unit(name);
                    Ok(())
                }
            }
        },
    );
    engine.register_fn(
        "track_count",
        |name: Dynamic, _category: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            Err(format!(
                "track_count name must be a string; got {}. Example: track_count(\"status\", e.status)",
                name.type_name()
            )
            .into())
        },
    );
    // kelora 2.0 tombstone: the 1.x single-argument form counted the value itself.
    engine.register_fn(
        "track_count",
        |_value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            Err(
                "track_count takes a metric name and a category since kelora 2.0: \
                 track_count(\"status\", e.status) counts each status value. \
                 For a single counter, use track_sum(\"errors\", 1)"
                    .into(),
            )
        },
    );
    // kelora 2.0 tombstone: track_bucket was folded into track_count.
    engine.register_fn(
        "track_bucket",
        |_key: Dynamic, _bucket: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            Err(
                "track_bucket was renamed in kelora 2.0: use track_count(name, category), \
                 e.g. track_count(\"status_class\", e.status / 100 * 100)"
                    .into(),
            )
        },
    );

    engine.register_fn(
        "track_sum",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_sum", "value", &value)? {
                Some(num) => {
                    ensure_operation_metadata(key, "sum")?;
                    with_user_tracking(|state| {
                        let updated = merge_numeric(state.get(key).cloned(), num.into_dynamic());
                        state.insert(key.to_string(), updated);
                    });
                    Ok(())
                }
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    // Stores both sum and count as a map for proper averaging in parallel mode
    engine.register_fn(
        "track_avg",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_avg", "value", &value)? {
                Some(num) => track_avg_impl(key, num.as_f64()),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    engine.register_fn(
        "track_min",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_min", "value", &value)? {
                Some(num) => track_min_impl(key, num.into_dynamic(), num.as_f64()),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    engine.register_fn(
        "track_max",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_max", "value", &value)? {
                Some(num) => track_max_impl(key, num.into_dynamic(), num.as_f64()),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    // track_percentiles - streaming percentile estimation using t-digest.
    // Auto-suffixes metric names with percentile (e.g., "latency_p95");
    // default percentiles are [0.50, 0.95, 0.99].
    engine.register_fn(
        "track_percentiles",
        |key: &str,
         value: Dynamic,
         percentiles: rhai::Array|
         -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_percentiles", "value", &value)? {
                Some(num) => track_percentiles_impl(key, num.as_f64(), percentiles),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );
    engine.register_fn(
        "track_percentiles",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_percentiles", "value", &value)? {
                Some(num) => track_percentiles_impl(key, num.as_f64(), default_percentiles()),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    // track_stats - comprehensive statistics tracking, auto-suffixing
    // _min, _max, _avg, _count, _sum and _pXX metric names.
    engine.register_fn(
        "track_stats",
        |key: &str,
         value: Dynamic,
         percentiles: rhai::Array|
         -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_stats", "value", &value)? {
                Some(num) => track_stats_impl(key, num.as_f64(), percentiles),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );
    engine.register_fn(
        "track_stats",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            match numeric_arg("track_stats", "value", &value)? {
                Some(num) => track_stats_impl(key, num.as_f64(), default_percentiles()),
                None => {
                    record_skipped_unit(key);
                    Ok(())
                }
            }
        },
    );

    // track_unique - exact set of distinct values (kept in memory, unbounded;
    // a one-time warning fires past a size threshold).
    engine.register_fn(
        "track_unique",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            if value.is_unit() {
                record_skipped_unit(key);
                return Ok(());
            }
            if let Ok(i) = value.as_int() {
                return track_unique_i64_impl(key, i);
            }
            if let Ok(f) = value.as_float() {
                return track_unique_f64_impl(key, f);
            }
            if let Some(i) = value.clone().try_cast::<i32>() {
                return track_unique_i64_impl(key, i as i64);
            }
            if let Some(f) = value.clone().try_cast::<f32>() {
                return track_unique_f64_impl(key, f as f64);
            }
            if let Ok(s) = value.clone().into_string() {
                return track_unique_string_impl(key, &s);
            }
            if let Some(b) = value.clone().try_cast::<bool>() {
                return track_unique_string_impl(key, if b { "true" } else { "false" });
            }
            Err(format!(
                "track_unique value must be a string, number, or bool; got {}",
                value.type_name()
            )
            .into())
        },
    );

    // track_cardinality - probabilistic cardinality estimation using HyperLogLog.
    // Uses ~12KB of memory regardless of cardinality, with ~1% standard error.
    // For the exact values of small sets, use track_unique instead.
    engine.register_fn(
        "track_cardinality",
        |key: &str, value: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            track_cardinality_dispatch(key, &value, None)
        },
    );
    engine.register_fn(
        "track_cardinality",
        |key: &str, value: Dynamic, error_rate: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            track_cardinality_dispatch(key, &value, Some(error_rate))
        },
    );

    // track_top / track_bottom - most/least frequent items (n defaults to 10)
    engine.register_fn(
        "track_top",
        |key: &str, item: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_count("track_top", key, &item, DEFAULT_RANK_N, true)
        },
    );
    engine.register_fn(
        "track_top",
        |key: &str, item: Dynamic, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_count("track_top", key, &item, n, true)
        },
    );
    engine.register_fn(
        "track_bottom",
        |key: &str, item: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_count("track_bottom", key, &item, DEFAULT_RANK_N, false)
        },
    );
    engine.register_fn(
        "track_bottom",
        |key: &str, item: Dynamic, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_count("track_bottom", key, &item, n, false)
        },
    );
    // kelora 2.0 tombstones: the 4-argument weighted forms moved to
    // track_top_by / track_bottom_by.
    engine.register_fn(
        "track_top",
        |_a: Dynamic,
         _b: Dynamic,
         _c: Dynamic,
         _d: Dynamic|
         -> Result<(), Box<rhai::EvalAltResult>> {
            Err("ranking by a score moved to track_top_by in kelora 2.0: \
                 track_top_by(\"slowest\", e.endpoint, e.latency_ms)"
                .into())
        },
    );
    engine.register_fn(
        "track_bottom",
        |_a: Dynamic,
         _b: Dynamic,
         _c: Dynamic,
         _d: Dynamic|
         -> Result<(), Box<rhai::EvalAltResult>> {
            Err(
                "ranking by a score moved to track_bottom_by in kelora 2.0: \
                 track_bottom_by(\"fastest\", e.endpoint, e.latency_ms)"
                    .into(),
            )
        },
    );

    // track_top_by / track_bottom_by - items ranked by highest/lowest score
    // (n defaults to 10)
    engine.register_fn(
        "track_top_by",
        |key: &str, item: Dynamic, score: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_by("track_top_by", key, &item, &score, DEFAULT_RANK_N, true)
        },
    );
    engine.register_fn(
        "track_top_by",
        |key: &str,
         item: Dynamic,
         score: Dynamic,
         n: i64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_by("track_top_by", key, &item, &score, n, true)
        },
    );
    engine.register_fn(
        "track_bottom_by",
        |key: &str, item: Dynamic, score: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_by("track_bottom_by", key, &item, &score, DEFAULT_RANK_N, false)
        },
    );
    engine.register_fn(
        "track_bottom_by",
        |key: &str,
         item: Dynamic,
         score: Dynamic,
         n: i64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            track_rank_by("track_bottom_by", key, &item, &score, n, false)
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
fn finalize_metric_value(key: &str, value: &Dynamic, operation: Option<&str>) -> Dynamic {
    // Average tracking: {sum, count} map → mean as f64. Gated on the recorded
    // operation rather than the map's shape: a user's track_count categories
    // may legitimately be named "sum" and "count".
    if operation == Some("avg") {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
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

/// Look up the recorded track operation for a metric key in an ops map
/// (typically the internal tracking state, which holds `__op_{key}` entries).
pub(crate) fn metric_operation(ops: &HashMap<String, Dynamic>, key: &str) -> Option<String> {
    ops.get(&format!("__op_{}", key))
        .and_then(|v| v.clone().into_string().ok())
}

/// Reserved user-state key prefix recording the requested N for a ranked
/// metric (track_top / track_bottom / track_top_by / track_bottom_by). Stored
/// in user tracking so it survives parallel worker merges (which propagate the
/// full user map plus `__op_` metadata, but not arbitrary internal keys), and
/// filtered from all metric output by its `__kelora_` prefix.
pub(crate) const TOPN_PREFIX: &str = "__kelora_topn_";

/// Requested N for a ranked metric, read from the user tracking map.
pub(crate) fn metric_top_n(metrics: &HashMap<String, Dynamic>, key: &str) -> Option<usize> {
    metrics
        .get(&format!("{}{}", TOPN_PREFIX, key))
        .and_then(|v| v.as_int().ok())
        .filter(|n| *n >= 1)
        .map(|n| n as usize)
}

/// Build a finalized `rhai::Map` suitable for exposing as the `metrics`
/// global inside `--end` / `--span-close` / other post-processing stages.
/// Internal bookkeeping keys (`__op_*`, `__kelora_stats_*`, `__kelora_error_*`,
/// `__kelora_track_*`) are filtered out, and sketch-backed metrics are
/// finalized to plain numbers using the per-key operation metadata in `ops`.
pub fn finalize_metrics_for_script(
    metrics: &HashMap<String, Dynamic>,
    ops: &HashMap<String, Dynamic>,
) -> rhai::Map {
    let mut out = rhai::Map::new();
    for (key, value) in metrics.iter() {
        if key.starts_with("__op_")
            || key.starts_with("__kelora_stats_")
            || key.starts_with("__kelora_error_")
            || key.starts_with("__kelora_track_")
            || key.starts_with(TOPN_PREFIX)
        {
            continue;
        }
        let operation = metric_operation(ops, key);
        // Ranked metrics retain every distinct item; sort and truncate to N
        // here so scripts see the same top/bottom list the CLI prints.
        let finalized = match operation.as_deref().and_then(format::ranked_op_params) {
            Some((is_top, field)) => match value.clone().into_array() {
                Ok(arr) => {
                    let n = metric_top_n(metrics, key).unwrap_or(arr.len());
                    Dynamic::from(format::rank_array(&arr, is_top, field, n))
                }
                Err(_) => value.clone(),
            },
            None => finalize_metric_value(key, value, operation.as_deref()),
        };
        out.insert(key.clone().into(), finalized);
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

    // Ranked metrics keep every distinct item in raw state and only sort /
    // truncate to N at finalize time, so tests assert against the finalized
    // (user-visible) array rather than the raw tally.
    fn finalized_ranked_array(key: &str) -> rhai::Array {
        let metrics = get_thread_tracking_state();
        let ops = get_thread_internal_state();
        let finalized = finalize_metrics_for_script(&metrics, &ops);
        finalized.get(key).unwrap().clone().into_array().unwrap()
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
    fn test_ensure_operation_metadata() {
        clear_tracking_state();

        merge::ensure_operation_metadata("test_key", "count").unwrap();

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

        // Re-recording the same operation is fine
        merge::ensure_operation_metadata("test_key", "count").unwrap();

        // A different operation on the same key is a conflict
        let err = merge::ensure_operation_metadata("test_key", "avg").unwrap_err();
        assert!(err.to_string().contains("already tracked"));

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
    fn test_track_top_late_heavy_hitter() {
        // Regression: a heavy hitter first seen after the N slots are already
        // filled must still win. The old code truncated to N every event, so
        // "zzz" (alphabetically last, re-entering at count 1 between fresh
        // singletons) was evicted before it could accumulate.
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_top("m", "aaa", 3)"#).unwrap();
        engine.eval::<()>(r#"track_top("m", "bbb", 3)"#).unwrap();
        engine.eval::<()>(r#"track_top("m", "ccc", 3)"#).unwrap();
        for i in 0..100 {
            engine.eval::<()>(r#"track_top("m", "zzz", 3)"#).unwrap();
            engine
                .eval::<()>(&format!(r#"track_top("m", "filler{}", 3)"#, i))
                .unwrap();
        }

        let result = finalized_ranked_array("m");
        assert_eq!(result.len(), 3);
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "zzz"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 100);

        clear_tracking_state();
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

        let result = finalized_ranked_array("test");

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

        let result = finalized_ranked_array("test");

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
    fn test_track_top_by_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom scores (latency)
        engine
            .eval::<()>(r#"track_top_by("slow", "/api/users", 150, 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top_by("slow", "/api/products", 50, 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top_by("slow", "/api/users", 200, 2)"#)
            .unwrap(); // Should update to max (200)
        engine
            .eval::<()>(r#"track_top_by("slow", "/api/orders", 75, 2)"#)
            .unwrap();

        let result = finalized_ranked_array("slow");

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

        let result = finalized_ranked_array("test");

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
    fn test_track_bottom_by_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom scores (latency - bottom = fastest)
        engine
            .eval::<()>(r#"track_bottom_by("fast", "/api/users", 150.5, 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom_by("fast", "/api/products", 30.0, 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom_by("fast", "/api/users", 100.0, 2)"#)
            .unwrap(); // Should update to min (100)
        engine
            .eval::<()>(r#"track_bottom_by("fast", "/api/orders", 75.0, 2)"#)
            .unwrap();

        let result = finalized_ranked_array("fast");

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
            .eval::<()>(r#"track_bottom_by("test", (), 10, 3)"#)
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

        // Unit scores should be silently ignored
        engine
            .eval::<()>(r#"track_top_by("test", "apple", (), 3)"#)
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
            .eval::<()>(r#"track_top_by("test", (), 10, 3)"#)
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

        let result = finalized_ranked_array("test");

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
        let finalized = finalize_metrics_for_script(&state, &get_thread_internal_state());

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

        let finalized = finalize_metrics_for_script(&state, &get_thread_internal_state());
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
        let finalized = finalize_metrics_for_script(&state, &get_thread_internal_state());

        assert!(finalized.contains_key("visible"));
        assert!(!finalized.contains_key("__op_hidden"));
        assert!(!finalized.contains_key("__kelora_stats_hidden"));
        assert!(!finalized.contains_key("__kelora_error_hidden"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_count_categories() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine
            .eval::<()>(r#"track_count("level", "ERROR")"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_count("level", "ERROR")"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_count("level", "INFO")"#)
            .unwrap();
        // Numbers and bools are stringified into category keys
        engine.eval::<()>(r#"track_count("status", 200)"#).unwrap();
        engine.eval::<()>(r#"track_count("status", 200)"#).unwrap();
        engine.eval::<()>(r#"track_count("status", 503)"#).unwrap();
        engine.eval::<()>(r#"track_count("tls", true)"#).unwrap();

        let state = get_thread_tracking_state();
        let level = state
            .get("level")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(level.get("ERROR").unwrap().as_int().unwrap(), 2);
        assert_eq!(level.get("INFO").unwrap().as_int().unwrap(), 1);

        let status = state
            .get("status")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(status.get("200").unwrap().as_int().unwrap(), 2);
        assert_eq!(status.get("503").unwrap().as_int().unwrap(), 1);

        let tls = state
            .get("tls")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(tls.get("true").unwrap().as_int().unwrap(), 1);

        // The op metadata drives the parallel merge strategy
        let internal = get_thread_internal_state();
        assert_eq!(
            internal
                .get("__op_level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "bucket"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_count_skips_unit_and_records_skip() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_count("level", ())"#).unwrap();
        engine.eval::<()>(r#"track_count("level", ())"#).unwrap();

        let state = get_thread_tracking_state();
        assert!(!state.contains_key("level"));

        let internal = get_thread_internal_state();
        assert_eq!(
            internal
                .get("__kelora_track_skipped_level")
                .unwrap()
                .as_int()
                .unwrap(),
            2
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_count_rejects_invalid_arguments() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Non-string metric name
        let err = engine
            .eval::<()>(r#"track_count(42, "x")"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("name must be a string"), "got: {}", err);

        // Map/array categories are not stringifiable
        let err = engine
            .eval::<()>(r#"track_count("x", #{a: 1})"#)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("category must be a string, number, or bool"),
            "got: {}",
            err
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_count_one_arg_tombstone() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let err = engine
            .eval::<()>(r#"track_count("errors")"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("track_sum"), "got: {}", err);
        assert!(err.contains("kelora 2.0"), "got: {}", err);
    }

    #[test]
    fn test_track_bucket_tombstone() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let err = engine
            .eval::<()>(r#"track_bucket("status", 200)"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("track_count"), "got: {}", err);
    }

    #[test]
    fn test_track_top_four_arg_tombstone() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let err = engine
            .eval::<()>(r#"track_top("slow", "/api", 10, 150)"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("track_top_by"), "got: {}", err);

        let err = engine
            .eval::<()>(r#"track_bottom("fast", "/api", 10, 150)"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("track_bottom_by"), "got: {}", err);
    }

    #[test]
    fn test_track_operation_conflict_errors() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_sum("x", 1)"#).unwrap();
        let err = engine
            .eval::<()>(r#"track_min("x", 5)"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("already tracked by track_sum"), "got: {}", err);

        // Mixing the count and score ranking modes on one name is a conflict too
        engine.eval::<()>(r#"track_top("ranked", "a")"#).unwrap();
        let err = engine
            .eval::<()>(r#"track_top_by("ranked", "a", 5)"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("already tracked by track_top"), "got: {}", err);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_default_n() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        for i in 0..15 {
            engine
                .eval::<()>(&format!(r#"track_top("items", "item{:02}")"#, i))
                .unwrap();
        }

        let result = finalized_ranked_array("items");
        assert_eq!(result.len(), 10);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_numeric_and_bool_items() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_top("status", 503, 5)"#).unwrap();
        engine.eval::<()>(r#"track_top("status", 503, 5)"#).unwrap();
        engine.eval::<()>(r#"track_top("flags", true, 5)"#).unwrap();

        let status = finalized_ranked_array("status");
        let first = status[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "503"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 2);

        let flags = finalized_ranked_array("flags");
        let first = flags[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "true"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_unique_bool_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_unique("tls", true)"#).unwrap();
        engine.eval::<()>(r#"track_unique("tls", true)"#).unwrap();
        engine.eval::<()>(r#"track_unique("tls", false)"#).unwrap();

        let state = get_thread_tracking_state();
        let arr = state.get("tls").unwrap().clone().into_array().unwrap();
        assert_eq!(arr.len(), 2);

        clear_tracking_state();
    }

    #[test]
    fn test_track_sum_skips_unit_and_records_skip() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_sum("bytes", ())"#).unwrap();
        engine.eval::<()>(r#"track_sum("bytes", 10)"#).unwrap();
        engine.eval::<()>(r#"track_sum("bytes", ())"#).unwrap();

        let state = get_thread_tracking_state();
        assert_eq!(state.get("bytes").unwrap().as_int().unwrap(), 10);

        let internal = get_thread_internal_state();
        assert_eq!(
            internal
                .get("__kelora_track_skipped_bytes")
                .unwrap()
                .as_int()
                .unwrap(),
            2
        );

        clear_tracking_state();
    }

    #[test]
    fn test_finalize_count_categories_named_sum_count() {
        // A track_count metric with categories literally named "sum" and
        // "count" must finalize as a category map, not as an average.
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        engine.eval::<()>(r#"track_count("ops", "sum")"#).unwrap();
        engine.eval::<()>(r#"track_count("ops", "count")"#).unwrap();
        engine.eval::<()>(r#"track_count("ops", "count")"#).unwrap();

        let state = get_thread_tracking_state();
        let finalized = finalize_metrics_for_script(&state, &get_thread_internal_state());

        let ops = finalized
            .get("ops")
            .expect("ops missing")
            .clone()
            .try_cast::<rhai::Map>()
            .expect("ops should remain a category map");
        assert_eq!(ops.get("sum").unwrap().as_int().unwrap(), 1);
        assert_eq!(ops.get("count").unwrap().as_int().unwrap(), 2);

        clear_tracking_state();
    }
}
