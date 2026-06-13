use rhai::{Engine, EvalAltResult};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Global RNG backing `rand()`, `rand_int()`, and `sample_prob()`.
///
/// Seeded from entropy by default. Set `KELORA_SEED` to a non-negative integer
/// for reproducible output (e.g. in tests or repeatable sampling). Mirrors the
/// `KELORA_SECRET` convention used for stable pseudonym hashing. Reproducibility
/// holds in sequential mode; under `--parallel`, thread scheduling still affects
/// which worker consumes which value.
static RNG: LazyLock<Mutex<fastrand::Rng>> = LazyLock::new(|| {
    let rng = match std::env::var("KELORA_SEED") {
        Ok(s) => match s.trim().parse::<u64>() {
            Ok(seed) => fastrand::Rng::with_seed(seed),
            Err(_) => {
                eprintln!("kelora: KELORA_SEED must be a non-negative integer; using random seed");
                fastrand::Rng::new()
            }
        },
        Err(_) => fastrand::Rng::new(),
    };
    Mutex::new(rng)
});

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

/// Probabilistic sampling - returns true with probability p (0.0-1.0).
///
/// Useful for "keep ~N% of events" without deterministic hashing.
/// For deterministic sampling across parallel threads, use bucket() instead.
fn sample_prob(p: f64) -> Result<bool, Box<EvalAltResult>> {
    if !(0.0..=1.0).contains(&p) {
        return Err(format!(
            "sample_prob: probability must be between 0.0 and 1.0, got {}",
            p
        )
        .into());
    }

    let mut rng = RNG.lock().unwrap();
    Ok(rng.f64() < p)
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("rand", rand_float);
    engine.register_fn("rand_int", rand_int_range);
    engine.register_fn("sample_every", sample_every);
    engine.register_fn("sample_prob", sample_prob);
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
    fn test_sample_prob_always_true() {
        // p = 1.0 should always return true
        for _ in 0..100 {
            assert!(sample_prob(1.0).unwrap());
        }
    }

    #[test]
    fn test_sample_prob_always_false() {
        // p = 0.0 should always return false
        for _ in 0..100 {
            assert!(!sample_prob(0.0).unwrap());
        }
    }

    #[test]
    fn test_sample_prob_invalid_range() {
        assert!(sample_prob(-0.1).is_err());
        assert!(sample_prob(1.1).is_err());
        assert!(sample_prob(-1.0).is_err());
        assert!(sample_prob(2.0).is_err());
    }

    #[test]
    fn test_sample_prob_approximate_rate() {
        // With p=0.5, roughly half should be true over many trials
        let mut count = 0;
        let trials = 10000;
        for _ in 0..trials {
            if sample_prob(0.5).unwrap() {
                count += 1;
            }
        }
        // Allow wide margin (40%-60%) for randomness
        assert!(
            count > 4000 && count < 6000,
            "Expected ~5000 true out of 10000, got {}",
            count
        );
    }

    #[test]
    fn test_sample_prob_with_rhai() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        // Valid probabilities
        let _: bool = engine.eval("sample_prob(0.5)").unwrap();
        let _: bool = engine.eval("sample_prob(0.0)").unwrap();
        let _: bool = engine.eval("sample_prob(1.0)").unwrap();

        // Invalid probabilities
        assert!(engine.eval::<bool>("sample_prob(-0.1)").is_err());
        assert!(engine.eval::<bool>("sample_prob(1.1)").is_err());
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
