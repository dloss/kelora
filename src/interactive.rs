// Interactive mode for kelora
// Provides a readline-based REPL for running kelora commands

use anyhow::Result;
use rustyline::completion::{Completer, FilenameCompleter};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, Helper};
use std::path::PathBuf;

/// Helper for interactive mode with file completion
#[derive(Default)]
struct KeloraHelper {
    completer: FilenameCompleter,
}

impl Completer for KeloraHelper {
    type Candidate = <FilenameCompleter as Completer>::Candidate;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for KeloraHelper {
    type Hint = String;
}

impl Highlighter for KeloraHelper {}

impl Validator for KeloraHelper {}

impl Helper for KeloraHelper {}

/// Run interactive mode
/// This provides a readline-based prompt where users can enter kelora commands
/// without dealing with shell quoting issues (especially helpful on Windows)
pub fn run_interactive_mode() -> Result<()> {
    // Configure editor with file completion
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let helper = KeloraHelper::default();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(helper));

    // Set up history file
    let history_path = get_history_path();
    if let Some(ref path) = history_path {
        // Ignore errors when loading history (file might not exist yet)
        let _ = rl.load_history(path);
    }

    println!("Kelora Interactive Mode — :quit to exit, :help for help\n");

    let mut consecutive_interrupts = 0;

    loop {
        let readline = rl.readline("kelora> ");
        match readline {
            Ok(line) => {
                // Reset interrupt counter on successful input
                consecutive_interrupts = 0;

                let trimmed = line.trim();

                // Skip empty lines
                if trimmed.is_empty() {
                    continue;
                }

                // Add to history
                let _ = rl.add_history_entry(trimmed);

                // Check for REPL commands (colon-prefixed)
                if trimmed == ":exit" || trimmed == ":quit" || trimmed == ":q" {
                    break;
                }

                // Bare `exit`/`quit` is a shell reflex; honour it unless a file
                // by that name exists, which the user may really mean to read.
                if (trimmed == "exit" || trimmed == "quit")
                    && !std::path::Path::new(trimmed).exists()
                {
                    break;
                }

                if trimmed == ":help" {
                    let eof_key = if cfg!(windows) { "Ctrl-Z" } else { "Ctrl-D" };
                    println!("Interactive mode - enter kelora commands without 'kelora' prefix");
                    println!();
                    println!("  TAB          Complete files/directories");
                    println!("  *.log        Glob patterns auto-expand");
                    println!("  'foo bar'    Use quotes when args contain spaces");
                    println!("  -h           Quick help (or --help for full)");
                    println!();
                    println!("  Ctrl-C       Cancel running command (press twice to exit)");
                    println!("  :quit        Exit (or :q, :exit, {})", eof_key);
                    println!();
                    println!("Example: -j mylog.json --filter 'e.status >= 500'");
                    continue;
                }

                // Parse the command line
                match parse_and_execute_command(trimmed) {
                    Ok(()) => {
                        // Command executed successfully
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C at the prompt - Node.js style two-press-to-quit
                consecutive_interrupts += 1;
                if consecutive_interrupts >= 2 {
                    println!("\nExiting...");
                    break;
                } else {
                    println!("(To exit, press Ctrl-C again or type :quit)");
                    continue;
                }
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D - exit
                break;
            }
            Err(err) => {
                eprintln!("Error reading line: {}", err);
                break;
            }
        }
    }

    // Save history
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    Ok(())
}

/// Parse a command line and execute it
fn parse_and_execute_command(line: &str) -> Result<()> {
    // Parse the line using shell-words to handle quoting
    let words = shell_words::split(line)?;

    if words.is_empty() {
        return Ok(());
    }

    // Expand globs in the arguments
    let expanded_args = expand_globs(&words)?;

    // Build the full argument vector (prepend program name)
    let mut args = vec!["kelora".to_string()];
    args.extend(expanded_args);

    // Execute the command by calling the main processing function
    // We'll need to refactor main.rs to expose this functionality
    execute_kelora_command(args)?;

    Ok(())
}

/// Expand glob patterns in arguments
fn expand_globs(args: &[String]) -> Result<Vec<String>> {
    let mut result = Vec::new();

    for arg in args {
        // Check if this looks like a glob pattern
        if arg.contains('*') || arg.contains('?') || arg.contains('[') {
            // Try to expand it
            let mut matches: Vec<String> = glob::glob(arg)?
                .filter_map(|path| path.ok())
                .map(|path| path.to_string_lossy().to_string())
                .collect();

            if matches.is_empty() {
                // No matches - keep the original pattern
                result.push(arg.clone());
            } else {
                // Sort for consistent ordering
                matches.sort();
                result.extend(matches);
            }
        } else {
            // Not a glob pattern - keep as is
            result.push(arg.clone());
        }
    }

    Ok(result)
}

/// Execute a kelora command with the given arguments
/// This spawns kelora as a subprocess with the given arguments
fn execute_kelora_command(args: Vec<String>) -> Result<()> {
    use std::process::Command;

    // Get the current executable path
    let exe_path = std::env::current_exe()?;

    // Skip the first argument (program name) since Command will add it
    let cmd_args = &args[1..];

    // Spawn kelora as a subprocess
    let status = Command::new(&exe_path).args(cmd_args).status()?;

    // Check if the command was successful
    if !status.success() {
        // The subprocess will have already printed error messages
        // We just note that it failed
        if let Some(code) = status.code() {
            if code != 0 {
                // Don't print anything - the error was already shown by the subprocess
            }
        }
    }

    Ok(())
}

/// Get the path to the history file
fn get_history_path() -> Option<PathBuf> {
    dirs::config_dir().and_then(|mut path| {
        path.push("kelora");

        // Create the directory if it doesn't exist
        if let Err(_e) = std::fs::create_dir_all(&path) {
            return None;
        }

        path.push("interactive_history.txt");
        Some(path)
    })
}
