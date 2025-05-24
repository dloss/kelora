use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

mod event;
mod formatters;
mod parsers;

use formatters::{DefaultFormatter, Formatter, JsonlFormatter};
use parsers::{JsonlParser, LogParser, LogfmtParser, SyslogParser};

#[derive(Parser)]
#[command(name = "kelora")]
#[command(about = "A fast, extensible log parser")]
#[command(version = "0.1.0")]
#[command(author = "Dirk Loss <mail@dirk-loss.de>")]
pub struct Cli {
    /// Input files (stdin if not specified)
    pub files: Vec<PathBuf>,

    /// Input format
    #[arg(short = 'f', long = "format", value_enum, default_value = "logfmt")]
    pub input_format: InputFormat,

    /// Output format  
    #[arg(
        short = 'F',
        long = "output-format",
        value_enum,
        default_value = "default"
    )]
    pub output_format: OutputFormat,

    /// Only show specific keys (comma-separated)
    #[arg(short = 'k', long = "keys", value_delimiter = ',')]
    pub keys: Vec<String>,

    /// Filter by log levels (comma-separated)
    #[arg(short = 'l', long = "level", value_delimiter = ',')]
    pub levels: Vec<String>,

    /// Show statistics only
    #[arg(short = 'S', long = "stats-only")]
    pub stats_only: bool,

    /// Show statistics alongside output
    #[arg(short = 's', long = "stats")]
    pub stats: bool,

    /// Enable debug output
    #[arg(long)]
    pub debug: bool,

    /// Show only core fields (timestamp, level, message)
    #[arg(short = 'c', long = "common")]
    pub common: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum InputFormat {
    Logfmt,
    Jsonl,
    Syslog,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    Default,
    Jsonl,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub lines_seen: usize,
    pub events_shown: usize,
    pub parse_errors: usize,
    pub filtered_out: usize,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub levels_seen: HashMap<String, usize>,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_event(&mut self, event: &event::Event) {
        self.events_shown += 1;

        if let Some(timestamp) = &event.timestamp {
            if self.start_time.is_none() || Some(*timestamp) < self.start_time {
                self.start_time = Some(*timestamp);
            }
            if self.end_time.is_none() || Some(*timestamp) > self.end_time {
                self.end_time = Some(*timestamp);
            }
        }

        if let Some(level) = &event.level {
            *self.levels_seen.entry(level.clone()).or_insert(0) += 1;
        }
    }

    pub fn print_stats(&self) {
        eprintln!(
            "Events shown: {} (parse errors: {}, lines seen: {}, filtered: {})",
            self.events_shown, self.parse_errors, self.lines_seen, self.filtered_out
        );

        if let (Some(start), Some(end)) = (&self.start_time, &self.end_time) {
            let duration = end.signed_duration_since(*start);
            eprintln!(
                "Time span: {} to {} (duration: {})",
                start.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                end.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                format_duration(duration)
            );
        }

        if !self.levels_seen.is_empty() {
            let mut levels: Vec<_> = self.levels_seen.iter().collect();
            levels.sort_by_key(|(level, _)| level.as_str());
            eprintln!(
                "Log levels: {}",
                levels
                    .iter()
                    .map(|(level, count)| format!("{}({})", level, count))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

fn format_duration(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h{}m{}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m{}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let parser = create_parser(&cli.input_format);
    let formatter = create_formatter(&cli.output_format);

    let readers: Vec<Box<dyn BufRead>> = if cli.files.is_empty() {
        vec![Box::new(io::stdin().lock())]
    } else {
        cli.files
            .iter()
            .map(|path| open_input_file(path))
            .collect::<Result<Vec<_>>>()?
    };

    let mut stats = Stats::new();
    let levels_filter = prepare_levels_filter(&cli.levels);
    let keys_filter = prepare_keys_filter(&cli);

    for reader in readers {
        process_reader(
            reader,
            &*parser,
            &*formatter,
            &mut stats,
            &levels_filter,
            &keys_filter,
            &cli,
        )?;
    }

    if cli.stats_only || cli.stats {
        stats.print_stats();
    }

    Ok(())
}

fn create_parser(format: &InputFormat) -> Box<dyn LogParser> {
    match format {
        InputFormat::Logfmt => Box::new(LogfmtParser::new()),
        InputFormat::Jsonl => Box::new(JsonlParser::new()),
        InputFormat::Syslog => Box::new(SyslogParser::new()),
    }
}

fn create_formatter(format: &OutputFormat) -> Box<dyn Formatter> {
    match format {
        OutputFormat::Default => Box::new(DefaultFormatter::new()),
        OutputFormat::Jsonl => Box::new(JsonlFormatter::new()),
    }
}

fn open_input_file(path: &PathBuf) -> Result<Box<dyn BufRead>> {
    let file =
        File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;

    // Future: Add compression support here
    // if path.extension() == Some(OsStr::new("gz")) {
    //     use flate2::read::GzDecoder;
    //     Ok(Box::new(BufReader::new(GzDecoder::new(file))))
    // } else {
    Ok(Box::new(BufReader::new(file)))
    // }
}

fn prepare_levels_filter(levels: &[String]) -> Option<Vec<String>> {
    if levels.is_empty() {
        None
    } else {
        Some(levels.iter().map(|level| level.to_uppercase()).collect())
    }
}

fn prepare_keys_filter(cli: &Cli) -> Option<Vec<String>> {
    if cli.common {
        // Show only core fields
        Some(vec![
            "timestamp".to_string(),
            "level".to_string(),
            "message".to_string(),
        ])
    } else if !cli.keys.is_empty() {
        Some(cli.keys.clone())
    } else {
        None
    }
}

fn process_reader(
    reader: Box<dyn BufRead>,
    parser: &dyn LogParser,
    formatter: &dyn Formatter,
    stats: &mut Stats,
    levels_filter: &Option<Vec<String>>,
    keys_filter: &Option<Vec<String>>,
    cli: &Cli,
) -> Result<()> {
    for (line_num, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| format!("Failed to read line {}", line_num + 1))?;
        stats.lines_seen += 1;

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        match parser.parse(&line) {
            Ok(mut event) => {
                // Apply level filtering first
                if let Some(ref levels) = levels_filter {
                    if let Some(ref level) = event.level {
                        if !levels.contains(&level.to_uppercase()) {
                            stats.filtered_out += 1;
                            continue;
                        }
                    } else {
                        // If we're filtering by level but event has no level, filter it out
                        stats.filtered_out += 1;
                        continue;
                    }
                }

                // Apply key filtering
                if let Some(ref keys) = keys_filter {
                    event.filter_keys(keys);

                    // Skip events that have no displayable content after filtering
                    if !event.has_displayable_content() {
                        stats.filtered_out += 1;
                        continue;
                    }
                }

                // Record the event for stats
                stats.record_event(&event);

                // Output the event (unless we're in stats-only mode)
                if !cli.stats_only {
                    // Handle broken pipe gracefully (e.g., when piping to `head`)
                    if let Err(e) = writeln!(io::stdout(), "{}", formatter.format(&event)) {
                        if e.kind() == std::io::ErrorKind::BrokenPipe {
                            // Broken pipe is expected when piping to tools like `head`
                            break;
                        } else {
                            return Err(anyhow::Error::from(e));
                        }
                    }
                }
            }
            Err(e) => {
                stats.parse_errors += 1;
                if cli.debug {
                    eprintln!("Parse error on line {}: {}", line_num + 1, e);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        let duration = chrono::Duration::seconds(3661); // 1h 1m 1s
        assert_eq!(format_duration(duration), "1h1m1s");

        let duration = chrono::Duration::seconds(61); // 1m 1s
        assert_eq!(format_duration(duration), "1m1s");

        let duration = chrono::Duration::seconds(30); // 30s
        assert_eq!(format_duration(duration), "30s");
    }

    #[test]
    fn test_prepare_levels_filter() {
        let levels = vec!["error".to_string(), "warn".to_string()];
        let result = prepare_levels_filter(&levels);
        assert_eq!(result, Some(vec!["ERROR".to_string(), "WARN".to_string()]));

        let empty_levels: Vec<String> = vec![];
        let result = prepare_levels_filter(&empty_levels);
        assert_eq!(result, None);
    }
}
