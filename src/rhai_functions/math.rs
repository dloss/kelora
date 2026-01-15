use rhai::{Array, Dynamic, Engine, EvalAltResult};

use super::arrays::{determine_array_type, ArrayType};

pub fn register_functions(engine: &mut Engine) {
    // Register modulo function since % operator seems to be missing
    engine.register_fn("mod", |a: i64, b: i64| -> i64 {
        if b == 0 {
            0 // Avoid division by zero
        } else {
            a % b
        }
    });

    // Also register it as % for completeness
    engine.register_fn("%", |a: i64, b: i64| -> i64 {
        if b == 0 {
            0 // Avoid division by zero
        } else {
            a % b
        }
    });

    // Register clamp function for integers
    engine.register_fn("clamp", clamp_i64);

    // Register clamp function for floats
    engine.register_fn("clamp", clamp_f64);

    // Register statistical functions for arrays
    engine.register_fn("sum", sum_array);
    engine.register_fn("mean", mean_array);
    engine.register_fn("variance", variance_array);
    engine.register_fn("stddev", stddev_array);
}

/// Helper function to convert numeric array values to f64
///
/// Only works on arrays that have already been validated as ArrayType::Numeric.
/// This ensures we only call it after type checking, matching min()/max() behavior.
fn extract_numeric_values(arr: &Array) -> Vec<f64> {
    arr.iter()
        .map(|val| {
            if val.is_int() {
                val.as_int().unwrap() as f64
            } else if val.is_float() {
                val.as_float().unwrap()
            } else {
                // Should never happen if called on validated numeric array
                0.0
            }
        })
        .collect()
}

/// Calculate sum of numeric values in an array
///
/// Returns the sum of all numeric values (int, float) in the array. All elements
/// must be actual numbers (i64, f64). Strings, booleans, and other types are NOT
/// accepted. Mixed-type arrays return `()`. Does NOT parse numeric strings as numbers
/// (use pluck_as_nums() for explicit coercion).
///
/// # Arguments
/// * `arr` - Array containing numeric values
///
/// # Returns
/// Sum as f64, or `()` if array is empty, contains non-numeric values, or has mixed types
///
/// # Examples
/// ```rhai
/// [1, 2, 3, 4, 5].sum()                    // 15.0
/// [1.5, 2.5, 3.0].sum()                    // 7.0
/// [10, 20.5, 30].sum()                     // 60.5 (mixed int/float)
/// [10, "20", 30].sum()                     // () (mixed types rejected)
/// ["10", "20", "30"].sum()                 // () (strings not parsed)
/// [].sum()                                 // () (empty array)
///
/// // Use pluck_as_nums() for explicit coercion
/// e.total_bytes = e.requests.pluck_as_nums("bytes").sum();
/// ```
fn sum_array(arr: Array) -> Dynamic {
    if arr.is_empty() {
        return Dynamic::UNIT;
    }

    let array_type = determine_array_type(&arr);

    match array_type {
        ArrayType::Empty => Dynamic::UNIT,
        ArrayType::Mixed => Dynamic::UNIT,  // Reject mixed types
        ArrayType::String => Dynamic::UNIT, // Reject non-numeric arrays
        ArrayType::Numeric => {
            let sum: f64 = extract_numeric_values(&arr).iter().sum();
            Dynamic::from(sum)
        }
    }
}

/// Calculate mean (average) of numeric values in an array
///
/// Returns the arithmetic mean of all numeric values (int, float). All elements
/// must be actual numbers (i64, f64). Strings, booleans, and other types are NOT
/// accepted. Does NOT parse numeric strings as numbers. Returns an error for
/// empty arrays or arrays with mixed/non-numeric types.
///
/// # Arguments
/// * `arr` - Array containing numeric values
///
/// # Returns
/// Mean as f64, or error if array is empty, contains non-numeric values, or has mixed types
///
/// # Examples
/// ```rhai
/// [1, 2, 3, 4, 5].mean()                   // 3.0
/// [10, 20, 30].mean()                      // 20.0
/// [1.5, 2.5, 3.0].mean()                   // 2.333...
/// [10, 20.5, 30].mean()                    // 20.166... (mixed int/float OK)
/// [10, "20", 30].mean()                    // ERROR (mixed types rejected)
/// [].mean()                                // ERROR (empty array)
///
/// // Use pluck_as_nums() for explicit coercion
/// e.avg_latency = e.latencies.pluck_as_nums("ms").mean();
/// ```
fn mean_array(arr: Array) -> Result<f64, Box<EvalAltResult>> {
    if arr.is_empty() {
        return Err("Cannot calculate mean of empty array".into());
    }

    let array_type = determine_array_type(&arr);

    match array_type {
        ArrayType::Empty => Err("Cannot calculate mean of empty array".into()),
        ArrayType::Mixed => Err("Cannot calculate mean of array with mixed types (numbers and non-numbers). Use pluck_as_nums() for type coercion.".into()),
        ArrayType::String => Err("Cannot calculate mean of array with non-numeric values".into()),
        ArrayType::Numeric => {
            let values = extract_numeric_values(&arr);
            let sum: f64 = values.iter().sum();
            Ok(sum / values.len() as f64)
        }
    }
}

/// Calculate variance of numeric values in an array
///
/// Returns the population variance (sum of squared deviations from mean divided by N).
/// All elements must be actual numbers (i64, f64). Strings, booleans, and other types
/// are NOT accepted. Does NOT parse numeric strings as numbers. Returns an error for
/// empty arrays or arrays with mixed/non-numeric types.
///
/// # Arguments
/// * `arr` - Array containing numeric values
///
/// # Returns
/// Variance as f64, or error if array is empty, contains non-numeric values, or has mixed types
///
/// # Examples
/// ```rhai
/// [1, 2, 3, 4, 5].variance()               // 2.0
/// [10, 20, 30].variance()                  // 66.666...
/// [5, 5, 5, 5].variance()                  // 0.0 (no variation)
/// [10, "20", 30].variance()                // ERROR (mixed types rejected)
///
/// // Use pluck_as_nums() for explicit coercion
/// e.latency_variance = e.latencies.pluck_as_nums("ms").variance();
/// ```
fn variance_array(arr: Array) -> Result<f64, Box<EvalAltResult>> {
    if arr.is_empty() {
        return Err("Cannot calculate variance of empty array".into());
    }

    let array_type = determine_array_type(&arr);

    match array_type {
        ArrayType::Empty => Err("Cannot calculate variance of empty array".into()),
        ArrayType::Mixed => Err("Cannot calculate variance of array with mixed types (numbers and non-numbers). Use pluck_as_nums() for type coercion.".into()),
        ArrayType::String => Err("Cannot calculate variance of array with non-numeric values".into()),
        ArrayType::Numeric => {
            let values = extract_numeric_values(&arr);
            let mean: f64 = values.iter().sum::<f64>() / values.len() as f64;
            let variance = values
                .iter()
                .map(|&val| {
                    let diff = val - mean;
                    diff * diff
                })
                .sum::<f64>()
                / values.len() as f64;
            Ok(variance)
        }
    }
}

/// Calculate standard deviation of numeric values in an array
///
/// Returns the population standard deviation (square root of variance).
/// All elements must be actual numbers (i64, f64). Strings, booleans, and other types
/// are NOT accepted. Does NOT parse numeric strings as numbers. Returns an error for
/// empty arrays or arrays with mixed/non-numeric types.
///
/// # Arguments
/// * `arr` - Array containing numeric values
///
/// # Returns
/// Standard deviation as f64, or error if array is empty, contains non-numeric values, or has mixed types
///
/// # Examples
/// ```rhai
/// [1, 2, 3, 4, 5].stddev()                 // 1.414...
/// [10, 20, 30].stddev()                    // 8.165...
/// [5, 5, 5, 5].stddev()                    // 0.0 (no variation)
/// [10, "20", 30].stddev()                  // ERROR (mixed types rejected)
///
/// // Use pluck_as_nums() for explicit coercion
/// e.latency_stddev = e.latencies.pluck_as_nums("ms").stddev();
/// ```
fn stddev_array(arr: Array) -> Result<f64, Box<EvalAltResult>> {
    let variance = variance_array(arr)?;
    Ok(variance.sqrt())
}

/// Clamp an integer value between a minimum and maximum
///
/// Constrains a value to be within a specified range. If the value is less than
/// the minimum, returns the minimum. If the value is greater than the maximum,
/// returns the maximum. Otherwise returns the value unchanged.
///
/// # Arguments
/// * `value` - The value to clamp
/// * `min` - The minimum allowed value
/// * `max` - The maximum allowed value
///
/// # Returns
/// The clamped value
///
/// # Examples
/// ```rhai
/// clamp(5, 0, 10)    // 5 (within range)
/// clamp(-5, 0, 10)   // 0 (below minimum)
/// clamp(15, 0, 10)   // 10 (above maximum)
/// clamp(50, 0, 100)  // 50 (within range)
///
/// // Useful for normalizing values
/// e.normalized_score = clamp(e.score, 0, 100);
/// e.safe_port = clamp(e.port, 1024, 65535);
/// e.cpu_pct = clamp(e.cpu_usage, 0, 100);
/// ```
///
/// # Panics
/// Panics if `min > max`.
fn clamp_i64(value: i64, min: i64, max: i64) -> i64 {
    value.clamp(min, max)
}

/// Clamp a floating-point value between a minimum and maximum
///
/// Constrains a value to be within a specified range. If the value is less than
/// the minimum, returns the minimum. If the value is greater than the maximum,
/// returns the maximum. Otherwise returns the value unchanged.
///
/// # Arguments
/// * `value` - The value to clamp
/// * `min` - The minimum allowed value
/// * `max` - The maximum allowed value
///
/// # Returns
/// The clamped value
///
/// # Examples
/// ```rhai
/// clamp(3.14, 0.0, 5.0)   // 3.14 (within range)
/// clamp(-1.5, 0.0, 5.0)   // 0.0 (below minimum)
/// clamp(7.8, 0.0, 5.0)    // 5.0 (above maximum)
///
/// // Useful for normalizing metrics
/// e.response_time_sec = clamp(e.response_time_ms / 1000.0, 0.0, 30.0);
/// e.normalized_latency = clamp(e.latency / e.baseline, 0.0, 10.0);
/// ```
///
/// # Panics
/// Panics if `min > max` or if either value is NaN.
fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Dynamic;

    #[test]
    fn test_clamp_i64_within_range() {
        assert_eq!(clamp_i64(5, 0, 10), 5);
        assert_eq!(clamp_i64(50, 0, 100), 50);
        assert_eq!(clamp_i64(0, 0, 10), 0);
        assert_eq!(clamp_i64(10, 0, 10), 10);
    }

    #[test]
    fn test_clamp_i64_below_min() {
        assert_eq!(clamp_i64(-5, 0, 10), 0);
        assert_eq!(clamp_i64(-100, 0, 100), 0);
        assert_eq!(clamp_i64(-1, 0, 10), 0);
    }

    #[test]
    fn test_clamp_i64_above_max() {
        assert_eq!(clamp_i64(15, 0, 10), 10);
        assert_eq!(clamp_i64(200, 0, 100), 100);
        assert_eq!(clamp_i64(11, 0, 10), 10);
    }

    #[test]
    fn test_clamp_i64_negative_range() {
        assert_eq!(clamp_i64(-5, -10, -1), -5);
        assert_eq!(clamp_i64(-15, -10, -1), -10);
        assert_eq!(clamp_i64(0, -10, -1), -1);
    }

    #[test]
    #[should_panic(expected = "assertion failed: min <= max")]
    fn test_clamp_i64_inverted_range() {
        // When min > max, clamp panics (Rust's behavior)
        clamp_i64(5, 10, 0);
    }

    #[test]
    fn test_clamp_f64_within_range() {
        assert_eq!(clamp_f64(3.5, 0.0, 5.0), 3.5);
        assert_eq!(clamp_f64(2.5, 0.0, 10.0), 2.5);
        assert_eq!(clamp_f64(0.0, 0.0, 5.0), 0.0);
        assert_eq!(clamp_f64(5.0, 0.0, 5.0), 5.0);
    }

    #[test]
    fn test_clamp_f64_below_min() {
        assert_eq!(clamp_f64(-1.5, 0.0, 5.0), 0.0);
        assert_eq!(clamp_f64(-100.0, 0.0, 100.0), 0.0);
        assert_eq!(clamp_f64(-0.1, 0.0, 10.0), 0.0);
    }

    #[test]
    fn test_clamp_f64_above_max() {
        assert_eq!(clamp_f64(7.8, 0.0, 5.0), 5.0);
        assert_eq!(clamp_f64(200.5, 0.0, 100.0), 100.0);
        assert_eq!(clamp_f64(10.1, 0.0, 10.0), 10.0);
    }

    #[test]
    fn test_clamp_f64_negative_range() {
        assert_eq!(clamp_f64(-5.5, -10.0, -1.0), -5.5);
        assert_eq!(clamp_f64(-15.0, -10.0, -1.0), -10.0);
        assert_eq!(clamp_f64(0.0, -10.0, -1.0), -1.0);
    }

    #[test]
    #[should_panic(expected = "min > max")]
    fn test_clamp_f64_inverted_range() {
        // When min > max, clamp panics (Rust's behavior)
        clamp_f64(5.0, 10.0, 0.0);
    }

    #[test]
    fn test_clamp_f64_fractional() {
        assert_eq!(clamp_f64(0.5, 0.0, 1.0), 0.5);
        assert_eq!(clamp_f64(1.5, 0.0, 1.0), 1.0);
        assert_eq!(clamp_f64(-0.5, 0.0, 1.0), 0.0);
    }

    #[test]
    fn test_sum_integers() {
        let arr: Array = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
            Dynamic::from(5i64),
        ];
        let result = sum_array(arr);
        assert_eq!(result.as_float().unwrap(), 15.0);
    }

    #[test]
    fn test_sum_floats() {
        let arr: Array = vec![
            Dynamic::from(1.5f64),
            Dynamic::from(2.5f64),
            Dynamic::from(3.0f64),
        ];
        let result = sum_array(arr);
        assert_eq!(result.as_float().unwrap(), 7.0);
    }

    #[test]
    fn test_sum_mixed_numeric() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from(20.5f64),
            Dynamic::from(30i64),
        ];
        let result = sum_array(arr);
        assert_eq!(result.as_float().unwrap(), 60.5);
    }

    #[test]
    fn test_sum_mixed_types_rejected() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from(true),
            Dynamic::from(20i64),
            Dynamic::from(false),
        ];
        // Mixed types (numbers + booleans) are rejected
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_sum_strings_rejected() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from("20".to_string()),
            Dynamic::from(30i64),
        ];
        // Mixed types (numbers + strings) are rejected
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_sum_non_numeric_rejected() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from("not a number".to_string()),
            Dynamic::from(20i64),
        ];
        // Mixed types are rejected
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_sum_empty_array() {
        let arr: Array = vec![];
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_sum_no_numeric_values() {
        let arr: Array = vec![
            Dynamic::from("abc".to_string()),
            Dynamic::from("def".to_string()),
        ];
        // Non-numeric arrays are rejected
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_mean_basic() {
        let arr: Array = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
            Dynamic::from(5i64),
        ];
        assert_eq!(mean_array(arr).unwrap(), 3.0);
    }

    #[test]
    fn test_mean_floats() {
        let arr: Array = vec![
            Dynamic::from(10.0f64),
            Dynamic::from(20.0f64),
            Dynamic::from(30.0f64),
        ];
        assert_eq!(mean_array(arr).unwrap(), 20.0);
    }

    #[test]
    fn test_mean_mixed_numeric() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from(20.0f64),
            Dynamic::from(30i64),
        ];
        assert_eq!(mean_array(arr).unwrap(), 20.0);
    }

    #[test]
    fn test_mean_mixed_types_rejected() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from(true),
            Dynamic::from(30i64),
            Dynamic::from(false),
        ];
        // Mixed types (numbers + booleans) are rejected
        assert!(mean_array(arr).is_err());
    }

    #[test]
    fn test_mean_mixed_numbers_strings_rejected() {
        let arr: Array = vec![
            Dynamic::from(10i64),
            Dynamic::from("not a number".to_string()),
            Dynamic::from(20i64),
            Dynamic::from(30i64),
        ];
        // Mixed types (numbers + strings) are rejected
        assert!(mean_array(arr).is_err());
    }

    #[test]
    fn test_mean_empty_array_error() {
        let arr: Array = vec![];
        assert!(mean_array(arr).is_err());
    }

    #[test]
    fn test_mean_no_numeric_values_error() {
        let arr: Array = vec![
            Dynamic::from("abc".to_string()),
            Dynamic::from("def".to_string()),
        ];
        // Non-numeric arrays are rejected
        assert!(mean_array(arr).is_err());
    }

    #[test]
    fn test_variance_basic() {
        let arr: Array = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
            Dynamic::from(5i64),
        ];
        assert_eq!(variance_array(arr).unwrap(), 2.0);
    }

    #[test]
    fn test_variance_no_variation() {
        let arr: Array = vec![
            Dynamic::from(5i64),
            Dynamic::from(5i64),
            Dynamic::from(5i64),
            Dynamic::from(5i64),
        ];
        assert_eq!(variance_array(arr).unwrap(), 0.0);
    }

    #[test]
    fn test_variance_floats() {
        let arr: Array = vec![
            Dynamic::from(2.0f64),
            Dynamic::from(4.0f64),
            Dynamic::from(6.0f64),
        ];
        // Mean = 4.0, variance = ((2-4)^2 + (4-4)^2 + (6-4)^2) / 3 = (4 + 0 + 4) / 3 = 8/3
        let result = variance_array(arr).unwrap();
        assert!((result - 8.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_variance_empty_array_error() {
        let arr: Array = vec![];
        assert!(variance_array(arr).is_err());
    }

    #[test]
    fn test_variance_no_numeric_values_error() {
        let arr: Array = vec![Dynamic::from("abc".to_string())];
        assert!(variance_array(arr).is_err());
    }

    #[test]
    fn test_stddev_basic() {
        let arr: Array = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
            Dynamic::from(4i64),
            Dynamic::from(5i64),
        ];
        // Variance = 2.0, stddev = sqrt(2.0) ≈ 1.414
        let result = stddev_array(arr).unwrap();
        assert!((result - 2.0f64.sqrt()).abs() < 1e-10);
    }

    #[test]
    fn test_stddev_no_variation() {
        let arr: Array = vec![
            Dynamic::from(5i64),
            Dynamic::from(5i64),
            Dynamic::from(5i64),
        ];
        assert_eq!(stddev_array(arr).unwrap(), 0.0);
    }

    #[test]
    fn test_stddev_floats() {
        let arr: Array = vec![
            Dynamic::from(10.0f64),
            Dynamic::from(20.0f64),
            Dynamic::from(30.0f64),
        ];
        // Mean = 20.0, variance = ((10-20)^2 + (20-20)^2 + (30-20)^2) / 3 = 200/3
        // stddev = sqrt(200/3) ≈ 8.165
        let result = stddev_array(arr).unwrap();
        assert!((result - (200.0f64 / 3.0).sqrt()).abs() < 1e-10);
    }

    #[test]
    fn test_stddev_empty_array_error() {
        let arr: Array = vec![];
        assert!(stddev_array(arr).is_err());
    }

    #[test]
    fn test_stddev_no_numeric_values_error() {
        let arr: Array = vec![Dynamic::from("abc".to_string())];
        assert!(stddev_array(arr).is_err());
    }

    #[test]
    fn test_sum_numeric_strings_rejected() {
        let arr: Array = vec![
            Dynamic::from("10".to_string()),
            Dynamic::from("20".to_string()),
            Dynamic::from("30".to_string()),
        ];
        // Numeric strings are not parsed as numbers
        assert!(sum_array(arr).is_unit());
    }

    #[test]
    fn test_mean_numeric_strings_rejected() {
        let arr: Array = vec![
            Dynamic::from("10".to_string()),
            Dynamic::from("20".to_string()),
            Dynamic::from("30".to_string()),
        ];
        // Numeric strings are not parsed as numbers
        assert!(mean_array(arr).is_err());
    }
}
