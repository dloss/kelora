#![allow(dead_code)] // Error-reporting helpers and legacy config paths are kept for planned CLI surfacing
use clap::ValueEnum;

use crate::config_file::ConfigExpansionInfo;

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
    pub merge_ts: bool,
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
    /// True when the UTC default for naive timestamps is a *silent* assumption:
    /// neither `--input-tz` nor a non-empty `TZ` was provided. Gates the #287
    /// naive-timestamp diagnostic so it never fires when the user chose a zone.
    pub timezone_assumed: bool,
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
    pub wrap: WrapMode,
    pub pretty: bool,
    pub color: ColorMode,
    pub emoji: EmojiMode,
    /// Whether map formatters append a trailing legend
    pub legend: LegendMode,
    pub stats: Option<crate::cli::StatsFormat>,
    pub stats_with_events: bool,
    pub metrics: Option<crate::cli::MetricsFormat>,
    pub metrics_with_events: bool,
    pub metrics_file: Option<String>,
    pub drain: Option<crate::cli::DrainFormat>,
    pub discover_fields: Option<crate::cli::DiscoverFieldsFormat>,
    pub discover_final: bool,
    pub discover_depth: usize,
    pub mark_gaps: Option<chrono::Duration>,
    /// Timestamp formatting configuration (display-only)
    pub timestamp_formatting: TimestampFormatConfig,
}

/// Ordered script stages that preserve CLI order
#[derive(Debug, Clone)]
pub enum ScriptStageType {
    Filter {
        script: String,
        includes: Vec<IncludeFile>,
    },
    Exec(String),
    Assert(String),
    LevelFilter {
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct IncludeFile {
    pub path: String,
    pub content: String,
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
    /// Abort on invalid UTF-8 instead of lossy decoding (--strict-utf8). Default
    /// (false) decodes non-UTF-8 input with U+FFFD substitution; see issue #239.
    pub strict_utf8: bool,
    /// Span aggregation configuration (--span / --span-close)
    pub span: Option<SpanConfig>,
    /// Show detailed error information (levels: 0-3) - new resiliency model
    pub verbose: u8,
    /// Suppress formatter/event output (-q/--quiet, -s, -m)
    pub quiet_events: bool,
    /// Suppress warnings 🔸 (--no-warnings / --no-diagnostics / KELORA_NO_WARNINGS)
    pub suppress_warnings: bool,
    /// Suppress hints 💡 (--no-hints / --no-diagnostics / KELORA_NO_HINTS)
    pub suppress_hints: bool,
    /// True only when the user *explicitly* suppressed hints (`--no-hints` /
    /// `--no-diagnostics`, not overridden by `--hints` / `--diagnostics`), as
    /// opposed to the implicit suppression a data-only mode
    /// (`-m`/`--drain`/`--discover`) applies to keep machine-readable stdout
    /// clean. Stuck-user signals (e.g. the "every tracked value was missing"
    /// typo hint) survive the implicit suppression but still honor this explicit
    /// request.
    pub hints_user_suppressed: bool,
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
    AutoPerFile,
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
    /// Built-in named format adapted from lnav (e.g. log4j, glog). Backed by a
    /// static regex definition; selectable via `-f <name>` and produced by
    /// auto-detection. See `crate::parsers::lnav_formats`.
    Named(&'static crate::parsers::lnav_formats::LnavFormat),
    /// Cascade: try each format in order, first success wins.
    /// Only contains formats that are safe to try per-line (no CSV/cols/regex/auto).
    Cascade(Vec<InputFormat>),
}

impl InputFormat {
    /// Convert format to display string for error messages and stats
    pub fn to_display_string(&self) -> String {
        match self {
            InputFormat::Auto => "auto".to_string(),
            InputFormat::AutoPerFile => "auto-per-file".to_string(),
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
            InputFormat::Named(fmt) => fmt.name.to_string(),
            InputFormat::Cascade(formats) => {
                let names: Vec<String> = formats.iter().map(|f| f.to_display_string()).collect();
                format!("cascade({})", names.join(","))
            }
        }
    }

    /// Returns true if this format is a cascade (multi-format per-line dispatch).
    pub fn is_cascade(&self) -> bool {
        matches!(self, InputFormat::Cascade(_))
    }

    /// Returns true for the CSV/TSV family (with or without headers). These are
    /// the formats whose values may legitimately contain embedded newlines inside
    /// quoted fields (RFC 4180), so they need quote-aware record reassembly rather
    /// than naive one-line-per-record splitting.
    pub fn is_csv_like(&self) -> bool {
        matches!(
            self,
            InputFormat::Csv(_) | InputFormat::Tsv(_) | InputFormat::Csvnh | InputFormat::Tsvnh
        )
    }

    /// Returns true if this format may appear in a comma-separated cascade list
    /// (e.g. `-f json,line`). Mirrors the allow-list in `parse_cascade_spec`:
    /// schema-based formats (csv/tsv) and spec formats (cols/regex) are excluded.
    pub fn is_cascade_eligible(&self) -> bool {
        matches!(
            self,
            InputFormat::Json
                | InputFormat::Line
                | InputFormat::Raw
                | InputFormat::Logfmt
                | InputFormat::Syslog
                | InputFormat::Cef
                | InputFormat::Combined
                | InputFormat::Named(_)
        )
    }

    /// Returns true if this format matches every line and so must be placed last
    /// in a cascade. Mirrors `validate_cascade_order`.
    pub fn is_cascade_catch_all(&self) -> bool {
        matches!(
            self,
            InputFormat::Line | InputFormat::Raw | InputFormat::Cols(_)
        )
    }

    /// Returns the short name of a format suitable for use inside a cascade list
    /// (without any spec/args). Used for validation error messages.
    pub fn cascade_name(&self) -> &'static str {
        match self {
            InputFormat::Auto => "auto",
            InputFormat::AutoPerFile => "auto-per-file",
            InputFormat::Json => "json",
            InputFormat::Line => "line",
            InputFormat::Raw => "raw",
            InputFormat::Logfmt => "logfmt",
            InputFormat::Syslog => "syslog",
            InputFormat::Cef => "cef",
            InputFormat::Csv(_) => "csv",
            InputFormat::Tsv(_) => "tsv",
            InputFormat::Csvnh => "csvnh",
            InputFormat::Tsvnh => "tsvnh",
            InputFormat::Combined => "combined",
            InputFormat::Cols(_) => "cols",
            InputFormat::Regex(_) => "regex",
            InputFormat::Named(fmt) => fmt.name,
            InputFormat::Cascade(_) => "cascade",
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

/// Legend output mode for map formatters (levelmap/keymap/tailmap)
#[derive(Clone, Debug)]
pub enum LegendMode {
    /// Show the legend only when stdout is a TTY (keeps pipes clean)
    Auto,
    /// Always append the legend
    Always,
    /// Never append the legend
    Never,
}

/// Word-wrap mode for the default output format.
#[derive(Clone, Debug)]
pub enum WrapMode {
    /// Wrap only when stdout is a TTY. Piped or redirected output stays one
    /// line per event, so `wc -l`, `head`, and other line-oriented tools see
    /// one record per line.
    Auto,
    /// Always wrap wide events onto indented continuation lines.
    Always,
    /// Never wrap; keep each event on a single line.
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
    pub join: MultilineJoin,
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

/// How multiline events join buffered lines
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq)]
pub enum MultilineJoin {
    #[default]
    Space,
    Newline,
    Empty,
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

        Ok(MultilineConfig {
            strategy,
            join: MultilineJoin::Space,
        })
    }
}

impl Default for MultilineConfig {
    fn default() -> Self {
        Self {
            strategy: MultilineStrategy::Timestamp {
                chrono_format: None,
            },
            join: MultilineJoin::Space,
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

    /// Whether warnings (🔸) may be emitted. Warnings show unless the user asked
    /// to hide them (`--no-warnings` / `--no-diagnostics` / KELORA_NO_WARNINGS)
    /// or silenced everything (`--silent`).
    pub fn warnings_allowed(&self) -> bool {
        !self.processing.silent && !self.processing.suppress_warnings
    }

    /// Whether hints (💡) may be emitted. Hints show unless the user asked to
    /// hide them (`--no-hints` / `--no-diagnostics` / KELORA_NO_HINTS) or
    /// silenced everything (`--silent`).
    pub fn hints_allowed(&self) -> bool {
        !self.processing.silent && !self.processing.suppress_hints
    }

    /// Whether *all* advisory output (both warnings and hints) is suppressed —
    /// the legacy `--no-diagnostics` umbrella. Used to gate informational output
    /// (config expansion) and per-line verbose error detail.
    pub fn diagnostics_suppressed(&self) -> bool {
        self.processing.suppress_warnings && self.processing.suppress_hints
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

    /// Display config expansion information at startup (if diagnostics enabled)
    pub fn display_config_expansion(
        info: &ConfigExpansionInfo,
        config: &KeloraConfig,
        stderr: &mut crate::platform::SafeStderr,
    ) {
        // Config expansion is informational status (🔹), not a diagnostic, so it
        // rides the visibility axis (`-q`/`--silent`) rather than the
        // --no-warnings/--no-hints/--no-diagnostics flags. (Details are further
        // gated by verbosity below.)
        if config.processing.silent || config.processing.quiet_events {
            return;
        }

        // Check if there's anything to display
        let show_verbose_details = config.processing.verbose > 0 || info.explicit_config_path;
        let show_loaded_path = show_verbose_details;
        let show_defaults = show_verbose_details;
        let show_aliases = show_verbose_details || !info.expanded_aliases.is_empty();

        let has_content = (show_loaded_path && info.loaded_config_path.is_some())
            || (show_defaults && info.applied_defaults.is_some())
            || (show_aliases && !info.expanded_aliases.is_empty());

        if !has_content {
            return;
        }

        // Build output lines
        let mut lines = Vec::new();

        // Config file loaded
        if show_loaded_path {
            if let Some(path) = &info.loaded_config_path {
                let msg = config.format_info_message(&format!("Config: {}", path.display()));
                lines.push(msg);
            }
        }

        // Defaults applied (use info message with indentation)
        if show_defaults {
            if let Some(defaults) = &info.applied_defaults {
                let msg = config.format_info_message(&format!("  Defaults: {}", defaults));
                lines.push(msg);
            }
        }

        // Aliases expanded (use info message with indentation)
        if show_aliases {
            for (alias_name, expansion) in &info.expanded_aliases {
                let msg = config
                    .format_info_message(&format!("  Alias: -a {} → {}", alias_name, expansion));
                lines.push(msg);
            }
        }

        // Write all lines
        for line in lines {
            stderr.writeln(&line).unwrap_or(());
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

pub fn format_hint_message_auto(message: &str) -> String {
    let use_emoji = crate::tty::should_use_emoji_for_stderr();

    if use_emoji {
        format!("💡 {}", message)
    } else {
        format!("kelora hint: {}", message)
    }
}

/// Format an input-open error and add a shell-glob hint when the path looks unexpanded.
pub fn format_input_open_error(path: &str, err: &str) -> String {
    let mut message = format!("Failed to open file '{}': {}", path, err);

    let looks_like_glob = path.contains('*') || path.contains('?') || path.contains('[');
    let missing_file = err.contains("No such file")
        || err.contains("not found")
        || err.contains("cannot find the path");

    if looks_like_glob && missing_file {
        message.push_str(
            ". Shell glob patterns must be expanded by the shell; remove the quotes or use interactive mode for glob expansion",
        );
    } else if missing_file && matches!(path, "json" | "table" | "short" | "full") {
        // --stats/--metrics/--discover take their format via '=' (require_equals),
        // so `kelora -s json` parses `json` as a filename rather than a format.
        // A missing "file" named exactly like a format value is almost always
        // that mistake.
        message.push_str(&format!(
            ". If you meant an output format, attach it with '=' — e.g. --stats={path} or --metrics={path} (these flags require '=')",
        ));
    }

    message
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
    // Per-line verbose error detail rides the umbrella (--no-diagnostics) gate.
    if let Some(cfg) = config {
        if cfg.processing.silent || cfg.diagnostics_suppressed() {
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
    // Per-line verbose error detail rides the umbrella (--no-diagnostics) gate.
    if let Some(cfg) = config {
        if cfg.silent || (cfg.suppress_warnings && cfg.suppress_hints) {
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

        // Determine legend mode from flags (last one wins via overrides_with)
        let legend_mode = if cli.no_legend {
            LegendMode::Never
        } else if cli.legend {
            LegendMode::Always
        } else {
            LegendMode::Auto
        };

        let default_timezone = determine_default_timezone(cli)?;
        // The naive-timestamp diagnostic (#287) must only fire when the UTC
        // default was assumed silently. Mirror determine_default_timezone's
        // precedence: an explicit --input-tz or a non-empty TZ means the user
        // chose a zone, so the assumption is not silent.
        let tz_from_env = std::env::var("TZ")
            .map(|tz| !tz.is_empty())
            .unwrap_or(false);
        let timezone_assumed = cli.input_tz.is_none() && !tz_from_env;
        let mut quiet_events = cli.quiet;
        // Advisory tiers: warnings (🔸) and hints (💡). Each resolves independently
        // with precedence: explicit per-tier flag > --diagnostics/--no-diagnostics
        // shortcut > env var > default (shown). The positive flags exist so a user
        // can override an env var or config default on a single run.
        let env_no_warnings = std::env::var("KELORA_NO_WARNINGS").is_ok();
        let env_no_hints = std::env::var("KELORA_NO_HINTS").is_ok();
        let suppress_warnings = if cli.warnings {
            false
        } else if cli.no_warnings {
            true
        } else if cli.diagnostics {
            false
        } else if cli.no_diagnostics {
            true
        } else {
            env_no_warnings
        };
        let mut suppress_hints = if cli.hints {
            false
        } else if cli.no_hints {
            true
        } else if cli.diagnostics {
            false
        } else if cli.no_diagnostics {
            true
        } else {
            env_no_hints
        };
        // Whether the user explicitly asked to *show* hints (per-tier flag or the
        // --diagnostics shortcut). Data-only modes consult this so an explicit
        // request survives their implicit hint suppression.
        let force_show_hints = cli.hints || cli.diagnostics;
        // Capture the *explicit* hint suppression now, before the data-only modes
        // below force `suppress_hints` on. Stuck-user signals key off this so they
        // survive a mode's implicit suppression but still obey a real --no-hints.
        let hints_user_suppressed = suppress_hints;
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
        let has_metric_sugar =
            !cli.freq.is_empty() || !cli.describe.is_empty() || !cli.card.is_empty();
        let metrics_format = if cli.no_metrics {
            None
        } else if cli.metrics.is_some() {
            cli.metrics.clone()
        } else if cli.with_metrics {
            Some(crate::cli::MetricsFormat::Auto)
        } else if has_metric_sugar {
            // --freq/--describe synthesize tracking; default to the auto view
            // (human table on a TTY, tsv when piped) unless an explicit format /
            // --no-metrics says otherwise.
            Some(crate::cli::MetricsFormat::Auto)
        } else {
            None
        };
        let metrics_with_events = cli.with_metrics;
        let suppress_events_for_metrics = metrics_format.is_some() && !metrics_with_events;
        let suppress_events_for_drain = cli.drain.is_some();
        let discover_fields = cli
            .discover_fields
            .clone()
            .or(cli.discover_final_fields.clone());
        let suppress_events_for_discover = discover_fields.is_some();

        // Combine suppressions from stats/metrics data-only modes
        if suppress_events_for_stats
            || suppress_events_for_metrics
            || suppress_events_for_drain
            || suppress_events_for_discover
        {
            quiet_events = true;
        }

        let output_format = if cli.json_output {
            OutputFormat::Json
        } else {
            cli.output_format.clone().into()
        };

        // Data-only modes (--metrics/--drain/--discover) suppress script output
        // and hush hints (advisory noise) to keep the machine-readable stdout
        // focused. Warnings are NOT auto-suppressed: they go to stderr (never
        // polluting the stdout data channel) and may flag a real problem — e.g.
        // recovered exec errors — that a stuck user needs to see (#239). Hide
        // them explicitly with --no-warnings or --silent. An explicit
        // --hints/--diagnostics re-enables hints even in these modes.
        let data_only_mode = suppress_events_for_metrics
            || suppress_events_for_drain
            || suppress_events_for_discover;
        if suppress_events_for_stats {
            suppress_script_output = true;
        }
        if data_only_mode {
            if !force_show_hints {
                suppress_hints = true;
            }
            suppress_script_output = true;
        }

        if silent {
            quiet_events = true;
        }

        let metrics_file = cli.metrics_file.clone();

        // The legacy "all advisory suppressed" umbrella, used for derived quiet
        // levels and per-line verbose error detail.
        let diagnostics_suppressed = suppress_warnings && suppress_hints;
        let quiet_level = if suppress_script_output {
            3
        } else if diagnostics_suppressed || silent {
            1
        } else {
            0
        };
        let verbose_level = if diagnostics_suppressed || silent {
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
                merge_ts: cli.merge_ts,
                skip_lines: cli.skip_lines.unwrap_or(0),
                head_lines: cli.head,
                section: None,      // Will be set after CLI parsing
                ignore_lines: None, // Will be set after CLI parsing
                keep_lines: None,   // Will be set after CLI parsing
                multiline: None,    // Will be set after CLI parsing
                ts_field: cli.ts_field.clone(),
                ts_format: cli.ts_format.clone(),
                default_timezone: default_timezone.clone(),
                timezone_assumed,
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
                // Default is Auto: wrap on a TTY, stay single-line when piped
                // or redirected. --wrap / --no-wrap force the mode explicitly.
                wrap: if cli.no_wrap {
                    WrapMode::Never
                } else if cli.wrap {
                    WrapMode::Always
                } else {
                    WrapMode::Auto
                },
                pretty: cli.expand_nested,
                color: color_mode,
                emoji: emoji_mode,
                legend: legend_mode,
                stats: stats_format,
                stats_with_events,
                metrics: metrics_format,
                metrics_with_events,
                metrics_file,
                drain: cli.drain.clone(),
                discover_fields,
                discover_final: cli.discover_final_fields.is_some(),
                discover_depth: cli
                    .discover_depth
                    .unwrap_or(crate::field_discovery::DEFAULT_FLATTEN_DEPTH),
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
                strict_utf8: cli.strict_utf8,
                verbose: verbose_level,
                quiet_events,
                suppress_warnings,
                suppress_hints,
                hints_user_suppressed,
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
        // Span aggregation and cross-event context (--window, -B/-C) are all
        // order-dependent: they need every event in original sequence. Under
        // parallel batching each worker keeps its own per-batch buffer, which
        // silently corrupts the results (issue #281), so force sequential the
        // same way spans always have.
        //
        // This guards the explicit flags only. A script may also reference the
        // `window` variable without --window (window_size == 0), but in that
        // case the window only ever holds the current event, so parallel and
        // sequential agree and there is nothing to protect.
        if self.processing.span.is_some()
            || self.processing.window_size > 0
            || self.processing.context.is_active()
        {
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
                merge_ts: false,
                skip_lines: 0,
                head_lines: None,
                section: None,
                ignore_lines: None,
                keep_lines: None,
                multiline: None,
                ts_field: None,
                ts_format: None,
                default_timezone: None,
                timezone_assumed: false,
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
                wrap: WrapMode::Auto,
                pretty: false,
                color: ColorMode::Auto,
                emoji: EmojiMode::Auto,
                legend: LegendMode::Auto,
                stats: None,
                stats_with_events: false,
                metrics: None,
                metrics_with_events: false,
                metrics_file: None,
                drain: None,
                discover_fields: None,
                discover_final: false,
                discover_depth: crate::field_discovery::DEFAULT_FLATTEN_DEPTH,
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
                strict_utf8: false,
                verbose: 0,
                quiet_events: false,
                suppress_warnings: false,
                suppress_hints: false,
                hints_user_suppressed: false,
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

/// Parse input format from CLI options, handling the --input-format option.
///
/// `-f` is repeatable. A single occurrence keeps the historical behavior
/// (including comma-separated cascades like `json,line`). Two or more
/// occurrences build a cascade from each spec in order, which is the only way
/// to put spec-based parsers (`cols:`, `regex:`) into a cascade — commas can't
/// safely delimit them because a regex pattern may itself contain commas.
fn parse_input_format_from_cli(cli: &crate::Cli) -> anyhow::Result<InputFormat> {
    match cli.format.as_slice() {
        [] => parse_input_format_spec("auto"),
        [single] => parse_input_format_spec(single),
        many => parse_repeated_format_specs(many),
    }
}

/// Build a cascade from repeated `-f` specs. Each spec is parsed on its own
/// (so `cols:`/`regex:` are allowed); any spec that is itself a comma cascade
/// is flattened into the result. Schema/auto formats remain illegal as cascade
/// members.
fn parse_repeated_format_specs(specs: &[String]) -> anyhow::Result<InputFormat> {
    let mut members: Vec<InputFormat> = Vec::with_capacity(specs.len());
    for spec in specs {
        match parse_input_format_spec(spec)? {
            InputFormat::Cascade(inner) => members.extend(inner),
            other => members.push(other),
        }
    }

    for fmt in &members {
        match fmt {
            InputFormat::Json
            | InputFormat::Line
            | InputFormat::Raw
            | InputFormat::Logfmt
            | InputFormat::Syslog
            | InputFormat::Cef
            | InputFormat::Combined
            | InputFormat::Cols(_)
            | InputFormat::Regex(_)
            | InputFormat::Named(_) => {}
            InputFormat::Auto | InputFormat::AutoPerFile => {
                return Err(anyhow::anyhow!(
                    "'{}' cannot be combined with other formats; list concrete formats instead",
                    fmt.cascade_name()
                ));
            }
            InputFormat::Csv(_) | InputFormat::Tsv(_) | InputFormat::Csvnh | InputFormat::Tsvnh => {
                return Err(anyhow::anyhow!(
                    "'{}' is a schema-based format and cannot be mixed per-line in a cascade",
                    fmt.cascade_name()
                ));
            }
            InputFormat::Cascade(_) => unreachable!("cascades were flattened above"),
        }
    }

    if members.len() < 2 {
        return Err(anyhow::anyhow!(
            "a cascade needs at least two formats; pass a single -f for just one"
        ));
    }

    validate_cascade_order(&members)?;
    Ok(InputFormat::Cascade(members))
}

/// Enforce that greedy catch-all parsers come last in a cascade. `line`, `raw`,
/// and `cols:` accept essentially any line (in resilient mode `cols:` fills
/// missing fields with () rather than failing), so anything after them would
/// never run. `regex:` is selective (it declines non-matching lines), so it is
/// allowed in any position.
fn validate_cascade_order(formats: &[InputFormat]) -> anyhow::Result<()> {
    for (idx, fmt) in formats.iter().enumerate() {
        let is_catch_all = matches!(
            fmt,
            InputFormat::Line | InputFormat::Raw | InputFormat::Cols(_)
        );
        if is_catch_all && idx != formats.len() - 1 {
            return Err(anyhow::anyhow!(
                "'{}' matches every line, so it must be the last format in a cascade; later formats would never run",
                fmt.cascade_name()
            ));
        }
    }
    Ok(())
}

/// Parse input format specification string (e.g., "cols:ts(2) level - *msg")
pub(crate) fn parse_input_format_spec(spec: &str) -> anyhow::Result<InputFormat> {
    // Cascade mode: comma-separated list of simple formats.
    // Detected by presence of a comma at the top level. We deliberately only
    // allow cascade with simple formats (no colons/specs) to avoid ambiguity
    // with "csv:spec" or "regex:pattern" that may contain commas.
    if spec.contains(',')
        && !spec.starts_with("regex:")
        && !spec.starts_with("cols:")
        && !spec.starts_with("csv:")
        && !spec.starts_with("csv ")
        && !spec.starts_with("tsv:")
        && !spec.starts_with("tsv ")
    {
        return parse_cascade_spec(spec);
    }

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
        "auto-per-file" => Ok(InputFormat::AutoPerFile),
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
        other => {
            // Built-in named formats (adapted from lnav), e.g. -f log4j
            if let Some(fmt) = crate::parsers::lnav_formats::by_name(other) {
                return Ok(InputFormat::Named(fmt));
            }
            Err(anyhow::anyhow!("Unknown input format: '{}'. Supported formats: auto, auto-per-file, json, line, raw, logfmt, syslog, cef, csv, tsv, csvnh, tsvnh, combined, cols:<spec>, regex:<pattern>, or a named format ({})", spec, crate::parsers::lnav_formats::names_csv()))
        }
    }
}

/// Parse a cascade format spec like "json,logfmt,line".
/// Only simple, schema-less formats are allowed; CSV/TSV/cols/regex/auto are rejected.
fn parse_cascade_spec(spec: &str) -> anyhow::Result<InputFormat> {
    let parts: Vec<&str> = spec.split(',').map(|s| s.trim()).collect();
    if parts.len() < 2 {
        return Err(anyhow::anyhow!(
            "cascade format requires at least two formats, e.g., 'json,line'"
        ));
    }
    let mut formats = Vec::with_capacity(parts.len());
    let mut seen = std::collections::HashSet::new();
    for part in parts {
        if part.is_empty() {
            return Err(anyhow::anyhow!(
                "cascade format contains an empty entry in '{}'",
                spec
            ));
        }
        let fmt = match part.to_lowercase().as_str() {
            "json" => InputFormat::Json,
            "line" => InputFormat::Line,
            "raw" => InputFormat::Raw,
            "logfmt" => InputFormat::Logfmt,
            "syslog" => InputFormat::Syslog,
            "cef" => InputFormat::Cef,
            "combined" => InputFormat::Combined,
            "auto" => {
                return Err(anyhow::anyhow!(
                    "'auto' is not allowed inside a cascade list; list the formats explicitly"
                ));
            }
            "auto-per-file" => {
                return Err(anyhow::anyhow!(
                    "'auto-per-file' is not allowed inside a cascade list; list the formats explicitly"
                ));
            }
            "csv" | "tsv" | "csvnh" | "tsvnh" => {
                return Err(anyhow::anyhow!(
                    "'{}' is not allowed inside a cascade list (schema-based formats cannot be mixed per-line)",
                    part
                ));
            }
            "cols" | "regex" | "cascade" => {
                return Err(anyhow::anyhow!(
                    "'{}' can't be a member of a comma-separated cascade (a regex pattern may contain commas). Use repeated -f flags instead, e.g. -f json -f 'cols:ts level *msg'",
                    part
                ));
            }
            other => {
                // Built-in named formats (adapted from lnav) are regex-backed and
                // safe to try per-line, so they are allowed in cascade lists.
                if let Some(fmt) = crate::parsers::lnav_formats::by_name(other) {
                    InputFormat::Named(fmt)
                } else {
                    return Err(anyhow::anyhow!(
                        "Unknown format '{}' in cascade list. Allowed: json, line, raw, logfmt, syslog, cef, combined, and named formats ({})",
                        part,
                        crate::parsers::lnav_formats::names_csv()
                    ));
                }
            }
        };
        let name = fmt.cascade_name();
        if !seen.insert(name) {
            return Err(anyhow::anyhow!(
                "cascade list contains duplicate format '{}'",
                name
            ));
        }
        formats.push(fmt);
    }

    validate_cascade_order(&formats)?;

    Ok(InputFormat::Cascade(formats))
}

/// Create timestamp formatting configuration from CLI options
fn create_timestamp_format_config(
    cli: &crate::Cli,
    default_timezone: Option<String>,
) -> TimestampFormatConfig {
    let auto_format_all = cli.format_timestamps_local || cli.format_timestamps_utc;

    let mut format_fields = Vec::new();
    if auto_format_all {
        if let Some(ref ts_field) = cli.ts_field {
            let trimmed = ts_field.trim();
            if !trimmed.is_empty() {
                format_fields.push(trimmed.to_string());
            }
        }
    }
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
            "Context options (-A, -B, -C) require active filtering because context is shown around matches. Add --filter, --levels, --since, or --until."
        ));
    }

    Ok(ContextConfig::new(before_context, after_context))
}

/// Determine the default timezone based on CLI options and environment
/// Following the new spec: --input-tz defaults to UTC
fn determine_default_timezone(cli: &crate::Cli) -> anyhow::Result<Option<String>> {
    // Priority 1: --input-tz option
    if let Some(ref input_tz) = cli.input_tz {
        if input_tz == "local" {
            return Ok(None); // None means local time
        }
        // Validate explicit timezones up front so a typo fails fast at config
        // time instead of silently falling back to the machine's local time
        // (which would shift every timestamp, and thus time filters and span
        // boundaries, without any visible error).
        if input_tz.parse::<chrono_tz::Tz>().is_err() {
            anyhow::bail!(
                "Invalid --input-tz '{}': expected 'local', 'UTC', or an IANA timezone name \
                 (e.g. Europe/Berlin, America/New_York)",
                input_tz
            );
        }
        return Ok(Some(input_tz.clone()));
    }

    // Priority 2: TZ environment variable
    if let Ok(tz) = std::env::var("TZ") {
        if !tz.is_empty() {
            return Ok(Some(tz));
        }
    }

    // DEFAULT: UTC (per spec, --input-tz defaults to UTC)
    Ok(Some("UTC".to_string()))
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
                "--span-close requires --span or --span-idle. Use --span N for fixed-size spans or --span-idle 30s for inactivity-based spans."
            ));
        }
        return Ok(None);
    }

    if span_spec.is_some() && idle_spec.is_some() {
        return Err(anyhow::anyhow!(
            "--span and --span-idle cannot be used together. Use --span N for fixed-size spans or --span-idle 30s for inactivity-based spans."
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
            crate::InputFormat::AutoPerFile => InputFormat::AutoPerFile,
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
            InputFormat::AutoPerFile => crate::InputFormat::AutoPerFile,
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
            // Named formats are regex-backed; map to Regex in the (unused) legacy
            // CLI-enum conversion path.
            InputFormat::Named(_) => crate::InputFormat::Regex,
            // Cascade has no direct equivalent in the CLI enum; fall back to Auto
            // for the (unused) legacy conversion path.
            InputFormat::Cascade(_) => crate::InputFormat::Auto,
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
        let _lock = crate::test_env::lock_env();
        let _guard = EnvGuard::new(keys);
        f();
    }

    #[test]
    fn determine_default_timezone_defaults_to_utc() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora"]);
            let tz = super::determine_default_timezone(&cli).unwrap();
            assert_eq!(tz.as_deref(), Some("UTC"));
        });
    }

    #[test]
    fn determine_default_timezone_respects_cli_local() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora", "--input-tz", "local"]);
            let tz = super::determine_default_timezone(&cli).unwrap();
            assert_eq!(tz, None);
        });
    }

    #[test]
    fn determine_default_timezone_prefers_cli_over_env() {
        with_env_lock(&["TZ"], || {
            std::env::set_var("TZ", "America/New_York");
            let cli = Cli::parse_from(["kelora", "--input-tz", "Europe/Berlin"]);
            let tz = super::determine_default_timezone(&cli).unwrap();
            assert_eq!(tz.as_deref(), Some("Europe/Berlin"));
        });
    }

    #[test]
    fn determine_default_timezone_uses_environment_when_present() {
        with_env_lock(&["TZ"], || {
            std::env::set_var("TZ", "Asia/Tokyo");
            let cli = Cli::parse_from(["kelora"]);
            let tz = super::determine_default_timezone(&cli).unwrap();
            assert_eq!(tz.as_deref(), Some("Asia/Tokyo"));
        });
    }

    #[test]
    fn determine_default_timezone_rejects_invalid_input_tz() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora", "--input-tz", "Europe/Berln"]);
            let result = super::determine_default_timezone(&cli);
            assert!(
                result.is_err(),
                "Invalid --input-tz should be rejected, got: {:?}",
                result
            );
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Invalid --input-tz") && msg.contains("Europe/Berln"),
                "Error should name the bad timezone, got: {}",
                msg
            );
        });
    }

    #[test]
    fn determine_default_timezone_accepts_utc_explicitly() {
        with_env_lock(&["TZ"], || {
            std::env::remove_var("TZ");
            let cli = Cli::parse_from(["kelora", "--input-tz", "UTC"]);
            let tz = super::determine_default_timezone(&cli).unwrap();
            assert_eq!(tz.as_deref(), Some("UTC"));
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

    #[test]
    fn parse_cascade_spec_rejects_line_before_last_position() {
        let err = parse_input_format_spec("json,line,logfmt")
            .expect_err("line before the last position should be rejected");
        let message = err.to_string();
        assert!(message.contains("line"));
        assert!(message.contains("must be the last format"));
    }

    #[test]
    fn parse_cascade_spec_rejects_raw_before_last_position() {
        let err = parse_input_format_spec("json,raw,logfmt")
            .expect_err("raw before the last position should be rejected");
        let message = err.to_string();
        assert!(message.contains("raw"));
        assert!(message.contains("must be the last format"));
    }

    #[test]
    fn parse_cascade_spec_allows_catch_all_last() {
        let parsed = parse_input_format_spec("json,logfmt,line")
            .expect("line should be allowed as the final fallback");
        assert!(matches!(parsed, InputFormat::Cascade(_)));

        let parsed = parse_input_format_spec("json,logfmt,raw")
            .expect("raw should be allowed as the final fallback");
        assert!(matches!(parsed, InputFormat::Cascade(_)));
    }

    #[test]
    fn parse_named_format_by_name() {
        match parse_input_format_spec("log4j").expect("log4j is a named format") {
            InputFormat::Named(fmt) => assert_eq!(fmt.name, "log4j"),
            other => panic!("expected Named(log4j), got {other:?}"),
        }
        // Case-insensitive for friendliness.
        match parse_input_format_spec("GLOG").expect("glog is a named format") {
            InputFormat::Named(fmt) => assert_eq!(fmt.name, "glog"),
            other => panic!("expected Named(glog), got {other:?}"),
        }
        assert_eq!(
            parse_input_format_spec("log4j")
                .unwrap()
                .to_display_string(),
            "log4j"
        );
    }

    #[test]
    fn parse_named_format_in_cascade() {
        let parsed = parse_input_format_spec("log4j,line")
            .expect("named formats are allowed in cascade lists");
        match parsed {
            InputFormat::Cascade(formats) => {
                assert!(matches!(formats[0], InputFormat::Named(f) if f.name == "log4j"));
                assert!(matches!(formats[1], InputFormat::Line));
            }
            other => panic!("expected cascade, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_format_lists_named_options() {
        let err = parse_input_format_spec("log4jx").expect_err("unknown format should error");
        let msg = err.to_string();
        assert!(
            msg.contains("log4j"),
            "error should list named formats: {msg}"
        );
    }
}
