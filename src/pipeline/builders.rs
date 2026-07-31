#![allow(dead_code)] // Builder API keeps unused setters for future CLI/config surfaces
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};

use crate::parsers::type_conversion::TypeMap;
use crate::pipeline::stages::ContextGroupStage;
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
        input_year: Option<i32>,
    ) -> Self {
        Self {
            inner,
            ts_config: crate::timestamp::TsConfig {
                custom_field: ts_field,
                custom_format: ts_format,
                default_timezone,
                input_year,
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

    // This wrapper only reconfigures timestamps; the level still comes from the
    // inner parser, so verbatim-ness is whatever the inner parser reports.
    fn level_appears_verbatim(&self) -> bool {
        self.inner.level_appears_verbatim()
    }

    // Projection support is whatever the inner parser reports. The custom
    // timestamp field (if any) is added to the projection by the builder so
    // `identify_timestamp_field` still finds it after pushdown.
    fn supports_projection(&self) -> bool {
        self.inner.supports_projection()
    }

    fn parse_projected(
        &self,
        line: &str,
        projection: &crate::projection::Projection,
    ) -> Result<crate::event::Event> {
        let mut event = self.inner.parse_projected(line, projection)?;
        event.extract_timestamp_with_config(None, &self.ts_config);
        Ok(event)
    }
}

use super::{
    create_multiline_chunker, AssertStage, BeginStage, CsvChunker, DrainDiffStage, DrainStage,
    EndStage, EventLimiter, EventParser, ExecStage, FilterStage, Formatter, KeyFilterStage,
    LevelFilterStage, MetaData, Pipeline, PipelineConfig, PipelineContext, ScriptStage,
    SimpleChunker, SimpleWindowManager, SlidingWindowManager, StdoutWriter, TakeNLimiter,
    TimestampConversionStage, TimestampFilterStage,
};
use crate::engine::{DebugConfig, RhaiEngine};
use crate::readers::MultiFileReader;
use crate::rhai_functions::file_ops::{self, RuntimeConfig};
use crate::rhai_functions::hashing;

/// Build a parser for a single cascade member. Handles the schema-less formats
/// plus the spec-based `cols:`/`regex:` parsers (reachable via repeated `-f`).
/// CSV-family and auto formats are rejected — they can't be mixed per line.
fn build_cascade_member_parser(
    format: &crate::config::InputFormat,
    custom_ts_config: bool,
    strict: bool,
    cols_sep: Option<&str>,
) -> Result<Box<dyn EventParser>> {
    let parser: Box<dyn EventParser> = match format {
        crate::config::InputFormat::Json => {
            if custom_ts_config {
                Box::new(
                    crate::parsers::JsonlParser::new_without_auto_timestamp().with_strict(strict),
                )
            } else {
                Box::new(crate::parsers::JsonlParser::new().with_strict(strict))
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
                Box::new(
                    crate::parsers::CefParser::new_without_auto_timestamp().with_strict(strict),
                )
            } else {
                Box::new(crate::parsers::CefParser::new().with_strict(strict))
            }
        }
        crate::config::InputFormat::Combined => {
            if custom_ts_config {
                Box::new(crate::parsers::CombinedParser::new_without_auto_timestamp()?)
            } else {
                Box::new(crate::parsers::CombinedParser::new()?)
            }
        }
        crate::config::InputFormat::Named(fmt) => {
            Box::new(crate::parsers::MultiRegexParser::new(fmt.patterns, strict)?)
        }
        crate::config::InputFormat::Cols(spec) => Box::new(
            crate::parsers::ColsParser::new(spec.clone(), cols_sep.map(str::to_string))
                .with_strict(strict),
        ),
        crate::config::InputFormat::Regex(pattern) => {
            Box::new(crate::parsers::RegexParser::new(pattern)?.with_strict(strict))
        }
        other => {
            return Err(anyhow::anyhow!(
                "format '{}' is not allowed inside a cascade list",
                other.cascade_name()
            ));
        }
    };
    Ok(parser)
}

/// Assemble a CascadingParser from a list of cascade members.
fn build_cascading_parser(
    formats: &[crate::config::InputFormat],
    custom_ts_config: bool,
    strict: bool,
    cols_sep: Option<&str>,
) -> Result<Box<dyn EventParser>> {
    if formats.len() < 2 {
        return Err(anyhow::anyhow!(
            "cascade format requires at least two formats"
        ));
    }
    let mut parsers: Vec<(String, Box<dyn EventParser>)> = Vec::with_capacity(formats.len());
    for fmt in formats {
        let name = fmt.cascade_name().to_string();
        let parser = build_cascade_member_parser(fmt, custom_ts_config, strict, cols_sep)?;
        parsers.push((name, parser));
    }
    Ok(Box::new(crate::parsers::CascadingParser::new(parsers)))
}

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
    drain_enabled: bool,
    drain_field: Option<String>,
    drain_diff_field: Option<String>,
    drain_diff_rule: Option<crate::config::DrainDiffRule>,
    ts_field: Option<String>,
    ts_format: Option<String>,
    default_timezone: Option<String>,
    input_year: Option<i32>,
    extract_prefix: Option<String>,
    prefix_sep: String,
    cols_spec: Option<String>,
    cols_sep: Option<String>,
    context_config: crate::config::ContextConfig,
    span: Option<crate::config::SpanConfig>,
    strict: bool,
    state_available: bool,
    csv_type_map: Option<TypeMap>,
    /// Whether the output mode permits the raw-line level pre-filter. False when
    /// `--stats` or `--discover` is active: those surface per-event counts and
    /// discovered fields that a pre-parse drop would change. Other conditions
    /// (parser, stage order, context, span, window) are checked in `build`.
    output_allows_prefilter: bool,
}

/// Hand `-B`/`-A`/`-C` to the leading run of match filters, as one group.
///
/// The run is the first contiguous stretch of stages that judge whether an event
/// matches (`--filter`, `--levels`), which is every filtering flag in a normal
/// invocation. `--since`/`--until` sit ahead of it and stay outside: they define
/// which events exist at all, so context is drawn from inside their window, not
/// across its edges. A filter separated from the run by an `--exec` also stays
/// outside — moving it into the group would run it before that `--exec` — and
/// instead lets context lines through unjudged.
fn install_context_group(
    script_stages: &mut Vec<Box<dyn ScriptStage>>,
    context_config: &crate::config::ContextConfig,
) {
    if !context_config.is_active() {
        return;
    }

    let Some(start) = script_stages
        .iter()
        .position(|stage| stage.is_match_filter())
    else {
        // Nothing to anchor to. Reaching here means the "context requires
        // filtering" check upstream let a run through with no filter at all;
        // leaving the pipeline untouched keeps that a no-op rather than a panic.
        return;
    };
    let mut end = start;
    while end + 1 < script_stages.len() && script_stages[end + 1].is_match_filter() {
        end += 1;
    }

    let group: Vec<Box<dyn ScriptStage>> = script_stages.drain(start..=end).collect();
    script_stages.insert(
        start,
        Box::new(ContextGroupStage::new(group, context_config)),
    );
}

impl PipelineBuilder {
    fn build_parser_internal(&self) -> Result<Box<dyn EventParser>> {
        // A named format may carry a default timestamp format for layouts the
        // adaptive parser can't resolve on its own (e.g. glog). The user's
        // explicit --ts-format always wins.
        let effective_ts_format = self.ts_format.clone().or_else(|| {
            if let crate::config::InputFormat::Named(fmt) = &self.input_format {
                fmt.ts_format.map(|s| s.to_string())
            } else {
                None
            }
        });

        let custom_ts_config = self.ts_field.is_some()
            || effective_ts_format.is_some()
            || self.default_timezone.is_some()
            || self.input_year.is_some();

        let base_parser: Box<dyn EventParser> = match self.input_format {
            crate::config::InputFormat::Auto => {
                return Err(anyhow::anyhow!(
                    "Auto format should be resolved before pipeline creation"
                ));
            }
            crate::config::InputFormat::AutoPerFile => Box::new(crate::parsers::LineParser::new()),
            crate::config::InputFormat::Json => {
                if custom_ts_config {
                    Box::new(
                        crate::parsers::JsonlParser::new_without_auto_timestamp()
                            .with_strict(self.strict),
                    )
                } else {
                    Box::new(crate::parsers::JsonlParser::new().with_strict(self.strict))
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
                    Box::new(
                        crate::parsers::CefParser::new_without_auto_timestamp()
                            .with_strict(self.strict),
                    )
                } else {
                    Box::new(crate::parsers::CefParser::new().with_strict(self.strict))
                }
            }
            crate::config::InputFormat::Csv(ref field_spec) => {
                let mut parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_csv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_csv()
                };

                if let Some(ref type_map) = self.csv_type_map {
                    parser = parser.with_type_map(type_map.clone());
                }

                // Strict applies even without a field spec: it governs row
                // shape (ragged rows) and header-annotation type conversion.
                let parser = parser.with_strict(self.strict);
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
                        .with_auto_timestamp(!custom_ts_config)
                } else if custom_ts_config {
                    parser.with_auto_timestamp(false)
                } else {
                    parser
                };

                Box::new(parser)
            }
            crate::config::InputFormat::Tsv(ref field_spec) => {
                let mut parser = if let Some(ref headers) = self.csv_headers {
                    crate::parsers::CsvParser::new_tsv_with_headers(headers.clone())
                } else {
                    crate::parsers::CsvParser::new_tsv()
                };

                if let Some(ref type_map) = self.csv_type_map {
                    parser = parser.with_type_map(type_map.clone());
                }

                // Strict applies even without a field spec: it governs row
                // shape (ragged rows) and header-annotation type conversion.
                let parser = parser.with_strict(self.strict);
                let parser = if let Some(ref spec) = field_spec {
                    parser
                        .with_field_spec(spec)?
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
                        crate::parsers::CsvParser::new_csv_no_headers_with_columns(headers.clone())
                            .with_strict(self.strict);
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser =
                        crate::parsers::CsvParser::new_csv_no_headers().with_strict(self.strict);
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
                        crate::parsers::CsvParser::new_tsv_no_headers_with_columns(headers.clone())
                            .with_strict(self.strict);
                    let parser = if custom_ts_config {
                        parser.with_auto_timestamp(false)
                    } else {
                        parser
                    };
                    Box::new(parser)
                } else {
                    let parser =
                        crate::parsers::CsvParser::new_tsv_no_headers().with_strict(self.strict);
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
            crate::config::InputFormat::Named(fmt) => Box::new(
                crate::parsers::MultiRegexParser::new(fmt.patterns, self.strict)?,
            ),
            crate::config::InputFormat::Cascade(ref formats) => build_cascading_parser(
                formats,
                custom_ts_config,
                self.strict,
                self.cols_sep.as_deref(),
            )?,
        };

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

        let parser: Box<dyn EventParser> = if custom_ts_config {
            Box::new(TimestampConfiguredParser::new(
                parser_with_prefix,
                self.ts_field.clone(),
                effective_ts_format,
                self.default_timezone.clone(),
                self.input_year,
            ))
        } else {
            parser_with_prefix
        };

        Ok(parser)
    }

    pub fn build_parser(&self) -> Result<Box<dyn EventParser>> {
        stats_set_timestamp_override(self.ts_field.clone(), self.ts_format.clone());
        self.build_parser_internal()
    }

    /// Compute the raw-line level pre-filter needles, or an empty vec when the
    /// safety gate forbids the optimization (spec §4). Empty means the pre-filter
    /// is inert and behavior is bit-identical to today.
    ///
    /// The gate requires, in addition to the output-mode check
    /// (`output_allows_prefilter`, set from `--stats`/`--discover`):
    ///  - a parser that extracts the level verbatim from the line text,
    ///  - no context (`-A`/`-B`/`-C`), span, or window feature (each observes
    ///    lines a pre-parse drop would skip),
    ///  - and an **include-only level filter as the first script stage**, so no
    ///    `--filter`/`--exec`/`--assert` or exclude-only filter runs first and
    ///    could observe or keep a line the pre-filter would drop.
    fn compute_level_prefilter_needles(
        &self,
        stages: &[crate::config::ScriptStageType],
        parser: &dyn EventParser,
    ) -> Vec<Vec<u8>> {
        use crate::config::ScriptStageType;

        // Internal escape hatch: force the pre-filter off, mirroring
        // `KELORA_NO_PROJECTION` in `compute_projection`. Undocumented; exists so
        // the differential test harness can compare pre-filter-on vs -off on
        // byte-identical commands — stdout *and* stderr, since this optimization's
        // one observable leak was into stderr diagnostics (#369) — and as an
        // operational safety valve.
        if std::env::var_os("KELORA_NO_LEVEL_PREFILTER").is_some() {
            return Vec::new();
        }

        if !self.output_allows_prefilter
            || self.context_config.is_active()
            || self.span.is_some()
            || self.window_size > 0
            || self.strict
            || !parser.level_appears_verbatim()
        {
            return Vec::new();
        }

        // Find the first stage that would actually be built, and require it to be
        // an include-only level filter.
        let mut include: Option<&[String]> = None;
        let mut decided = false;
        for stage in stages {
            match stage {
                ScriptStageType::Filter { .. }
                | ScriptStageType::Exec(_)
                | ScriptStageType::Assert(_) => {
                    decided = true; // a non-level stage runs first -> gate closed
                    break;
                }
                ScriptStageType::LevelFilter {
                    include: inc,
                    exclude: exc,
                } => {
                    if inc.is_empty() && exc.is_empty() {
                        continue; // inactive filter is not added; keep scanning
                    }
                    if !inc.is_empty() && exc.is_empty() {
                        include = Some(inc.as_slice());
                    }
                    decided = true;
                    break;
                }
            }
        }
        if !decided && !self.levels.is_empty() && self.exclude_levels.is_empty() {
            // No inline stage set the order: the appended fallback level filter
            // (from --levels or a config default) is the first stage.
            include = Some(self.levels.as_slice());
        }

        match include {
            Some(tokens) => super::level_prefilter_needles(tokens),
            None => Vec::new(),
        }
    }

    /// Compute the field [`Projection`] for the assembled pipeline (spec §3–4).
    ///
    /// Returns [`Projection::All`] — i.e. pushdown off, behavior byte-identical
    /// to before — unless a single setup-time check proves a bounded field set
    /// suffices. The gate is closed by any observer that can read arbitrary
    /// fields:
    ///  - no explicit `--keys` (default output prints every field; also covers
    ///    exclude-only `--exclude-keys`, whose `KeyFilterStage` demands `All`),
    ///  - a Rhai stage (`--filter`/`--exec`/`--assert`, default `Demand::All`)
    ///    or `--begin`/`--end` (which may run scripts touching events),
    ///  - `--stats` or `--discover` (they observe every field by design),
    ///  - `--span` or `--extract-prefix` (parser wrappers/observers not modeled
    ///    field-by-field in v1),
    ///  - a parser (or any cascade member) that does not support projection.
    ///
    /// Otherwise the needed set is the union of every stage's declared
    /// [`Demand::Fields`], plus the timestamp candidate fields (so `parsed_ts`,
    /// `_ts` output, and the `--stats` result-time span are unchanged) and any
    /// custom `--ts-field`.
    fn compute_projection(
        &self,
        script_stages: &[Box<dyn ScriptStage>],
        parser: &dyn EventParser,
    ) -> crate::projection::Projection {
        use crate::projection::{Demand, Projection};

        // Internal escape hatch: force projection off. Undocumented; exists so
        // the differential test harness can compare pushdown-on vs pushdown-off
        // on byte-identical commands, and as an operational safety valve.
        if std::env::var_os("KELORA_NO_PROJECTION").is_some() {
            return Projection::All;
        }

        // `output_allows_prefilter` is set from `--stats`/`--discover` being
        // absent — exactly the data-summary modes that observe all fields.
        let gate_open = !self.keys.is_empty()
            && self.begin.is_none()
            && self.end.is_none()
            && self.span.is_none()
            && self.extract_prefix.is_none()
            && self.output_allows_prefilter
            && parser.supports_projection();

        if !gate_open {
            return Projection::All;
        }

        let mut needed = crate::projection::FieldNameSet::default();
        for stage in script_stages {
            match stage.field_demands() {
                Demand::All => return Projection::All,
                Demand::Fields(fields) => needed.extend(fields),
                Demand::Nothing => {}
            }
        }

        // The parser derives `parsed_ts` from the timestamp candidate fields
        // (json/logfmt always attempt this, whether via auto-detection or the
        // TimestampConfiguredParser wrapper). Keeping them makes `parsed_ts`
        // — and everything that reads it — identical under projection.
        for name in crate::event::TIMESTAMP_FIELD_NAMES {
            needed.insert((*name).to_string());
        }
        if let Some(ref ts_field) = self.ts_field {
            needed.insert(ts_field.clone());
        }

        // Always keep the level-field *values*. The always-on stats collection
        // (active whenever diagnostics are not suppressed, not just under
        // `--stats`) records the discovered level by stringifying the first
        // present `LEVEL_FIELD_NAMES` value; a `UNIT` placeholder there would
        // drop it from that set. Names of unwanted fields are preserved by the
        // parser's placeholder, so `discovered_keys` needs no special handling.
        for name in crate::event::LEVEL_FIELD_NAMES {
            needed.insert((*name).to_string());
        }

        Projection::Only(needed)
    }

    /// The keep-list for [`KeyFilterStage`], which is `--keys` plus — for the
    /// compact map formats only — the timestamp candidate fields.
    ///
    /// The map formats prefix each line with a timestamp, which
    /// `compact_map_utils::extract_timestamp` reads from `parsed_ts`. That
    /// value is cleared and re-derived from the *surviving* fields immediately
    /// before formatting (see `Pipeline::process_event`), so a timestamp field
    /// removed by `--keys` reparses to `None` and every line falls back to
    /// `line N`. `keymap`/`tailmap` *require* `--keys` with exactly one field,
    /// which made that fallback unconditional for them.
    ///
    /// Keeping the candidates is invisible in the output — these formats render
    /// one glyph per event and never print fields — and an explicit
    /// `--exclude-keys ts` still wins, since exclusions apply on top of this
    /// list.
    fn key_filter_keys(&self) -> Vec<String> {
        // No `--keys` means no field selection to repair: the timestamp is
        // already present, and an exclude-only filter demands every field.
        if self.keys.is_empty() || !self.output_format.is_compact_map() {
            return self.keys.clone();
        }

        let mut keys = self.keys.clone();
        let mut keep = |name: &str| {
            if !keys.iter().any(|existing| existing == name) {
                keys.push(name.to_string());
            }
        };
        for name in crate::event::TIMESTAMP_FIELD_NAMES {
            keep(name);
        }
        if let Some(ref ts_field) = self.ts_field {
            keep(ts_field);
        }
        keys
    }

    pub fn new() -> Self {
        Self {
            config: PipelineConfig {
                brief: false,
                wrap: crate::config::WrapMode::Auto,
                pretty: false,
                color_mode: crate::config::ColorMode::Auto,
                timestamp_formatting: crate::config::TimestampFormatConfig::default(),
                format_name: None,
                strict: false,
                verbose: 0,
                quiet_events: false,
                suppress_warnings: false,
                suppress_hints: false,
                silent: false,
                suppress_script_output: false,
                quiet_level: 0,
                emoji_mode: crate::config::EmojiMode::Auto,
                legend_mode: crate::config::LegendMode::Auto,
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
            drain_enabled: false,
            drain_field: None,
            drain_diff_field: None,
            drain_diff_rule: None,
            ts_field: None,
            ts_format: None,
            default_timezone: None,
            input_year: None,
            extract_prefix: None,
            prefix_sep: "|".to_string(),
            cols_spec: None,
            cols_sep: None,
            context_config: crate::config::ContextConfig::disabled(),
            span: None,
            strict: false,
            state_available: true,
            csv_type_map: None,
            output_allows_prefilter: true,
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
        rhai_engine.set_state_available(self.state_available);
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
            quiet_level: self.config.quiet_level,
        });

        stats_set_timestamp_override(self.ts_field.clone(), self.ts_format.clone());
        let parser = self.build_parser_internal()?;
        let level_prefilter_needles =
            self.compute_level_prefilter_needles(&stages, parser.as_ref());
        // Tell the diagnostics layer the parser will only ever see lines carrying a
        // requested level token, so `discovered_levels` is a subset (#369).
        crate::stats::set_level_prefilter_active(!level_prefilter_needles.is_empty());

        // Create formatter
        let use_colors = crate::tty::should_use_colors_with_mode(&self.config.color_mode);
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        let show_legend = crate::tty::should_show_legend(&self.config.legend_mode);
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
                        crate::tty::should_wrap(&self.config.wrap),
                        self.config.pretty,
                    ))
                }
                crate::OutputFormat::Inspect => Box::new(crate::formatters::InspectFormatter::new(
                    self.config.verbose,
                )),
                crate::OutputFormat::Logfmt => Box::new(crate::formatters::LogfmtFormatter::new()),
                crate::OutputFormat::Levelmap => Box::new(
                    crate::formatters::LevelmapFormatter::new(use_colors, use_emoji, show_legend),
                ),
                crate::OutputFormat::Keymap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "keymap output requires exactly one field via --keys, e.g. --keys level. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::KeymapFormatter::new(
                        Some(self.keys[0].clone()),
                        use_emoji,
                        show_legend,
                    ))
                }
                crate::OutputFormat::Tailmap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "tailmap output requires exactly one numeric field via --keys, e.g. --keys latency_ms. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::TailmapFormatter::new(
                        Some(self.keys[0].clone()),
                        self.config.emoji_mode.clone(),
                        self.config.color_mode.clone(),
                        show_legend,
                    ))
                }
                crate::OutputFormat::Csv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSV output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new(self.keys.clone()))
                }
                crate::OutputFormat::Tsv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSV output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv(self.keys.clone()))
                }
                crate::OutputFormat::Csvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSVNH output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_csv_no_header(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSVNH output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
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

        let has_inline_level_stage = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::LevelFilter { .. }));

        // Time-window selection runs FIRST, ahead of every user stage. `--since`/`--until`
        // narrow which events exist for the rest of the run, so metrics accumulated in
        // script stages (`track_freq`, and the `--freq`/`--describe`/`--card` sugar that
        // compiles to it) must never see events outside the window. Filtering afterwards
        // produced whole-file aggregates alongside a correctly windowed event stream.
        if let Some(timestamp_filter_config) = self.timestamp_filter.clone() {
            let timestamp_filter_stage = TimestampFilterStage::new(timestamp_filter_config);
            script_stages.push(Box::new(timestamp_filter_stage));
        }

        for stage in stages {
            match stage {
                crate::config::ScriptStageType::Filter { script, includes } => {
                    let filter_stage = FilterStage::new(script, includes, &mut rhai_engine)?
                        .with_stage_number(stage_number);
                    script_stages.push(Box::new(filter_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Exec(exec) => {
                    let exec_stage =
                        ExecStage::new(exec, &mut rhai_engine)?.with_stage_number(stage_number);
                    script_stages.push(Box::new(exec_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Assert(assertion) => {
                    let assert_stage = AssertStage::new(assertion, &mut rhai_engine)?
                        .with_stage_number(stage_number);
                    script_stages.push(Box::new(assert_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::LevelFilter { include, exclude } => {
                    let level_stage = LevelFilterStage::new(include, exclude);
                    if level_stage.is_active() {
                        script_stages.push(Box::new(level_stage));
                        stage_number += 1;
                    }
                }
            }
        }

        if !has_inline_level_stage {
            let level_stage =
                LevelFilterStage::new(self.levels.clone(), self.exclude_levels.clone());
            if level_stage.is_active() {
                script_stages.push(Box::new(level_stage));
            }
        }

        install_context_group(&mut script_stages, &self.context_config);

        if self.normalize_timestamps {
            let conversion_stage = TimestampConversionStage::new(
                self.ts_field.clone(),
                self.ts_format.clone(),
                self.default_timezone.clone(),
                self.input_year,
            );
            script_stages.push(Box::new(conversion_stage));
        }

        if self.drain_enabled {
            let field = self.drain_field.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "--drain requires exactly one effective field in --keys after exclusions, e.g. --keys msg. Use -s to inspect available fields."
                )
            })?;
            script_stages.push(Box::new(DrainStage::new(field)));
        }

        if let Some(rule) = self.drain_diff_rule.clone() {
            let field = self.drain_diff_field.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "--drain-diff requires exactly one effective field in --keys after exclusions, e.g. --keys msg. Use -s to inspect available fields."
                )
            })?;
            let stage = DrainDiffStage::new(field, rule.clone());
            let stage = match &rule {
                crate::config::DrainDiffRule::Predicate { expr, includes, .. } => {
                    stage.with_cut_predicate(expr, includes, &mut rhai_engine)?
                }
                _ => stage,
            };
            script_stages.push(Box::new(stage));
        }

        // Add key filtering stage (runs after level filtering, before context processing)
        let key_filter_stage =
            KeyFilterStage::new(self.key_filter_keys(), self.exclude_keys.clone());
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

        // Projection pushdown: decide which fields the parser must materialize
        // now that every stage (and its field demands) is known. Computed before
        // `self` is partially moved into the pipeline context below.
        let projection = self.compute_projection(&script_stages, parser.as_ref());

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
            internal_stats: super::InternalStats::default(),
            window: Vec::new(),
            rhai: rhai_engine.clone(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
            pending_span_rows: Vec::new(),
        };

        // Create chunker based on multiline configuration. An explicit --multiline
        // strategy wins; otherwise CSV/TSV input gets the quote-aware chunker so
        // embedded-newline records are reassembled before parsing, and everything
        // else passes through one line at a time.
        let chunker = if let Some(ref multiline_config) = self.multiline {
            create_multiline_chunker(multiline_config)
                .map_err(|e| anyhow::anyhow!("Failed to create multiline chunker: {}", e))?
        } else if self.input_format.is_csv_like() {
            Box::new(CsvChunker::new()) as Box<dyn super::Chunker>
        } else {
            Box::new(SimpleChunker) as Box<dyn super::Chunker>
        };
        let chunker_is_passthrough = chunker.is_passthrough();

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
            input_year: self.input_year,
        };

        // Window maintenance is only needed if --window was set or a stage
        // reads the `window` variable.
        let window_active = self.window_size > 0 || script_stages.iter().any(|s| s.uses_window());

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
            timestamp_window: self.timestamp_filter.clone(),
            window_active,
            level_prefilter_needles,
            projection,
            chunk_buf: Vec::new(),
            chunker_is_passthrough,
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

    pub fn with_drain(mut self, enabled: bool, field: Option<String>) -> Self {
        self.drain_enabled = enabled;
        self.drain_field = field;
        self
    }

    pub fn with_drain_diff(
        mut self,
        field: Option<String>,
        rule: Option<crate::config::DrainDiffRule>,
    ) -> Self {
        self.drain_diff_field = field;
        self.drain_diff_rule = rule;
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
        if self.drain_enabled {
            return Err(anyhow::anyhow!(
                "--drain summary is not supported with --parallel. Rerun without --parallel to use Drain template mining."
            ));
        }
        if self.drain_diff_rule.is_some() {
            return Err(anyhow::anyhow!(
                "--drain-diff is not supported with --parallel. Rerun without --parallel."
            ));
        }
        let mut rhai_engine = RhaiEngine::new();
        rhai_engine.set_state_available(self.state_available);

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
            quiet_level: self.config.quiet_level,
        });

        stats_set_timestamp_override(self.ts_field.clone(), self.ts_format.clone());
        let parser = self.build_parser_internal()?;
        let level_prefilter_needles =
            self.compute_level_prefilter_needles(&stages, parser.as_ref());
        // Same subset caveat as the sequential path (#369).
        crate::stats::set_level_prefilter_active(!level_prefilter_needles.is_empty());

        // Create formatter (workers still need formatters for output)
        let use_colors = crate::tty::should_use_colors_with_mode(&self.config.color_mode);
        let use_emoji = crate::tty::should_use_emoji_with_mode(
            &self.config.emoji_mode,
            &self.config.color_mode,
        );
        let show_legend = crate::tty::should_show_legend(&self.config.legend_mode);
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
                        crate::tty::should_wrap(&self.config.wrap),
                        self.config.pretty,
                    ))
                }
                crate::OutputFormat::Inspect => Box::new(crate::formatters::InspectFormatter::new(
                    self.config.verbose,
                )),
                crate::OutputFormat::Logfmt => Box::new(crate::formatters::LogfmtFormatter::new()),
                crate::OutputFormat::Levelmap => Box::new(
                    crate::formatters::LevelmapFormatter::new(use_colors, use_emoji, show_legend),
                ),
                crate::OutputFormat::Keymap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "keymap output requires exactly one field via --keys, e.g. --keys level. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::KeymapFormatter::new(
                        Some(self.keys[0].clone()),
                        use_emoji,
                        show_legend,
                    ))
                }
                crate::OutputFormat::Tailmap => {
                    if self.keys.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "tailmap output requires exactly one numeric field via --keys, e.g. --keys latency_ms. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::TailmapFormatter::new(
                        Some(self.keys[0].clone()),
                        self.config.emoji_mode.clone(),
                        self.config.color_mode.clone(),
                        show_legend,
                    ))
                }
                crate::OutputFormat::Csv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSV output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsv => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSV output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_tsv_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Csvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "CSVNH output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
                        ));
                    }
                    Box::new(crate::formatters::CsvFormatter::new_csv_no_header_worker(
                        self.keys.clone(),
                    ))
                }
                crate::OutputFormat::Tsvnh => {
                    if self.keys.is_empty() {
                        return Err(anyhow::anyhow!(
                            "TSVNH output requires --keys to define column order, e.g. --keys ts,level,msg. Use -s to inspect available fields."
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

        let has_inline_level_stage = stages
            .iter()
            .any(|stage| matches!(stage, crate::config::ScriptStageType::LevelFilter { .. }));

        // Same ordering as the sequential builder: the time window is applied before
        // any user stage so worker-local metrics only ever see in-window events.
        if let Some(timestamp_filter_config) = self.timestamp_filter.clone() {
            let timestamp_filter_stage = TimestampFilterStage::new(timestamp_filter_config);
            script_stages.push(Box::new(timestamp_filter_stage));
        }

        for stage in stages {
            match stage {
                crate::config::ScriptStageType::Filter { script, includes } => {
                    let filter_stage = FilterStage::new(script, includes, &mut rhai_engine)?
                        .with_stage_number(stage_number);
                    script_stages.push(Box::new(filter_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Exec(exec) => {
                    let exec_stage =
                        ExecStage::new(exec, &mut rhai_engine)?.with_stage_number(stage_number);
                    script_stages.push(Box::new(exec_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::Assert(assertion) => {
                    let assert_stage = AssertStage::new(assertion, &mut rhai_engine)?
                        .with_stage_number(stage_number);
                    script_stages.push(Box::new(assert_stage));
                    stage_number += 1;
                }
                crate::config::ScriptStageType::LevelFilter { include, exclude } => {
                    let level_stage = LevelFilterStage::new(include, exclude);
                    if level_stage.is_active() {
                        script_stages.push(Box::new(level_stage));
                        stage_number += 1;
                    }
                }
            }
        }

        if !has_inline_level_stage {
            let level_stage =
                LevelFilterStage::new(self.levels.clone(), self.exclude_levels.clone());
            if level_stage.is_active() {
                script_stages.push(Box::new(level_stage));
            }
        }

        install_context_group(&mut script_stages, &self.context_config);

        // Add key filtering stage (runs after level filtering, before context processing)
        let key_filter_stage =
            KeyFilterStage::new(self.key_filter_keys(), self.exclude_keys.clone());
        if key_filter_stage.is_active() {
            script_stages.push(Box::new(key_filter_stage));
        }

        // Context processing is now handled within FilterStage

        // Projection pushdown: computed before `self` is partially moved into
        // the pipeline context below.
        let projection = self.compute_projection(&script_stages, parser.as_ref());

        // No limiter for parallel workers (limiting happens at the result sink level)
        let limiter: Option<Box<dyn EventLimiter>> = None;

        // Create pipeline context
        let ctx = PipelineContext {
            config: self.config,
            tracker: HashMap::new(),
            internal_tracker: HashMap::new(),
            internal_stats: super::InternalStats::default(),
            window: Vec::new(),
            rhai: rhai_engine.clone(),
            meta: MetaData::default(),
            pending_file_ops: Vec::new(),
            discovered_levels: std::collections::HashSet::new(),
            discovered_keys: std::collections::HashSet::new(),
            discovered_levels_output: std::collections::HashSet::new(),
            discovered_keys_output: std::collections::HashSet::new(),
            pending_span_rows: Vec::new(),
        };

        // Create chunker based on multiline configuration. Mirrors `build`: an
        // explicit --multiline strategy wins; otherwise csv-like input gets the
        // quote-aware chunker so embedded-newline records that span physical lines
        // *within a batch* are reassembled before parsing. The batcher guarantees
        // batches never end mid-record, so the chunker never has to span batches.
        let chunker = if let Some(ref multiline_config) = self.multiline {
            create_multiline_chunker(multiline_config)
                .map_err(|e| anyhow::anyhow!("Failed to create multiline chunker: {}", e))?
        } else if self.input_format.is_csv_like() {
            Box::new(CsvChunker::new()) as Box<dyn super::Chunker>
        } else {
            Box::new(SimpleChunker) as Box<dyn super::Chunker>
        };
        let chunker_is_passthrough = chunker.is_passthrough();

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
            input_year: self.input_year,
        };

        let window_active = self.window_size > 0 || script_stages.iter().any(|s| s.uses_window());

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
            timestamp_window: self.timestamp_filter.clone(),
            window_active,
            level_prefilter_needles,
            projection,
            chunk_buf: Vec::new(),
            chunker_is_passthrough,
        };

        Ok((pipeline, ctx))
    }

    pub fn with_csv_headers(mut self, headers: Vec<String>) -> Self {
        self.csv_headers = Some(headers);
        self
    }

    pub fn with_csv_type_map(mut self, type_map: TypeMap) -> Self {
        self.csv_type_map = Some(type_map);
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

/// The single field `--drain` and `--drain-diff` mine: the only key left in
/// `--keys` after `--exclude-keys`. `None` when the request does not resolve to
/// exactly one field, which both modes reject as a usage error before the
/// pipeline is built.
pub fn single_effective_key(config: &crate::config::KeloraConfig) -> Option<String> {
    let mut effective = config
        .output
        .keys
        .iter()
        .filter(|key| !config.output.exclude_keys.contains(key));
    let first = effective.next()?;
    match effective.next() {
        None => Some(first.clone()),
        Some(_) => None,
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
        wrap: config.output.wrap.clone(),
        pretty: config.output.pretty,
        color_mode: config.output.color.clone(),
        timestamp_formatting: config.output.timestamp_formatting.clone(),
        strict: config.processing.strict,
        verbose: config.processing.verbose,
        quiet_events: config.processing.quiet_events,
        suppress_warnings: config.processing.suppress_warnings,
        suppress_hints: config.processing.suppress_hints,
        silent: config.processing.silent,
        suppress_script_output: config.processing.suppress_script_output,
        quiet_level: config.processing.quiet_level,
        emoji_mode: config.output.emoji.clone(),
        legend_mode: config.output.legend.clone(),
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

    // --drain and --drain-diff mine the same single effective key.
    let drain_enabled = config.output.drain.is_some();
    let drain_field = if drain_enabled {
        single_effective_key(config)
    } else {
        None
    };
    let drain_diff_rule = config.processing.drain_diff_rule.clone();
    let drain_diff_field = if drain_diff_rule.is_some() {
        single_effective_key(config)
    } else {
        None
    };

    let mut builder = PipelineBuilder::new()
        .with_config(pipeline_config)
        .with_begin(config.processing.begin.clone())
        .with_end(config.processing.end.clone())
        .with_input_format(input_format)
        .with_output_format(config.output.format.clone().into())
        .with_drain(drain_enabled, drain_field)
        .with_drain_diff(drain_diff_field, drain_diff_rule)
        .with_cols_spec(cols_spec)
        .with_cols_sep(config.input.cols_sep.clone());
    builder.keys = config.output.get_effective_keys();
    builder.exclude_keys = config.output.exclude_keys.clone();
    builder.levels = config.processing.levels.clone();
    builder.exclude_levels = config.processing.exclude_levels.clone();
    builder.multiline = config.input.multiline.clone();
    builder.window_size = config.processing.window_size;
    builder.timestamp_filter = config.processing.timestamp_filter.clone();
    builder.normalize_timestamps = config.processing.normalize_timestamps;
    builder.ts_field = config.input.ts_field.clone();
    builder.ts_format = config.input.ts_format.clone();
    builder.default_timezone = config.input.default_timezone.clone();
    builder.input_year = config.input.input_year;
    builder.extract_prefix = config.input.extract_prefix.clone();
    builder.prefix_sep = config.input.prefix_sep.clone();
    builder.take_limit = config.processing.take_limit;
    builder.span = config.processing.span.clone();
    builder.context_config = config.processing.context.clone();
    builder.strict = config.processing.strict;
    builder.state_available = !config.should_use_parallel();
    // --stats and --discover surface per-event counts / discovered fields that a
    // pre-parse drop would change, so they disable the level pre-filter.
    builder.output_allows_prefilter =
        config.output.stats.is_none() && config.output.discover_fields.is_none();
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

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::event::Event;
    use crate::pipeline::{ScriptResult, ScriptStage};
    use crate::projection::{Demand, Projection};

    /// A stage that deliberately does NOT override `field_demands`, so it
    /// inherits the fail-safe `Demand::All`. Its presence must collapse the
    /// projection to `All` (spec §6 tripwire): a future stage added without
    /// considering projection can never silently narrow the field set.
    struct MockStageNoDemands;
    impl ScriptStage for MockStageNoDemands {
        fn apply(&mut self, event: Event, _ctx: &mut PipelineContext) -> ScriptResult {
            ScriptResult::Emit(event)
        }
    }

    fn builder_with_keys(keys: &[&str]) -> PipelineBuilder {
        let mut b = PipelineBuilder::new();
        b.keys = keys.iter().map(|s| s.to_string()).collect();
        b
    }

    fn json_parser() -> Box<dyn EventParser> {
        Box::new(crate::parsers::JsonlParser::new())
    }

    #[test]
    fn tripwire_unknown_stage_forces_all() {
        let builder = builder_with_keys(&["msg"]);
        let stages: Vec<Box<dyn ScriptStage>> = vec![
            Box::new(MockStageNoDemands),
            Box::new(KeyFilterStage::new(vec!["msg".to_string()], vec![])),
        ];
        let projection = builder.compute_projection(&stages, json_parser().as_ref());
        assert!(
            projection.is_all(),
            "a stage without an explicit field_demands must fail safe to All"
        );
    }

    #[test]
    fn keys_yield_only_projection_with_ts_and_level() {
        let builder = builder_with_keys(&["msg"]);
        let stages: Vec<Box<dyn ScriptStage>> = vec![Box::new(KeyFilterStage::new(
            vec!["msg".to_string()],
            vec![],
        ))];
        let projection = builder.compute_projection(&stages, json_parser().as_ref());
        match projection {
            Projection::Only(set) => {
                assert!(set.contains("msg"), "the -k field must be kept");
                // parsed_ts + discovered-level completeness rely on these.
                for name in crate::event::TIMESTAMP_FIELD_NAMES {
                    assert!(set.contains(*name), "ts candidate {name} must be kept");
                }
                for name in crate::event::LEVEL_FIELD_NAMES {
                    assert!(set.contains(*name), "level candidate {name} must be kept");
                }
            }
            Projection::All => panic!("expected a bounded projection for -k msg"),
        }
    }

    #[test]
    fn no_keys_forces_all() {
        let builder = PipelineBuilder::new(); // keys empty -> default output prints all
        let stages: Vec<Box<dyn ScriptStage>> = vec![];
        assert!(builder
            .compute_projection(&stages, json_parser().as_ref())
            .is_all());
    }

    #[test]
    fn exclude_only_key_filter_forces_all() {
        // --exclude-keys with no -k: KeyFilterStage demands All (it must see
        // every field to decide what to drop).
        let stage = KeyFilterStage::new(vec![], vec!["secret".to_string()]);
        assert!(matches!(stage.field_demands(), Demand::All));
    }

    #[test]
    fn rhai_stage_demand_defaults_to_all() {
        let mut engine = crate::engine::RhaiEngine::new();
        let filter = FilterStage::new("true".to_string(), vec![], &mut engine).unwrap();
        assert!(
            matches!(filter.field_demands(), Demand::All),
            "Rhai stages can read arbitrary fields and must demand All"
        );
    }

    #[test]
    fn unsupported_parser_forces_all() {
        // The line parser does not support projection; even with -k the gate
        // must stay closed.
        let builder = builder_with_keys(&["line"]);
        let stages: Vec<Box<dyn ScriptStage>> = vec![Box::new(KeyFilterStage::new(
            vec!["line".to_string()],
            vec![],
        ))];
        let parser: Box<dyn EventParser> = Box::new(crate::parsers::LineParser::new());
        assert!(builder
            .compute_projection(&stages, parser.as_ref())
            .is_all());
    }

    #[test]
    fn begin_end_span_prefix_force_all() {
        let stages: Vec<Box<dyn ScriptStage>> = vec![Box::new(KeyFilterStage::new(
            vec!["msg".to_string()],
            vec![],
        ))];

        let mut b = builder_with_keys(&["msg"]);
        b.begin = Some("x = 1".to_string());
        assert!(b
            .compute_projection(&stages, json_parser().as_ref())
            .is_all());

        let mut b = builder_with_keys(&["msg"]);
        b.extract_prefix = Some("app".to_string());
        assert!(b
            .compute_projection(&stages, json_parser().as_ref())
            .is_all());
    }
}
