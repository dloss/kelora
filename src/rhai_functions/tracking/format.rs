use super::merge::{deserialize_hll, deserialize_tdigest, is_hll_blob};
use super::{metric_operation, metric_top_n};
use rhai::Dynamic;
use std::collections::{HashMap, HashSet};

/// Map an internal ranked operation id to `(is_top, field)`, where `field` is
/// the per-entry map key holding the rank value. Returns `None` for non-ranked
/// operations.
pub(crate) fn ranked_op_params(op: &str) -> Option<(bool, &'static str)> {
    match op {
        "top" => Some((true, "count")),
        "bottom" => Some((false, "count")),
        "top_by" => Some((true, "value")),
        "bottom_by" => Some((false, "value")),
        _ => None,
    }
}

/// Sort a retained ranked array (one `{key, count|value}` map per distinct
/// item) into rank order and truncate to `n`. `is_top` selects descending
/// (top) vs ascending (bottom); ties break by key ascending, matching the
/// legacy per-event ordering. This is where track_top/track_bottom and their
/// `_by` variants pick the actual top/bottom N — the per-event path keeps every
/// item so late-arriving heavy hitters are no longer dropped.
pub(crate) fn rank_array(arr: &[Dynamic], is_top: bool, field: &str, n: usize) -> rhai::Array {
    let mut items: Vec<(String, f64)> = Vec::with_capacity(arr.len());
    for elem in arr {
        if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
            if let (Some(k), Some(v)) = (map.get("key"), map.get(field)) {
                let key = k.clone().into_string().unwrap_or_default();
                let num = if field == "count" {
                    v.as_int().unwrap_or(0) as f64
                } else {
                    v.as_float().unwrap_or(0.0)
                };
                items.push((key, num));
            }
        }
    }

    items.sort_by(|a, b| {
        let primary = if is_top {
            b.1.partial_cmp(&a.1)
        } else {
            a.1.partial_cmp(&b.1)
        }
        .unwrap_or(std::cmp::Ordering::Equal);
        primary.then_with(|| a.0.cmp(&b.0))
    });
    if items.len() > n {
        items.truncate(n);
    }

    items
        .into_iter()
        .map(|(k, num)| {
            let mut map = rhai::Map::new();
            map.insert("key".into(), Dynamic::from(k));
            if field == "count" {
                map.insert("count".into(), Dynamic::from(num as i64));
            } else {
                map.insert("value".into(), Dynamic::from(num));
            }
            Dynamic::from(map)
        })
        .collect()
}

/// Shape-based detection of a ranked array, used as a fallback when the
/// operation metadata is unavailable (the array is then shown as stored).
fn detect_ranked_field(arr: &[Dynamic]) -> Option<&'static str> {
    let first = arr.first()?.clone().try_cast::<rhai::Map>()?;
    if first.contains_key("key") && first.contains_key("count") {
        Some("count")
    } else if first.contains_key("key") && first.contains_key("value") {
        Some("value")
    } else {
        None
    }
}

/// Format metrics for CLI output according to specification.
/// `ops` holds the per-key `__op_{key}` operation metadata (the internal
/// tracking state) used to decide how values are finalized for display.
pub fn format_metrics_output(
    metrics: &HashMap<String, Dynamic>,
    ops: &HashMap<String, Dynamic>,
    metrics_level: u8,
) -> String {
    let mut output = String::new();

    // `__op_*` and `__kelora_*` are reserved bookkeeping prefixes; filter both
    // here to stay symmetric with the JSON formatter below.
    let mut user_values: Vec<_> = metrics
        .iter()
        .filter(|(k, _)| !k.starts_with("__op_") && !k.starts_with("__kelora_"))
        .collect();

    if user_values.is_empty() {
        return "No metrics tracked".to_string();
    }

    user_values.sort_by_key(|(k, _)| k.as_str());

    for (key, value) in user_values {
        if value.is::<rhai::Array>() {
            if let Ok(arr) = value.clone().into_array() {
                // Ranked metrics keep every distinct item; rank and truncate to
                // the requested N here at format time.
                let (arr, is_top_bottom, ranked_field) = match metric_operation(ops, key)
                    .as_deref()
                    .and_then(ranked_op_params)
                {
                    Some((is_top, field)) => {
                        let n = metric_top_n(metrics, key).unwrap_or(arr.len());
                        (rank_array(&arr, is_top, field, n), true, Some(field))
                    }
                    None => {
                        let field = detect_ranked_field(&arr);
                        (arr, field.is_some(), field)
                    }
                };
                let len = arr.len();

                if is_top_bottom {
                    let field_name = ranked_field.unwrap_or("count");

                    if metrics_level >= 2 || len <= 10 {
                        output.push_str(&format!("{:<12} ({} items):\n", key, len));
                        for (idx, item) in arr.iter().enumerate() {
                            push_ranked_item(&mut output, idx, item, field_name);
                        }
                    } else {
                        output.push_str(&format!("{:<12} ({} items):\n", key, len));
                        for (idx, item) in arr.iter().take(5).enumerate() {
                            push_ranked_item(&mut output, idx, item, field_name);
                        }
                        output.push_str(&format!(
                            "  [+{} more. Use --metrics=full or --metrics-file for full list]\n",
                            len - 5
                        ));
                    }
                } else if metrics_level >= 2 {
                    output.push_str(&format!("{:<12} ({} unique):\n", key, len));
                    for item in arr.iter() {
                        output.push_str(&format!("  {}\n", item));
                    }
                } else if len <= 10 {
                    output.push_str(&format!("{:<12} = {}\n", key, value));
                } else {
                    output.push_str(&format!("{:<12} ({} unique):\n", key, len));
                    for item in arr.iter().take(5) {
                        output.push_str(&format!("  {}\n", item));
                    }
                    output.push_str(&format!(
                        "  [+{} more. Use --metrics=full or --metrics-file for full list]\n",
                        len - 5
                    ));
                }
                continue;
            }
        }

        if metric_operation(ops, key).as_deref() == Some("avg") {
            if let Some(avg) = average_value(value) {
                output.push_str(&format!("{:<12} = {}\n", key, format_metric_float(avg)));
                continue;
            }
        }

        if let Ok(blob) = value.clone().into_blob() {
            if is_hll_blob(&blob) {
                if let Some(hll) = deserialize_hll(&blob) {
                    output.push_str(&format!("{:<12} ≈ {}\n", key, hll.len()));
                    continue;
                }
            }

            if let Some(digest) = deserialize_tdigest(&blob) {
                if let Some(p_pos) = key.rfind("_p") {
                    if let Ok(percentile) = key[p_pos + 2..].parse::<f64>() {
                        let quantile = percentile / 100.0;
                        let value = digest.estimate_quantile(quantile);
                        output.push_str(&format!("{:<12} = {}\n", key, format_metric_float(value)));
                        continue;
                    }
                }
            }
        }

        if value.is::<rhai::Map>() {
            if let Some(map) = value.clone().try_cast::<rhai::Map>() {
                push_count_map(&mut output, key, &map, metrics_level);
                continue;
            }
        }

        if value.is_int() {
            output.push_str(&format!("{:<12} = {}\n", key, value.as_int().unwrap_or(0)));
        } else if value.is_float() {
            output.push_str(&format!(
                "{:<12} = {}\n",
                key,
                format_metric_float(value.as_float().unwrap_or(0.0))
            ));
        } else {
            output.push_str(&format!("{:<12} = {}\n", key, value));
        }
    }

    output.trim_end().to_string()
}

/// Render a map-valued metric (e.g. from `track_freq`) as an aligned,
/// sorted list rather than dumping raw Rhai map syntax (`#{"500": 67, ...}`).
///
/// When every value is numeric the entries are sorted by value descending
/// (most frequent first), matching `track_top_count`; otherwise they fall back
/// to sorting by key so the order is at least stable. Like the array
/// formatters, the list is truncated to 5 entries unless `metrics_level >= 2`
/// (`--metrics=full`) or the map has 10 or fewer entries.
fn push_count_map(output: &mut String, key: &str, map: &rhai::Map, metrics_level: u8) {
    let len = map.len();

    if len == 0 {
        output.push_str(&format!("{:<12} (0 categories)\n", key));
        return;
    }

    let mut entries: Vec<(String, &Dynamic)> =
        map.iter().map(|(k, v)| (k.to_string(), v)).collect();

    let all_numeric = entries.iter().all(|(_, v)| v.is_int() || v.is_float());

    if all_numeric {
        entries.sort_by(|(ak, a), (bk, b)| {
            numeric_value(b)
                .partial_cmp(&numeric_value(a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| ak.cmp(bk))
        });
    } else {
        entries.sort_by(|(ak, _), (bk, _)| ak.cmp(bk));
    }

    let truncate = metrics_level < 2 && len > 10;
    let shown = if truncate { 5 } else { len };

    output.push_str(&format!("{:<12} ({} categories):\n", key, len));

    let label_width = entries
        .iter()
        .take(shown)
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        .min(40);

    for (cat, value) in entries.iter().take(shown) {
        output.push_str(&format!(
            "  {:<width$} {}\n",
            cat,
            format_metric_value(value),
            width = label_width
        ));
    }

    if truncate {
        output.push_str(&format!(
            "  [+{} more. Use --metrics=full or --metrics-file for full list]\n",
            len - shown
        ));
    }
}

/// Numeric value of a `Dynamic` for sorting, treating non-numbers as 0.
fn numeric_value(value: &Dynamic) -> f64 {
    if value.is_int() {
        value.as_int().unwrap_or(0) as f64
    } else if value.is_float() {
        value.as_float().unwrap_or(0.0)
    } else {
        0.0
    }
}

/// Format a single map value for the text view, reusing float trimming.
fn format_metric_value(value: &Dynamic) -> String {
    if value.is_int() {
        value.as_int().unwrap_or(0).to_string()
    } else if value.is_float() {
        format_metric_float(value.as_float().unwrap_or(0.0))
    } else {
        value.to_string()
    }
}

fn push_ranked_item(output: &mut String, idx: usize, item: &Dynamic, field_name: &str) {
    if let Some(map) = item.clone().try_cast::<rhai::Map>() {
        if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
            let key_str = k.clone().into_string().unwrap_or_default();
            if field_name == "count" {
                let count = v.as_int().unwrap_or(0);
                output.push_str(&format!("  #{:<2} {:<30} {}\n", idx + 1, key_str, count));
            } else {
                let val = v.as_float().unwrap_or(0.0);
                output.push_str(&format!(
                    "  #{:<2} {:<30} {}\n",
                    idx + 1,
                    key_str,
                    format_metric_float(val)
                ));
            }
        }
    }
}

/// Round a float to a fixed number of significant figures for the human-readable
/// `--metrics` text view, trimming trailing zeros (e.g. `146.6142714694471` →
/// `146.614`, `914.090` → `914.09`, `0.0004123` → `0.0004123`).
///
/// Display-only: the stored value and the JSON / `--metrics-file` output keep
/// full precision. Significant figures (rather than fixed decimals) keep
/// sub-1 values from collapsing to `0.00`.
fn format_metric_float(value: f64) -> String {
    const SIG_FIGS: i32 = 6;

    if !value.is_finite() {
        return format!("{value}");
    }
    if value == 0.0 {
        return "0".to_string();
    }

    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (SIG_FIGS - 1 - magnitude).max(0) as usize;
    let formatted = format!("{value:.decimals$}");

    if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    }
}

fn average_value(value: &Dynamic) -> Option<f64> {
    let map = value.clone().try_cast::<rhai::Map>()?;
    if !map.contains_key("sum") || !map.contains_key("count") {
        return None;
    }

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

    Some(if count > 0 { sum / count as f64 } else { 0.0 })
}

pub(crate) fn dynamic_to_json(value: Dynamic) -> serde_json::Value {
    if value.is_unit() {
        return serde_json::Value::Null;
    }

    if value.is::<rhai::Array>() {
        if let Ok(array) = value.clone().into_array() {
            let json_array = array.into_iter().map(dynamic_to_json).collect();
            return serde_json::Value::Array(json_array);
        }
    }

    if value.is::<rhai::Map>() {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                json_map.insert(k.into(), dynamic_to_json(v));
            }
            return serde_json::Value::Object(json_map);
        }
    }

    if value.is_int() {
        return serde_json::Value::Number(serde_json::Number::from(
            value.as_int().unwrap_or_default(),
        ));
    }

    if value.is_float() {
        if let Some(num) = serde_json::Number::from_f64(value.as_float().unwrap_or_default()) {
            return serde_json::Value::Number(num);
        }
    }

    if let Some(boolean) = value.clone().try_cast::<bool>() {
        return serde_json::Value::Bool(boolean);
    }

    if let Some(string) = value.clone().try_cast::<rhai::ImmutableString>() {
        return serde_json::Value::String(string.into());
    }

    serde_json::Value::String(value.to_string())
}

/// Format metrics for JSON output.
/// `ops` holds the per-key `__op_{key}` operation metadata; see
/// `format_metrics_output`.
pub fn format_metrics_json(
    metrics: &HashMap<String, Dynamic>,
    ops: &HashMap<String, Dynamic>,
) -> Result<String, serde_json::Error> {
    let mut json_obj = serde_json::Map::new();

    for (key, value) in metrics.iter() {
        if key.starts_with("__op_") || key.starts_with("__kelora_") {
            continue;
        }

        if metric_operation(ops, key).as_deref() == Some("avg") {
            if let Some(avg) = average_value(value) {
                if let Some(num) = serde_json::Number::from_f64(avg) {
                    json_obj.insert(key.clone(), serde_json::Value::Number(num));
                } else {
                    json_obj.insert(key.clone(), serde_json::Value::Null);
                }
                continue;
            }
        }

        if let Ok(blob) = value.clone().into_blob() {
            if is_hll_blob(&blob) {
                if let Some(hll) = deserialize_hll(&blob) {
                    let cardinality = hll.len() as i64;
                    json_obj.insert(
                        key.clone(),
                        serde_json::Value::Number(serde_json::Number::from(cardinality)),
                    );
                    continue;
                }
            }

            if let Some(digest) = deserialize_tdigest(&blob) {
                if let Some(p_pos) = key.rfind("_p") {
                    if let Ok(percentile) = key[p_pos + 2..].parse::<f64>() {
                        let quantile = percentile / 100.0;
                        let percentile_value = digest.estimate_quantile(quantile);
                        if let Some(num) = serde_json::Number::from_f64(percentile_value) {
                            json_obj.insert(key.clone(), serde_json::Value::Number(num));
                        } else {
                            json_obj.insert(key.clone(), serde_json::Value::Null);
                        }
                        continue;
                    }
                }
            }
        }

        // Ranked metrics retain every distinct item; rank and truncate to N.
        if let Some((is_top, field)) = metric_operation(ops, key)
            .as_deref()
            .and_then(ranked_op_params)
        {
            if let Ok(arr) = value.clone().into_array() {
                let n = metric_top_n(metrics, key).unwrap_or(arr.len());
                let ranked = rank_array(&arr, is_top, field, n);
                json_obj.insert(key.clone(), dynamic_to_json(Dynamic::from(ranked)));
                continue;
            }
        }

        json_obj.insert(key.clone(), dynamic_to_json(value.clone()));
    }

    serde_json::to_string_pretty(&json_obj)
}

/// Extract error summary from tracking state
#[allow(dead_code)] // Retained for potential future CLI summary output
pub fn extract_error_summary(metrics: &HashMap<String, Dynamic>) -> Option<String> {
    let mut has_errors = false;
    let mut summary = serde_json::Map::new();

    let mut error_types = HashSet::new();
    for key in metrics.keys() {
        if let Some(suffix) = key.strip_prefix("__kelora_error_count_") {
            error_types.insert(suffix.to_string());
        }
    }

    for error_type in error_types {
        let count_key = format!("__kelora_error_count_{}", error_type);
        let examples_key = format!("__kelora_error_examples_{}", error_type);

        if let Some(count_value) = metrics.get(&count_key) {
            let count = count_value.as_int().unwrap_or(0);
            if count > 0 {
                has_errors = true;
                let mut error_obj = serde_json::Map::new();
                error_obj.insert(
                    "count".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(count)),
                );

                if let Some(examples_value) = metrics.get(&examples_key) {
                    if let Ok(examples_array) = examples_value.clone().into_array() {
                        let examples: Vec<serde_json::Value> = examples_array
                            .iter()
                            .map(|v| {
                                serde_json::Value::String(
                                    v.clone().into_string().unwrap_or_default(),
                                )
                            })
                            .collect();
                        error_obj
                            .insert("examples".to_string(), serde_json::Value::Array(examples));
                    }
                }

                summary.insert(error_type, serde_json::Value::Object(error_obj));
            }
        }
    }

    if has_errors {
        Some(
            serde_json::to_string_pretty(&summary)
                .unwrap_or_else(|_| "Error serializing summary".to_string()),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average_value_from_int_sum() {
        let mut map = rhai::Map::new();
        map.insert("sum".into(), Dynamic::from(9i64));
        map.insert("count".into(), Dynamic::from(4i64));

        let avg = average_value(&Dynamic::from(map)).unwrap();
        assert!((avg - 2.25).abs() < 0.001);
    }

    #[test]
    fn test_format_metric_float_significant_figures() {
        // Trims noisy trailing digits to ~6 significant figures.
        assert_eq!(format_metric_float(146.6142714694471), "146.614");
        // Trailing zeros are trimmed.
        assert_eq!(format_metric_float(914.089985589136), "914.09");
        // Whole-number floats print without a decimal point.
        assert_eq!(format_metric_float(1000.0), "1000");
        // Sub-1 values survive instead of collapsing to "0.00".
        assert_eq!(format_metric_float(0.0004123), "0.0004123");
        // Zero and non-finite values have sane fallbacks.
        assert_eq!(format_metric_float(0.0), "0");
        assert_eq!(format_metric_float(f64::INFINITY), "inf");
    }

    fn avg_op(key: &str) -> HashMap<String, Dynamic> {
        let mut ops = HashMap::new();
        ops.insert(format!("__op_{}", key), Dynamic::from("avg".to_string()));
        ops
    }

    #[test]
    fn test_format_metrics_output_formats_average_maps() {
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        map.insert("sum".into(), Dynamic::from(12.0f64));
        map.insert("count".into(), Dynamic::from(3i64));
        metrics.insert("latency_avg".to_string(), Dynamic::from(map));

        let output = format_metrics_output(&metrics, &avg_op("latency_avg"), 1);
        assert!(output.contains("latency_avg"));
        assert!(output.contains("4"));
    }

    #[test]
    fn test_format_metrics_output_count_categories_named_sum_count() {
        // A track_freq metric whose categories happen to be called "sum" and
        // "count" must render as a category map, not be mistaken for an average.
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        map.insert("sum".into(), Dynamic::from(12i64));
        map.insert("count".into(), Dynamic::from(3i64));
        metrics.insert("ops".to_string(), Dynamic::from(map));
        let mut ops = HashMap::new();
        ops.insert("__op_ops".to_string(), Dynamic::from("bucket".to_string()));

        let output = format_metrics_output(&metrics, &ops, 1);
        assert!(output.contains("sum"), "output: {}", output);
        assert!(output.contains("count"), "output: {}", output);
    }

    #[test]
    fn test_format_metrics_output_count_map_sorted_by_count_desc() {
        // A track_freq map renders as an aligned list sorted by count desc,
        // not as raw Rhai map syntax.
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        map.insert("404".into(), Dynamic::from(12i64));
        map.insert("500".into(), Dynamic::from(67i64));
        map.insert("200".into(), Dynamic::from(40i64));
        metrics.insert("status".to_string(), Dynamic::from(map));
        let mut ops = HashMap::new();
        ops.insert(
            "__op_status".to_string(),
            Dynamic::from("bucket".to_string()),
        );

        let output = format_metrics_output(&metrics, &ops, 1);

        // No raw Rhai map syntax.
        assert!(!output.contains("#{"), "output: {}", output);
        assert!(
            output.contains("status       (3 categories):"),
            "output: {}",
            output
        );
        // Highest count first.
        let p500 = output.find("500").unwrap();
        let p200 = output.find("200").unwrap();
        let p404 = output.find("404").unwrap();
        assert!(p500 < p200 && p200 < p404, "output: {}", output);
    }

    #[test]
    fn test_format_metrics_output_count_map_truncates_above_ten() {
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        for i in 0..15 {
            map.insert(format!("cat{:02}", i).into(), Dynamic::from(i as i64));
        }
        metrics.insert("things".to_string(), Dynamic::from(map));

        // Default level truncates to 5 with a "more" line.
        let output = format_metrics_output(&metrics, &HashMap::new(), 1);
        assert!(output.contains("(15 categories):"), "output: {}", output);
        assert!(output.contains("[+10 more"), "output: {}", output);

        // Full level shows everything.
        let full = format_metrics_output(&metrics, &HashMap::new(), 2);
        assert!(!full.contains("more"), "output: {}", full);
        assert!(full.contains("cat00"), "output: {}", full);
    }

    #[test]
    fn test_format_metrics_output_formats_hll_cardinality() {
        let mut metrics = HashMap::new();
        let mut hll = super::super::merge::new_hll();
        hll.insert(&"alice");
        hll.insert(&"bob");
        hll.insert(&"alice");
        metrics.insert(
            "users".to_string(),
            Dynamic::from_blob(super::super::merge::serialize_hll(&hll)),
        );

        let output = format_metrics_output(&metrics, &HashMap::new(), 1);
        assert!(output.contains("users"));
        assert!(output.contains("≈ 2"));
    }

    #[test]
    fn test_format_metrics_json_formats_average_maps() {
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        map.insert("sum".into(), Dynamic::from(15.0f64));
        map.insert("count".into(), Dynamic::from(5i64));
        metrics.insert("latency_avg".to_string(), Dynamic::from(map));

        let json = format_metrics_json(&metrics, &avg_op("latency_avg")).unwrap();
        assert!(json.contains("\"latency_avg\""));
        assert!(json.contains("3.0") || json.contains("3"));
    }

    #[test]
    fn test_format_metrics_json_formats_hll_cardinality() {
        let mut metrics = HashMap::new();
        let mut hll = super::super::merge::new_hll();
        hll.insert(&"one");
        hll.insert(&"two");
        hll.insert(&"three");
        metrics.insert(
            "users".to_string(),
            Dynamic::from_blob(super::super::merge::serialize_hll(&hll)),
        );

        let json = format_metrics_json(&metrics, &HashMap::new()).unwrap();
        assert!(json.contains("\"users\""));
        assert!(json.contains("3"));
    }

    #[test]
    fn test_extract_error_summary_no_errors() {
        let metrics = HashMap::new();
        let summary = extract_error_summary(&metrics);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_error_summary_with_errors() {
        let mut metrics = HashMap::new();
        metrics.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(5i64),
        );

        let arr = vec![Dynamic::from("example error 1")];
        metrics.insert(
            "__kelora_error_examples_parse".to_string(),
            Dynamic::from(arr),
        );

        let summary = extract_error_summary(&metrics);
        assert!(summary.is_some());
        let text = summary.unwrap();
        assert!(text.contains("parse"));
        assert!(text.contains("\"count\": 5"));
    }

    #[test]
    fn test_extract_error_summary_zero_errors() {
        let mut metrics = HashMap::new();
        metrics.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(0i64),
        );

        let summary = extract_error_summary(&metrics);
        assert!(summary.is_none());
    }
}
