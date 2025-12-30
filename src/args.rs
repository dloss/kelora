//! CLI argument processing module
//!
//! This module handles parsing, validating, and processing command-line arguments,
//! including config file handling, aliases, and help text display.

use anyhow::Result;
use clap::{ArgMatches, CommandFactory, FromArgMatches};

use crate::cli::{Cli, OutputFormat};
use crate::config_file::ConfigFile;
use crate::help;
use crate::platform::{ExitCode, SafeStderr};
use crate::tty;

/// Validate CLI arguments for early error detection
pub fn validate_cli_args(cli: &Cli) -> Result<()> {
    // Validate --no-input conflicts
    if cli.no_input && !cli.files.is_empty() {
        return Err(anyhow::anyhow!(
            "--no-input cannot be used with input files"
        ));
    }

    // Check stdin usage
    let mut stdin_count = 0;
    for file_path in &cli.files {
        if file_path == "-" {
            stdin_count += 1;
            if stdin_count > 1 {
                return Err(anyhow::anyhow!("stdin (\"-\") can only be specified once"));
            }
        }
        // Note: File existence is checked at runtime during processing (exit 1),
        // not during CLI validation (exit 2)
    }

    // Check if exec files exist (if specified)
    for exec_file in &cli.exec_files {
        if !std::path::Path::new(exec_file).exists() {
            return Err(anyhow::anyhow!("Exec file not found: {}", exec_file));
        }
    }

    // Validate batch size
    if let Some(batch_size) = cli.batch_size {
        if batch_size == 0 {
            return Err(anyhow::anyhow!("Batch size must be greater than 0"));
        }
    }

    // Validate thread count
    if cli.threads > 1000 {
        return Err(anyhow::anyhow!("Thread count too high (max 1000)"));
    }

    if cli.span_close.is_some() && cli.span.is_none() && cli.span_idle.is_none() {
        return Err(anyhow::anyhow!(
            "--span-close requires --span or --span-idle to be specified"
        ));
    }

    // Check for --core with CSV/TSV formats (not allowed with these formats)
    if cli.core {
        match cli.output_format {
            OutputFormat::Csv => {
                return Err(anyhow::anyhow!(
                    "csv output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Tsv => {
                return Err(anyhow::anyhow!(
                    "tsv output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Csvnh => {
                return Err(anyhow::anyhow!(
                    "csvnh output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            OutputFormat::Tsvnh => {
                return Err(anyhow::anyhow!(
                    "tsvnh output format does not support --core flag. Use --keys to specify field names"
                ));
            }
            _ => {
                // Other formats are fine with --core
            }
        }
    }

    Ok(())
}

/// Extract --config-file argument from raw args
pub fn extract_config_file_arg(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "--config-file" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

/// Extract --save-alias argument from raw args
pub fn extract_save_alias_arg(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "--save-alias" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

/// Check if the given alias_name appears in any `-a` or `--alias` reference in the args
pub fn should_resolve_alias_references(args: &[String], alias_name: &str) -> bool {
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-a" || args[i] == "--alias") && i + 1 < args.len() {
            if args[i + 1] == alias_name {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// Handle --save-alias command
pub fn handle_save_alias(raw_args: &[String], alias_name: &str, use_emoji: bool) {
    // Extract --config-file if specified
    let mut config_file_path: Option<String> = None;
    let mut command_args = Vec::new();
    let mut i = 0;
    while i < raw_args.len() {
        if raw_args[i] == "--save-alias" {
            // Skip --save-alias and its argument
            i += 2;
        } else if raw_args[i] == "--config-file" && i + 1 < raw_args.len() {
            // Extract --config-file for saving
            config_file_path = Some(raw_args[i + 1].clone());
            i += 2;
        } else {
            command_args.push(raw_args[i].clone());
            i += 1;
        }
    }

    // Skip the program name (first argument)
    if !command_args.is_empty() {
        command_args.remove(0);
    }

    // Check if we have any command left to save
    if command_args.is_empty() {
        let prefix = if use_emoji { "⚠️" } else { "kelora:" };
        eprintln!("{} No command to save as alias '{}'", prefix, alias_name);
        std::process::exit(2);
    }

    // Check if we should resolve alias references (when updating self-referencing alias)
    let should_resolve = should_resolve_alias_references(&command_args, alias_name);

    // If we need to resolve OR validate, load the config file
    let alias_value = if command_args
        .iter()
        .any(|arg| arg == "-a" || arg == "--alias")
    {
        // Command contains alias references - need to load config
        let config_result = match config_file_path.as_ref() {
            Some(path) => ConfigFile::load_with_custom_path(Some(path)),
            None => ConfigFile::load_with_custom_path(None),
        };

        match config_result {
            Ok(config) => {
                if should_resolve {
                    // Resolution mode: flatten all aliases
                    match config.resolve_args_only(&command_args) {
                        Ok(resolved_args) => {
                            if resolved_args.is_empty() {
                                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                                eprintln!(
                                    "{} Resolved command is empty for alias '{}'",
                                    prefix, alias_name
                                );
                                std::process::exit(2);
                            }
                            shell_words::join(resolved_args)
                        }
                        Err(e) => {
                            let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                            eprintln!("{} Failed to resolve aliases in command: {}", prefix, e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    // Preservation mode: validate references exist but keep them
                    if let Err(e) = config.validate_alias_references(&command_args) {
                        let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                        eprintln!("{} {}", prefix, e);
                        eprintln!(
                            "{} Cannot save alias '{}' with reference to non-existent alias",
                            prefix, alias_name
                        );
                        std::process::exit(1);
                    }
                    shell_words::join(command_args)
                }
            }
            Err(_) if should_resolve => {
                // Trying to update non-existent alias
                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                eprintln!(
                    "{} Cannot update alias '{}' - no config file found",
                    prefix, alias_name
                );
                eprintln!(
                    "{} To create a new alias, use a command without referencing itself",
                    prefix
                );
                std::process::exit(1);
            }
            Err(_) => {
                // Preservation mode but config doesn't exist - that's an error
                // because we're referencing other aliases that don't exist
                let prefix = if use_emoji { "⚠️" } else { "kelora:" };
                eprintln!(
                    "{} Cannot save alias '{}' with alias references - no config file found",
                    prefix, alias_name
                );
                eprintln!(
                    "{} Create the referenced aliases first, or use a command without alias references",
                    prefix
                );
                std::process::exit(1);
            }
        }
    } else {
        // No alias references - just join and save
        shell_words::join(command_args)
    };

    // Save the alias to the specified config file or auto-detect
    let target_path = config_file_path.as_ref().map(std::path::Path::new);
    match ConfigFile::save_alias(alias_name, &alias_value, target_path) {
        Ok((config_path, previous_value)) => {
            let success_prefix = if use_emoji { "🔹" } else { "kelora:" };
            println!(
                "{} Alias '{}' saved to {}",
                success_prefix,
                alias_name,
                config_path.display()
            );

            if let Some(prev) = previous_value {
                let info_prefix = if use_emoji { "🔹" } else { "kelora:" };
                println!("{} Replaced previous alias:", info_prefix);
                println!("    {} = {}", alias_name, prev);
            }
        }
        Err(e) => {
            let error_prefix = if use_emoji { "⚠️" } else { "kelora:" };
            eprintln!(
                "{} Failed to save alias '{}': {}",
                error_prefix, alias_name, e
            );
            std::process::exit(1);
        }
    }
}

/// Process command line arguments with config file support
pub fn process_args_with_config(stderr: &mut SafeStderr) -> (ArgMatches, Cli) {
    // Get raw command line arguments
    let raw_args: Vec<String> = std::env::args().collect();

    // Extract --config-file argument early for use by config commands
    let config_file_path = extract_config_file_arg(&raw_args);

    // Check for config-related option conflicts
    let has_show_config = raw_args.iter().any(|arg| arg == "--show-config");
    let has_edit_config = raw_args.iter().any(|arg| arg == "--edit-config");
    let has_ignore_config = raw_args.iter().any(|arg| arg == "--ignore-config");

    if has_show_config && has_edit_config {
        stderr
            .writeln("kelora: Error: --show-config and --edit-config are mutually exclusive")
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    if has_ignore_config && has_edit_config {
        stderr
            .writeln("kelora: Error: --ignore-config and --edit-config are mutually exclusive")
            .unwrap_or(());
        ExitCode::InvalidUsage.exit();
    }

    // Check for --show-config first, before any other processing
    if has_show_config {
        ConfigFile::show_config();
        std::process::exit(0);
    }

    // Check for --edit-config
    if has_edit_config {
        ConfigFile::edit_config(config_file_path.as_deref());
        std::process::exit(0);
    }

    // Check for --help-time
    if raw_args.iter().any(|arg| arg == "--help-time") {
        help::print_time_format_help();
        std::process::exit(0);
    }

    // Check for --help-functions
    if raw_args.iter().any(|arg| arg == "--help-functions") {
        help::print_functions_help();
        std::process::exit(0);
    }

    // Check for -h (brief help)
    if raw_args.iter().any(|arg| arg == "-h") {
        help::print_quick_help();
        std::process::exit(0);
    }

    // Check for --help-examples
    if raw_args.iter().any(|arg| arg == "--help-examples") {
        help::print_examples_help();
        std::process::exit(0);
    }

    // Check for --help-rhai
    if raw_args.iter().any(|arg| arg == "--help-rhai") {
        help::print_rhai_help();
        std::process::exit(0);
    }

    // Check for --help-multiline
    if raw_args.iter().any(|arg| arg == "--help-multiline") {
        help::print_multiline_help();
        std::process::exit(0);
    }

    // Check for --help-regex
    if raw_args.iter().any(|arg| arg == "--help-regex") {
        help::print_regex_help();
        std::process::exit(0);
    }

    // Check for --help-formats
    if raw_args.iter().any(|arg| arg == "--help-formats") {
        help::print_formats_help();
        std::process::exit(0);
    }

    // Check for --save-alias before other processing
    if let Some(alias_name) = extract_save_alias_arg(&raw_args) {
        let use_emoji = tty::should_use_emoji_for_stderr();
        handle_save_alias(&raw_args, &alias_name, use_emoji);
        std::process::exit(0);
    }

    // Check for --ignore-config
    let ignore_config = has_ignore_config;

    let processed_args = if ignore_config {
        // Skip config file processing
        raw_args
    } else {
        // Load config file and process aliases
        match ConfigFile::load_with_custom_path(config_file_path.as_deref()) {
            Ok(config_file) => match config_file.process_args(raw_args) {
                Ok(processed) => processed,
                Err(e) => {
                    stderr
                        .writeln(&format!("kelora: Config error: {}", e))
                        .unwrap_or(());
                    std::process::exit(1);
                }
            },
            Err(e) => {
                stderr
                    .writeln(&format!("kelora: Config file error: {}", e))
                    .unwrap_or(());
                std::process::exit(1);
            }
        }
    };

    // Parse with potentially modified arguments
    let matches = Cli::command().get_matches_from(processed_args);
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| {
        stderr
            .writeln(&format!("kelora: Error: {}", e))
            .unwrap_or(());
        std::process::exit(1);
    });

    // Resolve inverted boolean flags
    cli.resolve_boolean_flags();

    // Config file defaults and aliases are already applied in process_args above

    // Check if we should enter interactive mode
    // Interactive mode is activated when:
    // - stdin is a TTY (not piped input)
    // - no input files are provided
    // - --no-input is not specified
    // - no other arguments are provided (just the program name)
    if crate::tty::is_stdin_tty() && cli.files.is_empty() && !cli.no_input {
        // Check if this is truly no arguments (interactive mode) or just missing input files
        let raw_args: Vec<String> = std::env::args().collect();

        // If only program name, enter interactive mode
        if raw_args.len() == 1 {
            // Enter interactive mode
            if let Err(e) = crate::interactive::run_interactive_mode() {
                eprintln!("Interactive mode error: {}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }

        // Otherwise, show error (user provided flags but no input files)
        eprintln!("error: no input files or stdin provided");
        eprintln!();
        eprintln!("{}", Cli::command().render_usage());
        eprintln!();
        eprintln!("For more information, try '-h'.");
        std::process::exit(2);
    }

    (matches, cli)
}
