//! Raw-line level pre-filter.
//!
//! When `--levels` restricts output to a small set of levels, most input lines
//! on a typical log never match and are dropped by [`LevelFilterStage`] *after*
//! being fully parsed into a `FieldMap`. That parse + allocation work is wasted.
//!
//! This module implements a cheap, zero-allocation, case-insensitive substring
//! scan of the raw (already multiline-assembled) line for the accepted level
//! tokens. If none of the tokens appear anywhere in the line, the parsed
//! event's level field cannot match, so the line can be dropped before parsing.
//!
//! Correctness contract:
//! - **No false negatives.** A line that would survive the level filter must
//!   never be dropped here. This holds only when the active parser extracts the
//!   level *verbatim from the line text* (see
//!   [`EventParser::level_appears_verbatim`]), so the accepted level string is
//!   guaranteed to appear as a substring of the raw line. Parsers that derive
//!   the level from non-textual encodings (syslog `<13>` priority, level
//!   mapping tables) must keep that method `false`, which disables the
//!   pre-filter by construction.
//! - **False positives are fine.** A token appearing in the message body (not
//!   the level field) simply falls through to the normal parse ->
//!   `LevelFilterStage` path, which drops it correctly.
//!
//! The token set is derived from the same include-level list that
//! [`LevelFilterStage`] matches against (case-insensitive equality, no
//! aliasing), so the two cannot drift.

/// Case-insensitive ASCII substring pre-filter over raw line bytes.
#[derive(Clone, Debug)]
pub struct LevelPrefilter {
    /// Accepted level tokens, pre-lowercased for the case-insensitive scan.
    needles: Vec<Box<[u8]>>,
}

impl LevelPrefilter {
    /// Build a pre-filter from a set of accepted include-level tokens.
    ///
    /// Returns `None` when there is nothing to scan for (empty token set), so
    /// callers can treat "no pre-filter" and "gate disabled" uniformly.
    pub fn new(tokens: &[String]) -> Option<Self> {
        let needles: Vec<Box<[u8]>> = tokens
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| t.to_ascii_lowercase().into_bytes().into_boxed_slice())
            .collect();
        if needles.is_empty() {
            None
        } else {
            Some(Self { needles })
        }
    }

    /// Returns `true` if the line should be parsed (at least one accepted level
    /// token appears somewhere in it), `false` if it can be safely dropped
    /// before parsing.
    #[inline]
    pub fn keep(&self, line: &str) -> bool {
        let hay = line.as_bytes();
        self.needles
            .iter()
            .any(|needle| contains_ignore_ascii_case(hay, needle))
    }
}

/// Case-insensitive ASCII substring search. `needle` is expected to be
/// pre-lowercased; `hay` is scanned as-is. Zero allocation.
///
/// The first byte is located with `memchr` (matching both ASCII cases at once),
/// then the full candidate window is verified with `eq_ignore_ascii_case`.
#[inline]
fn contains_ignore_ascii_case(hay: &[u8], needle: &[u8]) -> bool {
    let n = needle.len();
    match n {
        0 => return true,
        _ if hay.len() < n => return false,
        _ => {}
    }

    // needle[0] is already lowercased; also match its uppercase form.
    let lower = needle[0];
    let upper = lower.to_ascii_uppercase();
    // Last start index at which the full needle still fits.
    let last = hay.len() - n;

    let mut offset = 0usize;
    while offset <= last {
        let window = &hay[offset..=last];
        let found = if lower == upper {
            memchr::memchr(lower, window)
        } else {
            memchr::memchr2(lower, upper, window)
        };
        match found {
            Some(rel) => {
                let i = offset + rel;
                if hay[i..i + n].eq_ignore_ascii_case(needle) {
                    return true;
                }
                offset = i + 1;
            }
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pf(tokens: &[&str]) -> LevelPrefilter {
        LevelPrefilter::new(&tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn empty_token_set_disables() {
        assert!(LevelPrefilter::new(&[]).is_none());
        assert!(LevelPrefilter::new(&["".to_string()]).is_none());
    }

    #[test]
    fn matches_case_insensitively() {
        let f = pf(&["error"]);
        // The scan is case-insensitive, so every case variant survives.
        assert!(f.keep("level=error msg=boom"));
        assert!(f.keep("level=ERROR msg=boom"));
        assert!(f.keep("level=Error msg=boom"));
        assert!(f.keep("level=eRrOr msg=boom"));
        assert!(f.keep(r#"{"level":"ERROR"}"#));
    }

    #[test]
    fn drops_lines_without_token() {
        let f = pf(&["error"]);
        assert!(!f.keep("level=info msg=ok"));
        assert!(!f.keep("level=warn msg=slow"));
        assert!(!f.keep(""));
        assert!(!f.keep("err")); // prefix only, not the full token
    }

    #[test]
    fn false_positive_in_message_body_is_kept() {
        // "error" only in the message, level is info: the pre-filter keeps it
        // (a false positive that the parse -> level filter path drops).
        let f = pf(&["error"]);
        assert!(f.keep("level=info msg=\"connection error\""));
    }

    #[test]
    fn multiple_needles() {
        let f = pf(&["error", "warn"]);
        assert!(f.keep("level=warn msg=x"));
        assert!(f.keep("level=error msg=x"));
        assert!(!f.keep("level=info msg=x"));
    }

    #[test]
    fn token_at_boundaries() {
        let f = pf(&["error"]);
        assert!(f.keep("error")); // whole line is the token
        assert!(f.keep("error at start"));
        assert!(f.keep("ends with error"));
        assert!(!f.keep("erro")); // too short to contain the needle
    }

    #[test]
    fn substring_match_is_a_safe_superset() {
        // LevelFilterStage matches by case-insensitive *equality* with no
        // aliasing, so `--levels warn` does NOT accept a `warning` event. The
        // pre-filter uses substring matching, so needle "warn" *does* keep a
        // line containing "warning" — a false positive that the parse ->
        // LevelFilterStage path then correctly drops. What matters is the
        // absence of false negatives: any line the stage would keep is kept
        // here too.
        let f = pf(&["warn"]);
        assert!(f.keep("level=warn msg=x")); // real match, kept + matched later
        assert!(f.keep("level=warning msg=x")); // false positive, dropped later
                                                // The reverse: `--levels warning` never keeps a bare `warn` line.
        let f2 = pf(&["warning"]);
        assert!(!f2.keep("level=warn msg=x"));
        assert!(f2.keep("level=warning msg=x"));
    }

    #[test]
    fn multiline_continuation_line_carries_token() {
        // A multi-line record whose level token appears on a continuation line
        // must be kept: the scan runs over the whole assembled chunk.
        let f = pf(&["error"]);
        assert!(f.keep("first line no token\n  level=ERROR trailing"));
    }
}
