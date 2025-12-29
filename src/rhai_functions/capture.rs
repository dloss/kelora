//! Thread-local capture system for parallel processing mode.
//!
//! This module provides infrastructure for capturing print/eprint output
//! in parallel processing mode, where output needs to be buffered and
//! ordered correctly.

use std::cell::RefCell;

/// Represents a captured message with its target stream
#[derive(Debug, Clone)]
pub enum CapturedMessage {
    Stdout(String),
    Stderr(String),
}

thread_local! {
    static CAPTURED_PRINTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CAPTURED_EPRINTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CAPTURED_MESSAGES: RefCell<Vec<CapturedMessage>> = const { RefCell::new(Vec::new()) };
    static PARALLEL_MODE: RefCell<bool> = const { RefCell::new(false) };
    static SUPPRESS_SIDE_EFFECTS: RefCell<bool> = const { RefCell::new(false) };
}

/// Capture a print statement in thread-local storage for parallel processing
pub fn capture_print(message: String) {
    CAPTURED_PRINTS.with(|prints| {
        prints.borrow_mut().push(message);
    });
}

/// Capture an eprint statement in thread-local storage for parallel processing
pub fn capture_eprint(message: String) {
    CAPTURED_EPRINTS.with(|eprints| {
        eprints.borrow_mut().push(message);
    });
}

/// Get all captured prints and clear the buffer
pub fn take_captured_prints() -> Vec<String> {
    CAPTURED_PRINTS.with(|prints| std::mem::take(&mut *prints.borrow_mut()))
}

/// Get all captured eprints and clear the buffer
pub fn take_captured_eprints() -> Vec<String> {
    CAPTURED_EPRINTS.with(|eprints| std::mem::take(&mut *eprints.borrow_mut()))
}

/// Capture a message in the ordered message system for parallel processing
pub fn capture_message(message: CapturedMessage) {
    CAPTURED_MESSAGES.with(|messages| {
        messages.borrow_mut().push(message);
    });
}

/// Capture a stdout message in the ordered system
pub fn capture_stdout(message: String) {
    capture_message(CapturedMessage::Stdout(message));
}

/// Capture a stderr message in the ordered system
pub fn capture_stderr(message: String) {
    capture_message(CapturedMessage::Stderr(message));
}

/// Get all captured messages in order and clear the buffer
pub fn take_captured_messages() -> Vec<CapturedMessage> {
    CAPTURED_MESSAGES.with(|messages| std::mem::take(&mut *messages.borrow_mut()))
}

/// Clear captured prints without returning them
pub fn clear_captured_prints() {
    CAPTURED_PRINTS.with(|prints| {
        prints.borrow_mut().clear();
    });
}

/// Clear captured eprints without returning them
pub fn clear_captured_eprints() {
    CAPTURED_EPRINTS.with(|eprints| {
        eprints.borrow_mut().clear();
    });
}

/// Set whether we're in parallel processing mode
pub fn set_parallel_mode(enabled: bool) {
    PARALLEL_MODE.with(|mode| {
        *mode.borrow_mut() = enabled;
    });
}

/// Check if we're in parallel processing mode
pub fn is_parallel_mode() -> bool {
    PARALLEL_MODE.with(|mode| *mode.borrow())
}

/// Set whether to suppress side effects (print, eprint, etc.)
pub fn set_suppress_side_effects(suppress: bool) {
    SUPPRESS_SIDE_EFFECTS.with(|flag| {
        *flag.borrow_mut() = suppress;
    });
}

/// Check if side effects should be suppressed
pub fn is_suppress_side_effects() -> bool {
    SUPPRESS_SIDE_EFFECTS.with(|flag| *flag.borrow())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_print() {
        clear_captured_prints();
        capture_print("hello".to_string());
        capture_print("world".to_string());
        let prints = take_captured_prints();
        assert_eq!(prints, vec!["hello", "world"]);
        // Buffer should be cleared after take
        assert!(take_captured_prints().is_empty());
    }

    #[test]
    fn test_capture_eprint() {
        clear_captured_eprints();
        capture_eprint("error1".to_string());
        capture_eprint("error2".to_string());
        let eprints = take_captured_eprints();
        assert_eq!(eprints, vec!["error1", "error2"]);
        assert!(take_captured_eprints().is_empty());
    }

    #[test]
    fn test_capture_messages() {
        // Clear any existing messages
        take_captured_messages();

        capture_stdout("out1".to_string());
        capture_stderr("err1".to_string());
        capture_stdout("out2".to_string());

        let messages = take_captured_messages();
        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0], CapturedMessage::Stdout(s) if s == "out1"));
        assert!(matches!(&messages[1], CapturedMessage::Stderr(s) if s == "err1"));
        assert!(matches!(&messages[2], CapturedMessage::Stdout(s) if s == "out2"));
    }

    #[test]
    fn test_parallel_mode() {
        set_parallel_mode(false);
        assert!(!is_parallel_mode());
        set_parallel_mode(true);
        assert!(is_parallel_mode());
        set_parallel_mode(false);
        assert!(!is_parallel_mode());
    }

    #[test]
    fn test_suppress_side_effects() {
        set_suppress_side_effects(false);
        assert!(!is_suppress_side_effects());
        set_suppress_side_effects(true);
        assert!(is_suppress_side_effects());
        set_suppress_side_effects(false);
        assert!(!is_suppress_side_effects());
    }
}
