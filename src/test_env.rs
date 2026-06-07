//! Shared synchronization for tests that mutate process-global environment
//! variables (e.g. `TZ`, `NO_COLOR`).
//!
//! The default test harness runs tests in parallel threads within a single
//! process. Environment variables are process-global, so tests in different
//! modules that read or write the same variable race with each other. A
//! per-module lock is not enough — only a single, crate-wide lock serializes
//! every such test. Acquire it via [`lock_env`] for the full duration of any
//! test that touches the environment.

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the crate-wide environment lock, held until the returned guard is
/// dropped. Recovers from poisoning so one failing test doesn't cascade into
/// spurious failures elsewhere.
pub fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
