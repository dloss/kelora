//! Built-in named log formats, adapted from the lnav project.
//!
//! ## Attribution
//!
//! The log-format catalogue that inspired this module ships with
//! [lnav](https://lnav.org) by Timothy Stack and is distributed under the
//! BSD-3-Clause license. The regular expressions below were re-expressed in
//! Kelora's [`RegexParser`] syntax (`(?P<name>...)` named captures with optional
//! `:type` annotations) and adapted to Kelora's field-naming conventions
//! (`ts`, `level`, `msg`). See `THIRD_PARTY_LICENSES.md` for the full
//! license text and attribution.
//!
//! ## Why this exists
//!
//! Kelora's structured detectors (json, cef, syslog, combined, logfmt, csv)
//! cover wire formats and access logs, but many common *application* log layouts
//! — an ISO-8601 timestamp followed by a level and a message, log4j/Java,
//! Python `logging`, nginx error logs, glog/klog — previously fell through to
//! the plain `line` parser. This module recognises a small, curated set of those
//! layouts and hands them off to the existing [`RegexParser`], so auto-detection
//! extracts structured fields instead of a single opaque `line`.
//!
//! ## Design notes (kept deliberately small)
//!
//! - Each definition is a single anchored regex tried against the first
//!   non-empty line. Matching is all-or-nothing — there is **no** lnav-style
//!   weighted scoring engine here; the first definition that matches wins.
//! - Detection runs *after* every existing detector and immediately before the
//!   `line` fallback (see [`crate::parsers::auto_detect`]). It can therefore only
//!   reclassify input that would otherwise have become `line`, so it never
//!   changes a format Kelora already detected.
//! - A match is returned as `InputFormat::Named`, carrying the format's name and
//!   regex, so it can be displayed, selected via `-f <name>`, and reused across
//!   the parser-build/timestamp/strict pipeline unchanged.
//! - Every definition carries sample lines that are verified at test time, the
//!   same self-validation idea lnav uses for its format files.

use crate::parsers::RegexParser;
use crate::pipeline::EventParser;

/// A built-in named log format: an anchored [`RegexParser`] pattern plus
/// representative sample lines used for self-validation.
///
/// The `name` is a stable, user-facing identifier: it is shown in the
/// auto-detect notice, accepted by `-f <name>` (including inside cascade
/// lists), and listed in `--help-formats`. `samples` are consumed only by the
/// self-validation tests, but are kept as part of the definition so each entry
/// is self-describing and test-verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LnavFormat {
    /// Stable, user-facing identifier (also usable with `-f`).
    pub name: &'static str,
    /// Pattern in `RegexParser` syntax. Outer `^...$` anchors are added by
    /// `RegexParser`, so they are omitted here. The timestamp is captured into a
    /// group named `ts` to match the field name kelora's other parsers emit.
    pub pattern: &'static str,
    /// Optional strftime format for the captured `ts` field, applied only when
    /// the user has not passed an explicit `--ts-format`. Set this only for
    /// layouts kelora's adaptive timestamp parser does *not* already recognise
    /// (currently just glog's year-less `MMDD HH:MM:SS.ffffff`); leave it `None`
    /// when the default adaptive parser resolves the timestamp on its own.
    pub ts_format: Option<&'static str>,
    /// Lines this format must parse. Used by the self-validation test and as
    /// living documentation of what each format looks like.
    #[allow(dead_code)] // consumed only by the self-validation tests
    pub samples: &'static [&'static str],
}

/// Curated set of application log formats, ordered from most specific to most
/// general. Order matters: the generic ISO-8601 catch-all is intentionally last
/// so the structured layouts (Java/log4j, etc.) claim their lines first.
///
/// ## Naming convention (these names are user-facing API — see the guard test)
///
/// A format name is the `-f <name>` value, the `_format` field value in
/// cascades, and what `--stats` displays, so renaming one later is a breaking
/// change. New names must follow:
///
/// - lowercase ASCII, digits, and `-` only; start with a letter. No `:` (taken
///   by `cols:`/`regex:`/`csv:` field specs) and no `,` (cascade separator).
/// - must not collide with a built-in format keyword (`json`, `line`, `syslog`,
///   `cef`, `csv`, `cols`, `regex`, `auto`, …).
/// - bare token only when the name is itself a specific, canonical identifier
///   (`glog`, `log4j`). Otherwise use `source-subtype` (`nginx-error`,
///   `python-logging`) and leave the bare family word (`nginx`, `python`, `java`)
///   free for future siblings. Reserve purely structural names (`iso8601-level`)
///   for true generics with no single canonical source.
///
/// No built-in aliases: each format has exactly one name. Users wanting a
/// shorthand can alias at the shell or `.kelora.ini` level.
pub static LNAV_FORMATS: &[LnavFormat] = &[
    // glog / klog (Go, Kubernetes): `I0102 15:04:05.123456 1234 server.go:42] msg`
    // glog omits the year and timezone; its `MMDD HH:MM:SS.ffffff` layout is not
    // in the adaptive parser's list, so we pin the format here (the year-less
    // path in timestamp.rs then assumes the current year, like syslog does).
    LnavFormat {
        name: "glog",
        pattern: r"(?P<level>[IWEF])(?P<ts>\d{4} \d{2}:\d{2}:\d{2}\.\d{1,6})\s+(?P<pid:int>\d+)\s+(?P<source>[^:\s]+:\d+)\]\s+(?P<msg>.*)",
        ts_format: Some("%m%d %H:%M:%S%.f"),
        samples: &[
            "I0102 15:04:05.123456 1234 server.go:42] Starting controller",
            "E0612 09:10:11.000001 7 reflector.go:138] Failed to watch",
        ],
    },
    // nginx error log: `2024/01/02 15:04:05 [error] 29#29: *1 open() failed`
    LnavFormat {
        name: "nginx-error",
        pattern: r"(?P<ts>\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2})\s+\[(?P<level>\w+)\]\s+(?P<pid:int>\d+)#(?P<tid:int>\d+):\s*(?P<msg>.*)",
        ts_format: None,
        samples: &[
            "2024/01/02 15:04:05 [error] 29#29: *1 open() failed (2: No such file or directory)",
            "2024/06/12 08:00:00 [warn] 12#0: using uninitialized variable",
        ],
    },
    // log4j / Java: `2024-01-02 15:04:05,123 INFO [main] com.example.Foo - msg`
    LnavFormat {
        name: "log4j",
        pattern: r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+(?P<level>TRACE|DEBUG|INFO|WARN|ERROR|FATAL)\s+\[(?P<thread>[^\]]+)\]\s+(?P<logger>\S+)\s+-\s+(?P<msg>.*)",
        ts_format: None,
        samples: &[
            "2024-01-02 15:04:05,123 INFO [main] com.example.Service - Service started",
            "2024-06-12 08:00:00,001 ERROR [pool-1-thread-2] com.acme.Db - Connection refused",
        ],
    },
    // Python logging default: `2024-01-02 15:04:05,123 - myapp - INFO - msg`
    LnavFormat {
        name: "python-logging",
        pattern: r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+-\s+(?P<logger>\S+)\s+-\s+(?P<level>DEBUG|INFO|WARNING|ERROR|CRITICAL)\s+-\s+(?P<msg>.*)",
        ts_format: None,
        samples: &[
            "2024-01-02 15:04:05,123 - myapp.module - INFO - Service started",
            "2024-06-12 08:00:00,500 - root - ERROR - Unhandled exception",
        ],
    },
    // Generic ISO-8601 prefixed application log (catch-all, kept last):
    // `2024-01-02T15:04:05.123Z INFO message` or `2024-01-02 15:04:05 ERROR message`
    LnavFormat {
        name: "iso8601-level",
        pattern: r"(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})?)\s+(?P<level>TRACE|DEBUG|INFO|NOTICE|WARN|WARNING|ERROR|ERR|FATAL|CRIT|CRITICAL)\s+(?P<msg>.*)",
        ts_format: None,
        samples: &[
            "2024-01-02T15:04:05.123Z INFO Starting service on port 8080",
            "2024-06-12 08:00:00 ERROR database connection lost",
        ],
    },
];

/// Try to recognise `line` as one of the built-in named formats.
///
/// Returns the matching definition (in declaration order), or `None` if no
/// built-in format applies. Patterns are compiled on demand; this runs once per
/// input during auto-detection, so a fresh compile is cheaper than caching.
pub fn detect(line: &str) -> Option<&'static LnavFormat> {
    LNAV_FORMATS.iter().find(|fmt| {
        RegexParser::new(fmt.pattern)
            .map(|parser| parser.parse(line).is_ok())
            .unwrap_or(false)
    })
}

/// Look up a built-in named format by its identifier (e.g. for `-f log4j`).
/// Names are lowercase; lookup is case-insensitive for friendliness.
pub fn by_name(name: &str) -> Option<&'static LnavFormat> {
    LNAV_FORMATS
        .iter()
        .find(|fmt| fmt.name.eq_ignore_ascii_case(name))
}

/// Comma-separated list of built-in format names, for help and error text.
pub fn names_csv() -> String {
    LNAV_FORMATS
        .iter()
        .map(|fmt| fmt.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-validation: every definition must compile and parse all of its own
    /// sample lines. Mirrors lnav's requirement that a format's samples match.
    #[test]
    fn all_formats_parse_their_samples() {
        for fmt in LNAV_FORMATS {
            let parser = RegexParser::new(fmt.pattern)
                .unwrap_or_else(|e| panic!("format '{}' failed to compile: {e}", fmt.name));
            for sample in fmt.samples {
                assert!(
                    parser.parse(sample).is_ok(),
                    "format '{}' did not parse its sample: {sample:?}",
                    fmt.name
                );
            }
        }
    }

    /// Each sample must be claimed by its own format first (detection order is
    /// correct and the definitions do not shadow one another).
    #[test]
    fn samples_detect_to_their_own_format() {
        for fmt in LNAV_FORMATS {
            for sample in fmt.samples {
                let detected = detect(sample)
                    .unwrap_or_else(|| panic!("sample not detected at all: {sample:?}"));
                assert_eq!(
                    detected.name, fmt.name,
                    "sample {sample:?} detected as '{}' but belongs to '{}'",
                    detected.name, fmt.name
                );
            }
        }
    }

    #[test]
    fn extracts_expected_fields() {
        let parser = RegexParser::new(
            detect("2024-01-02 15:04:05,123 INFO [main] com.example.Service - up")
                .unwrap()
                .pattern,
        )
        .unwrap();
        let event = parser
            .parse("2024-01-02 15:04:05,123 INFO [main] com.example.Service - up")
            .unwrap();
        assert_eq!(
            event
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "INFO"
        );
        assert_eq!(
            event
                .fields
                .get("thread")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "main"
        );
        assert_eq!(
            event
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "up"
        );
    }

    #[test]
    fn does_not_match_plain_text_or_csv() {
        // Plain prose and CSV-of-words must not be misdetected.
        assert!(detect("just some random log text here").is_none());
        assert!(detect("name,age,city").is_none());
        assert!(detect("hello world").is_none());
        // A bare date without a level should not match the generic format.
        assert!(detect("2024-01-02T15:04:05Z just a message without level").is_none());
    }

    #[test]
    fn typed_fields_are_converted() {
        // glog pid is annotated :int and should arrive as an integer.
        let fmt = detect("I0102 15:04:05.123456 1234 server.go:42] hi").unwrap();
        let parser = RegexParser::new(fmt.pattern).unwrap();
        let event = parser
            .parse("I0102 15:04:05.123456 1234 server.go:42] hi")
            .unwrap();
        assert_eq!(event.fields.get("pid").unwrap().as_int().unwrap(), 1234);
    }

    #[test]
    fn every_format_captures_a_ts_field() {
        // The timestamp group is named `ts` to match kelora's other parsers, so
        // the standard timestamp field-name detection picks it up.
        for fmt in LNAV_FORMATS {
            let sample = fmt.samples[0];
            let event = RegexParser::new(fmt.pattern)
                .unwrap()
                .parse(sample)
                .unwrap();
            assert!(
                event.fields.contains_key("ts"),
                "format '{}' should capture a 'ts' field from {sample:?}",
                fmt.name
            );
        }
    }

    #[test]
    fn only_glog_pins_a_ts_format() {
        // glog's year-less MMDD layout is not in the adaptive parser's list, so
        // it carries an explicit format; the rest resolve adaptively.
        for fmt in LNAV_FORMATS {
            match fmt.name {
                "glog" => assert_eq!(fmt.ts_format, Some("%m%d %H:%M:%S%.f")),
                _ => assert_eq!(
                    fmt.ts_format, None,
                    "{} should not pin a ts_format",
                    fmt.name
                ),
            }
        }
    }

    /// Guards the naming convention documented on `LNAV_FORMATS`. These names are
    /// user-facing API, so a new format must not silently break the rules.
    #[test]
    fn names_follow_convention_and_dont_collide() {
        // Built-in `-f` keywords a named format must never shadow (see
        // config::parse_input_format_spec / cli::parse_format_value).
        const RESERVED: &[&str] = &[
            "auto",
            "auto-per-file",
            "json",
            "line",
            "raw",
            "logfmt",
            "syslog",
            "cef",
            "csv",
            "tsv",
            "csvnh",
            "tsvnh",
            "combined",
            "cols",
            "regex",
            "cascade",
        ];

        let mut seen = std::collections::HashSet::new();
        for fmt in LNAV_FORMATS {
            let name = fmt.name;

            // Unique across the catalogue.
            assert!(seen.insert(name), "duplicate format name '{name}'");

            // Never collide with a reserved keyword.
            assert!(
                !RESERVED.contains(&name),
                "format name '{name}' collides with a built-in format keyword"
            );

            // Charset: start with a letter, then lowercase ASCII / digits / '-'.
            // This also enforces "no ':' and no ','" implicitly.
            assert!(
                name.starts_with(|c: char| c.is_ascii_lowercase()),
                "format name '{name}' must start with a lowercase letter"
            );
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "format name '{name}' must be lowercase ASCII letters, digits, or '-'"
            );
            assert!(
                !name.ends_with('-') && !name.contains("--"),
                "format name '{name}' has a stray or doubled '-'"
            );
        }
    }
}
