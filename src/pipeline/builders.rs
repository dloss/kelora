#![allow(dead_code)] // Builder API keeps unused setters for future CLI/config surfaces
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};

use crate::stats::stats_set_timestamp_override;

/// Wrapper parser that applies timestamp configuration after parsing
struct TimestampConfiguredParser {
    inner: Box<dyn EventParser>,
    ts_config: crate::timestamp::TsConfig,
}

impl TimestampConfiguredParser {
    fn new(
        inner: Box<dyn EventParser>,
        ts_field: Option<String>,
        ts_format: Option<String>,
        default_timezone: Option<String>,
    ) -> Self {
        Self {
            inner,
            ts_config: crate::timestamp::TsConfig {
                custom_field: ts_field,
                custom_format: ts_format,
                default_timezone,
            },
        }
    }
}

impl EventParser for TimestampConfiguredParser {
    fn parse(&self, line: &str) -> Result<crate::event::Event> {
        let mut event = self.inner.parse(line)?;
        // Apply timestamp configuration
        event.extract_timestamp_with_config(None, &self.ts_config);
        Ok(event)
    }
}

use super::{
    create_multiline_chunker, BeginStage, EndStage, EventLimiter, EventParser, ExecStage,
    FilterStage, Formatter, KeyFilterStage, LevelFilterStage, MetaData, Pipeline, PipelineConfig,
    PipelineContext, ScriptStage, SimpleChunker, SimpleWindowManager, SlidingWindowManager,
    StdoutWriter, TakeNLimiter, TimestampConversionStage, TimestampFilterStage,
};
use crate::engine::{DebugConfig, RhaiEngine};
use crate::readers::MultiFileReader;
use crate::rhai_functions::file_ops::{self, RuntimeConfig};
use crate::rhai_functions::hashing;

/// Pipeline builder for easy construction from CLI arguments
#[derive(Clone)]
pub struct PipelineBuilder {
    config: PipelineConfig,
    begin: Option<String>,
    end: Option<String>,
    input_format: crate::config::InputFormat,
    output_format: crate::OutputFormat,
    take_limit: Option<usize>,
    keys: Vec<String>,
    exclude_keys: Vec<String>,
    // Fallback level filters when stages don't include explicit level entries
    levels: Vec<String>,
    exclude_levels: Vec<String>,
    multiline: Option<crate::config::MultilineConfig>,
    window_size: usize,
    csv_headers: Option<Vec<String>>, // Pre-processed CSV headers for parallel mode
    timestamp_filter: Option<crate::config::TimestampFilterConfig>,
    normalize_timestamps: bool,
    ts_field: Option<String>,
    ts_format: Option<String>,
    default_timezone: Option<String>,
    extract_prefix: Option<String>,
    prefix_sep: String,
    cols_spec: Option<String>,
    cols_sep: Option<String>,
    context_config: crate::config::ContextConfig,
    span: Option<crate::config::SpanConfig>,
    strict: bool,
    heat_range: Option<(f64, f64)>,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self {
            config: PipelineConfig {
                brief: false,
                wrap: true, // Default to enabled
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                format_name: None,
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_diagnostics: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                input_files: Vec::new(),
                allow_fs_writes: false,
            },
            begin: None,
            end: None,
            input_format: crate::config::InputFormat::Json,
            output_format: crate::OutputFormat::Default,
            take_limit: None,
            keys: Vec::new(),
            exclude_keys: Vec::new(),
            levels: Vec::new(),
            exclude_levels: Vec::new(),
            multiline: None,
            window_size: 0,
            csv_headers: None,
            timestamp_filter: None,
            normalize_timestamps: false,
            ts_field: None,
            ts_format: None,
            default_timezone: None,
            extract_prefix: None,
            prefix_sep: "|".to_string(),
            cols_spec: None,
            cols_sep: None,
            context_config: crate::config::ContextConfig::disabled(),
            span: None,
            strict: false,
            heat_range: None,
        }
    }

    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Build pipeline with stages
    pub fn build(
        self,
        stages: Vec<crate::config::ScriptStageType>,
    ) -> Result<(Pipeline, BeginStage, EndStage, PipelineContext)> {
        let mut rhai_engine = RhaiEngine::new();
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        rhai_engine.set_use_emoji(use_emoji);

        // Set up debugging if enabled
        let debug_config = DebugConfig::new(self.config.verbose).with_emoji(use_emoji);
        rhai_engine.setup_debugging(debug_config);

        // Set up side effect suppression when script output is disabled
        if self.config.suppress_script_output {
            rhai_engine.set_suppress_side_effects(true);
        }

        file_ops::set_runtime_config(RuntimeConfig {
            allow_fs_writes: self.config.allow_fs_writes,
            strict: self.config.strict,
            quiet_level: self.config.quiet_level,
        });

        hashing::set_runtime_config(hashing::HashingRuntimeConfig {
            verbose: self.config.verbose,
            use_emoji,
        });

        // Create parser
        let custom_ts_config =
            self.ts_field.is_some() || self.ts_format.is_some() || self.default_timezone.is_some();

        stats_set_timestamp_override(self.ts_field.clone(), self.ts_format.clone());

        let base_parser: Box<dyn EventParser> = match self.input_format {
            crate::config::InputFormat::Auto => {
                return Err(anyhow::anyhow!(
                    "Auto format should be resolved before pipeline creation"
                ));
            }
            crate::config::InputFormat::Json => {
                if custom_ts_config {
                    Box::new(crate::parsers::JsonlParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::JsonlParser::new())
                }
            }
            crate::config::InputFormat::Line => Box::new(crate::parsers::LineParser::new()),
            crate::config::InputFormat::Raw => Box::new(crate::parsers::RawParser::new()),
            crate::config::InputFormat::Logfmt => {
                if custom_ts_config {
                    Box::new(crate::parsers::LogfmtParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::LogfmtParser::new())
                }
            }
            crate::config::InputFormat::Syslog => {
                if custom_ts_config {
                    Box::new(crate::parsers::SyslogParser::new_without_auto_timestamp()?)
                } else {
                    Box::new(crate::parsers::SyslogParser::new()?)
                }
            }
            crate::config::InputFormat::Cef => {
                if custom_ts_config {
                    Box::new(crate::parsers::CefParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::CefParser::new())
                }
            }
            crate::config::InputFormat::Csv(ref field_spec) => {
                let parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_csv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_csv()
                };

                // Apply field spec if provided
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
                        .with_strict(self.strict)
                        .with_auto_timestamp(!custom_ts_config)
                } else if custom_ts_config {
                    parser.with_auto_timestamp(false)
                } else {
                    parser
                };

                Box::new(parser)
            }
            crate::config::InputFormat::Tsv(ref field_spec) => {
                let parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_tsv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_tsv()
                };

                // Apply field spec if provided
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
                        .with_strict(self.strict)
                        .with_auto_timestamp(!custom_ts_config)
                } else if custom_ts_config {
                    parser.with_auto_timestamp(false)
                } else {
                    parser
                };

                Box::new(parser)
            }
            crate::config::InputFormat::Csvnh => {
                if let Some(ref headers) = self.csv_headers {
                    let parser =
                        crate::parsers::CsvParser::new_csv_no_headers_with_columns(headers.clone());
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser = crate::parsers::CsvParser::new_csv_no_headers();
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                }
            }
            crate::config::InputFormat::Tsvnh => {
                if let Some(ref headers) = self.csv_headers {
                    let parser =
                        crate::parsers::CsvParser::new_tsv_no_headers_with_columns(headers.clone());
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser = crate::parsers::CsvParser::new_tsv_no_headers();
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                }
            }
            crate::config::InputFormat::Combined => {
                if custom_ts_config {
                    Box::new(crate::parsers::CombinedParser::new_without_auto_timestamp()?)
                } else {
                    Box::new(crate::parsers::CombinedParser::new()?)
                }
            }
            crate::config::InputFormat::Cols(_) => {
                if let Some(ref spec) = self.cols_spec {
                    Box::new(
                        crate::parsers::ColsParser::new(spec.clone(), self.cols_sep.clone())
                            .with_strict(self.strict),
                    )
                } else {
                    return Err(anyhow::anyhow!("Cols format requires a specification"));
                }
            }
            crate::config::InputFormat::Regex(ref pattern) => {
                Box::new(crate::parsers::RegexParser::new(pattern)?.with_strict(self.strict))
            }
        };

        // Wrap parser with prefix extraction if needed
        let parser_with_prefix: Box<dyn EventParser> = if self.extract_prefix.is_some() {
            let prefix_extractor = super::PrefixExtractor::new(
                self.extract_prefix.clone().unwrap(),
                self.prefix_sep.clone(),
            );
            Box::new(super::PrefixExtractingParser::new(
                base_parser,
                Some(prefix_extractor),
            ))
        } else {
            base_parser
        };

        // Wrap parser with timestamp configuration if needed
        let parser: Box<dyn EventParser> = if custom_ts_config {
            Box::new(TimestampConfiguredParser::new(
                parser_with_prefix,
                self.ts_field.clone(),
                self.ts_format.clone(),
                self.default_timezone.clone(),
            ))
        } else {
            parser_with_prefix
        };

        // Create formatter
        let use_colors = crate::tty::should_use_colors_with_mode(&self.config.color_mode);
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        let formatter: Box<dyn Formatter> = if self.config.quiet_events {
            Box::new(crate::formatters::HideFormatter::new())
        } else {
            match self.output_format {
                crate::OutputFormat::Json => Box::new(crate::formatters::JsonFormatter::new()),
                crate::OutputFormat::Default => {
                    Box::new(crate::formatters::DefaultFormatter::new_with_wrapping(
                        use_colors,
                        use_emoji,
                        self.config.brief,
                        self.config.timestamp_formatting.clone(),
                        self.config.wrap,
                        self.config.pretty,
                        self.config.quiet_level,
                    ))
                }
                crate::OutputFormat::Inspect => Box::new(crate::formatters::InspectFormatter::new(
                    self.config.verbose,
                )),
                crate::OutputFormat::Logfmt => Box::new(crate::formatters::LogfmtFormatter::new()),
                crate::OutputFormat::Levelmap => {
                    Box::new(crate::formatters::LevelmapFormatter::new(use_colors))
                }
                crate::OutputFormat::Keymap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "keymap output format requires --keys to specify exactly one field"
                        ));
                    }
                    Box::new(crate::formatters::KeymapFormatter::new(Some(
                        self.keys[0].clone(),
                    )))
                }
                crate::OutputFormat::Heatmap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "heatmap output format requires --keys to specify exactly one field"
                        ));
                    }
                    Box::new(crate::formatters::HeatmapFormatter::new(
                        Some(self.keys[0].clone()),
                        self.heat_range,
                    ))
                }
                crate::OutputFormat::Csv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSV output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new(self.keys.clone()))
                }
                crate::OutputFormat::Tsv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSV output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv(self.keys.clone()))
                }
                crate::OutputFormat::Csvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSVNH output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_csv_no_header(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSVNH output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv_no_header(
                        self.keys.clone(),
                    ))
                }
            }
        };

        // Create script stages with numbering
        let mut script_stages: Vec<Box<dyn ScriptStage>> = Vec::new();
        let mut stage_number = 1;

        let has_script_filters = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::Filter(_)));
        let has_inline_level_stage = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::LevelFilter { .. }));
        let mut level_context = if !has_script_filters && self.context_config.is_active() {
            Some(self.context_config.clone())
        } else {
            None
        };

        for stage in stages {
            match stage {
                crate::config::ScriptStageType::Filter(filter) => {
                    let filter_stage = FilterStage::new(filter, &mut rhai_engine)?
                        .with_stage_number(stage_number)
                        .with_context(self.context_config.clone());
                    script_stages.push(Box::new(filter_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Exec(exec) => {
                    let exec_stage =
                        ExecStage::new(exec, &mut rhai_engine)?.with_stage_number(stage_number);
                    script_stages.push(Box::new(exec_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::LevelFilter { include, exclude } => {
                    let mut level_stage = LevelFilterStage::new(include, exclude);
                    if level_stage.is_active() {
                        if let Some(context) = level_context.take() {
                            level_stage = level_stage.with_context(context);
                        }
                        script_stages.push(Box::new(level_stage));
                        stage_number += 1;
                    }
                }
            }
        }

        if !has_inline_level_stage {
            let mut level_stage =
                LevelFilterStage::new(self.levels.clone(), self.exclude_levels.clone());
            if level_stage.is_active() {
                if let Some(context) = level_context.take() {
                    level_stage = level_stage.with_context(context);
                }
                script_stages.push(Box::new(level_stage));
            }
        }

        // Add timestamp filtering stage (runs after script stages, before level filtering)
        if let Some(timestamp_filter_config) = self.timestamp_filter {
            let timestamp_filter_stage = TimestampFilterStage::new(timestamp_filter_config);
            script_stages.push(Box::new(timestamp_filter_stage));
        }

        if self.normalize_timestamps {
            let conversion_stage = TimestampConversionStage::new(
                self.ts_field.clone(),
                self.ts_format.clone(),
                self.default_timezone.clone(),
            );
            script_stages.push(Box::new(conversion_stage));
        }

        // Add key filtering stage (runs after level filtering, before context processing)
        let key_filter_stage = KeyFilterStage::new(self.keys.clone(), self.exclude_keys.clone());
        if key_filter_stage.is_active() {
            script_stages.push(Box::new(key_filter_stage));
        }

        // Context processing is now handled within FilterStage

        // Create limiter if specified
        let limiter: Option<Box<dyn EventLimiter>> = if let Some(limit) = self.take_limit {
            Some(Box::new(TakeNLimiter::new(limit)))
        } else {
            None
        };

        // Create begin and end stages
        let begin_stage = BeginStage::new(self.begin, &mut rhai_engine)?;
        let end_stage = EndStage::new(self.end, &mut rhai_engine)?;

        let span_processor = if let Some(ref span_config) = self.span {
            let compiled = if let Some(ref script) = span_config.close_script {
                Some(rhai_engine.compile_span_close(script)?)
            } else {
                None
            };
            Some(crate::pipeline::span::SpanProcessor::new(
                span_config.clone(),
                compiled,
            ))
        } else {
            None
        };

        // Create pipeline context
        let ctx = PipelineContext {
            config: self.config,
            tracker: HashMap::new(),
            internal_tracker: HashMap::new(),
            window: Vec::new(),
            rhai: rhai_engine.clone(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        // Create chunker based on multiline configuration
        let chunker = if let Some(ref multiline_config) = self.multiline {
            create_multiline_chunker(multiline_config, self.input_format.clone())
                .map_err(|e| anyhow::anyhow!("Failed to create multiline chunker: {}", e))?
        } else {
            Box::new(SimpleChunker) as Box<dyn super::Chunker>
        };

        // Create window manager based on window_size configuration
        let window_manager: Box<dyn super::WindowManager> = if self.window_size > 0 {
            Box::new(SlidingWindowManager::new(self.window_size))
        } else {
            Box::new(SimpleWindowManager::new())
        };

        // Create timestamp config for consistent timestamp parsing
        let ts_config = crate::timestamp::TsConfig {
            custom_field: self.ts_field.clone(),
            custom_format: self.ts_format.clone(),
            default_timezone: self.default_timezone.clone(),
        };

        // Create pipeline
        let pipeline = Pipeline {
            line_filter: None, // No line filter implementation yet
            chunker,
            parser,
            script_stages,
            limiter,
            formatter,
            output: Box::new(StdoutWriter),
            window_manager,
            span_processor,
            ts_config,
        };

        Ok((pipeline, begin_stage, end_stage, ctx))
    }

    pub fn with_begin(mut self, begin: Option<String>) -> Self {
        self.begin = begin;
        self
    }

    pub fn with_end(mut self, end: Option<String>) -> Self {
        self.end = end;
        self
    }

    pub fn with_input_format(mut self, format: crate::config::InputFormat) -> Self {
        self.input_format = format;
        self
    }

    pub fn with_output_format(mut self, format: crate::OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    pub fn with_take_limit(mut self, limit: Option<usize>) -> Self {
        self.take_limit = limit;
        self
    }

    /// Build a worker pipeline for parallel processing
    pub fn build_worker(
        self,
        stages: Vec<crate::config::ScriptStageType>,
    ) -> Result<(Pipeline, PipelineContext)> {
        let mut rhai_engine = RhaiEngine::new();

        // Set up debugging if enabled
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        let debug_config = DebugConfig::new(self.config.verbose).with_emoji(use_emoji);
        rhai_engine.setup_debugging(debug_config);

        // Set up side effect suppression when script output is disabled
        if self.config.suppress_script_output {
            rhai_engine.set_suppress_side_effects(true);
        }

        file_ops::set_runtime_config(RuntimeConfig {
            allow_fs_writes: self.config.allow_fs_writes,
            strict: self.config.strict,
            quiet_level: self.config.quiet_level,
        });

        hashing::set_runtime_config(hashing::HashingRuntimeConfig {
            verbose: self.config.verbose,
            use_emoji,
        });

        // Create parser (with pre-processed CSV headers if available)
        let custom_ts_config =
            self.ts_field.is_some() || self.ts_format.is_some() || self.default_timezone.is_some();

        stats_set_timestamp_override(self.ts_field.clone(), self.ts_format.clone());

        let base_parser: Box<dyn EventParser> = match self.input_format {
            crate::config::InputFormat::Auto => {
                return Err(anyhow::anyhow!(
                    "Auto format should be resolved before pipeline creation"
                ));
            }
            crate::config::InputFormat::Json => {
                if custom_ts_config {
                    Box::new(crate::parsers::JsonlParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::JsonlParser::new())
                }
            }
            crate::config::InputFormat::Line => Box::new(crate::parsers::LineParser::new()),
            crate::config::InputFormat::Raw => Box::new(crate::parsers::RawParser::new()),
            crate::config::InputFormat::Logfmt => {
                if custom_ts_config {
                    Box::new(crate::parsers::LogfmtParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::LogfmtParser::new())
                }
            }
            crate::config::InputFormat::Syslog => {
                if custom_ts_config {
                    Box::new(crate::parsers::SyslogParser::new_without_auto_timestamp()?)
                } else {
                    Box::new(crate::parsers::SyslogParser::new()?)
                }
            }
            crate::config::InputFormat::Cef => {
                if custom_ts_config {
                    Box::new(crate::parsers::CefParser::new_without_auto_timestamp())
                } else {
                    Box::new(crate::parsers::CefParser::new())
                }
            }
            crate::config::InputFormat::Csv(ref field_spec) => {
                let parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_csv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_csv()
                };

                // Apply field spec if provided
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
                        .with_strict(self.strict)
                        .with_auto_timestamp(!custom_ts_config)
                } else if custom_ts_config {
                    parser.with_auto_timestamp(false)
                } else {
                    parser
                };

                Box::new(parser)
            }
            crate::config::InputFormat::Tsv(ref field_spec) => {
                let parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_tsv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_tsv()
                };

                // Apply field spec if provided
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
                        .with_strict(self.strict)
                        .with_auto_timestamp(!custom_ts_config)
                } else if custom_ts_config {
                    parser.with_auto_timestamp(false)
                } else {
                    parser
                };

                Box::new(parser)
            }
            crate::config::InputFormat::Csvnh => {
                if let Some(ref headers) = self.csv_headers {
                    let parser =
                        crate::parsers::CsvParser::new_csv_no_headers_with_columns(headers.clone());
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser = crate::parsers::CsvParser::new_csv_no_headers();
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                }
            }
            crate::config::InputFormat::Tsvnh => {
                if let Some(ref headers) = self.csv_headers {
                    let parser =
                        crate::parsers::CsvParser::new_tsv_no_headers_with_columns(headers.clone());
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser = crate::parsers::CsvParser::new_tsv_no_headers();
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                }
            }
            crate::config::InputFormat::Combined => {
                if custom_ts_config {
                    Box::new(crate::parsers::CombinedParser::new_without_auto_timestamp()?)
                } else {
                    Box::new(crate::parsers::CombinedParser::new()?)
                }
            }
            crate::config::InputFormat::Cols(_) => {
                if let Some(ref spec) = self.cols_spec {
                    Box::new(
                        crate::parsers::ColsParser::new(spec.clone(), self.cols_sep.clone())
                            .with_strict(self.strict),
                    )
                } else {
                    return Err(anyhow::anyhow!("Cols format requires a specification"));
                }
            }
            crate::config::InputFormat::Regex(ref pattern) => {
                Box::new(crate::parsers::RegexParser::new(pattern)?.with_strict(self.strict))
            }
        };

        // Wrap parser with prefix extraction if needed
        let parser_with_prefix: Box<dyn EventParser> = if self.extract_prefix.is_some() {
            let prefix_extractor = super::PrefixExtractor::new(
                self.extract_prefix.clone().unwrap(),
                self.prefix_sep.clone(),
            );
            Box::new(super::PrefixExtractingParser::new(
                base_parser,
                Some(prefix_extractor),
            ))
        } else {
            base_parser
        };

        // Wrap parser with timestamp configuration if needed
        let parser: Box<dyn EventParser> = if custom_ts_config {
            Box::new(TimestampConfiguredParser::new(
                parser_with_prefix,
                self.ts_field.clone(),
                self.ts_format.clone(),
                self.default_timezone.clone(),
            ))
        } else {
            parser_with_prefix
        };

        // Create formatter (workers still need formatters for output)
        let use_colors = crate::tty::should_use_colors_with_mode(&self.config.color_mode);
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        let formatter: Box<dyn Formatter> = if self.config.quiet_events {
            Box::new(crate::formatters::HideFormatter::new())
        } else {
            match self.output_format {
                crate::OutputFormat::Json => Box::new(crate::formatters::JsonFormatter::new()),
                crate::OutputFormat::Default => {
                    Box::new(crate::formatters::DefaultFormatter::new_with_wrapping(
                        use_colors,
                        use_emoji,
                        self.config.brief,
                        self.config.timestamp_formatting.clone(),
                        self.config.wrap,
                        self.config.pretty,
                        self.config.quiet_level,
                    ))
                }
                crate::OutputFormat::Inspect => Box::new(crate::formatters::InspectFormatter::new(
                    self.config.verbose,
                )),
                crate::OutputFormat::Logfmt => Box::new(crate::formatters::LogfmtFormatter::new()),
                crate::OutputFormat::Levelmap => {
                    Box::new(crate::formatters::LevelmapFormatter::new(use_colors))
                }
                crate::OutputFormat::Keymap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "keymap output format requires --keys to specify exactly one field"
                        ));
                    }
                    Box::new(crate::formatters::KeymapFormatter::new(Some(
                        self.keys[0].clone(),
                    )))
                }
                crate::OutputFormat::Heatmap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "heatmap output format requires --keys to specify exactly one field"
                        ));
                    }
                    Box::new(crate::formatters::HeatmapFormatter::new(
                        Some(self.keys[0].clone()),
                        self.heat_range,
                    ))
                }
                crate::OutputFormat::Csv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSV output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSV output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Csvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSVNH output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_csv_no_header_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSVNH output format requires --keys to specify field order"
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv_no_header_worker(
                        self.keys.clone(),
                    ))
                }
            }
        };

        // Create script stages with numbering
        let mut script_stages: Vec<Box<dyn ScriptStage>> = Vec::new();
        let mut stage_number = 1;

        let has_script_filters = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::Filter(_)));
        let has_inline_level_stage = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::LevelFilter { .. }));
        let mut level_context = if !has_script_filters && self.context_config.is_active() {
            Some(self.context_config.clone())
        } else {
            None
        };

        for stage in stages {
            match stage {
                crate::config::ScriptStageType::Filter(filter) => {
                    let filter_stage = FilterStage::new(filter, &mut rhai_engine)?
                        .with_stage_number(stage_number)
                        .with_context(self.context_config.clone());
                    script_stages.push(Box::new(filter_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Exec(exec) => {
                    let exec_stage =
                        ExecStage::new(exec, &mut rhai_engine)?.with_stage_number(stage_number);
                    script_stages.push(Box::new(exec_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::LevelFilter { include, exclude } => {
                    let mut level_stage = LevelFilterStage::new(include, exclude);
                    if level_stage.is_active() {
                        if let Some(context) = level_context.take() {
                            level_stage = level_stage.with_context(context);
                        }
                        script_stages.push(Box::new(level_stage));
                        stage_number += 1;
                    }
                }
            }
        }

        if !has_inline_level_stage {
            let mut level_stage =
                LevelFilterStage::new(self.levels.clone(), self.exclude_levels.clone());
            if level_stage.is_active() {
                if let Some(context) = level_context.take() {
                    level_stage = level_stage.with_context(context);
                }
                script_stages.push(Box::new(level_stage));
            }
        }

        // Add timestamp filtering stage (runs after script stages, before level filtering)
        if let Some(timestamp_filter_config) = self.timestamp_filter {
            let timestamp_filter_stage = TimestampFilterStage::new(timestamp_filter_config);
            script_stages.push(Box::new(timestamp_filter_stage));
        }

        // Add key filtering stage (runs after level filtering, before context processing)
        let key_filter_stage = KeyFilterStage::new(self.keys.clone(), self.exclude_keys.clone());
        if key_filter_stage.is_active() {
            script_stages.push(Box::new(key_filter_stage));
        }

        // Context processing is now handled within FilterStage

        // No limiter for parallel workers (limiting happens at the result sink level)
        let limiter: Option<Box<dyn EventLimiter>> = None;

        // Create pipeline context
        let ctx = PipelineContext {
            config: self.config,
            tracker: HashMap::new(),
            internal_tracker: HashMap::new(),
            window: Vec::new(),
            rhai: rhai_engine.clone(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
        };

        // Create chunker based on multiline configuration
        let chunker = if let Some(ref multiline_config) = self.multiline {
            create_multiline_chunker(multiline_config, self.input_format.clone())
                .map_err(|e| anyhow::anyhow!("Failed to create multiline chunker: {}", e))?
        } else {
            Box::new(SimpleChunker) as Box<dyn super::Chunker>
        };

        // Create window manager based on window_size configuration
        let window_manager: Box<dyn super::WindowManager> = if self.window_size > 0 {
            Box::new(SlidingWindowManager::new(self.window_size))
        } else {
            Box::new(SimpleWindowManager::new())
        };

        // Create timestamp config for consistent timestamp parsing
        let ts_config = crate::timestamp::TsConfig {
            custom_field: self.ts_field.clone(),
            custom_format: self.ts_format.clone(),
            default_timezone: self.default_timezone.clone(),
        };

        // Create worker pipeline (no output writer - results are collected by the processor)
        let pipeline = Pipeline {
            line_filter: None,
            chunker,
            parser,
            script_stages,
            limiter,
            formatter,
            output: Box::new(StdoutWriter), // This won't actually be used in parallel mode
            window_manager,
            span_processor: None,
            ts_config,
        };

        Ok((pipeline, ctx))
    }

    pub fn with_csv_headers(mut self, headers: Vec<String>) -> Self {
        self.csv_headers = Some(headers);
        self
    }

    pub fn with_timestamp_filter(
        mut self,
        timestamp_filter: Option<crate::config::TimestampFilterConfig>,
    ) -> Self {
        self.timestamp_filter = timestamp_filter;
        self
    }

    pub fn with_ts_field(mut self, ts_field: Option<String>) -> Self {
        self.ts_field = ts_field;
        self
    }

    pub fn with_ts_format(mut self, ts_format: Option<String>) -> Self {
        self.ts_format = ts_format;
        self
    }

    pub fn with_default_timezone(mut self, default_timezone: Option<String>) -> Self {
        self.default_timezone = default_timezone;
        self
    }

    pub fn with_extract_prefix(mut self, extract_prefix: Option<String>) -> Self {
        self.extract_prefix = extract_prefix;
        self
    }

    pub fn with_prefix_sep(mut self, prefix_sep: String) -> Self {
        self.prefix_sep = prefix_sep;
        self
    }

    pub fn with_cols_spec(mut self, cols_spec: Option<String>) -> Self {
        self.cols_spec = cols_spec;
        self
    }

    pub fn with_cols_sep(mut self, cols_sep: Option<String>) -> Self {
        self.cols_sep = cols_sep;
        self
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a pipeline from configuration
pub fn create_pipeline_from_config(
    config: &crate::config::KeloraConfig,
) -> Result<(Pipeline, BeginStage, EndStage, PipelineContext)> {
    let builder = create_pipeline_builder_from_config(config);
    builder.build(config.processing.stages.clone())
}

/// Create a pipeline builder from configuration (useful for parallel processing)
pub fn create_pipeline_builder_from_config(
    config: &crate::config::KeloraConfig,
) -> PipelineBuilder {
    let pipeline_config = PipelineConfig {
        brief: config.output.brief,
        wrap: config.output.wrap,
        pretty: config.output.pretty,
        color_mode: config.output.color.clone(),
        timestamp_formatting: config.output.timestamp_formatting.clone(),
        strict: config.processing.strict,
        verbose: config.processing.verbose,
        quiet_events: config.processing.quiet_events,
        suppress_diagnostics: config.processing.suppress_diagnostics,
        silent: config.processing.silent,
        suppress_script_output: config.processing.suppress_script_output,
        quiet_level: config.processing.quiet_level,
        emoji_mode: config.output.emoji.clone(),
        input_files: config.input.files.clone(),
        allow_fs_writes: config.processing.allow_fs_writes,
        format_name: Some(config.input.format.to_display_string()),
    };

    // Extract cols spec if needed before conversion
    let (input_format, cols_spec) = match &config.input.format {
        crate::config::InputFormat::Cols(spec) => (
            crate::config::InputFormat::Cols(spec.clone()),
            Some(spec.clone()),
        ),
        other => (other.clone(), None),
    };

    let mut builder = PipelineBuilder::new()
        .with_config(pipeline_config)
        .with_begin(config.processing.begin.clone())
        .with_end(config.processing.end.clone())
        .with_input_format(input_format)
        .with_output_format(config.output.format.clone().into())
        .with_cols_spec(cols_spec)
        .with_cols_sep(config.input.cols_sep.clone());
    builder.keys = config.output.get_effective_keys();
    builder.exclude_keys = config.output.exclude_keys.clone();
    builder.heat_range = config.output.heat_range;
    builder.levels = config.processing.levels.clone();
    builder.exclude_levels = config.processing.exclude_levels.clone();
    builder.multiline = config.input.multiline.clone();
    builder.window_size = config.processing.window_size;
    builder.timestamp_filter = config.processing.timestamp_filter.clone();
    builder.normalize_timestamps = config.processing.normalize_timestamps;
    builder.ts_field = config.input.ts_field.clone();
    builder.ts_format = config.input.ts_format.clone();
    builder.default_timezone = config.input.default_timezone.clone();
    builder.extract_prefix = config.input.extract_prefix.clone();
    builder.prefix_sep = config.input.prefix_sep.clone();
    builder.take_limit = config.processing.take_limit;
    builder.span = config.processing.span.clone();
    builder.context_config = config.processing.context.clone();
    builder.strict = config.processing.strict;
    builder
}

/// Create input reader with optional decompression for parallel processing
pub fn create_input_reader(
    config: &crate::config::KeloraConfig,
) -> Result<Box<dyn BufRead + Send>> {
    if config.input.no_input {
        // Create empty input for --no-input mode
        Ok(Box::new(BufReader::new(std::io::Cursor::new(Vec::new()))))
    } else if config.input.files.is_empty() {
        // Use stdin reader with gzip/zstd detection for Send compatibility
        let stdin_reader = crate::readers::ChannelStdinReader::new()?;
        let processed_stdin = crate::decompression::maybe_decompress(stdin_reader)?;
        Ok(Box::new(BufReader::new(processed_stdin)))
    } else {
        let sorted_files = sort_files(&config.input.files, &config.input.file_order)?;
        Ok(Box::new(MultiFileReader::new(
            sorted_files,
            config.processing.strict,
        )?))
    }
}

/// Create file-aware input reader for parallel processing with filename tracking
pub fn create_file_aware_input_reader(
    config: &crate::config::KeloraConfig,
) -> Result<Box<dyn crate::readers::FileAwareRead>> {
    if config.input.files.is_empty() {
        // For stdin, we don't have filename information
        // We'll need to create a wrapper that implements FileAwareRead
        Err(anyhow::anyhow!("File-aware reader not supported for stdin"))
    } else {
        let sorted_files = sort_files(&config.input.files, &config.input.file_order)?;
        Ok(Box::new(crate::readers::FileAwareMultiFileReader::new(
            sorted_files,
            config.processing.strict,
        )?))
    }
}

/// Sort files according to the specified file order
pub fn sort_files(files: &[String], order: &crate::config::FileOrder) -> Result<Vec<String>> {
    let mut sorted_files = files.to_vec();

    match order {
        crate::config::FileOrder::Cli => {
            // Keep CLI order - no sorting needed
        }
        crate::config::FileOrder::Name => {
            sorted_files.sort();
        }
        crate::config::FileOrder::Mtime => {
            // Sort by modification time (oldest first)
            sorted_files.sort_by(|a, b| {
                let mtime_a = fs::metadata(a)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let mtime_b = fs::metadata(b)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                mtime_a.cmp(&mtime_b)
            });
        }
    }

    Ok(sorted_files)
}
