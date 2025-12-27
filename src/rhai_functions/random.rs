use rhai::{Engine, EvalAltResult};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref RNG: Mutex<fastrand::Rng> = Mutex::new(fastrand::Rng::new());
}

// Thread-local counters for sample_every() - each N value gets its own counter
thread_local! {
    static SAMPLE_COUNTERS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
}

fn rand_float() -> Result<f64, Box<EvalAltResult>> {
    let mut rng = RNG.lock().unwrap();
    Ok(rng.f64())
}

fn rand_int_range(min: i64, max: i64) -> Result<i64, Box<EvalAltResult>> {
    if min > max {
        return Err(format!(
            "rand_int: min ({}) cannot be greater than max ({})",
            min, max
        )
        .into());
    }

    let mut rng = RNG.lock().unwrap();
    Ok(rng.i64(min..=max))
}

/// Sample every Nth event - returns true on calls N, 2N, 3N, etc.
/// Each unique N value maintains its own counter (thread-local).
/// This provides fast approximate sampling without hashing.
/// For deterministic sampling across parallel threads, use bucket() instead.
fn sample_every(n: i64) -> Result<bool, Box<EvalAltResult>> {
    if n <= 0 {
        return Err(format!("sample_every: n must be positive, got {}", n).into());
    }

    SAMPLE_COUNTERS.with(|counters| {
        let mut map = counters.borrow_mut();
        let counter = map.entry(n).or_insert(0);
        *counter += 1;

        if *counter >= n {
            *counter = 0;
            Ok(true)
        } else {
            Ok(false)
        }
    })
}

/// Clear sample_every counters (for testing)
#[cfg(test)]
pub fn clear_sample_counters() {
    SAMPLE_COUNTERS.with(|counters| {
        counters.borrow_mut().clear();
    });
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("rand", rand_float);
    engine.register_fn("rand_int", rand_int_range);
    engine.register_fn("sample_every", sample_every);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rand_float() {
        for _ in 0..100 {
            let val = rand_float().unwrap();
            assert!(
                (0.0..1.0).contains(&val),
                "rand() should return value in [0.0, 1.0), got {}",
                val
            );
        }
    }

    #[test]
    fn test_rand_int_range() {
        // Test basic range
        for _ in 0..100 {
            let val = rand_int_range(1, 10).unwrap();
            assert!(
                (1..=10).contains(&val),
                "rand_int(1, 10) should return value in [1, 10], got {}",
                val
            );
        }

        // Test single value range
        let val = rand_int_range(5, 5).unwrap();
        assert_eq!(val, 5);

        // Test negative range
        for _ in 0..100 {
            let val = rand_int_range(-10, -1).unwrap();
            assert!(
                (-10..=-1).contains(&val),
                "rand_int(-10, -1) should return value in [-10, -1], got {}",
                val
            );
        }
    }

    #[test]
    fn test_rand_int_invalid_range() {
        let result = rand_int_range(10, 5);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("min (10) cannot be greater than max (5)"));
    }

    #[test]
    fn test_sample_every_basic() {
        clear_sample_counters();

        // sample_every(3) should return true on calls 3, 6, 9, etc.
        assert!(!sample_every(3).unwrap()); // Call 1
        assert!(!sample_every(3).unwrap()); // Call 2
        assert!(sample_every(3).unwrap()); // Call 3
        assert!(!sample_every(3).unwrap()); // Call 4
        assert!(!sample_every(3).unwrap()); // Call 5
        assert!(sample_every(3).unwrap()); // Call 6
    }

    #[test]
    fn test_sample_every_n_equals_1() {
        clear_sample_counters();

        // sample_every(1) should return true on every call
        assert!(sample_every(1).unwrap());
        assert!(sample_every(1).unwrap());
        assert!(sample_every(1).unwrap());
    }

    #[test]
    fn test_sample_every_independent_counters() {
        clear_sample_counters();

        // Different N values should have independent counters
        assert!(!sample_every(2).unwrap()); // 2: call 1
        assert!(!sample_every(3).unwrap()); // 3: call 1
        assert!(sample_every(2).unwrap()); // 2: call 2 -> true
        assert!(!sample_every(3).unwrap()); // 3: call 2
        assert!(!sample_every(2).unwrap()); // 2: call 3
        assert!(sample_every(3).unwrap()); // 3: call 3 -> true
    }

    #[test]
    fn test_sample_every_invalid_n() {
        clear_sample_counters();

        // n = 0 should error
        let result = sample_every(0);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("n must be positive"));

        // n < 0 should error
        let result = sample_every(-5);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("n must be positive"));
    }

    #[test]
    fn test_sample_every_large_n() {
        clear_sample_counters();

        // Test with larger N value
        for i in 1..=100 {
            let result = sample_every(100).unwrap();
            if i == 100 {
                assert!(result, "Should return true on 100th call");
            } else {
                assert!(!result, "Should return false on call {}", i);
            }
        }
    }

    #[test]
    fn test_sample_every_with_rhai() {
        clear_sample_counters();

        let mut engine = Engine::new();
        register_functions(&mut engine);

        // Test basic sampling
        let result: bool = engine.eval("sample_every(2)").unwrap();
        assert!(!result);

        let result: bool = engine.eval("sample_every(2)").unwrap();
        assert!(result);

        // Test error handling
        let result: Result<bool, _> = engine.eval("sample_every(0)");
        assert!(result.is_err());

        let result: Result<bool, _> = engine.eval("sample_every(-1)");
        assert!(result.is_err());
    }

    #[test]
    fn test_sample_every_use_case() {
        clear_sample_counters();

        // Simulate the use case: keep only every 100th event
        let mut kept = 0;
        let total = 1000;

        for _ in 0..total {
            if sample_every(100).unwrap() {
                kept += 1;
            }
        }

        assert_eq!(kept, 10, "Should keep 10 out of 1000 events (1%)");
    }
}
