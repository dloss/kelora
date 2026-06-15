use rhai::{Array, Engine, EvalAltResult};

/// Register window-related functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // Register array statistical functions. Accept both float and integer
    // percentiles so the idiomatic `arr.percentile(95)` works alongside
    // `arr.percentile(95.0)` (Rhai does not auto-coerce int args to f64).
    engine.register_fn("percentile", percentile);
    engine.register_fn("percentile", |arr: Array, p: i64| percentile(arr, p as f64));
}

/// Calculate percentile of numeric array
///
/// Calculates the specified percentile of numeric values in an array using linear
/// interpolation. Works with any array of numeric values, including results from
/// `window.pluck_as_nums("field")` or any other source.
///
/// # Arguments
/// * `arr` - Array of numeric values (integers, floats, or convertible strings)
/// * `p` - Percentile to calculate (0-100, e.g., 95 for P95)
///
/// # Returns
/// Percentile value as a float using linear interpolation method.
/// Non-numeric values in the array are filtered out before calculation.
///
/// # Examples
/// ```rhai
/// // P95 response time from window
/// let p95 = window.pluck_as_nums("response_time").percentile(95);
/// if p95 > 0.5 { print("SLA breach: P95=" + p95) }
///
/// // Median calculation
/// let median = [1, 2, 3, 4, 5].percentile(50);  // 3.0
///
/// // P99 latency monitoring
/// let p99 = window.pluck_as_nums("latency").percentile(99);
/// ```
///
/// # Error Cases
/// - Empty array or no numeric values: returns error
/// - Percentile outside 0-100 range: returns error
///
/// # Implementation Notes
/// - Uses linear interpolation for percentile calculation
/// - Automatically filters out non-numeric values
/// - Handles mixed type arrays (int, float, string numbers)
/// - Maintains precision with floating-point results
fn percentile(arr: Array, p: f64) -> Result<f64, Box<EvalAltResult>> {
    if arr.is_empty() {
        return Err("Cannot calculate percentile of empty array".into());
    }

    if !(0.0..=100.0).contains(&p) {
        return Err("Percentile must be between 0 and 100".into());
    }

    // Convert Dynamic values to f64, filtering out non-numeric values
    let mut values: Vec<f64> = Vec::new();

    for item in arr {
        let number_opt = if item.is_int() {
            Some(item.as_int().unwrap_or(0) as f64)
        } else if item.is_float() {
            Some(item.as_float().unwrap_or(0.0))
        } else if item.is_bool() {
            Some(if item.as_bool().unwrap_or(false) {
                1.0
            } else {
                0.0
            })
        } else if item.is_string() {
            // Try to parse string as number
            let str_value = item.clone().into_string().unwrap_or_default();
            str_value.parse::<f64>().ok()
        } else {
            // Try parsing string representation
            item.to_string().parse::<f64>().ok()
        };

        if let Some(number) = number_opt {
            values.push(number);
        }
    }

    if values.is_empty() {
        return Err("No numeric values found in array".into());
    }

    // Sort values for percentile calculation
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate percentile using linear interpolation
    let index = (p / 100.0) * (values.len() - 1) as f64;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;

    if lower == upper {
        // Exact index, no interpolation needed
        Ok(values[lower])
    } else {
        // Linear interpolation between lower and upper values
        let weight = index - lower as f64;
        Ok(values[lower] * (1.0 - weight) + values[upper] * weight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Dynamic;

    #[test]
    fn test_percentile_basic() {
        let arr = vec![
            Dynamic::from(1.0),
            Dynamic::from(2.0),
            Dynamic::from(3.0),
            Dynamic::from(4.0),
            Dynamic::from(5.0),
        ];

        // Test median (50th percentile)
        let median = percentile(arr.clone(), 50.0).unwrap();
        assert_eq!(median, 3.0);

        // Test minimum (0th percentile)
        let min = percentile(arr.clone(), 0.0).unwrap();
        assert_eq!(min, 1.0);

        // Test maximum (100th percentile)
        let max = percentile(arr, 100.0).unwrap();
        assert_eq!(max, 5.0);
    }

    #[test]
    fn test_percentile_integer_overload_via_engine() {
        // Regression: the idiomatic `arr.percentile(95)` (integer arg) must
        // work, not just `arr.percentile(95.0)`. Rhai does not auto-coerce
        // int args to f64, so an explicit i64 overload is required.
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let median: f64 = engine.eval("[1, 2, 3, 4, 5].percentile(50)").unwrap();
        assert_eq!(median, 3.0);

        let p100: f64 = engine.eval("[1, 2, 3, 4, 5].percentile(100)").unwrap();
        assert_eq!(p100, 5.0);

        // Float form must keep working alongside the integer form.
        let p50_float: f64 = engine.eval("[1, 2, 3, 4, 5].percentile(50.0)").unwrap();
        assert_eq!(p50_float, 3.0);
    }

    #[test]
    fn test_percentile_interpolation() {
        let arr = vec![
            Dynamic::from(1.0),
            Dynamic::from(2.0),
            Dynamic::from(3.0),
            Dynamic::from(4.0),
        ];

        // Test 25th percentile (should interpolate between 1 and 2)
        let p25 = percentile(arr.clone(), 25.0).unwrap();
        assert_eq!(p25, 1.75);

        // Test 75th percentile (should interpolate between 3 and 4)
        let p75 = percentile(arr, 75.0).unwrap();
        assert_eq!(p75, 3.25);
    }

    #[test]
    fn test_percentile_mixed_types() {
        let arr = vec![
            Dynamic::from(42i64),   // int
            Dynamic::from(2.5),     // float
            Dynamic::from("123.5"), // string number
            Dynamic::from(true),    // bool -> 1.0
            Dynamic::from(false),   // bool -> 0.0
        ];

        let median = percentile(arr, 50.0).unwrap();
        // Sorted: [0.0, 1.0, 2.5, 42.0, 123.5] -> median = 2.5
        assert_eq!(median, 2.5);
    }

    #[test]
    fn test_percentile_filters_non_numeric() {
        let arr = vec![
            Dynamic::from(1.0),
            Dynamic::from("not_a_number"),
            Dynamic::from(3.0),
            Dynamic::from(5.0),
        ];

        let median = percentile(arr, 50.0).unwrap();
        // Should ignore "not_a_number", work with [1.0, 3.0, 5.0] -> median = 3.0
        assert_eq!(median, 3.0);
    }

    #[test]
    fn test_percentile_error_cases() {
        let empty_arr = Array::new();
        assert!(percentile(empty_arr, 50.0).is_err());

        let non_numeric_arr = vec![Dynamic::from("text"), Dynamic::from("more_text")];
        assert!(percentile(non_numeric_arr, 50.0).is_err());

        let valid_arr = vec![Dynamic::from(1.0)];
        assert!(percentile(valid_arr.clone(), -1.0).is_err()); // Invalid percentile
        assert!(percentile(valid_arr, 101.0).is_err()); // Invalid percentile
    }

    #[test]
    fn test_percentile_single_value() {
        let arr = vec![Dynamic::from(42.0)];

        // All percentiles should return the single value
        assert_eq!(percentile(arr.clone(), 0.0).unwrap(), 42.0);
        assert_eq!(percentile(arr.clone(), 50.0).unwrap(), 42.0);
        assert_eq!(percentile(arr, 100.0).unwrap(), 42.0);
    }
}
