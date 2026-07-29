//! Language stack-trace presets for `--multiline java|python|go`.
//!
//! Each preset is a small state machine over physical lines (the model Fluent
//! Bit's built-in multiline parsers use, with one deliberate difference: a
//! trace-start line *continues* the event buffered before it, so a traceback
//! printed by `logger.exception(...)` stays attached to the header line that
//! logged it instead of becoming a separate event).
//!
//! Semantics: every line is its own event unless the machine recognizes it as
//! part of a trace. A start rule (exception line, `Traceback ...`, `panic:`)
//! enters a state and joins the current buffer; while in a state, only that
//! state's continuation rules (frames, `Caused by:`, goroutine blocks — plus
//! blank lines where the language's traces really contain them) keep joining.
//! The first unrecognized line flushes the buffer and starts a fresh event.
//!
//! The rules are data — `START_RULES` / `CONT_RULES` tables per language — so
//! extending a preset is a table edit, not new control flow. Known, accepted
//! limitations are documented in `docs/concepts/multiline-strategies.md`:
//! multi-line exception *messages* (a `str(e)`/`getMessage()` containing
//! newlines) are not recognized beyond the first line, and a Java exception
//! class that contains none of `Exception`/`Error`/`Throwable` is only caught
//! once its first indented `at ...` frame arrives.

use crate::config::TracePreset;
use regex::Regex;

/// Machine states. One flat enum across languages keeps the rule tables
/// simple; a machine only ever holds states of its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Inside a Java/JVM stack trace (exception line, frames, causes).
    Java,
    /// Inside a Python traceback (frames and code lines, all indented).
    PyTraceback,
    /// Seen the final `SomeError: message` line; only a chained-exception
    /// bridge sentence (or a fresh `Traceback`) may continue the event.
    PyAfter,
    /// Inside a Go panic / fatal error / goroutine dump.
    Go,
}

/// What the machine decided about one fed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceStep {
    /// The line belongs to the buffered event (trace start or continuation).
    Continues,
    /// The line starts a new event; flush the buffer first.
    Boundary,
}

/// A start rule: matching enters `State` (also as a fallback while already in
/// a trace, which is how Python's bridged `Traceback` re-entry and Go's
/// re-panic lines continue an event without dedicated continuation rules).
struct StartRule {
    pattern: &'static str,
    to: State,
}

/// A continuation rule, valid only while in `from`.
struct ContRule {
    from: State,
    pattern: &'static str,
    to: State,
}

// The exception line that opens a JVM trace: an optional `Exception in thread
// "..."` prefix, then a dotted class name whose last segment contains
// Exception/Error/Throwable. Requiring the dots keeps prose like
// "Error: disk full" from gluing onto the previous event; class names without
// the conventional suffix are picked up by the frame safety net below.
const JAVA_START_RULES: &[StartRule] = &[
    StartRule {
        pattern: r#"^(?:Exception in thread "[^"]*"\s+)?(?:[A-Za-z_$][\w$]*\.)+[A-Za-z_$][\w$]*(?:Exception|Error|Throwable)\b"#,
        to: State::Java,
    },
    // Frame safety net: an indented `at pkg.Cls.method(File.java:12)` line
    // groups with whatever preceded it (the exception line the start rule
    // above did not recognize), so an unconventional exception name costs at
    // most nothing rather than one event per frame.
    StartRule {
        pattern: r"^[ \t]+(?:eval )?at \S+\(.*\)",
        to: State::Java,
    },
    StartRule {
        pattern: r"^[ \t]*(?:Caused by|Suppressed): ",
        to: State::Java,
    },
];

const JAVA_CONT_RULES: &[ContRule] = &[
    ContRule {
        from: State::Java,
        pattern: r"^[ \t]+(?:eval )?at ",
        to: State::Java,
    },
    ContRule {
        from: State::Java,
        pattern: r"^[ \t]*(?:Caused by|Suppressed):",
        to: State::Java,
    },
    ContRule {
        from: State::Java,
        pattern: r"^[ \t]*\.\.\. \d+ (?:more|common frames omitted)",
        to: State::Java,
    },
    // Spring-style wrapping ("... ; nested exception is" split onto its own
    // line by some formatters).
    ContRule {
        from: State::Java,
        pattern: r"^[ \t]*nested exception is\b",
        to: State::Java,
    },
];

const PYTHON_START_RULES: &[StartRule] = &[
    StartRule {
        pattern: r"^Traceback \(most recent call last\):",
        to: State::PyTraceback,
    },
    // Exception groups (Python 3.11+): the whole rendering is indented and
    // `|`-prefixed, so once entered, the indented-line rule carries it.
    StartRule {
        pattern: r"^[ \t]*\+ Exception Group Traceback \(most recent call last\):",
        to: State::PyTraceback,
    },
    // A top-level SyntaxError has no `Traceback` header — it opens directly
    // with the indented File line. Doubles as a safety net for a torn-off
    // traceback whose header line is missing.
    StartRule {
        pattern: r#"^[ \t]+File "[^"]*", line \d+"#,
        to: State::PyTraceback,
    },
];

const PYTHON_CONT_RULES: &[ContRule] = &[
    // Everything inside a traceback is indented: File lines, source lines,
    // `^^^^` anchors (3.11+), and `|`/`+--` exception-group furniture.
    ContRule {
        from: State::PyTraceback,
        pattern: r"^[ \t]",
        to: State::PyTraceback,
    },
    // The final exception line is the first unindented line: a (dotted)
    // exception name, optionally with `: message`. Bare names cover
    // `KeyboardInterrupt` / `SystemExit`. Only recognized *inside* a
    // traceback — as a free-standing rule this shape would swallow ordinary
    // prose like `Note: ...`.
    ContRule {
        from: State::PyTraceback,
        pattern: r"^[A-Za-z_][\w.]*(?::.*)?$",
        to: State::PyAfter,
    },
    // Chained-exception bridges sit between two tracebacks, surrounded by
    // blank lines (blanks are continuations in both Python states).
    ContRule {
        from: State::PyAfter,
        pattern: r"^During handling of the above exception, another exception occurred:$",
        to: State::PyAfter,
    },
    ContRule {
        from: State::PyAfter,
        pattern: r"^The above exception was the direct cause of the following exception:$",
        to: State::PyAfter,
    },
];

const GO_START_RULES: &[StartRule] = &[
    StartRule {
        pattern: r"^panic: ",
        to: State::Go,
    },
    StartRule {
        pattern: r"^fatal error: ",
        to: State::Go,
    },
    // net/http's recovered-panic report, followed by a goroutine stack.
    StartRule {
        pattern: r"^http: panic serving ",
        to: State::Go,
    },
    // SIGQUIT / GOTRACEBACK goroutine dumps start directly with a goroutine
    // header, no panic line.
    StartRule {
        pattern: r"^goroutine \d+.*:$",
        to: State::Go,
    },
];

const GO_CONT_RULES: &[ContRule] = &[
    // Source locations and register dumps are always tab-indented.
    ContRule {
        from: State::Go,
        pattern: r"^\t",
        to: State::Go,
    },
    // Goroutine headers and "goroutine running on other thread; stack
    // unavailable".
    ContRule {
        from: State::Go,
        pattern: r"^goroutine ",
        to: State::Go,
    },
    ContRule {
        from: State::Go,
        pattern: r"^\[signal ",
        to: State::Go,
    },
    // Function-call lines: `main.(*Server).handle(0xc000010028, 0x2)` — no
    // whitespace before the argument list, which ends the line.
    ContRule {
        from: State::Go,
        pattern: r"^\S+\(.*\)$",
        to: State::Go,
    },
    ContRule {
        from: State::Go,
        pattern: r"^created by ",
        to: State::Go,
    },
    ContRule {
        from: State::Go,
        pattern: r"^runtime stack:$",
        to: State::Go,
    },
    ContRule {
        from: State::Go,
        pattern: r"^\.\.\.additional frames elided\.\.\.$",
        to: State::Go,
    },
    // Printed by `go run` after the dump; attaching it keeps the crash
    // report whole.
    ContRule {
        from: State::Go,
        pattern: r"^exit status \d+$",
        to: State::Go,
    },
];

/// States in which a blank line is a *continuation* (the language's traces
/// genuinely contain interior blanks). Java traces never do, so a blank ends
/// a Java trace event.
const BLANK_CONTINUES: &[State] = &[State::PyTraceback, State::PyAfter, State::Go];

/// The compiled per-preset machine. One instance lives inside the multiline
/// chunker for the run; `feed` classifies one line and advances the state.
pub(crate) struct TraceMachine {
    start_rules: Vec<(Regex, State)>,
    cont_rules: Vec<(State, Regex, State)>,
    state: Option<State>,
}

impl TraceMachine {
    pub(crate) fn new(preset: TracePreset) -> Result<Self, String> {
        let (starts, conts): (&[StartRule], &[ContRule]) = match preset {
            TracePreset::Java => (JAVA_START_RULES, JAVA_CONT_RULES),
            TracePreset::Python => (PYTHON_START_RULES, PYTHON_CONT_RULES),
            TracePreset::Go => (GO_START_RULES, GO_CONT_RULES),
        };
        let start_rules = starts
            .iter()
            .map(|r| Regex::new(r.pattern).map(|re| (re, r.to)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Invalid {} preset rule: {}", preset.name(), e))?;
        let cont_rules = conts
            .iter()
            .map(|r| Regex::new(r.pattern).map(|re| (r.from, re, r.to)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Invalid {} preset rule: {}", preset.name(), e))?;
        Ok(Self {
            start_rules,
            cont_rules,
            state: None,
        })
    }

    /// Forget any in-trace state (file boundary, flush, or a timestamped
    /// header line that overrides the trace grouping).
    pub(crate) fn reset(&mut self) {
        self.state = None;
    }

    /// Classify one line and advance the state. `Continues` means the line
    /// joins the buffered event — including a trace-*start* line, which
    /// attaches the trace to the line that logged it.
    pub(crate) fn feed(&mut self, line: &str) -> TraceStep {
        let stripped = line.trim_end_matches(['\n', '\r']);
        let blank = stripped.trim().is_empty();

        if let Some(state) = self.state {
            if blank {
                if BLANK_CONTINUES.contains(&state) {
                    return TraceStep::Continues;
                }
            } else {
                for (from, re, to) in &self.cont_rules {
                    if *from == state && re.is_match(stripped) {
                        self.state = Some(*to);
                        return TraceStep::Continues;
                    }
                }
            }
        }

        // Start rules run both as trace entry and as an in-trace fallback:
        // a re-`panic:` inside a Go dump or a bridged `Traceback` after a
        // Python exception line continues the same event.
        if !blank {
            for (re, to) in &self.start_rules {
                if re.is_match(stripped) {
                    self.state = Some(*to);
                    return TraceStep::Continues;
                }
            }
        }

        self.state = None;
        TraceStep::Boundary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(preset: TracePreset, text: &str) -> Vec<TraceStep> {
        let mut m = TraceMachine::new(preset).expect("preset rules compile");
        text.lines().map(|l| m.feed(l)).collect()
    }

    /// Split `text` into events the way the chunker would: a Boundary line
    /// flushes and starts a new buffer, a Continues line joins it.
    fn group(preset: TracePreset, text: &str) -> Vec<String> {
        let mut m = TraceMachine::new(preset).expect("preset rules compile");
        let mut events: Vec<String> = Vec::new();
        let mut buf: Vec<&str> = Vec::new();
        for line in text.lines() {
            if m.feed(line) == TraceStep::Boundary && !buf.is_empty() {
                events.push(buf.join("\n"));
                buf.clear();
            }
            buf.push(line);
        }
        if !buf.is_empty() {
            events.push(buf.join("\n"));
        }
        // Trailing-blank trim as the chunker does it.
        events
            .into_iter()
            .map(|e| e.trim_end_matches(['\n', ' ', '\t']).to_string())
            .filter(|e| !e.trim().is_empty())
            .collect()
    }

    #[test]
    fn all_preset_rule_tables_compile() {
        for preset in [TracePreset::Java, TracePreset::Python, TracePreset::Go] {
            TraceMachine::new(preset).expect("rules compile");
        }
    }

    #[test]
    fn java_trace_attaches_to_logging_header() {
        let events = group(
            TracePreset::Java,
            "Something failed while syncing\n\
             java.lang.IllegalStateException: boom\n\
             \tat com.example.Foo.bar(Foo.java:10)\n\
             \tat com.example.Main.main(Main.java:5)\n\
             Caused by: java.lang.NullPointerException\n\
             \tat com.example.Baz.qux(Baz.java:20)\n\
             \t... 3 more\n\
             next ordinary line\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].starts_with("Something failed"));
        assert!(events[0].contains("... 3 more"));
        assert_eq!(events[1], "next ordinary line");
    }

    #[test]
    fn java_exception_in_thread_form_and_suppressed() {
        let events = group(
            TracePreset::Java,
            "Exception in thread \"main\" java.lang.RuntimeException: outer\n\
             \tat com.example.Main.main(Main.java:5)\n\
             \tSuppressed: java.lang.Exception: cleanup failed\n\
             \t\tat com.example.Res.close(Res.java:9)\n\
             INFO all done\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("Suppressed"));
    }

    #[test]
    fn java_unconventional_exception_name_recovers_on_first_frame() {
        // `com.foo.Boom` matches no start rule; the first frame attaches it
        // and the frames to the same event.
        let events = group(
            TracePreset::Java,
            "com.foo.Boom: not a conventional name\n\
             \tat com.foo.A.b(A.java:1)\n\
             \tat com.foo.C.d(C.java:2)\n\
             plain line\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("com.foo.Boom"));
        assert!(events[0].contains("C.d"));
    }

    #[test]
    fn java_prose_error_line_is_not_a_trace_start() {
        let events = group(
            TracePreset::Java,
            "line one\nError: disk full\nline three\n",
        );
        assert_eq!(events.len(), 3, "undotted 'Error:' prose must not glue");
    }

    #[test]
    fn java_blank_line_ends_the_trace() {
        let events = group(
            TracePreset::Java,
            "java.lang.Error: x\n\tat a.b.C.d(C.java:1)\n\nunrelated\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("at a.b.C.d"));
        assert_eq!(events[1], "unrelated");
    }

    #[test]
    fn python_traceback_with_chained_exceptions() {
        let events = group(
            TracePreset::Python,
            "request failed\n\
             Traceback (most recent call last):\n  \
               File \"/app/db.py\", line 12, in fetch\n    \
                 return cache[key]\nKeyError: 'user'\n\
             \n\
             During handling of the above exception, another exception occurred:\n\
             \n\
             Traceback (most recent call last):\n  \
               File \"/app/main.py\", line 30, in handle\n    \
                 fetch(key)\nValueError: no such user\n\
             next request ok\n",
        );
        assert_eq!(events.len(), 2, "chained tracebacks stay one event");
        assert!(events[0].starts_with("request failed"));
        assert!(events[0].contains("KeyError"));
        assert!(events[0].contains("ValueError"));
        assert_eq!(events[1], "next request ok");
    }

    #[test]
    fn python_bare_exception_name_ends_trace() {
        let events = group(
            TracePreset::Python,
            "Traceback (most recent call last):\n  \
               File \"/app/main.py\", line 3, in <module>\n    \
                 time.sleep(100)\nKeyboardInterrupt\nafter\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("KeyboardInterrupt"));
        assert_eq!(events[1], "after");
    }

    #[test]
    fn python_syntax_error_without_traceback_header() {
        let events = group(
            TracePreset::Python,
            "starting\n  File \"/app/main.py\", line 3\n    \
             def f(:\n          ^\nSyntaxError: invalid syntax\ndone\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("SyntaxError"));
        assert_eq!(events[1], "done");
    }

    #[test]
    fn python_exception_group_stays_one_event() {
        let events = group(
            TracePreset::Python,
            "  + Exception Group Traceback (most recent call last):\n  \
             |   File \"/app/main.py\", line 5, in <module>\n  \
             |     raise ExceptionGroup('woes', [ValueError('a')])\n  \
             | ExceptionGroup: woes (1 sub-exception)\n  \
             +-+---------------- 1 ----------------\n    \
             | ValueError: a\n    \
             +------------------------------------\n\
             ordinary line\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("ExceptionGroup"));
        assert_eq!(events[1], "ordinary line");
    }

    #[test]
    fn python_ordinary_line_after_exception_does_not_continue() {
        // The unindented-name rule only fires inside a traceback; after the
        // exception line, prose must start a fresh event.
        let events = group(
            TracePreset::Python,
            "Traceback (most recent call last):\n  \
               File \"x.py\", line 1, in <module>\nValueError: boom\n\
             Note: this is unrelated prose\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[1].starts_with("Note:"));
    }

    #[test]
    fn go_panic_with_goroutine_dump_is_one_event() {
        let events = group(
            TracePreset::Go,
            "panic: runtime error: index out of range [3] with length 2\n\
             \n\
             goroutine 1 [running]:\n\
             main.foo(0x0?)\n\
             \t/app/main.go:10 +0x1d\n\
             main.main()\n\
             \t/app/main.go:5 +0x20\n\
             exit status 2\n\
             server restarted\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].starts_with("panic:"));
        assert!(events[0].contains("exit status 2"));
        assert_eq!(events[1], "server restarted");
    }

    #[test]
    fn go_fatal_error_with_signal_and_multiple_goroutines() {
        let events = group(
            TracePreset::Go,
            "fatal error: all goroutines are asleep - deadlock!\n\
             \n\
             goroutine 1 [chan receive]:\n\
             main.main()\n\
             \t/app/main.go:8 +0x2d\n\
             \n\
             goroutine 6 [select]:\n\
             main.worker()\n\
             \t/app/worker.go:14 +0x9a\n\
             created by main.main in goroutine 1\n\
             \t/app/main.go:7 +0x1e\n\
             back to normal\n",
        );
        assert_eq!(
            events.len(),
            2,
            "blank-separated goroutine blocks stay one event"
        );
        assert!(events[0].contains("goroutine 6"));
        assert_eq!(events[1], "back to normal");
    }

    #[test]
    fn go_repanic_line_continues_the_dump() {
        let events = group(
            TracePreset::Go,
            "panic: boom [recovered]\n\
             \tpanic: boom\n\
             \n\
             goroutine 1 [running]:\n\
             main.main()\n\
             \t/app/main.go:5 +0x20\n\
             after\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("[recovered]"));
    }

    #[test]
    fn go_standalone_goroutine_dump_groups() {
        // SIGQUIT dump: no panic line, opens with a goroutine header.
        let events = group(
            TracePreset::Go,
            "goroutine 12 [IO wait, 3 minutes]:\n\
             internal/poll.runtime_pollWait(0x7f2, 0x72)\n\
             \t/usr/local/go/src/runtime/netpoll.go:343 +0x85\n\
             plain\n",
        );
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("pollWait"));
    }

    #[test]
    fn machine_reset_forgets_trace_state() {
        let mut m = TraceMachine::new(TracePreset::Go).unwrap();
        assert_eq!(m.feed("panic: x"), TraceStep::Continues);
        assert_eq!(m.feed(""), TraceStep::Continues, "blank inside dump");
        m.reset();
        assert_eq!(
            m.feed(""),
            TraceStep::Boundary,
            "after reset a blank is no longer a trace continuation"
        );
    }

    #[test]
    fn ordinary_lines_are_boundaries() {
        for preset in [TracePreset::Java, TracePreset::Python, TracePreset::Go] {
            assert_eq!(
                steps(preset, "one plain line\nanother plain line\n"),
                vec![TraceStep::Boundary, TraceStep::Boundary],
                "{:?}",
                preset
            );
        }
    }
}
