use rhai::{Array, Dynamic, Engine, Map};
use std::cell::{Cell, RefCell};

// Thread-local storage for deferred event emission
thread_local! {
    static PENDING_EMISSIONS: RefCell<Vec<Map>> = const { RefCell::new(Vec::new()) };
    static SUPPRESS_CURRENT: Cell<bool> = const { Cell::new(false) };
    static EMIT_STRICT: Cell<bool> = const { Cell::new(false) };
    static EMIT_USE_EMOJI: Cell<bool> = const { Cell::new(true) };
    // When set, emit_each() is rejected with an error naming the stage. The
    // begin/end stages run outside the per-event loop that materializes
    // pending emissions, so events emitted there would be silently dropped
    // (or, before this guard, picked up only as an accidental side effect of
    // the first event). Disallow it outright rather than emit confusingly.
    static EMIT_DISALLOWED_STAGE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Mark emit_each() as disallowed for the current stage (e.g. "--begin").
/// Pass `None` to re-enable it before the per-event loop runs.
pub fn set_emit_disallowed(stage: Option<&'static str>) {
    EMIT_DISALLOWED_STAGE.with(|s| s.set(stage));
}

/// Get and clear pending emissions for this thread
pub fn get_and_clear_pending_emissions() -> Vec<Map> {
    PENDING_EMISSIONS.with(|emissions| {
        let mut vec = emissions.borrow_mut();
        let result = vec.clone();
        vec.clear();
        result
    })
}

/// Check if current event should be suppressed
pub fn should_suppress_current_event() -> bool {
    SUPPRESS_CURRENT.with(|suppress| suppress.get())
}

/// Clear suppression flag
pub fn clear_suppression_flag() {
    SUPPRESS_CURRENT.with(|suppress| suppress.set(false));
}

/// Configure strictness for emit_each error handling
pub fn set_emit_strict(strict: bool) {
    EMIT_STRICT.with(|flag| flag.set(strict));
}

fn is_emit_strict() -> bool {
    EMIT_STRICT.with(|flag| flag.get())
}

/// Configure whether emit_each diagnostics use emoji prefixes
pub fn set_emit_use_emoji(use_emoji: bool) {
    EMIT_USE_EMOJI.with(|flag| flag.set(use_emoji));
}

/// Warning prefix honoring the active emoji setting, matching Config::format_warning_message
fn emit_warn_prefix() -> &'static str {
    if EMIT_USE_EMOJI.with(|flag| flag.get()) {
        "🔸"
    } else {
        "kelora warning:"
    }
}

/// Rhai function: emit_each(items: array<map>) -> int
/// Fan out an array of event maps into individual events and suppress the original.
pub fn emit_each_single(items: Dynamic) -> Result<Dynamic, Box<rhai::EvalAltResult>> {
    emit_each_impl(items, Dynamic::UNIT)
}

/// Rhai function: emit_each(items: array<map>, base: map) -> int
/// Fan out an array of event maps into individual events with base defaults and suppress the original.
pub fn emit_each_with_base(
    items: Dynamic,
    base: Dynamic,
) -> Result<Dynamic, Box<rhai::EvalAltResult>> {
    emit_each_impl(items, base)
}

/// Core implementation for emit_each functionality
fn emit_each_impl(
    items_val: Dynamic,
    base_val: Dynamic,
) -> Result<Dynamic, Box<rhai::EvalAltResult>> {
    // Reject before any mutation so the pending buffer / suppress flag stay clean.
    if let Some(stage) = EMIT_DISALLOWED_STAGE.with(|s| s.get()) {
        return Err(format!(
            "emit_each() is not available in the {stage} stage; \
             it only works in a per-event stage (-e/--exec or --filter). \
             The {stage} stage runs outside the event loop, so emitted events \
             cannot be materialized there."
        )
        .into());
    }

    let strict = is_emit_strict();

    // Validate and extract items array
    let items = match items_val.clone().try_cast::<Array>() {
        Some(array) => array,
        None => {
            if strict {
                return Err(format!(
                    "emit_each(): items must be array<map>, got {}",
                    items_val.type_name()
                )
                .into());
            } else {
                // Log warning in resilient mode
                eprintln!(
                    "{} emit_each(): items must be array<map>; got {}; returning 0",
                    emit_warn_prefix(),
                    items_val.type_name()
                );
                return Ok(Dynamic::from(0i64));
            }
        }
    };

    // Validate and extract optional base map
    let base = if base_val.is_unit() {
        None
    } else {
        match base_val.clone().try_cast::<Map>() {
            Some(map) => Some(map),
            None => {
                if strict {
                    return Err(format!(
                        "emit_each(): base must be map, got {}",
                        base_val.type_name()
                    )
                    .into());
                } else {
                    eprintln!(
                        "{} emit_each(): base must be map; got {}; treating as empty",
                        emit_warn_prefix(),
                        base_val.type_name()
                    );
                    None
                }
            }
        }
    };

    let mut emitted = 0i64;

    // Mark current event for suppression
    SUPPRESS_CURRENT.with(|suppress| suppress.set(true));

    // Process each item in the array
    for (i, item) in items.iter().enumerate() {
        let item_map = match item.clone().try_cast::<Map>() {
            Some(map) => map,
            None => {
                if strict {
                    return Err(format!(
                        "emit_each(): items[{}] is not a map (got {})",
                        i,
                        item.type_name()
                    )
                    .into());
                } else {
                    eprintln!(
                        "{} emit_each(): skipping items[{}], expected map (got {})",
                        emit_warn_prefix(),
                        i,
                        item.type_name()
                    );
                    continue;
                }
            }
        };

        // Create the event with shallow merge: base defaults then item overrides
        let event_map = match &base {
            Some(base_map) => {
                let mut result = base_map.clone();
                for (key, value) in item_map {
                    result.insert(key, value);
                }
                result
            }
            None => item_map,
        };

        // Add to pending emissions
        PENDING_EMISSIONS.with(|emissions| {
            emissions.borrow_mut().push(event_map);
        });

        emitted += 1;
    }

    Ok(Dynamic::from(emitted))
}

/// Register emit functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("emit_each", emit_each_single);
    engine.register_fn("emit_each", emit_each_with_base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Array, Engine, Map};

    fn setup_test() {
        // Clear thread-local state
        PENDING_EMISSIONS.with(|emissions| emissions.borrow_mut().clear());
        SUPPRESS_CURRENT.with(|suppress| suppress.set(false));
    }

    #[test]
    fn test_emit_each_empty_array() {
        setup_test();

        let empty_array = Dynamic::from(Array::new());
        let result = emit_each_single(empty_array).unwrap();

        assert_eq!(result.as_int().unwrap(), 0);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 0);
    }

    #[test]
    fn test_emit_each_basic_array() {
        setup_test();

        // Create array with two maps
        let mut map1 = Map::new();
        map1.insert("name".into(), Dynamic::from("alice"));
        map1.insert("age".into(), Dynamic::from(25i64));

        let mut map2 = Map::new();
        map2.insert("name".into(), Dynamic::from("bob"));
        map2.insert("age".into(), Dynamic::from(30i64));

        let array = vec![Dynamic::from(map1), Dynamic::from(map2)];
        let items = Dynamic::from(array);

        let result = emit_each_single(items).unwrap();

        assert_eq!(result.as_int().unwrap(), 2);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 2);

        // Verify first emission
        let first = &emissions[0];
        assert_eq!(
            first.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );
        assert_eq!(first.get("age").unwrap().as_int().unwrap(), 25);

        // Verify second emission
        let second = &emissions[1];
        assert_eq!(
            second.get("name").unwrap().clone().into_string().unwrap(),
            "bob"
        );
        assert_eq!(second.get("age").unwrap().as_int().unwrap(), 30);
    }

    #[test]
    fn test_emit_each_with_base() {
        setup_test();

        // Create base map
        let mut base = Map::new();
        base.insert("host".into(), Dynamic::from("server1"));
        base.insert("app".into(), Dynamic::from("myapp"));

        // Create array with maps that override some base fields
        let mut item1 = Map::new();
        item1.insert("id".into(), Dynamic::from(1i64));
        item1.insert("app".into(), Dynamic::from("override_app")); // Override base

        let mut item2 = Map::new();
        item2.insert("id".into(), Dynamic::from(2i64));
        // This one inherits app from base

        let array = vec![Dynamic::from(item1), Dynamic::from(item2)];
        let items = Dynamic::from(array);
        let base_dynamic = Dynamic::from(base);

        let result = emit_each_with_base(items, base_dynamic).unwrap();

        assert_eq!(result.as_int().unwrap(), 2);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 2);

        // Verify first emission (with override)
        let first = &emissions[0];
        assert_eq!(
            first.get("host").unwrap().clone().into_string().unwrap(),
            "server1"
        );
        assert_eq!(
            first.get("app").unwrap().clone().into_string().unwrap(),
            "override_app"
        );
        assert_eq!(first.get("id").unwrap().as_int().unwrap(), 1);

        // Verify second emission (inherits base app)
        let second = &emissions[1];
        assert_eq!(
            second.get("host").unwrap().clone().into_string().unwrap(),
            "server1"
        );
        assert_eq!(
            second.get("app").unwrap().clone().into_string().unwrap(),
            "myapp"
        );
        assert_eq!(second.get("id").unwrap().as_int().unwrap(), 2);
    }

    #[test]
    fn test_emit_each_invalid_items_type() {
        setup_test();

        let not_array = Dynamic::from("not an array");
        let result = emit_each_single(not_array).unwrap();

        // Should return 0 in resilient mode
        assert_eq!(result.as_int().unwrap(), 0);
        assert!(!should_suppress_current_event()); // No suppression on error

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 0);
    }

    #[test]
    fn test_emit_each_invalid_base_type() {
        setup_test();

        let mut map1 = Map::new();
        map1.insert("name".into(), Dynamic::from("alice"));

        let array = vec![Dynamic::from(map1)];
        let items = Dynamic::from(array);
        let invalid_base = Dynamic::from("not a map");

        let result = emit_each_with_base(items, invalid_base).unwrap();

        // Should still emit 1 event (ignoring invalid base)
        assert_eq!(result.as_int().unwrap(), 1);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 1);
        assert_eq!(
            emissions[0]
                .get("name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice"
        );
    }

    #[test]
    fn test_emit_each_mixed_array_resilient() {
        setup_test();

        let mut valid_map = Map::new();
        valid_map.insert("valid".into(), Dynamic::from(true));

        let array = vec![
            Dynamic::from(valid_map),
            Dynamic::from("not a map"),
            Dynamic::from(42i64),
        ];
        let items = Dynamic::from(array);

        let result = emit_each_single(items).unwrap();

        // Should emit 1 event (skip the invalid ones)
        assert_eq!(result.as_int().unwrap(), 1);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 1);
        assert!(emissions[0].get("valid").unwrap().as_bool().unwrap());
    }

    #[test]
    fn test_emit_each_empty_maps() {
        setup_test();

        let empty_map = Map::new();
        let array = vec![Dynamic::from(empty_map)];
        let items = Dynamic::from(array);

        let result = emit_each_single(items).unwrap();

        assert_eq!(result.as_int().unwrap(), 1);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 1);
        assert!(emissions[0].is_empty()); // Empty map should emit empty event
    }

    #[test]
    fn test_emit_each_rhai_integration() {
        setup_test();

        let mut engine = Engine::new();
        register_functions(&mut engine);

        // Test single parameter version
        let result = engine
            .eval::<i64>(
                r#"
            let items = [#{name: "alice"}, #{name: "bob"}];
            emit_each(items)
        "#,
            )
            .unwrap();

        assert_eq!(result, 2);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 2);

        // Clear state for next test
        clear_suppression_flag();

        // Test two parameter version
        let result = engine
            .eval::<i64>(
                r#"
            let items = [#{id: 1}, #{id: 2}];
            let base = #{host: "server1"};
            emit_each(items, base)
        "#,
            )
            .unwrap();

        assert_eq!(result, 2);
        assert!(should_suppress_current_event());

        let emissions = get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 2);

        // Both emissions should have host field from base
        assert_eq!(
            emissions[0]
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server1"
        );
        assert_eq!(
            emissions[1]
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server1"
        );

        // And their respective id values
        assert_eq!(emissions[0].get("id").unwrap().as_int().unwrap(), 1);
        assert_eq!(emissions[1].get("id").unwrap().as_int().unwrap(), 2);
    }
}
