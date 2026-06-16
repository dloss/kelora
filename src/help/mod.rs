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

use crate::cli::Cli;
use crate::rhai_functions;
use clap::CommandFactory;

/// Print available Rhai functions help.
///
/// With `filter` set, only matching sections/functions are shown (a
/// smartcase keyword search: case-insensitive unless the keyword contains an
/// uppercase letter); otherwise the full catalogue is printed.
pub fn print_functions_help(filter: Option<&str>) {
    match filter {
        None => {
            let help_text = rhai_functions::docs::generate_help_text();
            println!("{}", help_text);
        }
        Some(keyword) => {
            let filtered = rhai_functions::docs::filter_help_text(keyword);
            if filtered.trim().is_empty() {
                println!(
                    "No functions matching \"{keyword}\". Run --help-functions for the full catalogue."
                );
            } else {
                println!("Functions matching \"{keyword}\":\n{filtered}");
            }
        }
    }
}

/// Print the full CLI reference filtered to entries matching `keyword`.
///
/// This is the search counterpart to plain `--help`: bare `--help` is left to
/// clap's renderer, while `--help=KEYWORD` (or `--help KEYWORD`) lands here.
/// Two kinds of search are supported:
/// - A bare word (`--help since`) is a smartcase substring search over flags and
///   descriptions (case-insensitive unless the keyword has an uppercase letter).
/// - A keyword that begins with `-` (`--help -j`, `--help --since`) is treated as
///   a flag query: a case-sensitive, whole-token match against each option's
///   declaration line, so `-j` finds only `-j` — not `-J` or `--multiline-join`.
pub fn print_cli_help_filtered(keyword: &str) {
    let full = Cli::command().render_long_help().to_string();
    let filtered = filter_cli_help(&full, keyword);
    if filtered.trim().is_empty() {
        println!(
            "No options matching \"{keyword}\". Run --help for the full reference \
             (search a flag like '-j' or '--since', or a bare word like 'time')."
        );
    } else {
        println!("Options matching \"{keyword}\":\n{filtered}");
    }
}

/// Filter clap's long help text down to the option entries that match `keyword`,
/// keeping each matched entry's section heading for context.
///
/// clap lays the long help out as column-0 section headers ending in `:`,
/// option entries indented two spaces when they have a short alias (`-K, --…`)
/// or six when long-only (`--since`), and continuation lines (descriptions,
/// `[default: ...]`, possible values) indented ten or more. An entry runs until
/// the next entry or section header, so blank lines inside an entry are kept.
fn filter_cli_help(full: &str, keyword: &str) -> String {
    // A keyword starting with '-' is a flag query: short options are
    // case-sensitive (`-j` != `-J`) and so short that a loose substring match
    // would be useless, so match the whole flag token on the declaration line.
    let flag_query = keyword.starts_with('-');

    // Smartcase substring search for bare-word queries: a lowercase keyword
    // matches any case; any uppercase letter makes the search case-sensitive.
    let case_sensitive = keyword.chars().any(|c| c.is_uppercase());
    let needle = if case_sensitive {
        keyword.to_string()
    } else {
        keyword.to_lowercase()
    };
    let contains = |haystack: &str| -> bool {
        if case_sensitive {
            haystack.contains(&needle)
        } else {
            haystack.to_lowercase().contains(&needle)
        }
    };

    let lines: Vec<&str> = full.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let is_header =
        |line: &str| !line.is_empty() && indent(line) == 0 && line.trim_end().ends_with(':');
    // Flag lines sit at indent 2 (short alias) or 6 (long-only) and begin with
    // a dash; positional args sit at indent 2 and begin with '['. Descriptions
    // and value lists are indented further, so the indent < 8 guard keeps a
    // description that happens to start with '-' from looking like an entry.
    let is_entry_start = |line: &str| {
        let trimmed = line.trim_start();
        indent(line) < 8 && (trimmed.starts_with('-') || trimmed.starts_with('['))
    };

    let mut out = String::new();
    let mut current_section: Option<&str> = None;
    let mut section_printed = false;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        if is_header(line) {
            current_section = Some(line);
            section_printed = false;
            i += 1;
            continue;
        }

        if !is_entry_start(line) {
            // Preamble text and the usage line live above the first section.
            i += 1;
            continue;
        }

        // Gather the entry: its start line plus everything (including blank
        // lines) up to the next entry start or section header.
        let entry_start = i;
        i += 1;
        while i < lines.len() && !is_header(lines[i]) && !is_entry_start(lines[i]) {
            i += 1;
        }
        let entry = &lines[entry_start..i];

        // For a flag query, only the declaration line (entry[0]) carries the
        // flag tokens, and a section heading never does; for a word query, any
        // line of the entry — or the heading — may match.
        let (header_matches, entry_matches) = if flag_query {
            (false, flag_token_match(entry[0], keyword))
        } else {
            let header_matches = matches!(current_section, Some(h) if contains(h));
            (header_matches, entry.iter().any(|l| contains(l)))
        };

        if header_matches || entry_matches {
            if !section_printed {
                if let Some(sec) = current_section {
                    out.push('\n');
                    out.push_str(sec);
                    out.push('\n');
                }
                section_printed = true;
            }
            for l in entry {
                out.push_str(l);
                out.push('\n');
            }
        }
    }

    out
}

/// Case-sensitive, whole-token match of a flag (e.g. `-j` or `--since`) against
/// an option's declaration line (e.g. `  -K, --exclude-keys <EXCLUDE_KEYS>`).
///
/// The token must be bounded by the start of the line or a space on the left and
/// by the end of the line, a space, or a comma on the right. This keeps `-j`
/// from matching the `-j` buried in `--multiline-join`, and keeps `-f` from
/// matching the second dash in `--file-order`.
fn flag_token_match(line: &str, flag: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find(flag) {
        let start = from + rel;
        let end = start + flag.len();
        let left_ok = start == 0 || bytes[start - 1] == b' ';
        let right_ok = end == bytes.len() || matches!(bytes[end], b' ' | b',');
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Print practical Rhai examples
pub fn print_examples_help() {
    let help_text = rhai_functions::docs::generate_examples_text();
    println!("{}", help_text);
}
