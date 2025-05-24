use clap::Parser;
use std::io::{self, BufRead, BufReader};
use std::fs::File;
use std::path::PathBuf;

mod event;
mod parsers;
mod formatters;

// use event::Event;  // Not needed in main
use parsers::{LogParser, LogfmtParser, JsonlParser, SyslogParser};
use formatters::{Formatter, DefaultFormatter, JsonlFormatter};

#[derive(Parser)]
#[command(name = "kelora")]
#[command(about = "A fast, extensible log parser")]
pub struct Cli {
    /// Input files (stdin if not specified)
    pub files: Vec<PathBuf>,
    
    /// Input format
    #[arg(short = 'f', long = "format", value_enum, default_value = "logfmt")]
    pub input_format: InputFormat,
    
    /// Output format  
    #[arg(short = 'F', long = "output-format", value_enum, default_value = "default")]
    pub output_format: OutputFormat,
    
    /// Only show specific keys
    #[arg(short = 'k', long = "keys")]
    pub keys: Option<String>,
    
    /// Filter by log levels
    #[arg(short = 'l', long = "level")]
    pub levels: Option<String>,
    
    /// Show statistics
    #[arg(short = 'S', long = "stats-only")]
    pub stats_only: bool,
    
    /// Enable debug output
    #[arg(long)]
    pub debug: bool,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    let parser = create_parser(&cli.input_format);
    let formatter = create_formatter(&cli.output_format);
    
    let readers: Vec<Box<dyn BufRead>> = if cli.files.is_empty() {
        vec![Box::new(io::stdin().lock())]
    } else {
        cli.files.iter()
            .map(|path| -> anyhow::Result<Box<dyn BufRead>> {
                let file = File::open(path)
                    .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", path.display(), e))?;
                Ok(Box::new(BufReader::new(file)))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    
    let mut stats = Stats::new();
    let levels_filter = parse_levels_filter(&cli.levels);
    let keys_filter = parse_keys_filter(&cli.keys);
    
    for reader in readers {
        process_reader(reader, &*parser, &*formatter, &mut stats, &levels_filter, &keys_filter, &cli)?;
    }
    
    if cli.stats_only {
        print_stats(&stats);
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

fn parse_levels_filter(levels: &Option<String>) -> Option<Vec<String>> {
    levels.as_ref().map(|s| {
        s.split(',')
            .map(|level| level.trim().to_uppercase())
            .collect()
    })
}

fn parse_keys_filter(keys: &Option<String>) -> Option<Vec<String>> {
    keys.as_ref().map(|s| {
        s.split(',')
            .map(|key| key.trim().to_string())
            .collect()
    })
}

fn process_reader(
    reader: Box<dyn BufRead>,
    parser: &dyn LogParser,
    formatter: &dyn Formatter,
    stats: &mut Stats,
    levels_filter: &Option<Vec<String>>,
    keys_filter: &Option<Vec<String>>,
    cli: &Cli,
) -> anyhow::Result<()> {
    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        stats.lines_seen += 1;
        
        match parser.parse(&line) {
            Ok(mut event) => {
                // Apply filters
                if let Some(ref levels) = levels_filter {
                    if let Some(ref level) = event.level {
                        if !levels.contains(&level.to_uppercase()) {
                            continue;
                        }
                    }
                }
                
                // Apply key filtering
                if let Some(ref keys) = keys_filter {
                    event.filter_keys(keys);
                }
                
                stats.events_shown += 1;
                
                if !cli.stats_only {
                    println!("{}", formatter.format(&event));
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

#[derive(Debug, Default)]
struct Stats {
    lines_seen: usize,
    events_shown: usize,
    parse_errors: usize,
}

impl Stats {
    fn new() -> Self {
        Self::default()
    }
}

fn print_stats(stats: &Stats) {
    eprintln!("Events shown: {} (parse errors: {}, lines seen: {})", 
              stats.events_shown, stats.parse_errors, stats.lines_seen);
}