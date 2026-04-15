#![allow(dead_code)] // Cross-platform I/O wrappers include spare helpers not yet wired in the bin CLI
use anyhow::Result;
use crossbeam_channel::Sender;
use std::fs::File;
use std::io::{self, Write};
use std::panic::{self, PanicHookInfo};
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Once;
use std::thread;

// Cross-platform signal handling
#[cfg(unix)]
use signal_hook::{consts::SIGINT, consts::SIGPIPE, consts::SIGTERM, iterator::Signals};

// Additional signals for stats printing
#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
use signal_hook::consts::SIGINFO;

#[cfg(unix)]
use signal_hook::consts::SIGUSR1;

#[cfg(windows)]
use signal_hook::{consts::SIGINT, flag};

/// Standard Unix exit codes
#[derive(Debug, Clone, Copy)]
pub enum ExitCode {
    Success = 0,
    GeneralError = 1,
    InvalidUsage = 2,
    SignalInt = 130,  // 128 + SIGINT (2)
    SignalPipe = 141, // 128 + SIGPIPE (13)
    SignalTerm = 143, // 128 + SIGTERM (15)
}

impl ExitCode {
    pub fn exit(self) -> ! {
        process::exit(self as i32)
    }
}

/// Global termination flag for graceful shutdown
pub static SHOULD_TERMINATE: AtomicBool = AtomicBool::new(false);
pub static TERMINATED_BY_SIGNAL: AtomicBool = AtomicBool::new(false);

/// Track which signal caused termination (for correct exit code)
/// 0 = no signal, 2 = SIGINT, 15 = SIGTERM, etc.
pub static TERMINATION_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Control messages broadcast by the signal handler to processing components
#[derive(Debug, Clone)]
pub enum Ctrl {
    Shutdown { immediate: bool },
    PrintStats,
}

/// Signal handler for graceful shutdown
pub struct SignalHandler {
    _handle: thread::JoinHandle<()>,
}

impl SignalHandler {
    /// Initialize signal handling - cross-platform
    pub fn new(ctrl_sender: Sender<Ctrl>) -> Result<Self> {
        #[cfg(unix)]
        {
            #[cfg(all(
                unix,
                any(
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "netbsd",
                    target_os = "dragonfly"
                )
            ))]
            let signals_to_handle = vec![SIGINT, SIGPIPE, SIGTERM, SIGUSR1, SIGINFO];

            #[cfg(not(all(
                unix,
                any(
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "netbsd",
                    target_os = "dragonfly"
                )
            )))]
            let signals_to_handle = vec![SIGINT, SIGPIPE, SIGTERM, SIGUSR1];

            let mut signals = Signals::new(&signals_to_handle)?;

            let sender = ctrl_sender.clone();
            let handle = thread::spawn(move || {
                let mut shutdown_count = 0;
                for sig in signals.forever() {
                    match sig {
                        SIGINT => {
                            SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                            TERMINATED_BY_SIGNAL.store(true, Ordering::Relaxed);
                            TERMINATION_SIGNAL.store(SIGINT, Ordering::Relaxed);
                            shutdown_count += 1;
                            let immediate = shutdown_count > 1;
                            let _ = sender.send(Ctrl::Shutdown { immediate });
                            if immediate {
                                ExitCode::SignalInt.exit();
                            }
                        }
                        SIGPIPE => {
                            // Broken pipe - exit quietly (normal for Unix pipes)
                            SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                            TERMINATED_BY_SIGNAL.store(true, Ordering::Relaxed);
                            ExitCode::SignalPipe.exit();
                        }
                        SIGTERM => {
                            eprintln!(
                                "{}",
                                crate::config::format_error_message_auto(&format!(
                                    "Received SIGTERM, shutting down gracefully{}",
                                    crate::tty::ellipsis(crate::tty::should_use_emoji_for_stderr())
                                ))
                            );
                            SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                            TERMINATED_BY_SIGNAL.store(true, Ordering::Relaxed);
                            TERMINATION_SIGNAL.store(SIGTERM, Ordering::Relaxed);
                            shutdown_count += 1;
                            let immediate = shutdown_count > 1;
                            let _ = sender.send(Ctrl::Shutdown { immediate });
                            if immediate {
                                ExitCode::SignalTerm.exit();
                            }
                            // Allow graceful shutdown to proceed. If still running, a
                            // subsequent SIGTERM/SIGINT will trigger immediate exit.
                        }
                        SIGUSR1 => {
                            // Print stats on SIGUSR1 (available on all Unix-like systems)
                            let _ = sender.send(Ctrl::PrintStats);
                        }
                        #[cfg(all(
                            unix,
                            any(
                                target_os = "macos",
                                target_os = "freebsd",
                                target_os = "openbsd",
                                target_os = "netbsd",
                                target_os = "dragonfly"
                            )
                        ))]
                        SIGINFO => {
                            // Print stats on SIGINFO (CTRL-T on BSD-like systems including macOS)
                            let _ = sender.send(Ctrl::PrintStats);
                        }
                        _ => {
                            // Unknown signal - should not happen with our registration
                            eprintln!(
                                "{}",
                                crate::config::format_error_message_auto(&format!(
                                    "Received unexpected signal: {}",
                                    sig
                                ))
                            );
                        }
                    }
                }
            });

            Ok(SignalHandler { _handle: handle })
        }

        #[cfg(windows)]
        {
            // Windows signal handling using flag-based approach
            let term_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            flag::register(SIGINT, std::sync::Arc::clone(&term_flag))?;

            let mut sender = ctrl_sender.clone();
            let handle = thread::spawn(move || {
                let mut shutdown_count = 0;
                loop {
                    thread::sleep(std::time::Duration::from_millis(100));
                    if term_flag.load(Ordering::Relaxed) {
                        SHOULD_TERMINATE.store(true, Ordering::Relaxed);
                        TERMINATED_BY_SIGNAL.store(true, Ordering::Relaxed);
                        TERMINATION_SIGNAL.store(SIGINT, Ordering::Relaxed);
                        shutdown_count += 1;
                        let immediate = shutdown_count > 1;
                        let _ = sender.send(Ctrl::Shutdown { immediate });
                        if immediate {
                            ExitCode::SignalInt.exit();
                        }
                    }
                }
            });

            Ok(SignalHandler { _handle: handle })
        }
    }

    /// Check if we should terminate processing
    pub fn should_terminate() -> bool {
        SHOULD_TERMINATE.load(Ordering::Relaxed)
    }
}

/// Safe wrapper for writing to stdout that handles broken pipes and other I/O errors
pub struct SafeStdout {
    stdout: io::Stdout,
}

impl SafeStdout {
    pub fn new() -> Self {
        Self {
            stdout: io::stdout(),
        }
    }

    /// Write a line to stdout, handling broken pipes gracefully (cross-platform)
    pub fn writeln(&mut self, data: &str) -> Result<()> {
        match writeln!(self.stdout, "{}", data) {
            Ok(()) => Ok(()),
            Err(e) if Self::is_broken_pipe(&e) => {
                // Broken pipe is normal in pipelines - exit quietly
                ExitCode::SignalPipe.exit();
            }
            Err(e) => {
                // Other I/O errors should be reported
                Err(anyhow::anyhow!("Failed to write to stdout: {}", e))
            }
        }
    }

    /// Flush stdout, handling errors gracefully (cross-platform)
    pub fn flush(&mut self) -> Result<()> {
        match self.stdout.flush() {
            Ok(()) => Ok(()),
            Err(e) if Self::is_broken_pipe(&e) => {
                // Broken pipe is normal - exit quietly
                ExitCode::SignalPipe.exit();
            }
            Err(e) => {
                // Other flush errors should be reported
                Err(anyhow::anyhow!("Failed to flush stdout: {}", e))
            }
        }
    }

    /// Cross-platform broken pipe detection
    fn is_broken_pipe(e: &io::Error) -> bool {
        #[cfg(unix)]
        {
            e.kind() == io::ErrorKind::BrokenPipe
        }
        #[cfg(windows)]
        {
            // On Windows, broken pipe manifests as different error codes
            e.kind() == io::ErrorKind::BrokenPipe
                || e.raw_os_error() == Some(232) // ERROR_NO_DATA "The pipe is being closed"
                || e.raw_os_error() == Some(109) // ERROR_BROKEN_PIPE "The pipe has been ended"
        }
    }
}

impl std::io::Write for SafeStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.stdout.write(buf) {
            Ok(size) => Ok(size),
            Err(e) if Self::is_broken_pipe(&e) => {
                // Exit quietly on broken pipe to match Unix conventions
                ExitCode::SignalPipe.exit();
            }
            Err(e) => Err(e),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.stdout.flush() {
            Ok(()) => Ok(()),
            Err(e) if Self::is_broken_pipe(&e) => {
                // Exit quietly on broken pipe to match Unix conventions
                ExitCode::SignalPipe.exit();
            }
            Err(e) => Err(e),
        }
    }
}

/// Safe wrapper for writing to stderr
pub struct SafeStderr {
    stderr: io::Stderr,
}

impl SafeStderr {
    pub fn new() -> Self {
        Self {
            stderr: io::stderr(),
        }
    }

    /// Write a line to stderr, handling errors gracefully
    pub fn writeln(&mut self, data: &str) -> Result<()> {
        match writeln!(self.stderr, "{}", data) {
            Ok(()) => Ok(()),
            Err(e) => {
                // If we can't write to stderr, there's not much we can do
                // Just exit with a general error
                eprintln!(
                    "{}",
                    crate::config::format_error_message_auto(&format!(
                        "Fatal: Failed to write to stderr: {}",
                        e
                    ))
                );
                ExitCode::GeneralError.exit();
            }
        }
    }
}

/// Create a helpful error message for file creation failures
fn create_helpful_error_message(path: &Path, error: &io::Error) -> String {
    let base_msg = format!("Cannot create output file '{}': {}", path.display(), error);

    let suggestion = match error.kind() {
        io::ErrorKind::PermissionDenied => {
            if path.parent().is_some_and(|p| !p.exists()) {
                "Suggestion: Parent directory does not exist, create it first"
            } else {
                "Suggestion: Check file permissions or choose a writable location"
            }
        }
        io::ErrorKind::NotFound => "Suggestion: Parent directory does not exist, create it first",
        io::ErrorKind::AlreadyExists if path.is_dir() => {
            "Suggestion: Path points to a directory, specify a filename instead"
        }
        io::ErrorKind::InvalidInput => "Suggestion: Check for invalid characters in filename",
        _ => return base_msg, // No suggestion for other errors
    };

    format!("{}\n{}", base_msg, suggestion)
}

/// Safe wrapper for writing to a file that handles I/O errors gracefully
pub struct SafeFileOut {
    file: File,
    path: String,
}

impl SafeFileOut {
    /// Create a new SafeFileOut, truncating the file if it exists
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let path_string = path_ref.to_string_lossy().to_string();

        match File::create(path_ref) {
            Ok(file) => Ok(Self {
                file,
                path: path_string,
            }),
            Err(e) => {
                let error_msg = create_helpful_error_message(path_ref, &e);
                Err(anyhow::anyhow!("{}", error_msg))
            }
        }
    }

    /// Write a line to the file and flush immediately
    pub fn writeln(&mut self, data: &str) -> Result<()> {
        match writeln!(self.file, "{}", data) {
            Ok(()) => {
                // Flush after each write for immediate visibility to file watchers
                match self.file.flush() {
                    Ok(()) => Ok(()),
                    Err(e) => Err(anyhow::anyhow!(
                        "Output file flush failed '{}': {}",
                        self.path,
                        e
                    )),
                }
            }
            Err(e) => Err(anyhow::anyhow!(
                "Output file write failed '{}': {}",
                self.path,
                e
            )),
        }
    }

    /// Explicit flush (already done after each write, but provided for consistency)
    pub fn flush(&mut self) -> Result<()> {
        match self.file.flush() {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(
                "Output file flush failed '{}': {}",
                self.path,
                e
            )),
        }
    }
}

impl std::io::Write for SafeFileOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Utility function to check for termination between processing steps
pub fn check_termination() -> Result<()> {
    if SignalHandler::should_terminate() {
        return Err(anyhow::anyhow!("Processing terminated by signal"));
    }
    Ok(())
}

/// Process cleanup utilities
pub struct ProcessCleanup {
    cleanup_tasks: Vec<Box<dyn FnOnce() + Send>>,
}

static HOOK_INIT: Once = Once::new();

/// Install a panic hook that treats stdout BrokenPipe panics as normal termination.
pub fn install_broken_pipe_panic_hook() {
    HOOK_INIT.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if is_stdout_broken_pipe_panic(info) {
                ExitCode::SignalPipe.exit();
            }
            default_hook(info);
        }));
    });
}

fn is_stdout_broken_pipe_panic(info: &PanicHookInfo<'_>) -> bool {
    let payload_matches = |payload: &str| {
        let lower = payload.to_ascii_lowercase();
        lower.contains("failed printing to stdout")
            && (lower.contains("broken pipe")
                || lower.contains("os error 32")
                || lower.contains("os error 109")
                || lower.contains("os error 232"))
    };

    if let Some(message) = info.payload().downcast_ref::<&str>() {
        if payload_matches(message) {
            return true;
        }
    }

    if let Some(message) = info.payload().downcast_ref::<String>() {
        if payload_matches(message) {
            return true;
        }
    }

    false
}

impl ProcessCleanup {
    pub fn new() -> Self {
        Self {
            cleanup_tasks: Vec::new(),
        }
    }
}

impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        // If ProcessCleanup is dropped without explicit cleanup,
        // we should still try to clean up
        while let Some(task) = self.cleanup_tasks.pop() {
            task();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(ExitCode::Success as i32, 0);
        assert_eq!(ExitCode::GeneralError as i32, 1);
        assert_eq!(ExitCode::InvalidUsage as i32, 2);
        assert_eq!(ExitCode::SignalInt as i32, 130);
        assert_eq!(ExitCode::SignalPipe as i32, 141);
        assert_eq!(ExitCode::SignalTerm as i32, 143);
    }

    #[test]
    fn test_should_terminate_initial_state() {
        // Should start as false
        assert!(!SignalHandler::should_terminate());
    }
}
