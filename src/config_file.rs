#![allow(dead_code)] // Config file helpers for future CLI edits are not exercised by current binary paths
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// Configuration file handler for kelora
#[derive(Default)]
pub struct ConfigFile {
    pub defaults: Option<String>,
    pub aliases: HashMap<String, String>,
}

/// Information about config file loading and expansion
#[derive(Debug, Clone, Default)]
pub struct ConfigExpansionInfo {
    /// Path to the config file that was loaded (if any)
    pub loaded_config_path: Option<PathBuf>,
    /// Whether the user explicitly selected a config file path via --config-file
    pub explicit_config_path: bool,
    /// Defaults that were applied (raw string from config)
    pub applied_defaults: Option<String>,
    /// Aliases that were expanded: (alias_name, final_expansion)
    pub expanded_aliases: Vec<(String, String)>,
}

impl ConfigFile {
    /// Find project-level .kelora.ini by walking up directory tree
    pub fn find_project_config() -> Option<PathBuf> {
        let mut current = std::env::current_dir().ok()?;
        loop {
            let config_path = current.join(".kelora.ini");
            if config_path.exists() {
                return Some(config_path);
            }
            if !current.pop() {
                // Reached filesystem root
                break;
            }
        }
        None
    }

    /// Get user config file location with XDG compliance
    pub fn get_user_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if cfg!(windows) {
            // Windows: %APPDATA%\kelora\kelora.ini
            if let Ok(appdata) = env::var("APPDATA") {
                paths.push(PathBuf::from(appdata).join("kelora").join("kelora.ini"));
            }
        } else {
            // Unix: $XDG_CONFIG_HOME/kelora/kelora.ini or ~/.config/kelora/kelora.ini
            let config_dir = env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    env::var("HOME")
                        .map(|h| PathBuf::from(h).join(".config"))
                        .unwrap_or_else(|_| PathBuf::from(".config"))
                });

            paths.push(config_dir.join("kelora").join("kelora.ini"));
        }

        paths
    }

    /// Get list of all config file locations in precedence order
    /// Order: project .kelora.ini > user kelora.ini
    pub fn get_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Project config has highest precedence
        if let Some(project_config) = Self::find_project_config() {
            paths.push(project_config);
        }

        // Add user config files
        paths.extend(Self::get_user_config_paths());

        paths
    }

    /// Find the first existing configuration file
    pub fn find_config_path() -> Option<PathBuf> {
        Self::get_config_paths().into_iter().find(|p| p.exists())
    }

    /// Load configuration with proper precedence: project > user > defaults
    /// Returns the config and the path of the highest-precedence config file loaded
    pub fn load() -> Result<(Self, Option<PathBuf>)> {
        let mut config = Self::default();
        let mut loaded_path: Option<PathBuf> = None;

        // First, load user config files (lowest precedence)
        for path in Self::get_user_config_paths() {
            if path.exists() {
                let user_config = Self::load_from_path(&path)?;
                config = Self::merge_configs(config, user_config);
                loaded_path = Some(path);
                break; // Use first existing user config file
            }
        }

        // Then, load project config (higher precedence)
        if let Some(project_path) = Self::find_project_config() {
            let project_config = Self::load_from_path(&project_path)?;
            config = Self::merge_configs(config, project_config);
            loaded_path = Some(project_path); // Project config takes precedence
        }

        Ok((config, loaded_path))
    }

    /// Load configuration with optional custom config file path
    /// Returns the config and the path of the config file that was loaded
    pub fn load_with_custom_path(custom_path: Option<&str>) -> Result<(Self, Option<PathBuf>)> {
        if let Some(path) = custom_path {
            // Use custom path
            let path_buf = std::path::PathBuf::from(path);
            let config = Self::load_from_path(&path_buf)?;
            Ok((config, Some(path_buf)))
        } else {
            // Use default loading logic
            Self::load()
        }
    }

    /// Load configuration from a specific path
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        use std::fs;

        // Read the file content first
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        // Parse the INI content
        let config = Self::parse_ini_content(&content)?;

        Ok(config)
    }

    /// Parse INI content from string
    fn parse_ini_content(content: &str) -> Result<Self> {
        let mut defaults = None;
        let mut aliases = HashMap::new();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }

            // Check for section headers
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_string();
                continue;
            }

            // Parse key=value pairs
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let value = line[eq_pos + 1..].trim();

                if current_section.is_empty() {
                    // Root-level configuration
                    if key == "defaults" {
                        defaults = Some(value.to_string());
                    }
                    // Ignore unknown root-level keys
                } else {
                    match current_section.as_str() {
                        "aliases" => {
                            aliases.insert(key.to_string(), value.to_string());
                        }
                        _ => {
                            // Ignore unknown sections
                        }
                    }
                }
            }
        }

        Ok(Self { defaults, aliases })
    }

    /// Merge two configuration objects, with the second taking precedence
    fn merge_configs(base: Self, overlay: Self) -> Self {
        Self {
            // Overlay defaults override base defaults
            defaults: overlay.defaults.or(base.defaults),
            // Merge aliases, with overlay taking precedence for conflicting keys
            aliases: {
                let mut merged = base.aliases;
                merged.extend(overlay.aliases);
                merged
            },
        }
    }

    /// Show configuration information with precedence details
    pub fn show_config() {
        let project_config_path = Self::find_project_config();
        let user_config_paths = Self::get_user_config_paths();
        let user_config_path = user_config_paths.iter().find(|p| p.exists());

        // Determine which config to show
        let config_path = project_config_path.as_ref().or(user_config_path);

        if let Some(path) = config_path {
            // Config found - show file path and full contents
            println!("# {}", path.display());
            println!();

            match std::fs::read_to_string(path) {
                Ok(content) => print!("{}", content),
                Err(e) => eprintln!("Error reading config: {}", e),
            }
        } else {
            // No config found - show where we looked and example
            println!(
                "No config found (searched: .kelora.ini from cwd upward, {})",
                user_config_paths[0].display()
            );
            println!();
            println!("Example .kelora.ini:");
            println!("defaults = -f auto --stats");
            println!();
            println!("[aliases]");
            println!("errors = -l error --stats");
            println!("json-errors = -f json -l error -F json");
            println!("slow-requests = --filter 'e.response_time.to_int() > 1000' --keys timestamp,method,path,response_time");
        }
    }

    /// Edit configuration file in default editor
    pub fn edit_config(custom_path: Option<&str>) {
        use std::fs;
        use std::process::Command;

        // Determine which config file to edit using same logic as show_config
        let config_path = if let Some(path) = custom_path {
            PathBuf::from(path)
        } else {
            let project_config_path = Self::find_project_config();
            let user_config_paths = Self::get_user_config_paths();
            let user_config_path = user_config_paths.iter().find(|p| p.exists());

            project_config_path
                .or_else(|| user_config_path.cloned())
                .unwrap_or_else(|| user_config_paths[0].clone())
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!(
                        "kelora: Failed to create directory {}: {}",
                        parent.display(),
                        e
                    );
                    std::process::exit(1);
                }
            }
        }

        // Get editor from environment
        let editor = if cfg!(windows) {
            env::var("EDITOR").unwrap_or_else(|_| "notepad.exe".to_string())
        } else {
            env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
        };

        // Print the file being edited
        println!("Opening {} in {}...", config_path.display(), editor);

        // Spawn editor and wait for it to complete
        match Command::new(&editor).arg(&config_path).status() {
            Ok(status) => {
                if !status.success() {
                    std::process::exit(status.code().unwrap_or(1));
                }
            }
            Err(e) => {
                eprintln!("kelora: Failed to launch editor '{}': {}", editor, e);
                eprintln!("You can set the EDITOR environment variable to your preferred editor.");
                std::process::exit(1);
            }
        }
    }

    /// Resolve a single alias, handling recursive references
    pub fn resolve_alias(
        &self,
        name: &str,
        seen: &mut std::collections::HashSet<String>,
        depth: usize,
    ) -> Result<Vec<String>> {
        const MAX_DEPTH: usize = 10;

        if depth > MAX_DEPTH {
            return Err(anyhow!("Alias chain too deep: {} levels", depth));
        }

        if seen.contains(name) {
            return Err(anyhow!("Circular dependency detected in alias: {}", name));
        }

        let alias_value = self
            .aliases
            .get(name)
            .ok_or_else(|| anyhow!("Unknown alias: {}", name))?;

        seen.insert(name.to_string());

        // Split the alias value into args using shell-like parsing
        let args = shell_words::split(alias_value)
            .with_context(|| format!("Invalid alias '{}': failed to parse arguments", name))?;

        let mut result = Vec::new();
        let mut i = 0;

        while i < args.len() {
            if (args[i] == "-a" || args[i] == "--alias") && i + 1 < args.len() {
                // Recursively resolve the referenced alias
                let ref_name = &args[i + 1];
                let mut new_seen = seen.clone();
                let resolved = self.resolve_alias(ref_name, &mut new_seen, depth + 1)?;
                result.extend(resolved);
                i += 2;
            } else {
                result.push(args[i].clone());
                i += 1;
            }
        }

        seen.remove(name);
        Ok(result)
    }

    /// Check if an alias exists in the config
    pub fn has_alias(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
    }

    /// Process command line arguments, expanding aliases and applying defaults
    /// Returns the processed args and information about what was expanded
    pub fn process_args(&self, args: Vec<String>) -> Result<(Vec<String>, ConfigExpansionInfo)> {
        let mut info = ConfigExpansionInfo::default();

        // First, apply defaults if they exist
        let mut result = Vec::new();

        // Add defaults at the beginning, but preserve the program name
        if let Some(defaults) = &self.defaults {
            info.applied_defaults = Some(defaults.clone());

            if !args.is_empty() {
                result.push(args[0].clone()); // Keep program name
            }

            // Parse defaults using shell_words and add them
            let default_args = shell_words::split(defaults)
                .with_context(|| "Invalid defaults: failed to parse arguments".to_string())?;
            result.extend(default_args);

            // Add remaining user args (skip program name)
            result.extend(args.into_iter().skip(1));
        } else {
            result = args;
        }

        // Then expand aliases
        let mut final_result = Vec::new();
        let mut i = 0;

        while i < result.len() {
            if (result[i] == "-a" || result[i] == "--alias") && i + 1 < result.len() {
                let name = result[i + 1].clone();
                let mut seen = std::collections::HashSet::new();
                let resolved = self.resolve_alias(&name, &mut seen, 0)?;

                // Track the expansion for display
                let expansion = resolved.join(" ");
                info.expanded_aliases.push((name, expansion));

                final_result.extend(resolved);
                i += 2;
            } else {
                final_result.push(result[i].clone());
                i += 1;
            }
        }

        Ok((final_result, info))
    }

    /// Resolve all `-a` and `--alias` flags in the given arguments, returning a flattened list
    /// This is similar to process_args but only handles alias resolution, not defaults
    pub fn resolve_args_only(&self, args: &[String]) -> Result<Vec<String>> {
        let mut result = Vec::new();
        let mut i = 0;

        while i < args.len() {
            if (args[i] == "-a" || args[i] == "--alias") && i + 1 < args.len() {
                let name = &args[i + 1];
                let mut seen = std::collections::HashSet::new();
                let resolved = self.resolve_alias(name, &mut seen, 0)?;
                result.extend(resolved);
                i += 2;
            } else {
                result.push(args[i].clone());
                i += 1;
            }
        }

        Ok(result)
    }

    /// Scan arguments for `-a` and `--alias` flags and validate that all referenced aliases exist
    /// Returns Ok(()) if all aliases exist, or an error with the name of the first missing alias
    pub fn validate_alias_references(&self, args: &[String]) -> Result<()> {
        let mut i = 0;
        while i < args.len() {
            if (args[i] == "-a" || args[i] == "--alias") && i + 1 < args.len() {
                let name = &args[i + 1];
                if !self.has_alias(name) {
                    return Err(anyhow!(
                        "Referenced alias '{}' does not exist in config",
                        name
                    ));
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    /// Write config file to specified path
    pub fn write_to_path(&self, path: &std::path::Path) -> Result<()> {
        use std::fs;
        use std::io::Write;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let mut content = String::new();

        // Write defaults section if present
        if let Some(defaults) = &self.defaults {
            content.push_str(&format!("defaults = {}\n", defaults));
        }

        // Write aliases section if not empty
        if !self.aliases.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str("[aliases]\n");

            // Sort aliases for consistent output
            let mut sorted_aliases: Vec<_> = self.aliases.iter().collect();
            sorted_aliases.sort_by_key(|(k, _)| k.as_str());

            for (name, value) in sorted_aliases {
                content.push_str(&format!("{} = {}\n", name, value));
            }
        }

        // Write to file atomically
        let temp_path = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&temp_path).with_context(|| {
                format!("Failed to create temporary file: {}", temp_path.display())
            })?;
            file.write_all(content.as_bytes()).with_context(|| {
                format!("Failed to write to temporary file: {}", temp_path.display())
            })?;
            file.sync_all().with_context(|| {
                format!("Failed to sync temporary file: {}", temp_path.display())
            })?;
        }

        // Atomic rename
        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to rename temporary file to: {}", path.display()))?;

        Ok(())
    }

    /// Save alias to config file, returns the previous value if it existed
    pub fn save_alias(
        alias_name: &str,
        alias_value: &str,
        target_path: Option<&std::path::Path>,
    ) -> Result<(PathBuf, Option<String>)> {
        // Validate alias name
        let alias_regex = regex::Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$").unwrap();
        if !alias_regex.is_match(alias_name) {
            return Err(anyhow!(
                "Invalid alias name '{}'. Must match pattern: ^[a-zA-Z_][a-zA-Z0-9_-]{{0,63}}$",
                alias_name
            ));
        }

        // Determine target config file path
        let config_path = if let Some(path) = target_path {
            path.to_path_buf()
        } else {
            // Use same logic as --show-config
            if let Some(project_path) = Self::find_project_config() {
                project_path
            } else if let Some(user_path) =
                Self::get_user_config_paths().iter().find(|p| p.exists())
            {
                user_path.clone()
            } else {
                Self::get_user_config_paths()[0].clone()
            }
        };

        // Read existing content or start fresh
        let (previous_value, new_content) = if config_path.exists() {
            use std::fs;
            let content = fs::read_to_string(&config_path).with_context(|| {
                format!("Failed to read config file: {}", config_path.display())
            })?;

            Self::update_alias_in_content(&content, alias_name, alias_value)?
        } else {
            // New file - create minimal content with just the alias
            let content = format!("[aliases]\n{} = {}\n", alias_name, alias_value);
            (None, content)
        };

        // Write the updated content
        use std::fs;

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        fs::write(&config_path, new_content.as_bytes())
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;

        Ok((config_path, previous_value))
    }

    /// Update or add an alias in existing INI content, preserving all other content
    fn update_alias_in_content(
        content: &str,
        alias_name: &str,
        alias_value: &str,
    ) -> Result<(Option<String>, String)> {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut in_aliases_section = false;
        let mut aliases_section_start: Option<usize> = None;
        let mut alias_line_index: Option<usize> = None;
        let mut previous_value: Option<String> = None;
        let mut last_section_line: Option<usize> = None;

        // Find the [aliases] section and the specific alias if it exists
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Check for section headers
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if in_aliases_section {
                    // We've hit a new section after [aliases]
                    last_section_line = Some(i - 1);
                    break;
                }

                let section_name = &trimmed[1..trimmed.len() - 1];
                if section_name == "aliases" {
                    in_aliases_section = true;
                    aliases_section_start = Some(i);
                }
                continue;
            }

            // If we're in the [aliases] section, look for our alias
            if in_aliases_section
                && !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !trimmed.starts_with(';')
            {
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim();
                    if key == alias_name {
                        // Found existing alias
                        let value = trimmed[eq_pos + 1..].trim();
                        previous_value = Some(value.to_string());
                        alias_line_index = Some(i);
                        break;
                    }
                }
            }
        }

        // If we didn't find the end of aliases section yet, it extends to the end
        if in_aliases_section && last_section_line.is_none() {
            last_section_line = Some(lines.len() - 1);
        }

        let new_alias_line = format!("{} = {}", alias_name, alias_value);

        // Now update the content based on what we found
        if let Some(idx) = alias_line_index {
            // Replace existing alias in place
            lines[idx] = new_alias_line;
        } else if let Some(section_start) = aliases_section_start {
            // [aliases] section exists but alias not found - add it at the end of the section
            let insert_pos = last_section_line
                .map(|pos| pos + 1)
                .unwrap_or(section_start + 1);
            lines.insert(insert_pos, new_alias_line);
        } else {
            // No [aliases] section - add it at the end
            if !lines.is_empty() && !lines[lines.len() - 1].is_empty() {
                lines.push(String::new()); // Add blank line before new section
            }
            lines.push("[aliases]".to_string());
            lines.push(new_alias_line);
        }

        // Ensure file ends with newline
        let mut result = lines.join("\n");
        if !result.ends_with('\n') {
            result.push('\n');
        }

        Ok((previous_value, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "defaults = -f json -F csv").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "[aliases]").unwrap();
        writeln!(file, "errors = -l error").unwrap();
        writeln!(file, "json-logs = -f json -F json").unwrap();
        file.flush().unwrap();

        let config = ConfigFile::load_from_path(&file.path().to_path_buf()).unwrap();

        assert_eq!(config.defaults, Some("-f json -F csv".to_string()));
        assert_eq!(config.aliases.get("errors"), Some(&"-l error".to_string()));
        assert_eq!(
            config.aliases.get("json-logs"),
            Some(&"-f json -F json".to_string())
        );
    }

    #[test]
    fn test_resolve_alias() {
        let mut config = ConfigFile::default();
        config
            .aliases
            .insert("errors".to_string(), "-l error".to_string());
        config
            .aliases
            .insert("json-errors".to_string(), "-f json -a errors".to_string());

        let mut seen = std::collections::HashSet::new();
        let resolved = config.resolve_alias("json-errors", &mut seen, 0).unwrap();

        assert_eq!(resolved, vec!["-f", "json", "-l", "error"]);
    }

    #[test]
    fn test_circular_alias_detection() {
        let mut config = ConfigFile::default();
        config
            .aliases
            .insert("alias1".to_string(), "-a alias2".to_string());
        config
            .aliases
            .insert("alias2".to_string(), "-a alias1".to_string());

        let mut seen = std::collections::HashSet::new();
        let result = config.resolve_alias("alias1", &mut seen, 0);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Circular dependency"));
    }

    #[test]
    fn test_process_args() {
        let mut config = ConfigFile::default();
        config
            .aliases
            .insert("errors".to_string(), "-l error --stats".to_string());

        let args = vec![
            "kelora".to_string(),
            "-a".to_string(),
            "errors".to_string(),
            "-f".to_string(),
            "json".to_string(),
        ];

        let (processed, _info) = config.process_args(args).unwrap();

        assert_eq!(
            processed,
            vec!["kelora", "-l", "error", "--stats", "-f", "json"]
        );
    }

    #[test]
    fn test_process_args_with_defaults() {
        let config = ConfigFile {
            defaults: Some("--stats --parallel".to_string()),
            ..ConfigFile::default()
        };

        let args = vec![
            "kelora".to_string(),
            "-f".to_string(),
            "json".to_string(),
            "input.log".to_string(),
        ];

        let (processed, _info) = config.process_args(args).unwrap();

        assert_eq!(
            processed,
            vec!["kelora", "--stats", "--parallel", "-f", "json", "input.log"]
        );
    }

    #[test]
    fn test_process_args_with_defaults_and_aliases() {
        let mut config = ConfigFile {
            defaults: Some("--stats".to_string()),
            ..ConfigFile::default()
        };
        config
            .aliases
            .insert("errors".to_string(), "-l error".to_string());

        let args = vec![
            "kelora".to_string(),
            "-a".to_string(),
            "errors".to_string(),
            "-f".to_string(),
            "json".to_string(),
        ];

        let (processed, _info) = config.process_args(args).unwrap();

        assert_eq!(
            processed,
            vec!["kelora", "--stats", "-l", "error", "-f", "json"]
        );
    }

    // RAII guard to restore working directory on drop
    use std::sync::LazyLock;
    use std::sync::{Mutex, MutexGuard};

    static CWD_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct WorkingDirGuard {
        original_dir: PathBuf,
        _lock: MutexGuard<'static, ()>,
    }

    impl WorkingDirGuard {
        fn new(new_dir: &Path) -> std::io::Result<Self> {
            let lock = CWD_MUTEX.lock().unwrap();
            let original_dir = std::env::current_dir()?;
            if let Err(err) = std::env::set_current_dir(new_dir) {
                drop(lock);
                return Err(err);
            }
            Ok(Self {
                original_dir,
                _lock: lock,
            })
        }
    }

    impl Drop for WorkingDirGuard {
        fn drop(&mut self) {
            // Best effort to restore - ignore errors during unwinding
            let _ = std::env::set_current_dir(&self.original_dir);
        }
    }

    #[test]
    fn test_project_config_discovery() {
        use tempfile::TempDir;

        // Create a temporary directory structure
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();
        let subdir = project_root.join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        // Create .kelora.ini in project root
        let config_path = project_root.join(".kelora.ini");
        std::fs::write(&config_path, "defaults = --project-test").unwrap();

        // Change to subdirectory and test discovery
        let _guard = WorkingDirGuard::new(&subdir).unwrap();

        let found_config = ConfigFile::find_project_config();

        assert_eq!(found_config, Some(config_path));
    }

    #[test]
    fn test_config_merging() {
        // Test that project config overrides user config properly
        let user_config = ConfigFile {
            defaults: Some("--user-defaults".to_string()),
            aliases: {
                let mut aliases = HashMap::new();
                aliases.insert("user-alias".to_string(), "--user-value".to_string());
                aliases.insert("shared-alias".to_string(), "--user-shared".to_string());
                aliases
            },
        };

        let project_config = ConfigFile {
            defaults: Some("--project-defaults".to_string()),
            aliases: {
                let mut aliases = HashMap::new();
                aliases.insert("project-alias".to_string(), "--project-value".to_string());
                aliases.insert("shared-alias".to_string(), "--project-shared".to_string());
                aliases
            },
        };

        let merged = ConfigFile::merge_configs(user_config, project_config);

        // Project defaults should override user defaults
        assert_eq!(merged.defaults, Some("--project-defaults".to_string()));

        // Aliases should be merged with project taking precedence for conflicts
        assert_eq!(
            merged.aliases.get("user-alias"),
            Some(&"--user-value".to_string())
        );
        assert_eq!(
            merged.aliases.get("project-alias"),
            Some(&"--project-value".to_string())
        );
        assert_eq!(
            merged.aliases.get("shared-alias"),
            Some(&"--project-shared".to_string())
        );
    }

    #[test]
    fn test_config_merging_with_none_defaults() {
        let base_config = ConfigFile {
            defaults: Some("--base-defaults".to_string()),
            aliases: HashMap::new(),
        };

        let overlay_config = ConfigFile {
            defaults: None,
            aliases: {
                let mut aliases = HashMap::new();
                aliases.insert("test-alias".to_string(), "--test-value".to_string());
                aliases
            },
        };

        let merged = ConfigFile::merge_configs(base_config, overlay_config);

        // Base defaults should remain since overlay has None
        assert_eq!(merged.defaults, Some("--base-defaults".to_string()));
        assert_eq!(
            merged.aliases.get("test-alias"),
            Some(&"--test-value".to_string())
        );
    }

    #[test]
    fn test_project_config_not_found() {
        use tempfile::TempDir;

        // Create a temporary directory without .kelora.ini
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("no-config");
        std::fs::create_dir_all(&subdir).unwrap();

        let _guard = WorkingDirGuard::new(&subdir).unwrap();

        let found_config = ConfigFile::find_project_config();

        assert_eq!(found_config, None);
    }

    #[test]
    fn test_user_config_paths() {
        let paths = ConfigFile::get_user_config_paths();

        // Should have at least one path
        assert!(!paths.is_empty());

        // Check that user paths use the new kelora/kelora.ini structure
        for path in &paths {
            let file_name = path.file_name().unwrap().to_string_lossy();

            // Should be kelora.ini
            assert_eq!(
                file_name, "kelora.ini",
                "Unexpected user config filename: {}",
                file_name
            );

            // Parent directory should be named 'kelora'
            let parent_dir = path.parent().unwrap();
            let parent_name = parent_dir.file_name().unwrap().to_string_lossy();
            assert_eq!(
                parent_name, "kelora",
                "Parent directory should be 'kelora', got: {}",
                parent_name
            );

            // Grandparent should be config directory
            let grandparent_path = parent_dir.parent().unwrap().to_string_lossy();
            let is_config_dir =
                grandparent_path.contains("config") || grandparent_path.contains("APPDATA");
            assert!(
                is_config_dir,
                "Grandparent not in expected config directory: {}",
                grandparent_path
            );
        }
    }

    #[test]
    fn test_get_config_paths_precedence() {
        use tempfile::TempDir;

        // Create temp project with .kelora.ini
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();
        let config_path = project_root.join(".kelora.ini");
        std::fs::write(&config_path, "test").unwrap();

        let _guard = WorkingDirGuard::new(&project_root).unwrap();

        let paths = ConfigFile::get_config_paths();

        // First path should be the project config
        assert_eq!(paths[0], config_path);

        // Remaining paths should be user config paths
        let user_paths = ConfigFile::get_user_config_paths();
        assert_eq!(&paths[1..], &user_paths[..]);
    }
}
