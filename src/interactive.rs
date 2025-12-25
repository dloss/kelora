// Interactive mode for kelora
// Provides a readline-based REPL for running kelora commands

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;

/// Run interactive mode
/// This provides a readline-based prompt where users can enter kelora commands
/// without dealing with shell quoting issues (especially helpful on Windows)
pub fn run_interactive_mode() -> Result<()> {
    let mut rl = DefaultEditor::new()?;

    // Set up history file
    let history_path = get_history_path();
    if let Some(ref path) = history_path {
        // Ignore errors when loading history (file might not exist yet)
        let _ = rl.load_history(path);
    }

    println!("Kelora Interactive Mode");
    println!("Type commands as you would on the command line.");
    println!("  - Type 'exit' or 'quit' to exit, or press Ctrl-D");
    println!("  - Press Ctrl-C to cancel running commands");
    println!("  - Use 'help' for kelora help, or try '--help' for full options");
    println!("  - Glob patterns like *.log are automatically expanded\n");

    loop {
        let readline = rl.readline("kelora> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();

                // Skip empty lines
                if trimmed.is_empty() {
                    continue;
                }

                // Add to history
                let _ = rl.add_history_entry(trimmed);

                // Check for exit commands
                if trimmed == "exit" || trimmed == "quit" {
                    break;
                }

                // Handle special commands
                if trimmed == "help" {
                    println!("Interactive mode help:");
                    println!("  - Enter kelora commands without the 'kelora' prefix");
                    println!("  - Example: -j access.log --filter 'e.status >= 500'");
                    println!("  - Use quotes for arguments with spaces");
                    println!("  - Glob patterns are automatically expanded (*.log, test?.json)");
                    println!("  - Type '--help' to see all kelora options");
                    println!("  - Type 'exit' or 'quit' to leave interactive mode");
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
                // Ctrl-C at the prompt - just show a new prompt
                continue;
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
