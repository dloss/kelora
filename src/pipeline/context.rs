//! Grep-style context tracking for `-B`/`-A`/`-C`.
//!
//! The marker semantics live here, in one place, rather than beside each
//! filter that can anchor context — `--filter` and `--levels` each carried a
//! copy, and the copies had drifted into opposite behaviour.
//! [`crate::pipeline::stages::ContextGroupStage`] is the only caller.
//!
//! ## Why output is held back
//!
//! A line's marker is not knowable the moment the line is seen. Whether a
//! line that trails a match (`\`) is *also* the run-up to a later match (`|`)
//! only becomes clear once up to `before_context` further lines have arrived.
//! So each non-matching line is parked until its fate is settled — either a
//! match arrives inside the before-window (the line is before-context, and
//! overlap if it was also after-context), or the window slides past it (the
//! line is plain after-context, or is dropped).
//!
//! A match settles every parked line at once, so the queue never holds more
//! than `before_context` entries and each line is released exactly once. An
//! earlier design emitted after-context eagerly and then re-walked its buffer
//! on the next match, which emitted overlapping lines twice — once as `\` and
//! again as `|`.

use crate::event::{ContextType, Event};
use std::collections::VecDeque;

/// A line parked until its context type is settled.
struct Held {
    event: Event,
    /// Inside the after-context window of an earlier match, so it is already
    /// destined for output — as `\` if nothing more happens, `|` if a match
    /// lands within the before-window.
    is_after: bool,
}

/// What one fed line settled: the lines whose markers are now final, in stream
/// order, plus how many parked lines were discarded without ever reaching
/// output.
pub struct ContextRelease {
    pub events: Vec<Event>,
    pub dropped: usize,
}

impl ContextRelease {
    fn empty() -> Self {
        Self {
            events: Vec::new(),
            dropped: 0,
        }
    }

    /// Whether this release settled nothing at all — the line was parked and
    /// no earlier line aged out. Callers must not treat this as "filtered":
    /// the line's fate is still open.
    pub fn is_deferred(&self) -> bool {
        self.events.is_empty() && self.dropped == 0
    }
}

/// Assigns `/`, `*`, `\` and `|` markers to a stream of match verdicts.
pub struct ContextTracker {
    before: usize,
    after: usize,
    /// Lines parked pending a verdict, oldest first. Never longer than
    /// `before`.
    held: VecDeque<Held>,
    /// Lines still owed to the after-context window of the last match.
    after_counter: usize,
}

impl ContextTracker {
    pub fn new(before: usize, after: usize) -> Self {
        Self {
            before,
            after,
            held: VecDeque::with_capacity(before),
            after_counter: 0,
        }
    }

    /// Feed one line along with whether it matched, and take whatever that
    /// settles.
    pub fn push(&mut self, event: Event, is_match: bool) -> ContextRelease {
        if is_match {
            // A match settles every parked line: each is before-context, and
            // overlap if it was already after-context.
            let mut events = Vec::with_capacity(self.held.len() + 1);
            for held in self.held.drain(..) {
                let context_type = if held.is_after {
                    ContextType::Both
                } else {
                    ContextType::Before
                };
                events.push(mark(held.event, context_type));
            }
            events.push(mark(event, ContextType::Match));
            self.after_counter = self.after;
            return ContextRelease { events, dropped: 0 };
        }

        let is_after = self.after_counter > 0;
        if is_after {
            self.after_counter -= 1;
        }
        self.held.push_back(Held { event, is_after });

        // Park the line, and settle whichever line just slid out of reach of
        // any future match. The queue grows by one per line, so this releases
        // at most one entry per call.
        let mut release = ContextRelease::empty();
        while self.held.len() > self.before {
            let held = self.held.pop_front().expect("len checked above");
            match held.is_after {
                true => release.events.push(mark(held.event, ContextType::After)),
                false => release.dropped += 1,
            }
        }
        release
    }

    /// Settle the lines still parked at end of input: after-context reaches
    /// output as `\`, the rest is dropped. No further match can arrive to make
    /// them before-context.
    pub fn finish(&mut self) -> ContextRelease {
        let mut release = ContextRelease::empty();
        for held in self.held.drain(..) {
            match held.is_after {
                true => release.events.push(mark(held.event, ContextType::After)),
                false => release.dropped += 1,
            }
        }
        self.after_counter = 0;
        release
    }
}

fn mark(mut event: Event, context_type: ContextType) -> Event {
    event.context_type = context_type;
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a run of lines whose matching positions are given, and render the
    /// released markers as one string per emitted line: "<marker><label>".
    fn run(labels: &[&str], matches: &[&str], before: usize, after: usize) -> Vec<String> {
        let mut tracker = ContextTracker::new(before, after);
        let mut out = Vec::new();
        let mut dropped = 0;

        for label in labels {
            let mut event = Event::default_with_line((*label).to_string());
            event.fields.insert("l".into(), (*label).into());
            let release = tracker.push(event, matches.contains(label));
            dropped += release.dropped;
            out.extend(release.events.into_iter().map(render));
        }
        let release = tracker.finish();
        dropped += release.dropped;
        out.extend(release.events.into_iter().map(render));

        assert_eq!(
            out.len() + dropped,
            labels.len(),
            "every line is either released exactly once or dropped: {:?}",
            out
        );
        out
    }

    fn render(event: Event) -> String {
        let marker = match event.context_type {
            ContextType::Match => '*',
            ContextType::Before => '/',
            ContextType::After => '\\',
            ContextType::Both => '|',
            ContextType::None => '.',
        };
        format!(
            "{}{}",
            marker,
            event
                .fields
                .get("l")
                .unwrap()
                .clone()
                .into_string()
                .unwrap()
        )
    }

    const LINES: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

    #[test]
    fn isolated_match_gets_before_and_after() {
        assert_eq!(run(LINES, &["3"], 1, 1), ["/2", "*3", "\\4"]);
    }

    #[test]
    fn overlapping_context_is_emitted_once_as_overlap() {
        // Line 4 trails match 3 and leads match 5: one line, marked `|`.
        assert_eq!(
            run(LINES, &["3", "5"], 1, 1),
            ["/2", "*3", "|4", "*5", "\\6"]
        );
    }

    #[test]
    fn wider_overlap_marks_every_shared_line() {
        assert_eq!(
            run(LINES, &["3", "6"], 2, 2),
            ["/1", "/2", "*3", "|4", "|5", "*6", "\\7", "\\8"]
        );
    }

    #[test]
    fn partial_overlap_keeps_after_before_and_overlap_distinct() {
        assert_eq!(
            run(LINES, &["2", "6"], 2, 2),
            ["/1", "*2", "\\3", "|4", "/5", "*6", "\\7", "\\8"]
        );
    }

    #[test]
    fn adjacent_matches_are_not_duplicated_as_each_others_context() {
        assert_eq!(run(LINES, &["3", "4"], 1, 1), ["/2", "*3", "*4", "\\5"]);
    }

    #[test]
    fn consecutive_matches_emit_each_line_once() {
        assert_eq!(
            run(LINES, &["4", "5", "6"], 2, 2),
            ["/2", "/3", "*4", "*5", "*6", "\\7", "\\8"]
        );
    }

    #[test]
    fn before_only_never_emits_after_context() {
        assert_eq!(
            run(LINES, &["3", "5"], 2, 0),
            ["/1", "/2", "*3", "/4", "*5"]
        );
    }

    #[test]
    fn after_only_never_emits_before_context() {
        assert_eq!(
            run(LINES, &["3", "5"], 0, 2),
            ["*3", "\\4", "*5", "\\6", "\\7"]
        );
    }

    #[test]
    fn match_at_start_and_end_of_input() {
        assert_eq!(run(LINES, &["1"], 2, 2), ["*1", "\\2", "\\3"]);
        assert_eq!(run(LINES, &["8"], 2, 2), ["/6", "/7", "*8"]);
    }

    #[test]
    fn trailing_after_context_survives_end_of_input() {
        // The `\` lines after match 7 are still parked when input ends.
        assert_eq!(run(LINES, &["7"], 1, 3), ["/6", "*7", "\\8"]);
    }

    #[test]
    fn no_match_emits_nothing() {
        assert!(run(LINES, &[], 2, 2).is_empty());
    }
}
