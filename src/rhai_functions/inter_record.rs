use crate::event::Event;
use crate::rhai_functions::strings::is_parallel_mode;
use rhai::{Dynamic, Engine, EvalAltResult, Position};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

const MAX_LAG: i64 = 10_000;

#[derive(Default, Clone)]
struct InterRecordState {
    current_event: Option<HashMap<String, Dynamic>>,
    history: VecDeque<HashMap<String, Dynamic>>,
    ewma: HashMap<String, f64>,
}

thread_local! {
    static INTER_RECORD_STATE: RefCell<InterRecordState> = RefCell::new(InterRecordState::default());
}

fn parallel_mode_error(function_name: &str) -> Box<EvalAltResult> {
    format!(
        "'{}' is not available in --parallel mode. Rerun without --parallel; inter-record helpers are sequential-only.",
        function_name
    )
    .into()
}

fn validate_lag_n(function_name: &str, n: i64) -> Result<usize, Box<EvalAltResult>> {
    if n < 1 {
        return Err(format!("{}: n must be >= 1; got {}", function_name, n).into());
    }
    if n > MAX_LAG {
        return Err(format!("{}: n must be <= {}; got {}", function_name, MAX_LAG, n).into());
    }
    Ok(n as usize)
}

fn get_lagged_value(field: &str, n: usize) -> Option<Dynamic> {
    INTER_RECORD_STATE.with(|state| {
        let state = state.borrow();
        state
            .history
            .iter()
            .nth_back(n - 1)
            .and_then(|event| event.get(field).cloned())
    })
}

fn current_value(field: &str) -> Option<Dynamic> {
    INTER_RECORD_STATE.with(|state| {
        let state = state.borrow();
        state
            .current_event
            .as_ref()
            .and_then(|event| event.get(field).cloned())
    })
}

fn dynamic_to_f64(value: &Dynamic) -> Option<f64> {
    if value.is::<i64>() {
        return value.clone().as_int().ok().map(|v| v as f64);
    }
    if value.is::<f64>() {
        return value.clone().as_float().ok();
    }
    None
}

fn lag_impl(
    field: &str,
    n: i64,
    function_name: &str,
    strict: bool,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if is_parallel_mode() {
        return Err(parallel_mode_error(function_name));
    }

    let n = validate_lag_n(function_name, n)?;
    match get_lagged_value(field, n) {
        Some(value) => Ok(value),
        None if strict => Err(format!(
            "{}(\"{}\", {}): insufficient history or missing field; expected a value {} record(s) back",
            function_name, field, n, n
        )
        .into()),
        None => Ok(Dynamic::UNIT),
    }
}

fn delta_impl(
    field: &str,
    n: i64,
    function_name: &str,
    strict: bool,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if is_parallel_mode() {
        return Err(parallel_mode_error(function_name));
    }

    let n = validate_lag_n(function_name, n)?;
    let current = current_value(field);
    let lagged = get_lagged_value(field, n);

    let Some(current) = current else {
        if strict {
            return Err(format!(
                "{}(\"{}\", {}): missing current field value",
                function_name, field, n
            )
            .into());
        }
        return Ok(Dynamic::UNIT);
    };

    let Some(lagged) = lagged else {
        if strict {
            return Err(format!(
                "{}(\"{}\", {}): insufficient history or missing lagged field value",
                function_name, field, n
            )
            .into());
        }
        return Ok(Dynamic::UNIT);
    };

    let Some(current_num) = dynamic_to_f64(&current) else {
        if strict {
            return Err(format!(
                "{}(\"{}\", {}): expected numeric current and lagged values; got current type={} value={:?}",
                function_name,
                field,
                n,
                current.type_name(),
                current
            )
            .into());
        }
        return Ok(Dynamic::UNIT);
    };

    let Some(lagged_num) = dynamic_to_f64(&lagged) else {
        if strict {
            return Err(format!(
                "{}(\"{}\", {}): expected numeric current and lagged values; got lagged type={} value={:?}",
                function_name,
                field,
                n,
                lagged.type_name(),
                lagged
            )
            .into());
        }
        return Ok(Dynamic::UNIT);
    };

    Ok(Dynamic::from(current_num - lagged_num))
}

fn ewma_impl(
    key: &str,
    value: f64,
    alpha: f64,
    function_name: &str,
) -> Result<f64, Box<EvalAltResult>> {
    if is_parallel_mode() {
        return Err(parallel_mode_error(function_name));
    }

    if !(alpha > 0.0 && alpha <= 1.0) {
        return Err(format!(
            "{}(\"{}\", {}, {}): alpha must be in (0, 1]",
            function_name, key, value, alpha
        )
        .into());
    }

    INTER_RECORD_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let next = match state.ewma.get(key) {
            Some(prev) => alpha * value + (1.0 - alpha) * *prev,
            None => value,
        };
        state.ewma.insert(key.to_string(), next);
        Ok(next)
    })
}

pub fn set_current_event(event: &Event) {
    INTER_RECORD_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.current_event = Some(
            event
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
    });
}

pub fn commit_current_event() {
    INTER_RECORD_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(current) = state.current_event.take() {
            state.history.push_back(current);
            let keep = MAX_LAG as usize;
            while state.history.len() > keep {
                state.history.pop_front();
            }
        }
    });
}

pub fn clear_current_event() {
    INTER_RECORD_STATE.with(|state| {
        state.borrow_mut().current_event = None;
    });
}

pub fn reset_state() {
    INTER_RECORD_STATE.with(|state| {
        *state.borrow_mut() = InterRecordState::default();
    });
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn(
        "prev",
        |field: &str| -> Result<Dynamic, Box<EvalAltResult>> { lag_impl(field, 1, "prev", false) },
    );
    engine.register_fn(
        "lag",
        |field: &str, n: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            lag_impl(field, n, "lag", false)
        },
    );

    engine.register_fn(
        "delta",
        |field: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            delta_impl(field, 1, "delta", false)
        },
    );
    engine.register_fn(
        "delta",
        |field: &str, n: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            delta_impl(field, n, "delta", false)
        },
    );

    engine.register_fn(
        "prev_strict",
        |field: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            lag_impl(field, 1, "prev_strict", true)
        },
    );
    engine.register_fn(
        "lag_strict",
        |field: &str, n: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            lag_impl(field, n, "lag_strict", true)
        },
    );

    engine.register_fn(
        "delta_strict",
        |field: &str| -> Result<f64, Box<EvalAltResult>> {
            delta_impl(field, 1, "delta_strict", true)?
                .as_float()
                .map_err(|_| {
                    EvalAltResult::ErrorRuntime(
                        "delta_strict internal type conversion error".into(),
                        Position::NONE,
                    )
                    .into()
                })
        },
    );
    engine.register_fn(
        "delta_strict",
        |field: &str, n: i64| -> Result<f64, Box<EvalAltResult>> {
            delta_impl(field, n, "delta_strict", true)?
                .as_float()
                .map_err(|_| {
                    EvalAltResult::ErrorRuntime(
                        "delta_strict internal type conversion error".into(),
                        Position::NONE,
                    )
                    .into()
                })
        },
    );

    engine.register_fn(
        "ewma",
        |key: &str, value: f64, alpha: f64| -> Result<f64, Box<EvalAltResult>> {
            ewma_impl(key, value, alpha, "ewma")
        },
    );
    engine.register_fn(
        "ewma_strict",
        |key: &str, value: f64, alpha: f64| -> Result<f64, Box<EvalAltResult>> {
            ewma_impl(key, value, alpha, "ewma_strict")
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Dynamic;

    fn event_with_field(name: &str, value: Dynamic) -> Event {
        let mut event = Event::default_with_line("line".to_string());
        event.set_field(name.to_string(), value);
        event
    }

    #[test]
    fn prev_and_lag_follow_history() {
        reset_state();

        let e1 = event_with_field("value", Dynamic::from(10_i64));
        set_current_event(&e1);
        assert!(
            lag_impl("value", 1, "lag", false).unwrap().is::<()>(),
            "first lag should be ()"
        );
        commit_current_event();

        let e2 = event_with_field("value", Dynamic::from(16_i64));
        set_current_event(&e2);
        assert_eq!(
            lag_impl("value", 1, "lag", false)
                .unwrap()
                .as_int()
                .unwrap(),
            10
        );
        commit_current_event();

        let e3 = event_with_field("value", Dynamic::from(21_i64));
        set_current_event(&e3);
        assert_eq!(
            lag_impl("value", 2, "lag", false)
                .unwrap()
                .as_int()
                .unwrap(),
            10
        );
    }

    #[test]
    fn delta_requires_native_numeric_values() {
        reset_state();
        let e1 = event_with_field("duration", Dynamic::from(100_i64));
        set_current_event(&e1);
        commit_current_event();

        let e2 = event_with_field("duration", Dynamic::from("120"));
        set_current_event(&e2);

        assert!(delta_impl("duration", 1, "delta", false)
            .unwrap()
            .is::<()>());
        assert!(delta_impl("duration", 1, "delta_strict", true).is_err());
    }

    #[test]
    fn ewma_initializes_and_updates() {
        reset_state();
        let first = ewma_impl("lat", 100.0, 0.2, "ewma").unwrap();
        assert_eq!(first, 100.0);
        let second = ewma_impl("lat", 200.0, 0.2, "ewma").unwrap();
        assert!((second - 120.0).abs() < 0.000001);
    }

    #[test]
    fn alpha_and_lag_contracts_are_enforced() {
        reset_state();
        assert!(ewma_impl("lat", 1.0, 0.0, "ewma").is_err());
        assert!(lag_impl("x", 0, "lag", false).is_err());
        assert!(lag_impl("x", MAX_LAG + 1, "lag", false).is_err());
    }
}
