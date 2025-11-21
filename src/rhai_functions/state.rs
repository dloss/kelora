//! Global state management for Rhai scripts
//!
//! Provides a mutable `state` map in sequential mode for tracking information
//! across events. In parallel mode, accessing state will panic with an error message.

use rhai::{Dynamic, Engine, Map};
use std::fmt;
use std::sync::{Arc, RwLock};

/// Wrapper for a shared mutable map that serves as the state store
#[derive(Debug, Clone)]
pub struct StateMap {
    inner: Arc<RwLock<Map>>,
}

impl StateMap {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Map::new())),
        }
    }
}

impl Default for StateMap {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for StateMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let map = self.inner.read().unwrap();
        write!(f, "{:?}", *map)
    }
}

/// Dummy type used in parallel mode that panics on any access
#[derive(Debug, Clone)]
pub struct StateNotAvailable;

impl fmt::Display for StateNotAvailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "state (unavailable in parallel mode)")
    }
}

/// Register state-related functions with the Rhai engine
#[allow(dependency_on_unit_never_type_fallback)]
pub fn register(engine: &mut Engine) {
    // Register StateMap type
    engine.register_type::<StateMap>();

    // Register indexer operations for StateMap
    engine
        .register_indexer_get(|state: &mut StateMap, key: &str| -> Dynamic {
            let map = state.inner.read().unwrap();
            map.get(key).cloned().unwrap_or(Dynamic::UNIT)
        })
        .register_indexer_set(|state: &mut StateMap, key: &str, value: Dynamic| {
            let mut map = state.inner.write().unwrap();
            map.insert(key.into(), value);
        });

    // Register StateMap methods
    engine
        .register_fn("contains", |state: &mut StateMap, key: &str| -> bool {
            let map = state.inner.read().unwrap();
            map.contains_key(key)
        })
        .register_fn("len", |state: &mut StateMap| -> i64 {
            let map = state.inner.read().unwrap();
            map.len() as i64
        })
        .register_fn("is_empty", |state: &mut StateMap| -> bool {
            let map = state.inner.read().unwrap();
            map.is_empty()
        })
        .register_fn("keys", |state: &mut StateMap| -> Vec<Dynamic> {
            let map = state.inner.read().unwrap();
            map.keys().map(|k| Dynamic::from(k.to_string())).collect()
        })
        .register_fn("clear", |state: &mut StateMap| {
            let mut map = state.inner.write().unwrap();
            map.clear();
        })
        .register_fn("get", |state: &mut StateMap, key: &str| -> Dynamic {
            let map = state.inner.read().unwrap();
            map.get(key).cloned().unwrap_or(Dynamic::UNIT)
        })
        .register_fn(
            "insert",
            |state: &mut StateMap, key: &str, value: Dynamic| {
                let mut map = state.inner.write().unwrap();
                map.insert(key.into(), value);
            },
        );

    // Register StateNotAvailable type
    engine.register_type::<StateNotAvailable>();

    // Register panic-inducing operations for StateNotAvailable
    engine
        .register_indexer_get(|_state: &mut StateNotAvailable, _key: &str| -> Dynamic {
            panic!("'state' is not available in --parallel mode (requires sequential processing)");
        })
        .register_indexer_set(
            |_state: &mut StateNotAvailable, _key: &str, _value: Dynamic| {
                panic!(
                    "'state' is not available in --parallel mode (requires sequential processing)"
                );
            },
        )
        .register_fn(
            "contains",
            |_state: &mut StateNotAvailable, _key: &str| -> bool {
                panic!(
                    "'state' is not available in --parallel mode (requires sequential processing)"
                );
            },
        )
        .register_fn("len", |_state: &mut StateNotAvailable| -> i64 {
            panic!("'state' is not available in --parallel mode (requires sequential processing)");
        })
        .register_fn("is_empty", |_state: &mut StateNotAvailable| -> bool {
            panic!("'state' is not available in --parallel mode (requires sequential processing)");
        })
        .register_fn("keys", |_state: &mut StateNotAvailable| -> Vec<Dynamic> {
            panic!("'state' is not available in --parallel mode (requires sequential processing)");
        })
        .register_fn("clear", |_state: &mut StateNotAvailable| {
            panic!("'state' is not available in --parallel mode (requires sequential processing)");
        })
        .register_fn(
            "get",
            |_state: &mut StateNotAvailable, _key: &str| -> Dynamic {
                panic!(
                    "'state' is not available in --parallel mode (requires sequential processing)"
                );
            },
        )
        .register_fn(
            "insert",
            |_state: &mut StateNotAvailable, _key: &str, _value: Dynamic| {
                panic!(
                    "'state' is not available in --parallel mode (requires sequential processing)"
                );
            },
        );
}
