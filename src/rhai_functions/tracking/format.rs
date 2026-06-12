use super::merge::{deserialize_hll, deserialize_tdigest, is_hll_blob};
use super::metric_operation;
use rhai::Dynamic;
use std::collections::{HashMap, HashSet};

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
                let len = arr.len();
                let is_top_bottom = if !arr.is_empty() {
                    if let Some(first_map) = arr[0].clone().try_cast::<rhai::Map>() {
                        first_map.contains_key("key")
                            && (first_map.contains_key("count") || first_map.contains_key("value"))
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_top_bottom {
                    let field_name = if let Some(first_map) = arr[0].clone().try_cast::<rhai::Map>()
                    {
                        if first_map.contains_key("count") {
                            "count"
                        } else {
                            "value"
                        }
                    } else {
                        "count"
                    };

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
        // A track_count metric whose categories happen to be called "sum" and
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
