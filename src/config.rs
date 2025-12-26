#![allow(dead_code)] // Error-reporting helpers and legacy config paths are kept for planned CLI surfacing
use clap::ValueEnum;

/// Main configuration struct for Kelora
#[derive(Debug, Clone)]
pub struct KeloraConfig {
    pub input: InputConfig,
    pub output: OutputConfig,
    pub processing: ProcessingConfig,
    pub performance: PerformanceConfig,
}

/// Input configuration
#[derive(Debug, Clone)]
pub struct InputConfig {
    pub files: Vec<String>,
    pub no_input: bool,
    pub format: InputFormat,
    pub file_order: FileOrder,
    pub skip_lines: usize,
    pub head_lines: Option<usize>,
    pub section: Option<SectionConfig>,
    pub ignore_lines: Option<regex::Regex>,
    pub keep_lines: Option<regex::Regex>,
    pub multiline: Option<MultilineConfig>,
    /// Custom timestamp field name (reserved for --since/--until features)
    pub ts_field: Option<String>,
    /// Custom timestamp format string
    pub ts_format: Option<String>,
    /// Default timezone for naive timestamps (None = local time)
    pub default_timezone: Option<String>,
    /// Extract text before separator to specified field (runs before parsing)
    pub extract_prefix: Option<String>,
    /// Separator string for prefix extraction (default: pipe '|')
    pub prefix_sep: String,
    /// Column separator for cols format (None = whitespace)
    pub cols_sep: Option<String>,
}

/// Output configuration
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub keys: Vec<String>,
    pub exclude_keys: Vec<String>,
    pub core: bool,
    pub brief: bool,
    pub wrap: bool,
    pub pretty: bool,
    pub color: ColorMode,
    pub emoji: EmojiMode,
    pub stats: Option<crate::cli::StatsFormat>,
    pub stats_with_events: bool,
    pub metrics: Option<crate::cli::MetricsFormat>,
    pub metrics_with_events: bool,
    pub metrics_file: Option<String>,
    pub mark_gaps: Option<chrono::Duration>,
    /// Timestamp formatting configuration (display-only)
    pub timestamp_formatting: TimestampFormatConfig,
}

/// Ordered script stages that preserve CLI order
#[derive(Debug, Clone)]
pub enum ScriptStageType {
    Filter(String),
    Exec(String),
    LevelFilter {
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

/// Error reporting configuration
#[derive(Debug, Clone)]
pub struct ErrorReportConfig {
    pub style: ErrorReportStyle,
}

#[derive(Debug, Clone)]
pub enum ErrorReportStyle {
    Off,
    Summary,
    Print,
}

/// Context options configuration
#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub before_context: usize,
    pub after_context: usize,
    pub enabled: bool,
}

impl ContextConfig {
    pub fn new(before_context: usize, after_context: usize) -> Self {
        Self {
            before_context,
            after_context,
            enabled: before_context > 0 || after_context > 0,
        }
    }

    pub fn disabled() -> Self {
        Self {
            before_context: 0,
            after_context: 0,
            enabled: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.enabled && (self.before_context > 0 || self.after_context > 0)
    }

    pub fn required_window_size(&self) -> usize {
        if self.is_active() {
            self.before_context + self.after_context + 1
        } else {
            0
        }
    }
}

/// Processing configuration
#[derive(Debug, Clone)]
pub struct ProcessingConfig {
    pub begin: Option<String>,
    pub stages: Vec<ScriptStageType>,
    pub end: Option<String>,
    pub error_report: ErrorReportConfig,
    pub levels: Vec<String>,
    pub exclude_levels: Vec<String>,
    /// Window size for sliding window functionality (0 = disabled)
    pub window_size: usize,
    /// Timestamp filtering configuration
    pub timestamp_filter: Option<TimestampFilterConfig>,
    /// Normalize the primary timestamp field to RFC3339 output
    pub normalize_timestamps: bool,
    /// Limit output to the first N events (None = no limit)
    pub take_limit: Option<usize>,
    /// Exit on first error (fail-fast behavior) - new resiliency model
    pub strict: bool,
    /// Span aggregation configuration (--span / --span-close)
    pub span: Option<SpanConfig>,
    /// Show detailed error information (levels: 0-3) - new resiliency model
    pub verbose: u8,
    /// Suppress formatter/event output (-q/--quiet, -s, -m)
    pub quiet_events: bool,
    /// Suppress diagnostics and summaries (--no-diagnostics)
    pub suppress_diagnostics: bool,
    /// Suppress pipeline stdout/stderr emitters except the single fatal line (--silent)
    pub silent: bool,
    /// Suppress Rhai print/eprint and side-effect warnings (--no-script-output, data-only modes)
    pub suppress_script_output: bool,
    /// Legacy quiet level used by some helpers (derived from the above flags)
    pub quiet_level: u8,
    /// Context options for showing surrounding lines around matches
    pub context: ContextConfig,
    /// Allow Rhai scripts to create directories and write files on disk
    pub allow_fs_writes: bool,
}

/// Performance configuration
#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    pub parallel: bool,
    pub threads: usize,
    pub batch_size: Option<usize>,
    pub batch_timeout: u64,
    pub no_preserve_order: bool,
}

/// Span aggregation mode (--span)
#[derive(Debug, Clone)]
pub enum SpanMode {
    Count { events_per_span: usize },
    Time { duration_ms: i64 },
    Field { field_name: String },
    Idle { timeout_ms: i64 },
}

/// Span aggregation configuration (--span / --span-close)
#[derive(Debug, Clone)]
pub struct SpanConfig {
    pub mode: SpanMode,
    pub close_script: Option<String>,
}

/// Input format enumeration
#[derive(Clone, Debug, PartialEq)]
pub enum InputFormat {
    Auto,
    Json,
    Line,
    Raw,
    Logfmt,
    Syslog,
    Cef,
    Csv(Option<String>), // Optional field spec with type annotations
    Tsv(Option<String>), // Optional field spec with type annotations
    Csvnh,               // No type annotations (no field names)
    Tsvnh,               // No type annotations (no field names)
    Combined,
    Cols(String),  // Contains the column spec
    Regex(String), // Contains the regex pattern with optional type annotations
}

impl InputFormat {
    /// Convert format to display string for error messages and stats
    pub fn to_display_string(&self) -> String {
        match self {
            InputFormat::Auto => "auto".to_string(),
            InputFormat::Json => "json".to_string(),
            InputFormat::Line => "line".to_string(),
            InputFormat::Raw => "raw".to_string(),
            InputFormat::Logfmt => "logfmt".to_string(),
            InputFormat::Syslog => "syslog".to_string(),
            InputFormat::Cef => "cef".to_string(),
            InputFormat::Csv(_) => "csv".to_string(),
            InputFormat::Tsv(_) => "tsv".to_string(),
            InputFormat::Csvnh => "csvnh".to_string(),
            InputFormat::Tsvnh => "tsvnh".to_string(),
            InputFormat::Combined => "combined".to_string(),
            InputFormat::Cols(_) => "cols".to_string(),
            InputFormat::Regex(_) => "regex".to_string(),
        }
    }
}

/// Output format enumeration
#[derive(ValueEnum, Clone, Debug, Default, PartialEq)]
pub enum OutputFormat {
    Json,
    #[default]
    Default,
    Logfmt,
    Inspect,
    Levelmap,
    Keymap,
    Tailmap,
    Csv,
    Tsv,
    Csvnh,
    Tsvnh,
}

/// File processing order
#[derive(ValueEnum, Clone, Debug)]
pub enum FileOrder {
    Cli,
    Name,
    Mtime,
}

/// Color output mode
#[derive(ValueEnum, Clone, Debug)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

/// Emoji output mode
#[derive(Clone, Debug)]
pub enum EmojiMode {
    Auto,
    Always,
    Never,
}

/// Timestamp filtering configuration
#[derive(Debug, Clone)]
pub struct TimestampFilterConfig {
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
}

/// Timestamp formatting configuration (display-only, affects default output format only)
#[derive(Debug, Clone, Default)]
pub struct TimestampFormatConfig {
    /// Specific fields to format as timestamps
    pub format_fields: Vec<String>,
    /// Auto-format all known timestamp fields
    pub auto_format_all: bool,
    /// Target timezone for formatting (true = UTC, false = local)
    pub format_as_utc: bool,
    /// Explicit parsing format hint (from --ts-format) evaluated before adaptive parsing
    pub parse_format_hint: Option<String>,
    /// Default timezone hint reused when parsing timestamps for display
    pub parse_timezone_hint: Option<String>,
}

/// Multi-line event detection configuration
#[derive(Debug, Clone)]
pub struct MultilineConfig {
    pub strategy: MultilineStrategy,
}

/// Multi-line event detection strategies
#[derive(Debug, Clone)]
pub enum MultilineStrategy {
    /// Events start when a timestamp-like prefix is detected
    Timestamp { chrono_format: Option<String> },
    /// Continuation lines are indented
    Indent,
    /// Events start (and optionally end) with explicit regexes
    Regex { start: String, end: Option<String> },
    /// Read entire input as a single event
    All,
}

/// Section selection configuration
#[derive(Debug, Clone)]
pub struct SectionConfig {
    pub start: Option<SectionStart>,
    pub end: Option<SectionEnd>,
    pub max_sections: i64,
}

/// Section start boundary semantics
#[derive(Debug, Clone)]
pub enum SectionStart {
    /// Begin emitting with the matching line
    From(regex::Regex),
    /// Begin emitting after the matching line
    After(regex::Regex),
}

/// Section end boundary semantics
#[derive(Debug, Clone)]
pub enum SectionEnd {
    /// Stop before the matching line
    Before(regex::Regex),
    /// Stop after emitting the matching line
    Through(regex::Regex),
}

impl MultilineConfig {
    /// Parse multiline configuration from CLI string
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.trim().is_empty() {
            return Err("Empty multiline configuration".to_string());
        }

        let mut segments = value.split(':');
        let strategy_name = segments
            .next()
            .ok_or_else(|| "Empty multiline configuration".to_string())?;

        let strategy = match strategy_name {
            "timestamp" => {
                let mut chrono_format: Option<String> = None;

                for segment in segments {
                    if let Some(format) = segment.strip_prefix("format=") {
                        if chrono_format.replace(format.to_string()).is_some() {
                            return Err("timestamp:format specified more than once".to_string());
                        }
                    } else {
                        return Err(format!(
                            "Unknown timestamp option: {} (supported: format=...)",
                            segment
                        ));
                    }
                }

                MultilineStrategy::Timestamp { chrono_format }
            }
            "indent" => {
                if segments.next().is_some() {
                    return Err("indent does not accept options".to_string());
                }
                MultilineStrategy::Indent
            }
            "regex" => {
                let mut start_pattern: Option<String> = None;
                let mut end_pattern: Option<String> = None;

                for segment in segments {
                    if let Some(pattern) = segment.strip_prefix("match=") {
                        if start_pattern.replace(pattern.to_string()).is_some() {
                            return Err("regex:match specified more than once".to_string());
                        }
                    } else if let Some(pattern) = segment.strip_prefix("end=") {
                        if end_pattern.replace(pattern.to_string()).is_some() {
                            return Err("regex:end specified more than once".to_string());
                        }
                    } else {
                        return Err(format!(
                            "Unknown regex option: {} (supported: match=..., end=...)",
                            segment
                        ));
                    }
                }

                let start = start_pattern.ok_or_else(|| {
                    "regex strategy requires match=REGEX (e.g. regex:match=^PID=)".to_string()
                })?;

                MultilineStrategy::Regex {
                    start,
                    end: end_pattern,
                }
            }
            "all" => {
                if segments.next().is_some() {
                    return Err("all does not accept options".to_string());
                }
                MultilineStrategy::All
            }
            other => {
                return Err(format!(
                    "Unknown multiline strategy: {} (supported: timestamp, indent, regex, all)",
                    other
                ));
            }
        };

        Ok(MultilineConfig { strategy })
    }
}

impl Default for MultilineConfig {
    fn default() -> Self {
        Self {
            strategy: MultilineStrategy::Timestamp {
                chrono_format: None,
            },
        }
    }
}

impl KeloraConfig {
    /// Get the list of core field names (ts, level, msg variants)
    pub fn get_core_field_names() -> Vec<String> {
        let mut core_fields = Vec::new();

        // Use constants from event.rs to ensure consistency
        core_fields.extend(
            crate::event::TIMESTAMP_FIELD_NAMES
                .iter()
                .map(|s| s.to_string()),
        );
        core_fields.extend(
            crate::event::LEVEL_FIELD_NAMES
                .iter()
                .map(|s| s.to_string()),
        );
        core_fields.extend(
            crate::event::MESSAGE_FIELD_NAMES
                .iter()
                .map(|s| s.to_string()),
        );

        core_fields
    }

    /// Format an error message with appropriate prefix (emoji or "kelora:")
    pub fn format_error_message(&self, message: &str) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if use_emoji {
            format!("⚠️ {}", message)
        } else {
            format!("kelora: {}", message)
        }
    }

    /// Format an informational message with appropriate prefix (emoji or "kelora:")
    pub fn format_info_message(&self, message: &str) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if use_emoji {
            format!("🔹 {}", message)
        } else {
            format!("kelora: {}", message)
        }
    }

    /// Format a hint/tip message with a lightbulb emoji when allowed
    pub fn format_hint_message(&self, message: &str) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if use_emoji {
            format!("💡 {}", message)
        } else {
            format!("kelora hint: {}", message)
        }
    }

    /// Format a warning message with appropriate prefix (emoji or "kelora warning:")
    pub fn format_warning_message(&self, message: &str) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if use_emoji {
            format!("🔸 {}", message)
        } else {
            format!("kelora warning: {}", message)
        }
    }

    /// Format a stats message with appropriate prefix (emoji or "Stats:")
    /// If `with_header` is true, includes the "📈 Stats:" header
    pub fn format_stats_message(&self, message: &str, with_header: bool) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if with_header {
            if use_emoji {
                format!("\n📈 Stats:\n{}", message)
            } else {
                format!("\nkelora: Stats:\n{}", message)
            }
        } else {
            format!("\n{}", message)
        }
    }

    /// Format a metrics message with appropriate prefix (emoji or "Metrics:")
    /// If `with_header` is true, includes the "📊 Tracked metrics:" header
    pub fn format_metrics_message(&self, message: &str, with_header: bool) -> String {
        let use_emoji =
            crate::tty::should_use_emoji_with_mode(&self.output.emoji, &self.output.color);

        if with_header {
            if use_emoji {
                format!("\n📊 Tracked metrics:\n{}", message)
            } else {
                format!("\nkelora: Tracked metrics:\n{}", message)
            }
        } else {
            format!("\n{}", message)
        }
    }
}

/// Format an error message with appropriate prefix when config is not available
/// Uses auto color detection for stderr and allows NO_EMOJI environment variable override
pub fn format_error_message_auto(message: &str) -> String {
    let use_emoji = crate::tty::should_use_emoji_for_stderr();

    if use_emoji {
        format!("⚠️ {}", message)
    } else {
        format!("kelora: {}", message)
    }
}

/// Format a warning message with appropriate prefix when config is not available
/// Uses auto color detection for stderr and allows NO_EMOJI environment variable override
pub fn format_warning_message_auto(message: &str) -> String {
    let use_emoji = crate::tty::should_use_emoji_for_stderr();

    if use_emoji {
        format!("🔸 {}", message)
    } else {
        format!("kelora warning: {}", message)
    }
}

/// Format a verbose error message with line number and error type
pub fn format_verbose_error(line_num: Option<usize>, error_type: &str, message: &str) -> String {
    format_verbose_error_with_config(line_num, error_type, message, None)
}

/// Format a verbose error message with explicit configuration
pub fn format_verbose_error_with_config(
    line_num: Option<usize>,
    error_type: &str,
    message: &str,
    config: Option<&KeloraConfig>,
) -> String {
    // Determine emoji usage
    let use_emoji = if let Some(cfg) = config {
        crate::tty::should_use_emoji_with_mode(&cfg.output.emoji, &cfg.output.color)
    } else {
        crate::tty::should_use_emoji_for_stderr()
    };
    let prefix = if use_emoji { "⚠️ " } else { "kelora: " };

    if let Some(line) = line_num {
        format!("{}line {}: {} - {}", prefix, line, error_type, message)
    } else {
        format!("{}{} - {}", prefix, error_type, message)
    }
}

/// Print a verbose error message to stderr with proper formatting
/// Always goes directly to stderr, bypassing any capture mechanisms for immediate output
pub fn print_verbose_error_to_stderr(
    line_num: Option<usize>,
    error_type: &str,
    message: &str,
    config: Option<&KeloraConfig>,
) {
    // Check if output is suppressed (quiet mode)
    if let Some(cfg) = config {
        if cfg.processing.silent || cfg.processing.suppress_diagnostics {
            return;
        }
    }

    let formatted = format_verbose_error_with_config(line_num, error_type, message, config);
    eprintln!("{}", formatted);
}

/// Print a verbose error message to stderr with PipelineConfig
/// Always goes directly to stderr, bypassing any capture mechanisms for immediate output
pub fn print_verbose_error_to_stderr_pipeline(
    line_num: Option<usize>,
    error_type: &str,
    message: &str,
    config: Option<&crate::pipeline::PipelineConfig>,
) {
    // Check if output is suppressed (quiet mode)
    if let Some(cfg) = config {
        if cfg.silent || cfg.suppress_diagnostics {
            return;
        }
    }

    let formatted =
        format_verbose_error_with_pipeline_config(line_num, error_type, message, config);
    eprintln!("{}", formatted);
}

/// Format a verbose error message with PipelineConfig
pub fn format_verbose_error_with_pipeline_config(
    line_num: Option<usize>,
    error_type: &str,
    message: &str,
    config: Option<&crate::pipeline::PipelineConfig>,
) -> String {
    // Determine emoji usage
    let use_emoji = if let Some(cfg) = config {
        crate::tty::should_use_emoji_with_mode(&cfg.emoji_mode, &cfg.color_mode)
    } else {
        crate::tty::should_use_emoji_for_stderr()
    };
    let prefix = if use_emoji { "⚠️ " } else { "kelora: " };

    if let Some(line) = line_num {
        format!("{}line {}: {} - {}", prefix, line, error_type, message)
    } else {
        format!("{}{} - {}", prefix, error_type, message)
    }
}

/// Format input line for error messages with smart handling of special characters
pub fn format_error_line(line: &str) -> String {
    if line.chars().any(|c| c.is_control() && c != '\n') {
        format!("{:?}", line) // Use Debug for control chars
    } else if line.ends_with('\n') {
        line.trim_end().to_string() // Suppress newlines, they are an artifact of our handling
    } else {
        line.to_string() // Raw for clean content
    }
}

impl OutputConfig {
    /// Get the effective keys for filtering, combining core fields with user-specified keys
    pub fn get_effective_keys(&self) -> Vec<String> {
        if self.core {
            let mut keys = KeloraConfig::get_core_field_names();
            // Add user-specified keys to the core fields, avoiding duplicates
            for key in &self.keys {
                if !keys.contains(key) {
                    keys.push(key.clone());
                }
            }
            keys
        } else {
            self.keys.clone()
        }
    }
}

impl KeloraConfig {
    /// Create configuration from CLI arguments
    pub fn from_cli(cli: &crate::Cli) -> anyhow::Result<Self> {
        // Determine color mode from flags (last one wins via overrides_with)
        let color_mode = if cli.no_color {
            ColorMode::Never
        } else if cli.force_color {
            ColorMode::Always
        } else {
            ColorMode::Auto
        };

        // Determine emoji mode from flags (last one wins via overrides_with)
        let emoji_mode = if cli.no_emoji {
            EmojiMode::Never
        } else if cli.force_emoji {
            EmojiMode::Always
        } else {
            EmojiMode::Auto
        };

        let default_timezone = determine_default_timezone(cli);
        let mut quiet_events = cli.quiet;
        // Diagnostics: positive flag enables, negative flag disables (last one wins via overrides_with)
        let mut suppress_diagnostics = if cli.diagnostics {
            false
        } else if cli.no_diagnostics {
            true
        } else {
            false // Default: diagnostics enabled
        };
        let mut silent = cli.silent;
        if cli.no_silent {
            silent = false;
        }
        // Script output: positive flag enables, negative flag disables (last one wins via overrides_with)
        let mut suppress_script_output = if cli.script_output {
            false
        } else if cli.no_script_output {
            true
        } else {
            false // Default: script output enabled
        };

        let flatten_levels = |values: &[String]| -> Vec<String> {
            values
                .iter()
                .flat_map(|value| value.split(','))
                .map(|part| part.trim())
                .filter(|part| !part.is_empty())
                .map(|part| part.to_string())
                .collect()
        };
        let include_levels = flatten_levels(&cli.levels);
        let exclude_levels = flatten_levels(&cli.exclude_levels);

        // Stats logic: determine format and whether events should be shown
        // Check no_stats first to handle flag conflicts
        let stats_format = if cli.no_stats {
            None
        } else if cli.stats.is_some() {
            cli.stats.clone()
        } else if cli.with_stats {
            Some(crate::cli::StatsFormat::Table)
        } else {
            None
        };
        let stats_with_events = cli.with_stats;
        let suppress_events_for_stats = stats_format.is_some() && !stats_with_events;

        // Metrics logic: determine format and whether events should be shown
        // Check no_metrics first to handle flag conflicts
        let metrics_format = if cli.no_metrics {
            None
        } else if cli.metrics.is_some() {
            cli.metrics.clone()
        } else if cli.with_metrics {
            Some(crate::cli::MetricsFormat::Full)
        } else {
            None
        };
        let metrics_with_events = cli.with_metrics;
        let suppress_events_for_metrics = metrics_format.is_some() && !metrics_with_events;

        // Combine suppressions from stats/metrics data-only modes
        if suppress_events_for_stats || suppress_events_for_metrics {
            quiet_events = true;
        }

        let output_format = if cli.json_output {
            OutputFormat::Json
        } else {
            cli.output_format.clone().into()
        };

        // Data-only modes suppress script output
        if suppress_events_for_stats {
            suppress_script_output = true;
        }
        if suppress_events_for_metrics {
            suppress_diagnostics = true;
            suppress_script_output = true;
        }

        if silent {
            quiet_events = true;
        }

        let metrics_file = cli.metrics_file.clone();

        let quiet_level = if suppress_script_output {
            3
        } else if suppress_diagnostics || silent {
            1
        } else {
            0
        };
        let verbose_level = if suppress_diagnostics || silent {
            0
        } else {
            cli.verbose
        };

        Ok(Self {
            input: InputConfig {
                files: cli.files.clone(),
                no_input: cli.no_input,
                format: if cli.json_input {
                    InputFormat::Json
                } else {
                    parse_input_format_from_cli(cli)?
                },
                file_order: cli.file_order.clone().into(),
                skip_lines: cli.skip_lines.unwrap_or(0),
                head_lines: cli.head,
                section: None,      // Will be set after CLI parsing
                ignore_lines: None, // Will be set after CLI parsing
                keep_lines: None,   // Will be set after CLI parsing
                multiline: None,    // Will be set after CLI parsing
                ts_field: cli.ts_field.clone(),
                ts_format: cli.ts_format.clone(),
                default_timezone: default_timezone.clone(),
                extract_prefix: cli.extract_prefix.clone(),
                prefix_sep: cli.prefix_sep.clone(),
                cols_sep: cli.cols_sep.clone(),
            },
            output: OutputConfig {
                format: output_format,
                keys: cli.keys.clone(),
                exclude_keys: cli.exclude_keys.clone(),
                core: cli.core,
                brief: cli.brief,
                wrap: !cli.no_wrap, // Default true, disabled by --no-wrap
                pretty: cli.expand_nested,
                color: color_mode,
                emoji: emoji_mode,
                stats: stats_format,
                stats_with_events,
                metrics: metrics_format,
                metrics_with_events,
                metrics_file,
                mark_gaps: None,
                timestamp_formatting: create_timestamp_format_config(cli, default_timezone.clone()),
            },
            processing: ProcessingConfig {
                begin: cli.begin.clone(),
                stages: Vec::new(), // Will be set by main() after CLI parsing
                end: cli.end.clone(),
                error_report: parse_error_report_config(cli),
                levels: include_levels,
                exclude_levels,
                span: parse_span_config(cli)?,
                window_size: cli.window_size.unwrap_or(0),
                timestamp_filter: None, // Will be set in main() after parsing since/until
                normalize_timestamps: cli.normalize_ts,
                take_limit: cli.take,
                strict: cli.strict,
                verbose: verbose_level,
                quiet_events,
                suppress_diagnostics,
                silent,
                suppress_script_output,
                quiet_level,
                context: create_context_config(cli)?,
                allow_fs_writes: cli.allow_fs_writes,
            },
            performance: PerformanceConfig {
                parallel: cli.parallel,
                threads: cli.threads,
                batch_size: cli.batch_size,
                batch_timeout: cli.batch_timeout,
                no_preserve_order: cli.no_preserve_order,
            },
        })
    }

    /// Check if parallel processing should be used
    pub fn should_use_parallel(&self) -> bool {
        if self.processing.span.is_some() {
            return false;
        }
        self.performance.parallel
            || self.performance.threads > 0
            || self.performance.batch_size.is_some()
    }

    /// Get effective batch size with defaults
    pub fn effective_batch_size(&self) -> usize {
        self.performance.batch_size.unwrap_or(1000)
    }

    /// Get effective thread count with defaults
    pub fn effective_threads(&self) -> usize {
        if self.performance.threads == 0 {
            num_cpus::get()
        } else {
            self.performance.threads
        }
    }
}

impl Default for KeloraConfig {
    fn default() -> Self {
        Self {
            input: InputConfig {
                files: Vec::new(),
                no_input: false,
                format: InputFormat::Auto,
                file_order: FileOrder::Cli,
                skip_lines: 0,
                head_lines: None,
                section: None,
                ignore_lines: None,
                keep_lines: None,
                multiline: None,
                ts_field: None,
                ts_format: None,
                default_timezone: None,
                extract_prefix: None,
                prefix_sep: "|".to_string(),
                cols_sep: None,
            },
            output: OutputConfig {
                format: OutputFormat::Default,
                keys: Vec::new(),
                exclude_keys: Vec::new(),
                core: false,
                brief: false,
                wrap: true, // Default to enabled
                pretty: false,
                color: ColorMode::Auto,
                emoji: EmojiMode::Auto,
                stats: None,
                stats_with_events: false,
                metrics: None,
                metrics_with_events: false,
                metrics_file: None,
                mark_gaps: None,
                timestamp_formatting: TimestampFormatConfig::default(),
            },
            processing: ProcessingConfig {
                begin: None,
                stages: Vec::new(),
                end: None,
                error_report: ErrorReportConfig {
                    style: ErrorReportStyle::Summary,
                },
                span: None,
                levels: Vec::new(),
                exclude_levels: Vec::new(),
                window_size: 0,
                timestamp_filter: None,
                normalize_timestamps: false,
                take_limit: None,
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                context: ContextConfig::disabled(),
                allow_fs_writes: false,
            },
            performance: PerformanceConfig {
                parallel: false,
                threads: 0,
                batch_size: None,
                batch_timeout: 200,
                no_preserve_order: false,
            },
        }
    }
}

/// Parse input format from CLI options, handling the --input-format option
fn parse_input_format_from_cli(cli: &crate::Cli) -> anyhow::Result<InputFormat> {
    parse_input_format_spec(&cli.format)
}

/// Parse input format specification string (e.g., "cols:ts(2) level - *msg")
fn parse_input_format_spec(spec: &str) -> anyhow::Result<InputFormat> {
    // Helper to parse field spec after format name
    let parse_field_spec = |_prefix: &str, name: &str| -> Option<String> {
        // Handle both "csv:" and "csv " (optional colon)
        if let Some(field_spec) = spec.strip_prefix(&format!("{}:", name)) {
            Some(field_spec.trim().to_string())
        } else {
            spec.strip_prefix(&format!("{} ", name))
                .map(|field_spec| field_spec.trim().to_string())
        }
    };

    // Check for regex format with pattern
    if let Some(regex_pattern) = spec.strip_prefix("regex:") {
        if regex_pattern.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "regex format requires a pattern, e.g., 'regex:(?P<field>\\d+)'"
            ));
        }
        return Ok(InputFormat::Regex(regex_pattern.to_string()));
    }

    // Check for cols format with spec
    if let Some(cols_spec) = spec.strip_prefix("cols:") {
        if cols_spec.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "cols format requires a specification, e.g., 'cols:ts level *msg'"
            ));
        }
        return Ok(InputFormat::Cols(cols_spec.to_string()));
    }

    // Check for CSV/TSV variants with optional field specs (only for formats with headers)
    if let Some(field_spec) = parse_field_spec(spec, "csv") {
        return Ok(InputFormat::Csv(Some(field_spec)));
    }
    if let Some(field_spec) = parse_field_spec(spec, "tsv") {
        return Ok(InputFormat::Tsv(Some(field_spec)));
    }

    // Parse standard formats (no field specs)
    match spec.to_lowercase().as_str() {
        "auto" => Ok(InputFormat::Auto),
        "json" => Ok(InputFormat::Json),
        "line" => Ok(InputFormat::Line),
        "raw" => Ok(InputFormat::Raw),
        "logfmt" => Ok(InputFormat::Logfmt),
        "syslog" => Ok(InputFormat::Syslog),
        "cef" => Ok(InputFormat::Cef),
        "csv" => Ok(InputFormat::Csv(None)),
        "tsv" => Ok(InputFormat::Tsv(None)),
        "csvnh" => Ok(InputFormat::Csvnh),
        "tsvnh" => Ok(InputFormat::Tsvnh),
        "combined" => Ok(InputFormat::Combined),
        _ => Err(anyhow::anyhow!("Unknown input format: '{}'. Supported formats: json, line, csv, syslog, cef, logfmt, raw, tsv, csvnh, tsvnh, combined, auto, cols:<spec>, and regex:<pattern>", spec)),
    }
}

/// Create timestamp formatting configuration from CLI options
fn create_timestamp_format_config(
    cli: &crate::Cli,
    default_timezone: Option<String>,
) -> TimestampFormatConfig {
    let mut format_fields = Vec::new();
    if let Some(ref ts_field) = cli.ts_field {
        let trimmed = ts_field.trim();
        if !trimmed.is_empty() {
            format_fields.push(trimmed.to_string());
        }
    }

    let auto_format_all = cli.format_timestamps_local || cli.format_timestamps_utc;
    let format_as_utc = cli.format_timestamps_utc;

    TimestampFormatConfig {
        format_fields,
        auto_format_all,
        format_as_utc,
        parse_format_hint: cli.ts_format.clone(),
        parse_timezone_hint: default_timezone,
    }
}

/// Parse error report configuration from CLI
fn parse_error_report_config(cli: &crate::Cli) -> ErrorReportConfig {
    // Default error report style based on new resiliency model
    let style = if cli.strict {
        ErrorReportStyle::Print // Show each error immediately in strict mode
    } else {
        ErrorReportStyle::Summary // Show summary in resilient mode
    };

    ErrorReportConfig { style }
}

/// Create context configuration from CLI arguments
fn create_context_config(cli: &crate::Cli) -> anyhow::Result<ContextConfig> {
    let (before_context, after_context) = if let Some(context) = cli.context {
        // -C sets both before and after context
        (context, context)
    } else {
        // Use individual -A and -B settings
        (
            cli.before_context.unwrap_or(0),
            cli.after_context.unwrap_or(0),
        )
    };

    // Validate that context requires filtering
    let has_filtering = !cli.filters.is_empty()
        || !cli.levels.is_empty()
        || !cli.exclude_levels.is_empty()
        || cli.since.is_some()
        || cli.until.is_some();

    if (before_context > 0 || after_context > 0) && !has_filtering {
        return Err(anyhow::anyhow!(
            "Context options (-A, -B, -C) require active filtering (use --filter, --levels, --since, --until, etc.)"
        ));
    }

    Ok(ContextConfig::new(before_context, after_context))
}

/// Determine the default timezone based on CLI options and environment
/// Following the new spec: --input-tz defaults to UTC
fn determine_default_timezone(cli: &crate::Cli) -> Option<String> {
    // Priority 1: --input-tz option
    if let Some(ref input_tz) = cli.input_tz {
        if input_tz == "local" {
            return None; // None means local time
        } else {
            return Some(input_tz.clone());
        }
    }

    // Priority 2: TZ environment variable
    if let Ok(tz) = std::env::var("TZ") {
        if !tz.is_empty() {
            return Some(tz);
        }
    }

    // DEFAULT: UTC (per spec, --input-tz defaults to UTC)
    Some("UTC".to_string())
}

fn parse_span_config(cli: &crate::Cli) -> anyhow::Result<Option<SpanConfig>> {
    let span_spec = cli
        .span
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let idle_spec = cli
        .span_idle
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    if span_spec.is_none() && idle_spec.is_none() {
        if cli.span_close.is_some() {
            return Err(anyhow::anyhow!(
                "--span-close requires --span or --span-idle to be specified"
            ));
        }
        return Ok(None);
    }

    if span_spec.is_some() && idle_spec.is_some() {
        return Err(anyhow::anyhow!(
            "--span and --span-idle cannot be used together"
        ));
    }

    if let Some(spec) = idle_spec {
        let duration = humantime::parse_duration(spec).map_err(|e| {
            anyhow::anyhow!(
                "Invalid --span-idle duration '{}': {}. Use formats like 30s, 5m, 1h.",
                spec,
                e
            )
        })?;

        if duration.is_zero() {
            return Err(anyhow::anyhow!(
                "--span-idle duration must be greater than zero"
            ));
        }

        let timeout_ms: i64 = duration
            .as_millis()
            .try_into()
            .map_err(|_| anyhow::anyhow!("--span-idle duration is too large"))?;

        return Ok(Some(SpanConfig {
            mode: SpanMode::Idle { timeout_ms },
            close_script: cli.span_close.clone(),
        }));
    }

    let span_spec = span_spec.expect("span presence checked above");

    if let Ok(count) = span_spec.parse::<usize>() {
        if count == 0 {
            return Err(anyhow::anyhow!(
                "--span <N> must be a positive integer greater than zero"
            ));
        }

        return Ok(Some(SpanConfig {
            mode: SpanMode::Count {
                events_per_span: count,
            },
            close_script: cli.span_close.clone(),
        }));
    }

    if let Ok(duration) = humantime::parse_duration(span_spec) {
        if duration.is_zero() {
            return Err(anyhow::anyhow!("--span duration must be greater than zero"));
        }

        let duration_ms: i64 = duration
            .as_millis()
            .try_into()
            .map_err(|_| anyhow::anyhow!("--span duration is too large"))?;

        return Ok(Some(SpanConfig {
            mode: SpanMode::Time { duration_ms },
            close_script: cli.span_close.clone(),
        }));
    }

    if !is_valid_field_name(span_spec) {
        return Err(anyhow::anyhow!(
            "Invalid --span field name '{}': must start with a letter and contain only letters, digits, or underscores",
            span_spec
        ));
    }

    Ok(Some(SpanConfig {
        mode: SpanMode::Field {
            field_name: span_spec.to_string(),
        },
        close_script: cli.span_close.clone(),
    }))
}

fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// Conversion traits to maintain compatibility with existing CLI types
impl From<crate::InputFormat> for InputFormat {
    fn from(format: crate::InputFormat) -> Self {
        match format {
            crate::InputFormat::Auto => InputFormat::Auto,
            crate::InputFormat::Json => InputFormat::Json,
            crate::InputFormat::Line => InputFormat::Line,
            crate::InputFormat::Raw => InputFormat::Raw,
            crate::InputFormat::Logfmt => InputFormat::Logfmt,
            crate::InputFormat::Syslog => InputFormat::Syslog,
            crate::InputFormat::Cef => InputFormat::Cef,
            crate::InputFormat::Csv => InputFormat::Csv(None),
            crate::InputFormat::Tsv => InputFormat::Tsv(None),
            crate::InputFormat::Csvnh => InputFormat::Csvnh,
            crate::InputFormat::Tsvnh => InputFormat::Tsvnh,
            crate::InputFormat::Combined => InputFormat::Combined,
            crate::InputFormat::Cols => {
                // This should not happen since CLI Cols enum has no parameters
                // But if it does, create an empty spec as fallback
                InputFormat::Cols(String::new())
            }
            crate::InputFormat::Regex => {
                // This should not happen since CLI Regex enum has no parameters
                // But if it does, create an empty pattern as fallback
                InputFormat::Regex(String::new())
            }
        }
    }
}

impl From<InputFormat> for crate::InputFormat {
    fn from(format: InputFormat) -> Self {
        match format {
            InputFormat::Auto => crate::InputFormat::Auto,
            InputFormat::Json => crate::InputFormat::Json,
            InputFormat::Line => crate::InputFormat::Line,
            InputFormat::Raw => crate::InputFormat::Raw,
            InputFormat::Logfmt => crate::InputFormat::Logfmt,
            InputFormat::Syslog => crate::InputFormat::Syslog,
            InputFormat::Cef => crate::InputFormat::Cef,
            InputFormat::Csv(_) => crate::InputFormat::Csv,
            InputFormat::Tsv(_) => crate::InputFormat::Tsv,
            InputFormat::Csvnh => crate::InputFormat::Csvnh,
            InputFormat::Tsvnh => crate::InputFormat::Tsvnh,
            InputFormat::Combined => crate::InputFormat::Combined,
            InputFormat::Cols(_) => crate::InputFormat::Cols,
            InputFormat::Regex(_) => crate::InputFormat::Regex,
        }
    }
}

impl From<crate::OutputFormat> for OutputFormat {
    fn from(format: crate::OutputFormat) -> Self {
        match format {
            crate::OutputFormat::Json => OutputFormat::Json,
            crate::OutputFormat::Default => OutputFormat::Default,
            crate::OutputFormat::Logfmt => OutputFormat::Logfmt,
            crate::OutputFormat::Inspect => OutputFormat::Inspect,
            crate::OutputFormat::Levelmap => OutputFormat::Levelmap,
            crate::OutputFormat::Keymap => OutputFormat::Keymap,
            crate::OutputFormat::Tailmap => OutputFormat::Tailmap,
            crate::OutputFormat::Csv => OutputFormat::Csv,
            crate::OutputFormat::Tsv => OutputFormat::Tsv,
            crate::OutputFormat::Csvnh => OutputFormat::Csvnh,
            crate::OutputFormat::Tsvnh => OutputFormat::Tsvnh,
        }
    }
}

impl From<OutputFormat> for crate::OutputFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Json => crate::OutputFormat::Json,
            OutputFormat::Default => crate::OutputFormat::Default,
            OutputFormat::Logfmt => crate::OutputFormat::Logfmt,
            OutputFormat::Inspect => crate::OutputFormat::Inspect,
            OutputFormat::Levelmap => crate::OutputFormat::Levelmap,
            OutputFormat::Keymap => crate::OutputFormat::Keymap,
            OutputFormat::Tailmap => crate::OutputFormat::Tailmap,
            OutputFormat::Csv => crate::OutputFormat::Csv,
            OutputFormat::Tsv => crate::OutputFormat::Tsv,
            OutputFormat::Csvnh => crate::OutputFormat::Csvnh,
            OutputFormat::Tsvnh => crate::OutputFormat::Tsvnh,
        }
    }
}

impl From<crate::FileOrder> for FileOrder {
    fn from(order: crate::FileOrder) -> Self {
        match order {
            crate::FileOrder::Cli => FileOrder::Cli,
            crate::FileOrder::Name => FileOrder::Name,
            crate::FileOrder::Mtime => FileOrder::Mtime,
        }
    }
}

impl From<FileOrder> for crate::FileOrder {
    fn from(order: FileOrder) -> Self {
        match order {
            FileOrder::Cli => crate::FileOrder::Cli,
            FileOrder::Name => crate::FileOrder::Name,
            FileOrder::Mtime => crate::FileOrder::Mtime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    struct EnvGuard {
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            let vars = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            Self { vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.vars {
                if let Some(v) = value {
                    std::env::set_var(key, v);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    fn with_env_lock<F: FnOnce()>(keys: &[&'static str], f: F) {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(keys);
        f();
    }

    #[test]
    fn determine_default_timezone_defaults_to_utc() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora"]);
            let tz = super::determine_default_timezone(&cli);
            assert_eq!(tz.as_deref(), Some("UTC"));
        });
    }

    #[test]
    fn determine_default_timezone_respects_cli_local() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora", "--input-tz", "local"]);
            let tz = super::determine_default_timezone(&cli);
            assert_eq!(tz, None);
        });
    }

    #[test]
    fn determine_default_timezone_prefers_cli_over_env() {
        with_env_lock(&["TZ"], || {
            std::env::set_var("TZ", "America/New_York");
            let cli = Cli::parse_from(["kelora", "--input-tz", "Europe/Berlin"]);
            let tz = super::determine_default_timezone(&cli);
            assert_eq!(tz.as_deref(), Some("Europe/Berlin"));
        });
    }

    #[test]
    fn determine_default_timezone_uses_environment_when_present() {
        with_env_lock(&["TZ"], || {
            std::env::set_var("TZ", "Asia/Tokyo");
            let cli = Cli::parse_from(["kelora"]);
            let tz = super::determine_default_timezone(&cli);
            assert_eq!(tz.as_deref(), Some("Asia/Tokyo"));
        });
    }

    #[test]
    fn format_error_message_respects_color_settings() {
        with_env_lock(&["NO_COLOR", "NO_EMOJI", "FORCE_COLOR"], || {
            std::env::remove_var("NO_COLOR");
            std::env::remove_var("NO_EMOJI");
            std::env::remove_var("FORCE_COLOR");

            let mut config = KeloraConfig::default();
            config.output.color = ColorMode::Always;
            config.output.emoji = EmojiMode::Always;

            let message = config.format_error_message("problem");
            assert!(message.starts_with("⚠️"));
            assert!(message.ends_with("problem"));
        });
    }

    #[test]
    fn format_error_message_without_colors_falls_back_to_plain_prefix() {
        let mut config = KeloraConfig::default();
        config.output.color = ColorMode::Never;
        config.output.emoji = EmojiMode::Never;

        let message = config.format_error_message("issue");
        assert_eq!(message, "kelora: issue");
    }

    #[test]
    fn output_config_get_effective_keys_includes_core_fields() {
        let mut config = KeloraConfig::default();
        config.output.core = true;
        config.output.keys = vec!["custom".to_string(), "ts".to_string()];

        let keys = config.output.get_effective_keys();
        let core = KeloraConfig::get_core_field_names();

        for required in &core {
            assert!(keys.contains(required), "missing core key {required}");
        }
        assert!(keys.contains(&"custom".to_string()));

        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            keys.len(),
            "keys should not contain duplicates"
        );
    }

    #[test]
    fn output_config_get_effective_keys_respects_non_core_mode() {
        let mut config = KeloraConfig::default();
        config.output.core = false;
        config.output.keys = vec!["alpha".to_string(), "beta".to_string()];

        let keys = config.output.get_effective_keys();
        assert_eq!(keys, vec!["alpha".to_string(), "beta".to_string()]);
    }
}
