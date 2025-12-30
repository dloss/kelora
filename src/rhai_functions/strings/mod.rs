//! String transformations and regex helpers for Rhai scripts.
//! Parsing helpers live in `crate::rhai_functions::parsers`.

use rhai::Engine;

mod core;
mod output;
mod regex;
mod slice;
mod substring;
mod trim;

// Re-export capture module functions for backward compatibility
#[allow(unused_imports)]
pub use crate::rhai_functions::capture::{
    capture_eprint, capture_print, capture_stderr, capture_stdout, clear_captured_eprints,
    clear_captured_prints, is_parallel_mode, is_suppress_side_effects, set_parallel_mode,
    set_suppress_side_effects, take_captured_eprints, take_captured_messages, take_captured_prints,
    CapturedMessage,
};

pub fn register_functions(engine: &mut Engine) {
    output::register_functions(engine);
    core::register_functions(engine);
    slice::register_functions(engine);
    substring::register_functions(engine);
    trim::register_functions(engine);
    regex::register_functions(engine);
}

#[cfg(test)]
mod tests;
