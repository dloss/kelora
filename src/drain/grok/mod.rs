//! Grok pattern compilation for drain's value masking.
//!
//! Derived from the [`grok`](https://github.com/daschl/grok) crate 1.2.0
//! (Apache-2.0, see `LICENSE` in this directory), trimmed to what drain uses
//! and extended with [`Pattern::find_at`] and [`Pattern::alias`]. It lives in
//! the crate rather than as a `[patch.crates-io]` override because `cargo
//! publish` ignores patches: the published package silently built against
//! upstream grok, which lacks both methods.
//!
//! The bundled definitions in `patterns/` come from logstash-patterns-core
//! (Apache-2.0, see `patterns/README.md`).

use onig::{Regex, Region, SearchOptions};
use std::collections::BTreeMap;
use std::fmt;

const MAX_RECURSION: usize = 1024;

const GROK_PATTERN: &str = r"%\{(?<name>(?<pattern>[A-z0-9]+)(?::(?<alias>[A-z0-9_:;\/\s\.]+))?)(?:=(?<definition>(?:(?:[^{}]+|\.+)+)+))?\}";
const NAME_INDEX: usize = 1;
const PATTERN_INDEX: usize = 2;
const ALIAS_INDEX: usize = 3;
const DEFINITION_INDEX: usize = 4;

/// The bundled pattern files, in file-name order: a name defined in more than
/// one file keeps its last definition.
const PATTERN_FILES: &[&str] = &[
    include_str!("patterns/aws.pattern"),
    include_str!("patterns/bacula.pattern"),
    include_str!("patterns/bind.pattern"),
    include_str!("patterns/bro.pattern"),
    include_str!("patterns/exim.pattern"),
    include_str!("patterns/firewalls.pattern"),
    include_str!("patterns/grok.pattern"),
    include_str!("patterns/haproxy.pattern"),
    include_str!("patterns/httpd.pattern"),
    include_str!("patterns/java.pattern"),
    include_str!("patterns/junos.pattern"),
    include_str!("patterns/linux-syslog.pattern"),
    include_str!("patterns/maven.pattern"),
    include_str!("patterns/mcollective.pattern"),
    include_str!("patterns/mongodb.pattern"),
    include_str!("patterns/nagios.pattern"),
    include_str!("patterns/postgresql.pattern"),
    include_str!("patterns/rails.pattern"),
    include_str!("patterns/redis.pattern"),
    include_str!("patterns/ruby.pattern"),
    include_str!("patterns/squid.pattern"),
];

/// `(name, definition)` pairs from the bundled pattern files: one `NAME regex`
/// per line, `#` comments and blank lines skipped.
fn bundled_patterns() -> impl Iterator<Item = (&'static str, &'static str)> {
    PATTERN_FILES
        .iter()
        .flat_map(|file| file.lines())
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| line.split_once(' '))
}

/// A compiled grok expression.
#[derive(Debug)]
pub struct Pattern {
    regex: Regex,
    alias: Option<String>,
}

impl Pattern {
    fn new(regex: &str, aliases: &BTreeMap<String, String>) -> Result<Self, Error> {
        let regex =
            Regex::new(regex).map_err(|_| Error::RegexCompilationFailed(regex.to_string()))?;
        // The first named group in declaration order (lowest capture index).
        let mut first: Option<(u32, String)> = None;
        regex.foreach_name(|cap_name, cap_idx| {
            let index = cap_idx[0];
            if first.as_ref().is_none_or(|(best, _)| index < *best) {
                let name = aliases
                    .get(cap_name)
                    .cloned()
                    .unwrap_or_else(|| cap_name.to_string());
                first = Some((index, name));
            }
            true
        });
        Ok(Pattern {
            regex,
            alias: first.map(|(_, name)| name),
        })
    }

    /// Byte offsets `(start, end)` of the leftmost match beginning at or after
    /// `start`, or `None` if the pattern does not match there.
    ///
    /// Unlike slicing the text first, the whole `text` stays visible to the
    /// engine, so lookbehind assertions (the bundled `IPV4` pattern opens with
    /// `(?<![0-9])`) still see what precedes `start`.
    pub fn find_at(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let mut region = Region::new();
        self.regex.search_with_options(
            text,
            start,
            text.len(),
            SearchOptions::SEARCH_OPTION_NONE,
            Some(&mut region),
        )?;
        region.pos(0)
    }

    /// The alias of this pattern's first named capture group, in declaration
    /// order — the name a match should be reported under. `None` for a pattern
    /// with no named captures (compiling with `with_alias_only` strips the
    /// unaliased ones).
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

/// A set of named pattern definitions that expressions are compiled against.
#[derive(Debug)]
pub struct Grok {
    definitions: BTreeMap<String, String>,
}

impl Grok {
    /// A `Grok` with all the bundled definitions loaded.
    pub fn with_patterns() -> Self {
        Grok {
            definitions: bundled_patterns()
                .map(|(name, definition)| (name.to_string(), definition.to_string()))
                .collect(),
        }
    }

    /// Adds (or replaces) a named definition.
    pub fn insert_definition<S: Into<String>>(&mut self, name: S, pattern: S) {
        self.definitions.insert(name.into(), pattern.into());
    }

    /// Compiles a grok expression. With `with_alias_only`, only
    /// `%{NAME:alias}` references become capture groups.
    pub fn compile(&mut self, pattern: &str, with_alias_only: bool) -> Result<Pattern, Error> {
        let mut named_regex = String::from(pattern);
        let mut alias: BTreeMap<String, String> = BTreeMap::new();

        let mut index = 0;
        let mut iteration_left = MAX_RECURSION;
        let mut continue_iteration = true;

        let grok_regex = Regex::new(GROK_PATTERN)
            .map_err(|_| Error::RegexCompilationFailed(GROK_PATTERN.into()))?;

        while continue_iteration {
            continue_iteration = false;
            if iteration_left == 0 {
                return Err(Error::RecursionTooDeep);
            }
            iteration_left -= 1;

            if let Some(m) = grok_regex.captures(&named_regex.clone()) {
                continue_iteration = true;
                let raw_pattern = m.at(PATTERN_INDEX).ok_or_else(|| {
                    Error::GenericCompilationFailure("Could not find pattern in matches".into())
                })?;

                let mut name = m.at(NAME_INDEX).map(String::from).ok_or_else(|| {
                    Error::GenericCompilationFailure("Could not find name in matches".into())
                })?;

                if let Some(definition) = m.at(DEFINITION_INDEX) {
                    self.insert_definition(raw_pattern, definition);
                    name = format!("{}={}", name, definition);
                }

                // A pattern with a given name can show up more than once, so
                // apply the replacement once per occurrence.
                for _ in 0..named_regex.matches(&format!("%{{{}}}", name)).count() {
                    let pattern_definition = self
                        .definitions
                        .get(raw_pattern)
                        .ok_or_else(|| Error::DefinitionNotFound(raw_pattern.into()))?;

                    // Unaliased references under `with_alias_only` become
                    // non-capturing groups; everything else a named group,
                    // reported under its alias (or the pattern name).
                    let replacement = if with_alias_only && m.at(ALIAS_INDEX).is_none() {
                        format!("(?:{})", pattern_definition)
                    } else {
                        alias.insert(
                            format!("name{}", index),
                            match m.at(ALIAS_INDEX) {
                                Some(a) => a.into(),
                                None => name.clone(),
                            },
                        );
                        format!("(?<name{}>{})", index, pattern_definition)
                    };

                    named_regex = named_regex.replacen(&format!("%{{{}}}", name), &replacement, 1);
                    index += 1;
                }
            }
        }

        if named_regex.is_empty() {
            Err(Error::CompiledPatternIsEmpty(pattern.into()))
        } else {
            Pattern::new(&named_regex, &alias)
        }
    }
}

/// Why a grok expression failed to compile.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Expanding nested references exceeded the recursion limit.
    RecursionTooDeep,
    /// The expression expanded to an empty regex.
    CompiledPatternIsEmpty(String),
    /// A referenced pattern name has no definition.
    DefinitionNotFound(String),
    /// The expanded regex was rejected by the engine.
    RegexCompilationFailed(String),
    /// The expression could not be expanded.
    GenericCompilationFailure(String),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::RecursionTooDeep => write!(f, "recursion while compiling reached the limit"),
            Error::CompiledPatternIsEmpty(p) => write!(f, "the compiled pattern {} is empty", p),
            Error::DefinitionNotFound(d) => write!(f, "pattern definition {} not found", d),
            Error::RegexCompilationFailed(r) => write!(f, "regex compilation of {} failed", r),
            Error::GenericCompilationFailure(d) => write!(f, "compilation failed: {}", d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bundled_patterns_compile() {
        let mut grok = Grok::with_patterns();
        let names: Vec<String> = grok.definitions.keys().cloned().collect();
        assert!(names.len() > 300, "loaded only {} definitions", names.len());
        for name in names {
            let expr = format!("%{{{}}}", name);
            assert!(
                grok.compile(&expr, false).is_ok(),
                "{name} failed to compile"
            );
        }
    }

    #[test]
    fn bundled_definitions_load() {
        let grok = Grok::with_patterns();
        for name in ["USERNAME", "IPV4", "NUMBER", "TIMESTAMP_ISO8601", "UUID"] {
            assert!(grok.definitions.contains_key(name), "{name} missing");
        }
    }

    #[test]
    fn alias_only_reports_the_first_alias() {
        let mut grok = Grok::with_patterns();
        let pattern = grok
            .compile("%{WORD} %{NUMBER:num} %{IPV4:ip}", true)
            .unwrap();
        assert_eq!(pattern.alias(), Some("num"));
        let pattern = grok.compile("%{WORD} %{NUMBER}", true).unwrap();
        assert_eq!(pattern.alias(), None);
        let pattern = grok.compile("%{NUMBER}", false).unwrap();
        assert_eq!(pattern.alias(), Some("NUMBER"));
    }

    #[test]
    fn find_at_keeps_lookbehind_context() {
        let mut grok = Grok::with_patterns();
        let pattern = grok.compile("%{IPV4:ip}", true).unwrap();
        assert_eq!(pattern.find_at("at 10.0.0.1", 0), Some((3, 11)));
        // Starting mid-number: the lookbehind sees the preceding digit.
        assert_eq!(pattern.find_at("110.0.0.1", 1), None);
    }

    #[test]
    fn inline_definition_and_missing_definition() {
        let mut grok = Grok::with_patterns();
        let pattern = grok.compile("%{FOO:foo=ab+c}", true).unwrap();
        assert_eq!(pattern.find_at("xxabbbc", 0), Some((2, 7)));
        assert_eq!(
            grok.compile("%{NOPE_NOT_DEFINED}", false).unwrap_err(),
            Error::DefinitionNotFound("NOPE_NOT_DEFINED".into())
        );
    }
}
