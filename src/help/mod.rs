//! Help text modules for CLI --help-* options
//!
//! Each submodule contains a single help text printing function
//! for a specific help topic.

mod formats;
mod multiline;
mod quick;
mod regex;
mod rhai;
mod time;

pub use formats::print_formats_help;
pub use multiline::print_multiline_help;
pub use quick::print_quick_help;
pub use regex::print_regex_help;
pub use rhai::print_rhai_help;
pub use time::print_time_format_help;

use crate::rhai_functions;

/// Print available Rhai functions help
pub fn print_functions_help() {
    let help_text = rhai_functions::docs::generate_help_text();
    println!("{}", help_text);
}

/// Print practical Rhai examples
pub fn print_examples_help() {
    let help_text = rhai_functions::docs::generate_examples_text();
    println!("{}", help_text);
}
